//! Native unresolved prerequisites; a wait is not a provider failure.
use super::*;
use serde::{Deserialize, Serialize};
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct PendingDecision {
    pub id: String,
    pub version: u64,
    pub status: String,
    pub task_id: String,
    pub question: String,
}
pub fn pending_for_task(
    root: &std::path::Path,
    task: &str,
) -> crate::Result<Option<PendingDecision>> {
    if !policy(root)?.enabled {
        return Ok(None);
    }
    let (store, plan) = active_plan(root)?;
    for d in store.applicable_decisions(&plan)? {
        if matches!(
            d.status.as_str(),
            "pending_human"
                | "blocked_external"
                | "conditional"
                | "expired"
                | "rejected"
                | "conflict"
                | "waiting_dependency"
                | "investigating"
        ) && store.decision_applies(&plan, &d, task)?
        {
            return Ok(Some(PendingDecision {
                id: d.id,
                version: d.version,
                status: d.status,
                task_id: task.into(),
                question: d.request.question,
            }));
        }
    }
    Ok(None)
}
impl PendingDecision {
    pub fn reason(&self) -> String {
        format!(
            "DM_PENDING: Decision {} ({}) awaits operator resolution: {}",
            self.id, self.status, self.question
        )
    }
    pub fn apply(&self, runtime: &mut crate::coordinator::model::TaskRuntime) {
        runtime
            .extra
            .insert("decision_wait".into(), serde_json::json!(self.id));
        runtime.extra.insert(
            "decision_wait_status".into(),
            serde_json::json!(self.status),
        );
        runtime.set_last_error_details("E903", "decisions", self.reason());
        runtime.last_error = Some(self.reason());
        runtime.completion_kind = Some("precondition_unmet".into());
        runtime.result_explanation = Some(self.reason());
        runtime.unmet_preconditions = vec![self.reason()];
        runtime.set_status(crate::coordinator::RuntimeStatus::Idle);
    }
}

/// Catch requests created after the cycle's initial reconciliation.
pub fn refresh_waits(
    root: &std::path::Path,
    registry: &mut crate::coordinator::model::TaskRegistry,
) -> crate::Result<bool> {
    let mut changed = false;
    for task in &mut registry.tasks {
        if task.state == "blocked" && !task.task_runtime.extra.contains_key("decision_wait") {
            if let Some(pending) = pending_for_task(root, &task.id)? {
                pending.apply(&mut task.task_runtime);
                changed = true;
            } else if task
                .task_runtime
                .last_error_message
                .as_deref()
                .is_some_and(|e| e.contains("DM_STALE:"))
            {
                task.task_runtime
                    .extra
                    .insert("decision_wait".into(), serde_json::json!("context_refresh"));
                changed = true;
            }
        }
    }
    Ok(changed)
}

/// Only applicable native revisions are relevant to a cached human-gate diagnosis.
pub fn gate_revision(
    root: &std::path::Path,
    tasks: &[String],
) -> crate::Result<Vec<serde_json::Value>> {
    if !policy(root)?.enabled {
        return Ok(vec![]);
    }
    let (store, plan) = active_plan(root)?;
    let mut result = Vec::new();
    for d in store.applicable_decisions(&plan)? {
        let mut applies = false;
        for task in tasks {
            applies |= store.decision_applies(&plan, &d, task)?;
        }
        if applies {
            result.push(serde_json::json!({"id":d.id,"version":d.version,"status":d.status}));
        }
    }
    result.sort_by_key(|v| v["id"].as_str().unwrap_or_default().to_owned());
    Ok(result)
}
