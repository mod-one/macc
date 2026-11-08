//! Shared primary-checkout storage. Retention and Git visibility are separate policies.
mod migration;
use crate::{MaccError, Result};
use std::path::{Path, PathBuf};
pub const LAYOUT: &str = "{\"version\":1,\"authority\":\"primary-worktree\"}\n";
pub const FENCE: &str =
    "MACC state moved to the primary worktree .macc/persistent/. Use the current MACC binary.\n";
pub fn error(message: impl std::fmt::Display) -> MaccError {
    MaccError::Validation(format!("MACC-PERSISTENT-LAYOUT: {message}"))
}
pub(crate) fn io<T>(result: std::io::Result<T>) -> Result<T> {
    result.map_err(error)
}
/// The caller may be primary, linked, or use --separate-git-dir.
pub fn primary_root(root: &Path) -> Result<PathBuf> {
    let common = io(crate::git::git_common_dir(root)?.canonicalize())?;
    let (caller_git, caller_root) = checkout_identity(root)?;
    if caller_git == common {
        return Ok(caller_root);
    }
    let locator = common.join("macc-primary-worktree.json");
    no_symlink(&locator)?;
    if let Ok(bytes) = std::fs::read(&locator) {
        if let Ok(candidate) = serde_json::from_slice::<PathBuf>(&bytes) {
            if let Ok((git, path)) = checkout_identity(&candidate) {
                if git == common {
                    return Ok(path);
                }
            }
        }
    }
    let raw = crate::git::worktree_list_porcelain(root)?;
    if let Some(candidate) = raw
        .split("\n\n")
        .next()
        .and_then(|r| r.lines().find_map(|l| l.strip_prefix("worktree ")))
    {
        if let Ok((git, path)) = checkout_identity(Path::new(candidate)) {
            if git == common {
                return Ok(path);
            }
        }
    }
    Err(error("Cannot resolve the primary checkout; run macc apply there first (separate Git directory or relocated repository)"))
}
fn checkout_identity(root: &Path) -> Result<(PathBuf, PathBuf)> {
    let output = crate::git::run_git_output_mapped(
        root,
        &[
            "rev-parse",
            "--path-format=absolute",
            "--git-dir",
            "--show-toplevel",
        ],
        "resolve primary checkout identity",
    )?;
    if !output.status.success() {
        return Err(error("A non-bare primary checkout is required"));
    }
    let text = String::from_utf8_lossy(&output.stdout);
    let mut lines = text.lines();
    let git = lines.next().ok_or_else(|| error("Missing Git directory"))?;
    let top = lines.next().ok_or_else(|| error("Missing checkout root"))?;
    Ok((
        io(Path::new(git).canonicalize())?,
        io(Path::new(top).canonicalize())?,
    ))
}
pub fn directory(root: &Path) -> Result<PathBuf> {
    Ok(primary_root(root)?.join(".macc/persistent"))
}
pub fn integration_path(root: &Path) -> Result<PathBuf> {
    Ok(primary_root(root)?.join(".macc/worktree/integration"))
}
pub fn prepare(root: &Path) -> Result<PathBuf> {
    migration::prepare(root)
}
pub(crate) fn no_symlink(path: &Path) -> Result<()> {
    if std::fs::symlink_metadata(path).is_ok_and(|m| m.file_type().is_symlink()) {
        return Err(error(format!(
            "Symlink is not permitted: {}",
            path.display()
        )));
    }
    Ok(())
}
#[cfg(test)]
mod tests;

/// Clear owns all authority resources even when decision management is disabled.
pub(crate) fn idle_guards(root: &Path) -> Result<Vec<crate::fs_lock::AdvisoryLock>> {
    if crate::git::git_common_dir(root).is_err() {
        return Ok(Vec::new());
    }
    let directory = prepare(root)?;
    ["controller.lock", "mutation.lock", "integration.lock"]
        .into_iter()
        .map(|name| {
            crate::fs_lock::AdvisoryLock::acquire(
                &directory.join(name),
                std::time::Duration::ZERO,
                "MACC clear (stop active state users first)",
            )
        })
        .collect()
}
