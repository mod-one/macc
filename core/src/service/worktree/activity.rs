//! Observed task execution versus retained Git slots. Directory count is not concurrency.
use crate::{
    coordinator::model::TaskRegistry,
    coordinator_storage::{
        CoordinatorStorage, CoordinatorStoragePaths, JsonStorage, SqliteStorage,
    },
    ProjectPaths, Result, WorktreeEntry,
};
use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
};

pub fn classify(
    entry: &WorktreeEntry,
    root: &Path,
    dirty: bool,
    registry: &TaskRegistry,
    alive: impl Fn(i64) -> bool,
) -> &'static str {
    if entry.path == root {
        return "project";
    }
    if entry.path == root.join(".git/macc/integration") {
        return "integration";
    }
    if entry.path.parent() == Some(root.join(".macc/worktree").as_path())
        && entry
            .path
            .file_name()
            .and_then(|s| s.to_str())
            .is_some_and(|s| s.starts_with("supervisor-"))
    {
        return "supervisor";
    }
    if entry.prunable {
        return "prunable";
    }
    if entry.locked {
        return "locked";
    }
    let tasks: Vec<_> = registry
        .tasks
        .iter()
        .filter(|task| {
            task.worktree
                .as_ref()
                .and_then(|w| w.worktree_path.as_deref())
                .is_some_and(|p| super::canonicalize_path_fallback(Path::new(p)) == entry.path)
        })
        .collect();
    if tasks.iter().any(|t| {
        matches!(
            t.state.as_str(),
            "claimed" | "in_progress" | "changes_requested" | "queued" | "pr_open"
        ) && t.task_runtime.pid.is_some_and(&alive)
    }) {
        return "in_progress";
    }
    if tasks.iter().any(|t| t.state == "blocked") {
        return "blocked";
    }
    if tasks.iter().any(|t| {
        matches!(
            t.state.as_str(),
            "claimed" | "in_progress" | "changes_requested" | "queued" | "pr_open"
        )
    }) {
        return "waiting";
    }
    if dirty {
        "retained"
    } else {
        "idle"
    }
}

pub fn inspect(
    paths: &ProjectPaths,
    entries: &[WorktreeEntry],
) -> Result<BTreeMap<PathBuf, String>> {
    let storage = CoordinatorStoragePaths::from_project_paths(paths);
    let sqlite = SqliteStorage::new(storage.clone());
    let registry = if storage.sqlite_path.exists() && sqlite.has_snapshot_data()? {
        sqlite.load_registry()?
    } else {
        JsonStorage::new(storage).load_snapshot()?.registry
    };
    let root = super::canonicalize_path_fallback(&paths.root);
    entries
        .iter()
        .map(|entry| {
            let mut entry = entry.clone();
            entry.path = super::canonicalize_path_fallback(&entry.path);
            let dirty = super::git_worktree_is_dirty(&entry.path)?;
            let status = classify(
                &entry,
                &root,
                dirty,
                &registry,
                crate::coordinator::helpers::is_pid_running,
            );
            Ok((entry.path, status.into()))
        })
        .collect()
}

pub fn summary(states: &BTreeMap<PathBuf, String>) -> String {
    let count = |state: &str| states.values().filter(|s| s.as_str() == state).count();
    format!("Running: {} | Blocked slots: {} | Waiting: {} | Retained work: {} | Idle: {} | Supervisor: {} | Integration: {}",
        count("in_progress"),count("blocked"),count("waiting"),count("retained"),count("idle"),count("supervisor"),count("integration"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    #[test]
    fn blocked_dirty_slot_and_integration_are_not_extra_running_workers() {
        let root = Path::new("/project");
        let entry = WorktreeEntry {
            path: root.join(".macc/worktree/worker-01"),
            head: None,
            branch: None,
            locked: false,
            prunable: false,
        };
        let mut registry: TaskRegistry=serde_json::from_value(json!({"tasks":[{"id":"A","state":"blocked","worktree":{"worktree_path":entry.path},"task_runtime":{"pid":42}}]})).unwrap();
        assert_eq!(classify(&entry, root, true, &registry, |_| true), "blocked");
        registry.tasks[0].state = "in_progress".into();
        assert_eq!(
            classify(&entry, root, true, &registry, |_| true),
            "in_progress"
        );
        assert_eq!(
            classify(&entry, root, true, &registry, |_| false),
            "waiting"
        );
        registry.tasks.clear();
        assert_eq!(
            classify(&entry, root, true, &registry, |_| false),
            "retained"
        );
        assert_eq!(classify(&entry, root, false, &registry, |_| false), "idle");
        let mut reserved = entry;
        reserved.path = root.join(".macc/worktree/supervisor-01");
        assert_eq!(
            classify(&reserved, root, false, &registry, |_| true),
            "supervisor"
        );
        reserved.path = root.join(".git/macc/integration");
        assert_eq!(
            classify(&reserved, root, false, &registry, |_| true),
            "integration"
        );
    }
}
