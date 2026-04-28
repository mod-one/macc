use super::base::*;
pub(super) type SanitizeStepFailure = Option<&'static str>;

#[derive(Debug, Clone, Copy)]
pub(super) struct SanitizeOptions {
    fetch_remote: bool,
    fail_on_fetch_error: bool,
    tag_abandoned: bool,
}

pub(super) async fn prepare_clean_worktree(
    worktree_path: &Path,
    base_branch: &str,
    options: SanitizeOptions,
) -> Result<SanitizeStepFailure> {
    if !crate::git::reset_hard_async(worktree_path, "HEAD").await? && !options.tag_abandoned {
        return Ok(Some("reset_hard_head"));
    }
    if !crate::git::clean_fd_async(worktree_path).await? && !options.tag_abandoned {
        return Ok(Some("clean_fd"));
    }
    if !crate::git::checkout_async(worktree_path, base_branch, options.tag_abandoned).await?
        && !crate::git::checkout_reset_branch_async(
            worktree_path,
            base_branch,
            options.tag_abandoned,
        )
        .await?
    {
        // Base branch may be checked out in the main worktree; detach HEAD as fallback.
        if !crate::git::checkout_detach_async(worktree_path).await? {
            return Ok(Some("checkout_base_branch"));
        }
    }
    if options.fetch_remote {
        if !crate::git::git_remote_exists_async(worktree_path, "origin").await? {
            tracing::info!(
                "prepare_clean_worktree fetch skipped path={} base={} remote=origin reason=remote_missing",
                worktree_path.display(),
                base_branch
            );
        } else if !crate::git::fetch_async(worktree_path, "origin").await? {
            if options.fail_on_fetch_error {
                return Ok(Some("fetch_origin"));
            }
            tracing::warn!(
                "prepare_clean_worktree fetch failed but continuing path={} base={} remote=origin",
                worktree_path.display(),
                base_branch
            );
        }
    }
    if !crate::git::reset_hard_async(worktree_path, base_branch).await? {
        return Ok(Some("reset_hard_base_branch"));
    }
    if !options.tag_abandoned {
        if !crate::git::reset_hard_async(worktree_path, "HEAD").await? {
            return Ok(Some("reset_hard_head_final"));
        }
        if !crate::git::clean_fd_async(worktree_path).await? {
            return Ok(Some("clean_fd_final"));
        }
    }
    Ok(None)
}

pub(super) async fn sanitize_worktree_to_base(
    worktree_path: &Path,
    base_branch: &str,
) -> Result<Option<&'static str>> {
    prepare_clean_worktree(
        worktree_path,
        base_branch,
        SanitizeOptions {
            fetch_remote: true,
            fail_on_fetch_error: false,
            tag_abandoned: false,
        },
    )
    .await
}

fn append_worktree_orphan_cleaned_event(
    repo_root: &Path,
    task_id: &str,
    worktree_path: &Path,
    sanitize_step: &str,
) -> Result<()> {
    let run_id = crate::coordinator::helpers::ensure_coordinator_run_id();
    let now = now_iso_coordinator();
    let seq = chrono::Utc::now().timestamp_nanos_opt().unwrap_or_default() as u64;
    let path = worktree_path.to_string_lossy().to_string();
    let message = format!(
        "worktree orphan cleaned task={} path={} sanitize_step={}",
        task_id, path, sanitize_step
    );
    let payload = serde_json::json!({
        "schema_version":"1",
        "event_id": format!("evt-worktree_orphan_cleaned-{}-{}", task_id, seq),
        "run_id": run_id,
        "seq": seq,
        "ts": now,
        "source": "coordinator:native",
        "task_id": task_id,
        "type": "worktree_orphan_cleaned",
        "phase": "dev",
        "status": "success",
        "severity": "info",
        "payload": {
            "message": message,
            "worktree_path": path,
            "sanitize_step": sanitize_step
        }
    });
    let project_paths = crate::ProjectPaths::from_root(repo_root);
    let _ = crate::coordinator_storage::append_event_sqlite(&project_paths, &payload)?;
    Ok(())
}

pub(super) fn maybe_rollback_new_worktree_on_sanitize_failure(
    repo_root: &Path,
    state: &mut CoordinatorRunState,
    task_id: &str,
    sanitize_step: &str,
    rollback_path: Option<&Path>,
    worktree_was_newly_created: bool,
    rollback_enabled: bool,
    logger: Option<&dyn CoordinatorLog>,
) -> bool {
    if !rollback_enabled || !worktree_was_newly_created {
        return false;
    }
    let Some(path) = rollback_path else {
        return false;
    };

    let path_key = path.to_string_lossy().to_string();
    state.last_session_activity_at.remove(&path_key);

    match crate::remove_worktree(repo_root, path, true) {
        Ok(()) => {
            let _ = append_worktree_orphan_cleaned_event(repo_root, task_id, path, sanitize_step);
            if let Some(log) = logger {
                let _ = log.note(format!(
                    "- worktree orphan cleaned task={} path={} sanitize_step={}",
                    task_id,
                    path.display(),
                    sanitize_step
                ));
            }
            true
        }
        Err(err) => {
            let msg = format!(
                "worktree orphan cleanup failed task={} path={} sanitize_step={} error={}",
                task_id,
                path.display(),
                sanitize_step,
                err
            );
            let _ = append_coordinator_event_with_severity(
                repo_root,
                "worktree_orphan_cleanup_failed",
                task_id,
                "dev",
                "warning",
                &msg,
                "warning",
            );
            if let Some(log) = logger {
                let _ = log.note(format!("- {}", msg));
            }
            false
        }
    }
}
