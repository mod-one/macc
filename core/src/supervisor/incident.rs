//! Canonical incident detection shared by the supervisor and its clients.
use crate::coordinator::model::Task;
use crate::coordinator_storage::{
    CoordinatorStorage, CoordinatorStoragePaths, JsonStorage, SqliteStorage,
};
use crate::{ProjectPaths, Result};
use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Incident {
    pub id: String,
    pub run_id: String,
    pub epoch: i64,
    pub reason: String,
    pub tasks: Vec<Task>,
    pub recent_events: Vec<Value>,
}

/// Dependency blocks are repaired through their root. Human decisions remain external.
pub fn repair_candidate(task: &Task) -> bool {
    matches!(
        task.state.as_str(),
        "blocked" | "failed" | "claimed" | "in_progress"
    ) && task.task_runtime.last_error_code.as_deref() != Some("E905")
        && !task.is_human_approval_gate()
        && task.blocked_on_external.is_none()
}

pub fn inspect(paths: &ProjectPaths) -> Result<Option<Incident>> {
    inspect_with_config(paths, None)
}

pub fn inspect_with_config(
    paths: &ProjectPaths,
    config: Option<&crate::config::CoordinatorConfig>,
) -> Result<Option<Incident>> {
    let mut storage_paths = CoordinatorStoragePaths::from_project_paths(paths);
    if let Some(registry) = config.and_then(|c| c.task_registry_file.as_ref()) {
        storage_paths.registry_json_path = paths.root.join(registry);
    }
    let json_mode = config.and_then(|c| c.storage_mode.as_deref()) == Some("json");
    let sqlite_exists = storage_paths.sqlite_path.exists();
    let sqlite = SqliteStorage::new(storage_paths.clone());
    let run = if sqlite_exists {
        sqlite.get_latest_coordinator_run()?
    } else {
        None
    };
    let snapshot = if !json_mode && sqlite_exists && sqlite.has_snapshot_data()? {
        sqlite.load_snapshot()?
    } else {
        JsonStorage::new(storage_paths).load_snapshot()?
    };
    if run.as_ref().is_some_and(|r| {
        matches!(
            r.status.as_str(),
            "stopped" | "stopped_by_user" | "completed" | "success"
        )
    }) {
        return Ok(None);
    }
    let tasks: Vec<_> = snapshot
        .registry
        .tasks
        .into_iter()
        .filter(repair_candidate)
        .collect();
    // A terminal block consisting only of protected gates is an operator decision.
    if tasks.is_empty() && run.as_ref().is_some_and(|r| r.status == "blocked") {
        return Ok(None);
    }
    let terminal_failure = run
        .as_ref()
        .is_some_and(|r| matches!(r.status.as_str(), "blocked" | "failed" | "crashed"));
    let crashed = run.as_ref().is_some_and(|r| {
        matches!(r.status.as_str(), "running" | "draining")
            && !crate::coordinator::helpers::is_pid_running(r.pid)
    });
    if !terminal_failure && !crashed && tasks.is_empty() {
        return Ok(None);
    }
    // A running engine owns its worktrees. Intervention waits for it to exit.
    if run.as_ref().is_some_and(|r| {
        crate::coordinator::helpers::is_pid_running(r.pid)
            && matches!(r.status.as_str(), "running" | "draining")
    }) {
        return Ok(None);
    }
    let run_id = run
        .as_ref()
        .map(|r| r.run_id.clone())
        .unwrap_or_else(|| "legacy".into());
    let epoch = run.as_ref().map(|r| r.epoch).unwrap_or(0);
    let reason = run
        .and_then(|r| r.stop_reason)
        .unwrap_or_else(|| "Coordinator stopped with unfinished tasks".into());
    let recent_events = snapshot
        .events
        .iter()
        .rev()
        .take(60)
        .filter_map(|e| serde_json::to_value(e).ok())
        .collect();
    Ok(Some(Incident {
        id: format!("{}-{}", run_id, epoch),
        run_id,
        epoch,
        reason,
        tasks,
        recent_events,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::coordinator_storage::{CoordinatorRun, CoordinatorSnapshot};
    #[test]
    fn sqlite_only_blocked_preconditions_are_incidents() {
        let temp = tempfile::tempdir().unwrap();
        let paths = ProjectPaths::from_root(temp.path());
        let db = SqliteStorage::new(CoordinatorStoragePaths::from_project_paths(&paths));
        let mut snapshot = CoordinatorSnapshot::empty();
        snapshot.registry.tasks = vec![Task {
            id: "root".into(),
            state: "blocked".into(),
            ..Task::default()
        }];
        snapshot.registry.tasks[0].task_runtime.unmet_preconditions =
            vec!["schema migration".into()];
        db.save_snapshot(&snapshot).unwrap();
        db.upsert_coordinator_run(&CoordinatorRun {
            run_id: "run-test".into(),
            pid: 99999999,
            hostname: "test".into(),
            started_at: "2026-10-03T00:00:00Z".into(),
            last_tick_at: None,
            stopped_at: None,
            status: "blocked".into(),
            epoch: 4,
            version: "test".into(),
            stop_reason: Some("blocked".into()),
        })
        .unwrap();
        let incident = inspect(&paths).unwrap().unwrap();
        assert_eq!(incident.epoch, 4);
        assert_eq!(
            incident.tasks[0].task_runtime.unmet_preconditions,
            vec!["schema migration"]
        );
        assert!(!paths
            .root
            .join(".macc/automation/task/task_registry.json")
            .exists());
    }
    #[test]
    fn excludes_dependency_blocks_and_human_gates() {
        let mut task = Task {
            state: "blocked".into(),
            ..Task::default()
        };
        assert!(repair_candidate(&task));
        task.task_runtime.last_error_code = Some("E905".into());
        assert!(!repair_candidate(&task));
        task.task_runtime.last_error_code = None;
        task.gate =
            Some(serde_json::from_value(serde_json::json!({"kind":"human_approval"})).unwrap());
        assert!(!repair_candidate(&task));
    }
}
