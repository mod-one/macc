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
    Ok(())
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
