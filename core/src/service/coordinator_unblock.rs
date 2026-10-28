//! The same validated unblock/repair operation for offline and owner-loop execution.
use super::*;
use crate::coordinator::control::Request;
pub(super) fn execute(
    paths: &ProjectPaths,
    coordinator_cfg: Option<&CoordinatorConfig>,
    env_cfg: &CoordinatorEnvConfig,
    task_id: &str,
    evidence: &str,
) -> Result<()> {
    let mut args = BTreeMap::new();
    apply_storage_mode_args(&mut args, env_cfg, coordinator_cfg);
    let value = crate::coordinator::state::coordinator_state_registry_load(&paths.root, &args)?;
    let registry = crate::coordinator::model::TaskRegistry::from_value(&value)?;
    let task = registry.find_task(task_id).ok_or_else(|| {
        MaccError::Validation(format!("Cannot unblock unknown task '{task_id}'."))
    })?;
    let request = Request::new(task, evidence)?;
    let pid_path = paths.root.join(".macc/state/coordinator.pid");
    let live = std::fs::read_to_string(pid_path)
        .ok()
        .and_then(|s| s.trim().parse::<i64>().ok())
        .is_some_and(crate::coordinator::helpers::is_pid_running);
    if live {
        crate::coordinator::control::send_sync(&paths.root, &request)
    } else {
        let _guard = crate::fs_lock::AdvisoryLock::acquire(
            &paths.macc_dir.join("state/unblock.lock"),
            Duration::ZERO,
            "offline task unblock",
        )?;
        apply(paths, coordinator_cfg, env_cfg, &request)
    }
}
pub(crate) fn apply(
    paths: &ProjectPaths,
    coordinator_cfg: Option<&CoordinatorConfig>,
    env_cfg: &CoordinatorEnvConfig,
    request: &Request,
) -> Result<()> {
    request.validate()?;
    let task_id = request.task_id.as_str();
    let evidence = request.evidence.as_str();
    let mut state_args = BTreeMap::new();
    apply_storage_mode_args(&mut state_args, env_cfg, coordinator_cfg);
    let value =
        crate::coordinator::state::coordinator_state_registry_load(&paths.root, &state_args)?;
    let mut registry = crate::coordinator::model::TaskRegistry::from_value(&value)?;
    let task = registry.find_task_mut(task_id).ok_or_else(|| {
        MaccError::Validation(format!("Cannot unblock unknown task '{task_id}'."))
    })?;
    if let Some(receipt) = task.task_runtime.extra.get("unblock_control") {
        if receipt["request_id"] == request.request_id {
            if receipt["request_hash"] == request.digest()? {
                return Ok(());
            }
            return Err(MaccError::Validation(
                "Unblock request ID was reused with different content".into(),
            ));
        }
    }
    if crate::coordinator::control::task_revision(task)? != request.expected_revision {
        return Err(MaccError::Validation(
            "Unblock task revision changed; reload and review the blocked task".into(),
        ));
    }
    if task.workflow_state() != Some(WorkflowState::Blocked) {
        return Err(MaccError::Validation(format!(
            "Task '{task_id}' is not blocked."
        )));
    }
    if task.task_runtime.last_error_code.as_deref() == Some("E905") {
        return Err(MaccError::Validation(format!(
            "Task '{task_id}' is blocked by a dependency. Resolve or unblock the root task shown in its error instead."
        )));
    }
    // A successful unblock must not immediately redispatch a stale checkpoint.
    crate::session_store::checkpoints::recovery::ensure_task_ready(&paths.root, task_id)?;
    // Reconcile before changing task state. A conflict preserves the original
    // worktree and leaves the task blocked, with durable content/index refs.
    let repaired_base = if task.has_worktree_attached() {
        let reference =
            crate::config::CoordinatorConfigResolved::resolve(coordinator_cfg).reference_branch;
        Some(crate::coordinator::repair_handoff::reconcile(
            &paths.root,
            task,
            &reference,
        )?)
    } else {
        None
    };
    // The repaired base may also have changed a referenced specification.
    crate::session_store::checkpoints::recovery::ensure_task_ready(&paths.root, task_id)?;
    if let Some(block) = task.blocked_on_external.as_ref() {
        task.task_runtime.external_block_resolution =
            Some(crate::coordinator::model::ExternalBlockResolution {
                evidence: evidence.to_string(),
                resolved_at: crate::coordinator::helpers::now_iso_coordinator(),
                block_reason: block.reason.clone(),
                tracking_id: block.tracking_id.clone(),
            });
    }
    task.set_workflow_state(WorkflowState::Todo);
    let runtime = task.ensure_runtime();
    runtime.set_status(RuntimeStatus::Idle);
    runtime.pid = None;
    if let Some(base) = repaired_base {
        runtime.extra.insert(
            crate::coordinator::repair_handoff::RESUME_KEY.into(),
            serde_json::json!({"base":base,"evidence":evidence}),
        );
    }
    runtime.last_error = None;
    runtime.clear_last_error_details();
    runtime
        .extra
        .remove(crate::coordinator::precondition_scope::OUT_OF_SCOPE_FIX_KEY);
    runtime.extra.insert(
        "unblock_control".into(),
        serde_json::json!({"request_id":request.request_id,"request_hash":request.digest()?}),
    );
    let changed = crate::coordinator::task_selector::reconcile_task_blocks(
        &mut registry,
        &crate::coordinator::helpers::now_iso_coordinator(),
    );
    crate::coordinator::state::coordinator_state_registry_save(
        &paths.root,
        &state_args,
        &registry.to_value()?,
    )?;
    let detail = format!(
        "Task {task_id} unblocked with evidence: {evidence}. Reconciled: {}",
        if changed.is_empty() {
            "none".to_string()
        } else {
            changed.join(", ")
        }
    );
    crate::coordinator::helpers::append_coordinator_event_with_severity(
        &paths.root,
        "task_unblocked",
        task_id,
        "dispatch",
        "todo",
        &detail,
        "info",
    )?;
    tracing::info!(task_id,request_id=%request.request_id,"Unblock command applied by registry owner");
    Ok(())
}
