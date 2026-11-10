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
        if self.status == "blocked_external" {
            return format!("DM_PENDING: blocked: external intervention required; decision {}: {}. Coordinator must hand off repairable prerequisites to the supervisor; retained work is preserved", self.id, self.question);
        }
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

/// Only a real human decision keeps an idle engine waiting. External recovery
/// belongs to a stopped engine and its supervisor, not an endless idle loop.
pub fn human_wait_count(registry: &crate::coordinator::model::TaskRegistry) -> usize {
    if registry.tasks.iter().any(|t| {
        t.state == "blocked"
            && t.task_runtime.extra.contains_key("decision_wait")
            && t.task_runtime
                .extra
                .get("decision_wait_status")
                .and_then(serde_json::Value::as_str)
                != Some("pending_human")
    }) {
        return 0;
    }
    registry.waiting_approval_count()
        + registry
            .tasks
            .iter()
            .filter(|t| {
                t.task_runtime
                    .extra
                    .get("decision_wait_status")
                    .and_then(serde_json::Value::as_str)
                    == Some("pending_human")
            })
            .count()
}

#[cfg(test)]
mod handoff_tests {
    use super::*;
    #[test]
    fn external_prerequisite_cannot_masquerade_as_a_human_approval_wait() {
        use crate::coordinator::{
            engine::{ControlPlaneLoopConfig, CoordinatorCounts, CoordinatorRunController},
            model::TaskRegistry,
        };
        let registry = |status: &str| {
            TaskRegistry::from_value(&serde_json::json!({"tasks":[{"id":"A","state":"blocked","task_runtime":{"decision_wait":"request","decision_wait_status":status}}]})).unwrap()
        };
        assert_eq!(human_wait_count(&registry("pending_human")), 1);
        let blocked = registry("blocked_external");
        assert_eq!(human_wait_count(&blocked), 0);
        let mut controller = CoordinatorRunController::new(ControlPlaneLoopConfig {
            timeout: None,
            max_no_progress_cycles: 5,
        });
        let result = controller.on_cycle_counts(
            CoordinatorCounts {
                total: 1,
                todo: 0,
                active: 0,
                blocked: 1,
                merged: 0,
                waiting_approval: human_wait_count(&blocked),
            },
            None,
        );
        assert!(result.unwrap_err().to_string().contains("blocked"));
    }
}
