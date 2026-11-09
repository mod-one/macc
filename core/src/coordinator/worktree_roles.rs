//! Physical worktree roles. Repository membership alone never grants cleanup rights.
use super::model::TaskRegistry;
use crate::{fs_lock::AdvisoryLock, MaccError, ProjectPaths, Result};
use std::{
    path::{Path, PathBuf},
    time::Duration,
};

pub const OWNERSHIP_CODE: &str = "worktree_ownership_conflict";
pub fn conflict(message: impl Into<String>) -> MaccError {
    MaccError::Coordinator {
        code: OWNERSHIP_CODE,
        message: message.into(),
    }
}
pub fn lock(root: &Path) -> Result<AdvisoryLock> {
    let authority = crate::persistent::prepare(root)?;
    AdvisoryLock::acquire(
        &authority.join("worktree-roles.lock"),
        Duration::from_secs(5),
        "worktree role ownership",
    )
}
/// Lock order shared with scratch acquisition: integration, then pool roles.
pub(crate) fn retained_locks(
    root: &Path,
    task: &super::model::Task,
) -> Result<(Option<AdvisoryLock>, AdvisoryLock)> {
    let integration = if task
        .worktree_path()
        .is_some_and(|p| Path::new(p).file_name().and_then(|n| n.to_str()) == Some("integration"))
    {
        Some(AdvisoryLock::acquire(
            &crate::persistent::prepare(root)?.join("integration.lock"),
            Duration::ZERO,
            "integration ownership recovery (wait for the active merge to finish)",
        )?)
    } else {
        None
    };
    Ok((integration, lock(root)?))
}
pub fn reserved(path: &Path) -> bool {
    path.file_name()
        .and_then(|p| p.to_str())
        .is_none_or(|name| {
            name == "integration" || name.starts_with("supervisor-") || name.starts_with('.')
        })
}
pub fn same_path(left: &Path, right: &Path) -> bool {
    match (left.canonicalize(), right.canonicalize()) {
        (Ok(a), Ok(b)) => a == b,
        _ => left == right,
    }
}
pub fn performer(root: &Path, path: &Path) -> bool {
    if root
        .join(".macc/worktree")
        .symlink_metadata()
        .is_ok_and(|m| m.file_type().is_symlink())
        || reserved(path)
        || path
            .symlink_metadata()
            .is_ok_and(|m| m.file_type().is_symlink())
    {
        return false;
    }
    let (Ok(pool), Ok(path)) = (
        root.join(".macc/worktree").canonicalize(),
        path.canonicalize(),
    ) else {
        return false;
    };
    if path.parent() != Some(pool.as_path()) {
        return false;
    }
    if crate::project_ownership::validate_path(
        &ProjectPaths::from_root(root),
        &crate::relative_to_root(
            &ProjectPaths::from_root(root),
            &path.join(".macc/worktree.json"),
        )
        .unwrap_or_default(),
    )
    .is_err()
    {
        return false;
    }
    crate::read_worktree_metadata(&path)
        .ok()
        .flatten()
        .is_some_and(|metadata| {
            path.file_name().and_then(|n| n.to_str()) == Some(metadata.id.as_str())
                && crate::git::current_branch_name(&path).ok().as_deref()
                    == Some(metadata.branch.as_str())
                && !metadata.tool.is_empty()
                && metadata
                    .branch
                    .starts_with(&format!("ai/{}/", metadata.tool))
        })
}

fn registered_tasks(root: &Path) -> Result<TaskRegistry> {
    let paths = ProjectPaths::from_root(root);
    // Do not create a coordinator database merely to prepare merge scratch.
    if !paths.macc_dir.join("state/coordinator.sqlite").exists()
        && !root
            .join(super::COORDINATOR_TASK_REGISTRY_REL_PATH)
            .exists()
    {
        return Ok(TaskRegistry::default());
    }
    let mut result = TaskRegistry::default();
    for (file, mode) in [
        (paths.macc_dir.join("state/coordinator.sqlite"), "sqlite"),
        (root.join(super::COORDINATOR_TASK_REGISTRY_REL_PATH), "json"),
    ] {
        if file.exists() {
            let args = std::collections::BTreeMap::from([("storage-mode".into(), mode.into())]);
            result.tasks.extend(
                TaskRegistry::from_value(&super::state::coordinator_state_registry_load(
                    root, &args,
                )?)?
                .tasks,
            );
        }
    }
    Ok(result)
}

