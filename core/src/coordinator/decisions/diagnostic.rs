//! Verified diagnostic delivery is evidence, never feature acceptance or consent.
use super::{store::*, *};
use serde::{Deserialize, Serialize};
use std::path::{Component, Path};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct DiagnosticDelivery {
    pub report_path: String,
    pub report_digest: String,
    pub feature_accepted: bool,
    pub release_authorized: bool,
    pub completed: bool,
    #[serde(default)]
    pub unverified_decisions: Vec<String>,
}
fn contract(task: &serde_json::Value) -> bool {
    task["category"]=="verification" && (task.pointer("/execution_contract/diagnostic_completion")==Some(&serde_json::Value::Bool(true)) || task.pointer("/execution_contract/completion_vs_acceptance").and_then(|v|v.as_str()).is_some_and(|s|s.starts_with("Implementation and honest diagnostic review completion are distinct from feature acceptance.")))
}
impl Store {
    /// Native post-check after complete() has already persisted a failed receipt.
    /// Only explicit report evidence for intervention failures can gain this label.
    pub fn diagnostic_delivery(
        &mut self,
        context: &ContextPackage,
        task: &serde_json::Value,
        worktree: &Path,
        expected_revision: &str,
        output: &str,
    ) -> crate::Result<Option<DiagnosticDelivery>> {
        if context.phase != "review" || !contract(task) {
            return Ok(None);
        }
        if crate::coordinator::runtime::parse_review_verdict(output)
            != Some(crate::coordinator::engine::ReviewVerdict::Ok)
        {
            return Ok(None);
        }
        if crate::git::head_commit(worktree)? != expected_revision {
            return Err(error("Diagnostic review changed the source commit"));
        }
        let status = crate::git::run_git_output_mapped(
            worktree,
            &["status", "--porcelain"],
            "verify diagnostic review postchecks",
        )?;
        if !status.status.success() || !status.stdout.is_empty() {
            return Err(error("Diagnostic review changed worktree files"));
        }

        let raw = crate::coordinator::result_protocol::projection(output);
        let Some(marker) = raw
            .lines()
            .filter_map(|l| l.strip_prefix("MACC_DIAGNOSTIC_RESULT:"))
            .last()
        else {
            return Ok(None);
        };
        let mut delivery: DiagnosticDelivery = serde_json::from_str(marker.trim())
            .map_err(|e| error(format!("Invalid diagnostic result: {e}")))?;
        if !delivery.completed
            || delivery.feature_accepted
            || delivery.release_authorized
            || !delivery.unverified_decisions.is_empty()
        {
            return Err(error("Diagnostic delivery cannot claim feature acceptance, release or its own verification outcome"));
        }
        let relative = Path::new(&delivery.report_path);
        if relative.is_absolute()
            || relative
                .components()
                .any(|c| !matches!(c, Component::Normal(_)))
        {
            return Err(error("Diagnostic report must be inside the task worktree"));
        }
        let allowed = task
            .pointer("/change_scope/allowed_paths")
            .and_then(|v| v.as_array())
            .is_some_and(|paths| {
                paths.iter().filter_map(|v| v.as_str()).any(|p| {
                    crate::coordinator::precondition_scope::path_matches(p, &delivery.report_path)
                })
            });
        if !allowed {
            return Err(error(
                "Diagnostic report is outside the declared task scope",
            ));
        }
        let path = worktree.join(relative);
        let root = std::fs::canonicalize(worktree).map_err(|e| error(e.to_string()))?;
        if !std::fs::canonicalize(&path)
            .map_err(|e| error(e.to_string()))?
            .starts_with(&root)
        {
            return Err(error("Diagnostic report escapes the task worktree"));
        }
        let metadata = std::fs::metadata(&path).map_err(|e| error(e.to_string()))?;
        if metadata.len() > 1048576 {
            return Err(error("Diagnostic report exceeds 1 MiB"));
        }
        let bytes = std::fs::read(&path).map_err(|e| error(e.to_string()))?;
        use sha2::{Digest, Sha256};
        if delivery.report_digest != format!("sha256:{:x}", Sha256::digest(&bytes)) {
            return Err(error(
                "Diagnostic report digest does not match its exact bytes",
            ));
        }
        let report: serde_json::Value =
            serde_json::from_slice(&bytes).map_err(|e| error(e.to_string()))?;
        if report["task"] != context.task_id
            || report["verdict"] != "changes_requested"
            || report["featureAccepted"] != false
            || report["releaseAuthorized"] != false
        {
            return Err(error("Diagnostic artifact must explicitly withhold feature/release acceptance and name the current task"));
        }
        let tracked = crate::git::run_git_output_mapped(
            worktree,
            &["ls-files", "--error-unmatch", "--", &delivery.report_path],
            "inspect diagnostic artifact",
        )?;
        let clean = crate::git::run_git_output_mapped(
            worktree,
            &["status", "--porcelain", "--", &delivery.report_path],
            "inspect diagnostic artifact status",
        )?;
        if !tracked.status.success() || !clean.status.success() || !clean.stdout.is_empty() {
            return Err(error("Diagnostic artifact must be committed and unchanged"));
        }
        let _fence = self.mutation_lock()?;
        let fresh = self.context(
            &self.plan(&context.plan_id)?,
            &context.task_id,
            &context.attempt_id,
            "review",
            &policy(&self.root)?,
        )?;
        if fresh.context_digest != context.context_digest {
            return Err(error("DM_STALE: diagnostic review context changed"));
        }
        let receipt: ContextReceipt = decode(&sql(self.db.query_row(
            "SELECT receipt FROM contexts WHERE plan_id=?1 AND attempt_id=?2 AND phase=?3",
            rusqlite::params![context.plan_id, context.attempt_id, context.phase],
            |r| r.get::<_, String>(0),
        ))?)?;
        if !receipt.acknowledged || receipt.completion_status != "compliance_failed" {
            return Ok(None);
        }
        for binding in &context.bindings {
            if binding.resolution.as_ref().is_some_and(|r| r.advisory) {
                continue;
            }
            let Some(c) = receipt.compliance.get(&binding.id) else {
                return Ok(None);
            };
            if c.status != "pass" {
                if binding.request.kind != "intervention" {
                    return Ok(None);
                }
                delivery.unverified_decisions.push(binding.id.clone());
            }
        }
        if delivery.unverified_decisions.is_empty() {
            return Ok(None);
        }
        let mut receipt = receipt;
        receipt.diagnostic_delivery = Some(delivery.clone());
        receipt.reviewed_commit = Some(expected_revision.into());
        receipt.completion_status = "diagnostic_complete_external_pending".into();
        // Failure remains explicit. Merge still requires status=accepted.
        let tx = sql(self
            .db
            .transaction_with_behavior(rusqlite::TransactionBehavior::Immediate))?;
        sql(tx.execute(
            "UPDATE contexts SET receipt=?4 WHERE plan_id=?1 AND attempt_id=?2 AND phase=?3",
            rusqlite::params![
                context.plan_id,
                context.attempt_id,
                context.phase,
                encode(&receipt)?
            ],
        ))?;
        event(
            &tx,
            &context.plan_id,
            "task.diagnostic.delivered",
            &delivery,
        )?;
        sql(tx.commit())?;
        Ok(Some(delivery))
    }
    pub fn pending_diagnostic(
        &self,
        plan: &str,
        attempt: &str,
        worktree: &Path,
    ) -> crate::Result<Option<DiagnosticDelivery>> {
        let context = self.read_context(plan, attempt, "review")?;
        let receipt: ContextReceipt = decode(&sql(self.db.query_row(
            "SELECT receipt FROM contexts WHERE plan_id=?1 AND attempt_id=?2 AND phase='review'",
            rusqlite::params![plan, attempt],
            |r| r.get::<_, String>(0),
        ))?)?;
        if receipt.completion_status != "diagnostic_complete_external_pending"
            || !receipt.acknowledged
            || receipt.context_digest != context.context_digest
        {
            return Ok(None);
        }
        let fresh = self.context(
            &self.plan(plan)?,
            &context.task_id,
            attempt,
            "review",
            &policy(&self.root)?,
        )?;
        if fresh.context_digest != context.context_digest
            || receipt.reviewed_commit.as_deref()
                != Some(crate::git::head_commit(worktree)?.as_str())
        {
            return Err(error(
                "DM_STALE: diagnostic receipt requires a fresh review",
            ));
        }
        let status = crate::git::run_git_output_mapped(
            worktree,
            &["status", "--porcelain"],
            "verify retained diagnostic artifact",
        )?;
        if !status.status.success() || !status.stdout.is_empty() {
            return Err(error("DM_STALE: diagnostic artifact worktree changed"));
        }
        if let Some(delivery) = &receipt.diagnostic_delivery {
            let path = worktree.join(&delivery.report_path);
            if !std::fs::canonicalize(&path)
                .map_err(|e| error(e.to_string()))?
                .starts_with(std::fs::canonicalize(worktree).map_err(|e| error(e.to_string()))?)
                || std::fs::metadata(&path)
                    .map_err(|e| error(e.to_string()))?
                    .len()
                    > 1048576
            {
                return Err(error(
                    "Diagnostic artifact is no longer bounded inside its worktree",
                ));
            }
            use sha2::{Digest, Sha256};
            if delivery.report_digest
                != format!(
                    "sha256:{:x}",
                    Sha256::digest(std::fs::read(&path).map_err(|e| error(e.to_string()))?)
                )
            {
                return Err(error("DM_STALE: diagnostic report bytes changed"));
            }
        }
        Ok(receipt.diagnostic_delivery)
    }
}
