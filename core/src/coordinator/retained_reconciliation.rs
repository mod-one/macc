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

fn has_interrupted_invocation(task: &Task) -> bool {
    let rt = &task.task_runtime;
    rt.current_phase.as_deref() != Some("merge")
        && rt.extra.contains_key("decision_wait")
        && (rt.last_error_origin.as_deref() == Some("decisions")
            || (rt.last_error_origin.as_deref() == Some("decision_context_refresh")
                && rt.extra.contains_key("checkpoint_previous_decision"))
            || (rt.last_error_origin.as_deref() == Some("precondition")
                && rt.extra.contains_key("suspended_phase")
                && super::phase_diagnostics::decision_wait_id(task).is_some()))
}

/// Native invocation pins identify the interrupted phase when current_phase still names its predecessor.
fn interrupted_phase(task: &Task) -> Option<&str> {
    let rt = &task.task_runtime;
    has_interrupted_invocation(task).then_some(())?;
    rt.extra
        .get("suspended_phase")
        .or_else(|| rt.extra.get("decision_context"))
        .and_then(|v| v["phase"].as_str())
        .filter(|p| matches!(*p, "dev" | "test" | "fix" | "review"))
}

pub(crate) fn retained_phase(task: &Task) -> &str {
    interrupted_phase(task)
        .or(task.task_runtime.current_phase.as_deref())
        .unwrap_or("dev")
}

/// A repaired validation/publication failure must not restart implementation.
pub fn resume_state(root: &std::path::Path, task: &mut Task) -> WorkflowState {
    if has_interrupted_invocation(task) {
        if let Some(phase) = interrupted_phase(task) {
            task.task_runtime.current_phase = Some(phase.to_owned());
        }
        if let Some(receipt) = task.task_runtime.extra.remove("suspended_phase") {
            task.task_runtime
                .extra
                .insert("previous_suspended_phase".into(), receipt);
        }
    }
    match task.task_runtime.current_phase.as_deref() {
        Some("fix") => {
            task.task_runtime.extra.insert(
                "suspended_fix_review_required".into(),
                serde_json::json!(true),
            );
            WorkflowState::ChangesRequested
        }
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