#[derive(serde::Serialize, serde::Deserialize)]
struct ScratchReceipt {
    role: String,
    path: PathBuf,
    git_directory: PathBuf,
    owner_id: String,
}
fn git_directory(path: &Path) -> Result<PathBuf> {
    let out = crate::git::run_git_output_mapped(
        path,
        &["rev-parse", "--absolute-git-dir"],
        "identify integration checkout",
    )?;
    if !out.status.success() {
        return Err(conflict("Cannot identify integration Git checkout"));
    }
    Path::new(String::from_utf8_lossy(&out.stdout).trim())
        .canonicalize()
        .map_err(crate::persistent::error)
}
/// Called under the integration and role locks before every scratch mutation.
pub fn validate_scratch(root: &Path, path: &Path, base: &str) -> Result<()> {
    crate::persistent::no_symlink(path.parent().unwrap())?;
    crate::persistent::no_symlink(&path.join(".macc"))?;
    crate::persistent::no_symlink(&path.join(".macc/integration-owner"))?;
    if path
        .symlink_metadata()
        .is_ok_and(|m| m.file_type().is_symlink())
    {
        return Err(conflict(format!(
            "Integration path {} is a symlink; preserved",
            path.display()
        )));
    }
    let tasks = registered_tasks(root)?;
    if let Some(task) = tasks.tasks.iter().find(|task| {
        task.worktree_path()
            .is_some_and(|p| same_path(Path::new(p), path))
    }) {
        return Err(conflict(format!("Integration path {} belongs to task {} claim={} expected_branch={}; no cleanup performed", path.display(), task.id, task.task_runtime.claim_id.as_deref().unwrap_or("-"), task.branch().unwrap_or("-"))));
    }
    if path.join(".macc/worktree.json").exists() || path.join("worktree.prd.json").exists() {
        return Err(conflict(format!(
            "Integration path {} contains performer metadata; no cleanup performed",
            path.display()
        )));
    }
    if !path.join(".git").exists() {
        return Ok(());
    }
    if crate::git::git_common_dir(path)?
        .canonicalize()
        .map_err(crate::persistent::error)?
        != crate::git::git_common_dir(root)?
            .canonicalize()
            .map_err(crate::persistent::error)?
    {
        return Err(conflict(
            "Integration slot belongs to another Git repository; no cleanup performed",
        ));
    }
    let branch = crate::git::current_branch(path)?;
    if !branch.is_empty() && branch != "HEAD" {
        return Err(conflict(format!(
            "Integration path {} has attached branch {branch}; no cleanup performed",
            path.display()
        )));
    }
    let receipt =
        crate::persistent::primary_root(root)?.join(".macc/persistent/integration-slot.json");
    crate::persistent::no_symlink(&receipt)?;
    if receipt.exists() {
        let proof: ScratchReceipt =
            serde_json::from_slice(&std::fs::read(&receipt).map_err(crate::persistent::error)?)
                .map_err(crate::persistent::error)?;
        if proof.role != "integration"
            || !same_path(&proof.path, path)
            || proof.git_directory != git_directory(path)?
            || std::fs::read_to_string(path.join(".macc/integration-owner"))
                .ok()
                .as_deref()
                != Some(proof.owner_id.as_str())
        {
            return Err(conflict("Integration checkout identity differs from its ownership receipt; no cleanup performed"));
        }
    } else if path.join(".macc/integration-owner").exists()
        || crate::git::is_dirty(path)?
        || !crate::git::merge_base_is_ancestor(path, "HEAD", base)?
    {
        return Err(conflict("Unproven integration scratch contains local work; preserve it and recover ownership before retrying"));
    }
    if !crate::project_ownership::unowned_ignored_files(path)?.is_empty() {
        return Err(conflict(
            "Integration scratch contains unowned ignored files; preserved before checkout",
        ));
    }
    Ok(())
}
pub fn record_scratch(root: &Path, path: &Path) -> Result<()> {
    let paths = ProjectPaths::from_root(crate::persistent::primary_root(root)?);
    let receipt = paths.macc_dir.join("persistent/integration-slot.json");
    crate::persistent::no_symlink(&receipt)?;
    let proof = ScratchReceipt {
        role: "integration".into(),
        path: path.canonicalize().map_err(crate::persistent::error)?,
        git_directory: git_directory(path)?,
        owner_id: uuid::Uuid::new_v4().to_string(),
    };
    crate::persistent::no_symlink(&path.join(".macc"))?;
    crate::persistent::no_symlink(&path.join(".macc/integration-owner"))?;
    crate::atomic_write(
        &paths,
        &path.join(".macc/integration-owner"),
        proof.owner_id.as_bytes(),
    )?;
    crate::atomic_write(
        &paths,
        &receipt,
        &serde_json::to_vec_pretty(&proof).map_err(crate::persistent::error)?,
    )
}

#[cfg(test)]
#[path = "worktree_ownership_tests.rs"]
mod tests;
