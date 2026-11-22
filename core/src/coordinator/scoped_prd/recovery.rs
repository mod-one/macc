//! Recover retained claims and recognize durable recovery receipts.
use super::*;
/// Bridge old E901 records only at the pre-spawn boundary, with exact claim,
/// phase and path. Similar text in model output never authorizes recovery.
pub fn is_dispatch_failure(task: &Task) -> bool {
    let rt = &task.task_runtime;
    if task.state != "blocked"
        || rt.last_error_code.as_deref() != Some("E901")
        || rt.pid.is_some()
        || rt.current_phase.as_deref() != Some("dev")
    {
        return false;
    }
    if rt.last_error_origin.as_deref() == Some("worker_preflight") {
        return rt
            .extra
            .get(INCIDENT_KEY)
            .and_then(|v| serde_json::from_value::<Incident>(v.clone()).ok())
            .is_some_and(|i| {
                i.before_launch
                    && i.incoming_task == task.id
                    && task
                        .worktree_path()
                        .is_none_or(|p| Path::new(p).join(NAME) == i.path)
            });
    }
    if rt.last_error_origin.as_deref() != Some("dispatch_preparation") {
        return false;
    }
    let Some(record) = rt.extra.get("dispatch_preparation_failure") else {
        return false;
    };
    if record["phase"] != "dev"
        || record["claim_id"].as_str() != rt.claim_id.as_deref()
        || rt.claim_id.is_none()
    {
        return false;
    }
    let Some(path) = task.worktree_path().map(|p| Path::new(p).join(NAME)) else {
        return false;
    };
    if let Some(value) = rt.extra.get(INCIDENT_KEY) {
        return serde_json::from_value::<Incident>(value.clone())
            .is_ok_and(|i| i.before_launch && i.incoming_task == task.id && i.path == path);
    }
    record["cause"].as_str() == Some(format!("Validation error: Cannot replace {}: preserve or move the existing scoped PRD before dispatch",path.display()).as_str())
}

/// Handle an old or current pre-launch incident under the registry owner's
/// revision check. Return true if the task must acquire a different slot.
pub(crate) fn recover_dispatch(
    root: &Path,
    registry: &TaskRegistry,
    task: &Task,
    prd: &Path,
    reference: &str,
) -> Result<bool> {
    let _roles = crate::coordinator::worktree_roles::lock(root)?;
    if let Some(recorded) = task
        .task_runtime
        .extra
        .get("checkpoint_prd_file")
        .and_then(Value::as_str)
    {
        if !crate::coordinator::worktree_roles::same_path(prd, &root.join(recorded)) {
            return Err(error(
                "The active PRD differs from the retained dispatch contract",
            ));
        }
    }
    let Some(path) = task.worktree_path().map(Path::new) else {
        // No claim was created: the source branch itself may occupy the
        // reserved path. Recheck it before allowing the scheduler to allocate.
        check_base(root, &task.base_branch(reference), &task.id)?;
        return Ok(true);
    };
    let metadata = crate::coordinator::worktree_roles::performer_metadata(root, path)
        .ok_or_else(|| error("Worker ownership changed"))?;
    if task.branch() != Some(metadata.branch.as_str())
        || crate::git::current_branch_name(path)? != metadata.branch
        || crate::git::git_common_dir(root)?
            .canonicalize()
            .map_err(error)?
            != crate::git::git_common_dir(path)?
                .canonicalize()
                .map_err(error)?
        || !crate::coordinator::engine::is_worktree_healthy(path)
        || worker_busy(registry, path)
        || registry.tasks.iter().any(|other| {
            other.id != task.id
                && (crate::coordinator::drain::is_pending(&other.state)
                    || other.is_awaiting_same_worktree_retry()
                    || other.state == "blocked")
                && other.worktree_path().is_some_and(|p| {
                    crate::coordinator::worktree_roles::same_path(Path::new(p), path)
                })
        })
    {
        return Err(error(
            "Worker ownership or activity changed; scoped PRD recovery deferred",
        ));
    }
    match prepare(path, &task.id) {
        Ok(info) => {
            if let Some(incident) = info {
                record_event(root, &incident, "scoped_prd_restored")?;
            }
            crate::coordinator::helpers::write_worktree_prd_for_task(prd, &task.id, path)?;
            Ok(false)
        }
        Err(MaccError::ScopedPrd(incident)) => {
            // Leave this checkout intact. Reassignment is safe only when the
            // incoming task never started and holds no work on this branch.
            let head = crate::git::head_commit(path)?;
            let status = crate::git::status_porcelain_v1(path, true)?;
            let reusable_elsewhere = task.task_runtime.last_error_origin.as_deref()
                == Some("dispatch_preparation")
                && !task.is_awaiting_repair_resume()
                && task.task_runtime.retries.unwrap_or_default() == 0
                && task.last_commit() == Some(head.as_str())
                && crate::git::merge_base_is_ancestor(path, "HEAD", &metadata.base)?
                && status
                    .iter()
                    .all(|e| e.path == NAME && e.index_status == '?' && e.worktree_status == '?')
                && crate::project_ownership::unowned_ignored_files(path)?
                    .iter()
                    .all(|p| p == NAME)
                && !crate::session_store::checkpoints::recovery::has_task_checkpoint(
                    root, &task.id,
                )?;
            if !reusable_elsewhere || tracked(path)? {
                return Err(MaccError::ScopedPrd(incident));
            }
            record_event(root, &incident, "scoped_prd_worker_skipped")?;
            Ok(true)
        }
        Err(error) => Err(error),
    }
}

/// A process may stop between durable requeue and managed restart.
pub fn requeued(task: &Task) -> bool {
    let rt = &task.task_runtime;
    task.state == "todo"
        && rt.pid.is_none()
        && rt.run_id.is_some()
        && rt.coordinator_epoch.is_some()
        && rt.extra.get(RECOVERY_KEY).is_some_and(|receipt| {
            receipt["claim_id"].as_str() == rt.claim_id.as_deref()
                && receipt["run_id"].as_str() == rt.run_id.as_deref()
                && receipt["epoch"].as_i64() == rt.coordinator_epoch
        })
}
