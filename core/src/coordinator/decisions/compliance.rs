//! Native review objections, scoped to the exact task/context/decision versions.
use super::{store::*, *};
use serde::{Deserialize, Serialize};
use serde_json::Value;

pub const COMPLIANCE_BLOCK_KEY: &str = "review_compliance_block";

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ComplianceFinding {
    pub decision_id: String,
    pub decision_kind: String,
    pub version: u64,
    pub status: String,
    pub evidence: String,
    pub resolution_origin: Option<String>,
}

impl ComplianceFinding {
    pub(super) fn from_binding(binding: &Decision, check: Option<&Compliance>) -> Self {
        Self {
            decision_id: binding.id.clone(),
            decision_kind: binding.request.kind.clone(),
            version: binding.version,
            status: check
                .map(|c| c.status.clone())
                .unwrap_or_else(|| "missing".into()),
            evidence: check
                .map(|c| c.evidence.clone())
                .unwrap_or_else(|| "No independent compliance evidence was provided".into()),
            resolution_origin: binding.resolution.as_ref().map(|r| r.origin.clone()),
        }
    }
    pub fn describe(&self) -> String {
        let label = match self.status.as_str() {
            "needs_human_review" => "independent review requires governed reconciliation",
            "fail" => "independent compliance failed",
            "missing" => "independent compliance evidence is missing",
            other => other,
        };
        format!(
            "Decision {} v{} ({}): {}",
            self.decision_id, self.version, label, self.evidence
        )
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ReviewComplianceBlock {
    pub task_id: String,
    pub plan_id: String,
    pub attempt_id: String,
    pub context_digest: String,
    pub findings: Vec<ComplianceFinding>,
}
impl ReviewComplianceBlock {
    pub fn needs_human_review(&self) -> bool {
        self.findings
            .iter()
            .any(|f| f.status == "needs_human_review")
    }
    pub fn describe(&self) -> String {
        format!(
            "DM_COMPLIANCE: {}",
            self.findings
                .iter()
                .map(ComplianceFinding::describe)
                .collect::<Vec<_>>()
                .join("; ")
        )
    }
    pub fn next_action(&self) -> String {
        let refs = self
            .findings
            .iter()
            .map(|f| format!("{} v{}", f.decision_id, f.version))
            .collect::<Vec<_>>()
            .join(", ");
        format!("Open Decisions for {} and inspect {} with the recorded independent finding. Reconcile the exact binding through an explicit governed revision, or provide the missing independent evidence. Mandatory project requirements remain binding. Then resume the retained phase with fresh context and independent review; an unchanged restart cannot establish compliance.",self.task_id,refs)
    }
}

impl Store {
    /// An arbitrary error string or cached projection cannot supply this proof.
    pub fn review_compliance_block(
        &self,
        task: &str,
        pin: &Value,
    ) -> crate::Result<Option<ReviewComplianceBlock>> {
        let (Some(plan_id), Some(attempt), Some(digest)) = (
            pin["plan_id"].as_str(),
            pin["attempt_id"].as_str(),
            pin["digest"].as_str(),
        ) else {
            return Ok(None);
        };
        if pin["phase"] != "review" {
            return Ok(None);
        }
        let plan = self.plan(plan_id)?;
        if plan.project_id != self.project_id {
            return Ok(None);
        }
        let context = self.read_context(plan_id, attempt, "review")?;
        if context.task_id != task || context.context_digest != digest {
            return Ok(None);
        }
        let policy = policy(&self.root)?;
        if !self
            .context(&plan, task, attempt, "review", &policy)
            .is_ok_and(|fresh| fresh.context_digest == digest)
        {
            return Ok(None);
        }
        let raw: Option<String> = sql(self.db.query_row(
            "SELECT receipt FROM contexts WHERE plan_id=?1 AND attempt_id=?2 AND phase='review'",
            rusqlite::params![plan_id, attempt],
            |r| r.get(0),
        ))?;
        let Some(raw) = raw else { return Ok(None) };
        let receipt: ContextReceipt = decode(&raw)?;
        if !receipt.acknowledged
            || receipt.context_digest != digest
            || receipt.completion_status != "compliance_failed"
        {
            return Ok(None);
        }
        let findings = context
            .bindings
            .iter()
            .filter(|b| !b.resolution.as_ref().is_some_and(|r| r.advisory))
            .filter(|b| {
                receipt
                    .compliance
                    .get(&b.id)
                    .is_none_or(|c| c.status != "pass")
            })
            .map(|b| ComplianceFinding::from_binding(b, receipt.compliance.get(&b.id)))
            .collect::<Vec<_>>();
        if findings.is_empty() {
            return Ok(None);
        }
        Ok(Some(ReviewComplianceBlock {
            task_id: task.into(),
            plan_id: plan_id.into(),
            attempt_id: attempt.into(),
            context_digest: digest.into(),
            findings,
        }))
    }
}

/// Re-read authoritative evidence; task projections never authorize governance.
pub fn review_compliance_for_task(
    root: &std::path::Path,
    task: &crate::coordinator::model::Task,
) -> crate::Result<Option<ReviewComplianceBlock>> {
    if task.state != "blocked"
        || task.task_runtime.last_error_code.as_deref() != Some("E903")
        || task.task_runtime.last_error_origin.as_deref() != Some("decisions")
        || task.task_runtime.current_phase.as_deref() != Some("review")
        || !policy(root)?.enabled
    {
        return Ok(None);
    }
    let Some(pin) = task.task_runtime.extra.get("decision_context") else {
        return Ok(None);
    };
    let (store, plan) = active_plan(root)?;
    if pin["plan_id"] != plan.id {
        return Ok(None);
    }
    store.review_compliance_block(&task.id, pin)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::coordinator::model::Task;
    use serde_json::json;
    #[test]
    fn native_objection_retains_exact_version_evidence_and_cannot_be_forged_or_reused() {
        let (tmp, mut store, plan, _, mut p) = super::super::tests::fixture();
        p.human_question_min_risk = Consultation::NeverAsk;
        let paths = crate::ProjectPaths::from_root(tmp.path());
        let mut config = crate::load_canonical_config(&paths.config_path).unwrap();
        config.automation.coordinator.as_mut().unwrap().decisions = Some(p.clone());
        crate::save_canonical_config(&paths, &config).unwrap();
        let request=serde_json::from_value(json!({"idempotency_key":"source-choice","task_id":"A","kind":"decision","question":"Which compliant implementation?","context":"Existing normative constraints","investigated":["Read project rules"],"recommendation":"Preserve the required invariant","risk":"high","impact":{"evidence":["Normative contract"]}})).unwrap();
        let d = store
            .submit(&plan, request, "agent:A", "agent", &p)
            .unwrap();
        assert_eq!(
            d.resolution.as_ref().unwrap().origin,
            "delegated_autonomous"
        );
        let plan = store.plan(&plan.id).unwrap();
        let (context, _) = store
            .issue(&plan, "A", "review-1", "review", tmp.path(), &p)
            .unwrap();
        store
            .issued(&plan.id, "review-1", "review", &context.render(), &p)
            .unwrap();
        let evidence =
            "src/issuer.rs:26: the selected option contradicts the mandatory concurrency invariant";
        let output=format!("MACC_DECISION_ACK: {}\nMACC_DECISION_COMPLIANCE: {}\nREVIEW_VERDICT: CHANGES_REQUESTED",context.context_digest,json!({d.id.clone():{"status":"needs_human_review","evidence":evidence}}));
        let error = store
            .complete(&plan.id, "review-1", "review", &output, &p)
            .unwrap_err()
            .to_string();
        assert!(error.contains(evidence) && error.contains(&format!("{} v1", d.id)));
        let pin = json!({"plan_id":plan.id,"attempt_id":"review-1","phase":"review","digest":context.context_digest});
        let mut task:Task=serde_json::from_value(json!({"id":"A","state":"blocked","task_runtime":{"current_phase":"review","last_error_code":"E903","last_error_origin":"decisions","decision_context":pin}})).unwrap();
        let block = review_compliance_for_task(tmp.path(), &task)
            .unwrap()
            .unwrap();
        assert!(block.needs_human_review());
        assert_eq!(block.findings[0].decision_id, d.id);
        assert_eq!(block.findings[0].version, 1);
        assert_eq!(block.findings[0].evidence, evidence);
        assert_eq!(
            block.findings[0].resolution_origin.as_deref(),
            Some("delegated_autonomous")
        );
        assert!(
            pending_for_task(tmp.path(), "A").unwrap().is_none(),
            "a resolved decision can still have an independent compliance objection"
        );
        assert!(block.next_action().contains("governed revision"));
        let source = serde_json::to_value(store.context_history(&plan.id, "A").unwrap()).unwrap();
        task.task_runtime.extra.get_mut("decision_context").unwrap()["digest"] = json!("forged");
        assert!(review_compliance_for_task(tmp.path(), &task)
            .unwrap()
            .is_none());
        assert_eq!(
            serde_json::to_value(store.context_history(&plan.id, "A").unwrap()).unwrap(),
            source,
            "inspection never resolves or accepts the objection"
        );
        task.task_runtime
            .extra
            .insert("decision_context".into(), pin);
        let output = format!(
            "MACC_DECISION_ACK: {}\nMACC_DECISION_COMPLIANCE: {}\nREVIEW_VERDICT: OK",
            context.context_digest,
            json!({d.id.clone():{"status":"pass","evidence":"Independent test verified the mandatory invariant"}})
        );
        store
            .complete(&plan.id, "review-1", "review", &output, &p)
            .unwrap();
        assert!(
            review_compliance_for_task(tmp.path(), &task)
                .unwrap()
                .is_none(),
            "an old projected block cannot overrule a fresh native receipt"
        );
    }
}
