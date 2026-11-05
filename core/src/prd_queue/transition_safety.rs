//! Read-only workspace and runtime guards used only for a changed queue.
use super::transition::CODE;
use super::*;
use crate::coordinator::model::TaskRegistry;
use crate::{
    config::CoordinatorConfig,
    coordinator_storage::{CoordinatorStoragePaths, SqliteStorage},
    ProjectPaths,
};

pub(super) fn check(
    root: &Path,
    config: &CoordinatorConfig,
    registry: &TaskRegistry,
) -> Result<()> {
    let fail = |reason: &str| {
        invalid(format!("{CODE}: {reason}; keep the current queue until the work is integrated or explicitly resolved"))
    };
    let paths = CoordinatorStoragePaths::from_project_paths(&ProjectPaths::from_root(root));
    if paths.sqlite_path.exists() {
        if let Some(run) = SqliteStorage::new(paths).get_active_coordinator_run()? {
            if run.pid != i64::from(std::process::id())
                && crate::coordinator::helpers::is_pid_running(run.pid)
            {
                return Err(fail("a durable coordinator run is still active"));
            }
        }
    }
    let reference =
        crate::config::CoordinatorConfigResolved::resolve(Some(config)).reference_branch;
    let mut worktrees: Vec<PathBuf> = registry
        .tasks
        .iter()
        .filter_map(|t| t.worktree_path())
        .map(|p| root.join(p))
        .collect();
    let slots = root.join(".macc/worktree");
    if slots.exists() {
        for entry in std::fs::read_dir(&slots).map_err(|e| invalid(e.to_string()))? {
            let entry = entry.map_err(|e| invalid(e.to_string()))?;
            let name = entry.file_name();
            let name = name.to_string_lossy();
            if name.starts_with("supervisor-") || name.starts_with("worker-") {
                worktrees.push(entry.path());
            }
        }
    }
    worktrees.sort();
    worktrees.dedup();
    for path in worktrees.into_iter().filter(|p| p.exists()) {
        let status = std::process::Command::new("git")
            .arg("-C")
            .arg(&path)
            .args(["status", "--porcelain", "--untracked-files=all"])
            .output()
            .map_err(|e| invalid(e.to_string()))?;
        if !status.status.success() || !status.stdout.is_empty() {
            return Err(fail(&format!(
                "worktree {} has unintegrated files or cannot be inspected",
                path.display()
            )));
        }
        let merged = std::process::Command::new("git")
            .arg("-C")
            .arg(&path)
            .args(["merge-base", "--is-ancestor", "HEAD", &reference])
            .output()
            .map_err(|e| invalid(e.to_string()))?;
        if !merged.status.success() {
            return Err(fail(&format!(
                "worktree {} has commits not verified on {reference}",
                path.display()
            )));
        }
    }
    Ok(())
}

/// Explicit baseline acceptance never hides unfinished tasks outside the target.
pub(super) fn target(registry: &TaskRegistry, entry: &Entry) -> Result<()> {
    let unfinished: Vec<_> = registry
        .tasks
        .iter()
        .filter(|t| {
            !entry.task_ids.contains(&t.id)
                && !t.satisfies_dependants()
                && (t.state != "todo" || t.has_worktree_attached())
        })
        .map(|t| t.id.as_str())
        .collect();
    if !unfinished.is_empty() {
        return Err(invalid(format!("{CODE}: cannot replace the registry while tasks {} have unfinished work outside {}; resolve that work before launching the new series",unfinished.join(", "),entry.path)));
    }
    Ok(())
}
