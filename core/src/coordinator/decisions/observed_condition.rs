//! Machine observation releases only the prerequisite of an existing retained phase.
use super::{store::*, *};
use serde_json::json;

impl Store {
    pub(crate) fn check_observable_intervention(
        &self,
        d: &Decision,
        criterion: &str,
    ) -> crate::Result<()> {
        let r = &d.request;
        let p = policy(&self.root)?;
        let plan = self.plan(&d.plan_id)?;
        let node = super::graph::task(&plan.graph, &r.task_id)?;
        if !p.enabled
            || r.kind != "intervention"
            || r.scope != "task"
            || !matches!(d.status.as_str(), "blocked_external" | "pending_human")
            || d.requires_human_renewal
            || d.resolution.is_some()
            || r.has_effects()
            || r.operation_spec.is_some()
            || r.operation_id.is_some()
            || !r.supersedes.is_empty()
            || r.affected_tasks.iter().any(|id| id != &r.task_id)
            || r.impact.production
            || r.impact.irreversible
            || r.impact.security_privacy
            || r.impact.shared_contract
            || r.impact.product_scope
            || r.response_fields
                .iter()
                .any(|f| f.required && !matches!(f.id.as_str(), "performed" | "result"))
            || node.get("gate").is_some()
            || d.subject_digest != digest(&super::graph::contract(node))
            || r.intervention
                .as_ref()
                .is_none_or(|i| i.expected_result != criterion || !i.source_repair_paths.is_empty())
        {
            return Err(error(
                "The intervention changed or requires separate human authority",
            ));
        }
        let mut authority = r.clone();
        authority.kind = "question".into();
        let t = self.triage(&plan, &mut authority, &p)?;
        if t.mandatory
            || !t.report.missing_sources.is_empty()
            || authority.scope != "task"
            || authority
                .response_fields
                .iter()
                .any(|f| f.required && !matches!(f.id.as_str(), "performed" | "result"))
        {
            return Err(error(
                "Governance requires independent authority or evidence",
            ));
        }
        Ok(())
    }

    pub(crate) fn record_observed_condition(
        &mut self,
        reference: &DecisionRef,
        task: &str,
        criterion: &str,
        check: &str,
        detail: &str,
        commit: &str,
    ) -> crate::Result<()> {
        let _lock = self.mutation_lock()?;
        let mut d = self.decision(&reference.id)?;
        if d.version != reference.version || d.request.task_id != task {
            return Err(error("Observed decision version or owner changed"));
        }
        self.check_observable_intervention(&d, criterion)?;
        let mut plan = self.plan(&d.plan_id)?;
        let p = policy(&self.root)?;
        let tx = sql(self
            .db
            .transaction_with_behavior(rusqlite::TransactionBehavior::Immediate))?;
        d.status = "resolved".into();
        d.version += 1;
        d.resolution = Some(Resolution {
            text: format!("Prerequisite observed for retained phase: {detail}"),
            selected_option_id: None,
            actor: "coordinator".into(),
            origin: "resume_verifier".into(),
            outcome: "condition_observed".into(),
            source_revision: Some(commit.into()),
            answered_version: Some(reference.version),
            policy_digest: digest(&p),
            recorded_at: now(),
            fields: Default::default(),
            conditions: vec![],
            artifacts: vec![],
            attestation: None,
            effects: "none".into(),
            patch_applied: false,
            applied_patch: None,
            advisory: false,
        });
        sql(tx.execute(
            "UPDATE requests SET payload=?2 WHERE id=?1",
            rusqlite::params![d.id, encode(&d)?],
        ))?;
        plan.decision_epoch += 1;
        save_plan(&tx, &plan)?;
        event(
            &tx,
            &plan.id,
            "decision.condition_observed",
            &json!({"check":check,"decision":d}),
        )?;
        sql(tx.commit())
    }
}
