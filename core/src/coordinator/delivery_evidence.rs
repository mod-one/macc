//! Reconcile the active PRD's durable deliveries before any performer is dispatched.
use crate::config::CoordinatorConfig;
use crate::coordinator::control_plane::CoordinatorLog;
use crate::coordinator::model::Task;
use crate::coordinator::types::CoordinatorEnvConfig;
use crate::{ProjectPaths, Result};
use std::path::Path;

pub fn reconcile_before_dispatch(
    root: &Path,
    prd: &Path,
    config: Option<&CoordinatorConfig>,
    env: &CoordinatorEnvConfig,
    logger: Option<&dyn CoordinatorLog>,
) -> Result<()> {
    crate::coordinator::control_plane::sync_registry_from_prd_native(root, prd, logger)?;
    crate::service::coordinator_workflow::coordinator_sync_prd(
        &ProjectPaths::from_root(root),
        config,
        env,
        logger,
    )?;
    crate::coordinator::resume_check::replay_at_startup(root, prd, config, env)
}

/// Link a verified no-change completion to the commit already published on its base.
pub fn attach_reference_commit(task: &mut Task, root: &Path, reference: &str) -> Result<()> {
    if let Some(sha) = crate::git::latest_commit_with_task_trailer(root, reference, &task.id)? {
        task.ensure_runtime()
            .extra
            .insert("completion_commit_sha".into(), sha.clone().into());
        if let Some(worktree) = task.worktree.as_mut() {
            worktree.last_commit = Some(sha);
        }
    }
    Ok(())
}

pub fn attach_reference_completion(
    value: &mut serde_json::Value,
    id: &str,
    root: &Path,
    reference: &str,
) -> Result<()> {
    let mut registry = crate::coordinator::model::TaskRegistry::from_value(value)?;
    if let Some(task) = registry.find_task_mut(id).filter(|task| task.is_merged()) {
        attach_reference_commit(task, root, reference)?;
        *value = registry.to_value()?;
    }
    Ok(())
}

/// A retry may acknowledge an existing delivery, never publish an unreviewed tip.
/// Bind the registry proof to both the exact local branch and the task on its base.
pub(crate) fn retained_tip_delivered(
    root: &Path,
    task: &Task,
    branch: &str,
    base: &str,
) -> Result<bool> {
    let Some(proof) = task
        .task_runtime
        .extra
        .get("completion_commit_sha")
        .and_then(serde_json::Value::as_str)
    else {
        return Ok(false);
    };
    if branch == base
        || crate::git::resolve_ref(root, &format!("refs/heads/{branch}")).as_deref() != Some(proof)
    {
        return Ok(false);
    }
    if let Some(path) = task
        .worktree_path()
        .or(task.task_runtime.last_worktree_path.as_deref())
    {
        let path = root.join(path);
        if !path.is_dir()
            || crate::git::current_branch(&path)? != branch
            || crate::git::head_commit(&path)? != proof
            || !crate::git::status_porcelain(&path)?.is_empty()
        {
            return Ok(false);
        }
    }
    let base_ref = format!("refs/heads/{base}");
    if crate::git::latest_commit_with_task_trailer(root, &base_ref, &task.id)?.as_deref()
        != Some(proof)
    {
        return Ok(false);
    }
    crate::coordinator::decisions::merge_ready(root, &task.id)?;
    Ok(true)
}
