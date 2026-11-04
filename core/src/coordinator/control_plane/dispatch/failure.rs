//! Finalize an owned claim only when no performer has been started.
use super::*;
use crate::coordinator::{model::TaskRegistry, RuntimeStatus, WorkflowState};

pub(super) fn record(
    root: &Path,
    claim: &DispatchClaim,
    cause: MaccError,
    logger: Option<&dyn CoordinatorLog>,
) -> Result<MaccError> {
    let detail = format!(
        "task={} phase=dev claim={} worktree={}: {}",
        claim.task_id,
        claim.claim_id,
        claim.worktree_path.display(),
        cause
    );
    let value = crate::coordinator::state::coordinator_state_registry_load(root, &BTreeMap::new())?;
    let mut registry = TaskRegistry::from_value(&value)?;
    let task = registry.find_task_mut(&claim.task_id).ok_or_else(|| {
        MaccError::Validation(format!(
            "Launch preparation failed ({detail}); task disappeared before claim cleanup"
        ))
    })?;
    if task.task_runtime.claim_id.as_deref() != Some(&claim.claim_id)
        || task.workflow_state() != Some(WorkflowState::Claimed)
        || task.task_runtime.pid.is_some()
    {
        return Err(MaccError::Validation(format!("Launch preparation failed ({detail}); ownership changed, so the newer task state was preserved")));
    }
    task.set_workflow_state(WorkflowState::Blocked);
    task.touch_state_changed(&now_iso_coordinator());
    let runtime = task.ensure_runtime();
    runtime.set_status(RuntimeStatus::Failed);
    runtime.clear_last_error_details();
    runtime.last_error = Some(detail.clone());
    runtime.set_last_error_details("E901", "dispatch_preparation", detail.clone());
    // Retain the branch, full worktree and provider-session identity for recovery.
    runtime.pid = None;
    runtime.completion_kind = None;
    runtime.extra.insert(
        "dispatch_preparation_failure".into(),
        serde_json::json!({"claim_id":claim.claim_id,"phase":"dev","cause":cause.to_string()}),
    );
    let mut value = registry.to_value()?;
    recompute_resource_locks_from_tasks(&mut value);
    set_registry_updated_at(&mut value);
    crate::coordinator::state::coordinator_state_registry_save(root, &BTreeMap::new(), &value)
        .map_err(|error| {
            MaccError::Validation(format!(
                "Launch preparation failed ({detail}); could not persist claim cleanup: {error}"
            ))
        })?;
    append_coordinator_event_with_severity(
        root,
        "dispatch_preparation_failed",
        &claim.task_id,
        "dev",
        "failed",
        &detail,
        "error",
    )?;
    tracing::error!(task_id=%claim.task_id,claim_id=%claim.claim_id,code="E901",cause=%cause,"Performer launch preparation failed; retained task is blocked");
    if let Some(log) = logger {
        log.note(format!("- Dispatch preparation failed: {detail}"))?;
    }
    Ok(MaccError::Coordinator {
        code: "E901",
        message: detail,
    })
}
