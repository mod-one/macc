//! Task-local reconciliation failures must not abort the coordinator refresh loop.
use super::{model::Task, RuntimeStatus, WorkflowState};

pub(crate) fn reconcile(
    root: &std::path::Path,
    task: &mut Task,
    reference: &str,
) -> Option<String> {
    match super::repair_handoff::reconcile_task(root, task, reference) {
        Ok(base) => {
            task.task_runtime
                .extra
                .remove("retained_reconciliation_error");
            Some(base)
        }
        Err(error) => {
            let actual = task
                .worktree_path()
                .and_then(|p| crate::git::current_branch(std::path::Path::new(p)).ok())
                .unwrap_or_else(|| "unavailable".into());
            let journal = super::worktree_recovery::journal_path(root, &task.id);
            let message = format!("{error}; task={} phase={} path={} claim={} expected_branch={} actual_branch={}; original work retained{}",task.id,task.task_runtime.current_phase.as_deref().unwrap_or("run"),task.worktree_path().unwrap_or("-"),task.task_runtime.claim_id.as_deref().unwrap_or("-"),task.branch().unwrap_or("-"),actual,if journal.exists(){format!("; recovery_journal={}",journal.display())}else{String::new()});
            let changed = task
                .task_runtime
                .extra
                .get("retained_reconciliation_error")
                .and_then(serde_json::Value::as_str)
                != Some(message.as_str());
            task.set_workflow_state(WorkflowState::Blocked);
            task.task_runtime.set_status(RuntimeStatus::Idle);
            task.task_runtime
                .set_last_error_details("E901", "worktree_ownership", &message);
            task.task_runtime.extra.insert(
                "retained_reconciliation_error".into(),
                serde_json::json!(message),
            );
            if changed {
                tracing::warn!(task_id=%task.id,phase=?task.task_runtime.current_phase,error=%error,"Retained task reconciliation blocked; unrelated tasks remain eligible");
                let _ = super::helpers::append_coordinator_event_with_severity(
                    root,
                    "task_blocked",
                    &task.id,
                    task.task_runtime.current_phase.as_deref().unwrap_or("run"),
                    super::worktree_roles::OWNERSHIP_CODE,
                    &message,
                    "blocking",
                );
            }
            None
        }
    }
}

/// Governance freshness is independent of the optional review-cycle budget.
pub(crate) fn advance_override(task: &Task) -> Option<super::engine::AdvancePlan> {
    use super::engine::{AdvancePlan, PhaseTransition};
    let (mode, next_state) = if task.workflow_state() == Some(WorkflowState::Testing)
        && task
            .task_runtime
            .extra
            .contains_key("review_refresh_required")
    {
        ("review", WorkflowState::Reviewing)
    } else if task.workflow_state() == Some(WorkflowState::ChangesRequested)
        && task
            .task_runtime
            .extra
            .contains_key("review_refresh_correction")
    {
        ("fix", WorkflowState::InProgress)
    } else {
        return None;
    };
    Some(AdvancePlan::RunPhase(PhaseTransition {
        mode,
        next_state,
        runtime_phase: mode,
    }))
}

pub(crate) fn review_completed(
    runtime: &mut super::model::TaskRuntime,
    changes_requested: bool,
) -> bool {
    let refresh = runtime.extra.remove("review_refresh_required").is_some();
    if refresh && changes_requested {
        runtime
            .extra
            .insert("review_refresh_correction".into(), serde_json::json!(true));
    }
    refresh
}

/// A repaired validation/publication failure must not restart implementation.
pub fn resume_state(root: &std::path::Path, task: &mut Task) -> WorkflowState {
    match task.task_runtime.current_phase.as_deref() {
        Some("fix") => WorkflowState::ChangesRequested,
        Some("test") => WorkflowState::InProgress,
        Some("review") => {
            task.task_runtime.extra.insert(
                "review_refresh_required".into(),
                serde_json::json!("Retained review requires fresh independent validation"),
            );
            WorkflowState::Testing
        }
        Some("merge") => match super::decisions::merge_ready(root, &task.id) {
            Ok(()) => WorkflowState::PrOpen,
            Err(error) => {
                task.task_runtime.extra.insert(
                    "review_refresh_required".into(),
                    serde_json::json!(error.to_string()),
                );
                WorkflowState::Testing
            }
        },
        _ => WorkflowState::Todo,
    }
}

#[cfg(test)]
#[path = "retained_reconciliation_tests.rs"]
mod tests;
