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
    /// Status of the run the incident belongs to (`failed`, `crashed`,
    /// `blocked`…); `crashed` also covers a `running` run whose engine died.
    #[serde(default)]
    pub run_status: Option<String>,
}

/// Workflow states in which a task is between phases (test, review, fix,
/// merge): the coordinator resumes the pending phase by itself when it runs.
pub fn in_flight_phase(task: &Task) -> bool {
    matches!(
        task.state.as_str(),
        "testing" | "reviewing" | "changes_requested" | "pr_open" | "queued"
    )
}

/// True when restarting the coordinator is the whole recovery: the engine
/// failed or crashed while every incident task was between phases, so no
/// task failed and no source repair is needed — the restarted run resumes
/// their pending test, review, fix or merge. (A review finding is handled by
/// the fix phase the run schedules, not by the supervisor.)
pub fn resumable_by_restart(incident: &Incident) -> bool {
    matches!(incident.run_status.as_deref(), Some("failed" | "crashed"))
        && !incident.tasks.is_empty()
        && incident.tasks.iter().all(in_flight_phase)
}

/// Dependency blocks are repaired through their root. Human decisions remain external.
/// A task left between phases (testing, reviewing…) by a stopped engine is
/// part of the incident too: it is what the restarted run must resume.
pub fn repair_candidate(task: &Task) -> bool {
    (matches!(
        task.state.as_str(),
        "blocked" | "failed" | "claimed" | "in_progress"
    ) || in_flight_phase(task))
        && task.task_runtime.last_error_code.as_deref() != Some("E905")
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
    let (registry, events) = if !json_mode && sqlite_exists && sqlite.has_snapshot_data()? {
        (sqlite.load_registry()?, sqlite.load_recent_events(60)?)
    } else {
        let snapshot = JsonStorage::new(storage_paths).load_snapshot()?;
        (snapshot.registry, snapshot.events)
    };
    if run.as_ref().is_some_and(|r| {
        matches!(
            r.status.as_str(),
            "stopped" | "stopped_by_user" | "completed" | "success"
        )
    }) {
        return Ok(None);
    }
    let tasks: Vec<_> = registry
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
    let run_status = run.as_ref().map(|r| r.status.clone());
    let run_id = run
        .as_ref()
        .map(|r| r.run_id.clone())
        .unwrap_or_else(|| "legacy".into());
    let epoch = run.as_ref().map(|r| r.epoch).unwrap_or(0);
    let reason = run
        .and_then(|r| r.stop_reason)
        .unwrap_or_else(|| "Coordinator stopped with unfinished tasks".into());
    let recent_events = events
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
        run_status: if crashed {
            Some("crashed".into())
        } else {
            run_status
        },
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
    fn failed_run_with(state: &str) -> (tempfile::TempDir, ProjectPaths) {
        let temp = tempfile::tempdir().unwrap();
        let paths = ProjectPaths::from_root(temp.path());
        let db = SqliteStorage::new(CoordinatorStoragePaths::from_project_paths(&paths));
        let mut snapshot = CoordinatorSnapshot::empty();
        snapshot.registry.tasks = vec![
            Task {
                id: "L4L-BASELINE-001".into(),
                state: state.into(),
                ..Task::default()
            },
            Task {
                id: "L4L-NEXT-001".into(),
                state: "todo".into(),
                ..Task::default()
            },
        ];
        db.save_snapshot(&snapshot).unwrap();
        db.upsert_coordinator_run(&CoordinatorRun {
            run_id: "queue-1".into(),
            pid: 99999999,
            hostname: "test".into(),
            started_at: "2026-10-14T13:49:00Z".into(),
            last_tick_at: None,
            stopped_at: Some("2026-10-14T14:30:10Z".into()),
            status: "failed".into(),
            epoch: 1,
            version: "test".into(),
            stop_reason: Some(
                "Coordinator error [invalid_transition]: Invalid coordinator FSM transition: from=testing event=ReviewChangesRequested".into(),
            ),
        })
        .unwrap();
        (temp, paths)
    }

    #[test]
    fn a_task_left_in_testing_by_a_failed_engine_is_part_of_the_incident() {
        let (_temp, paths) = failed_run_with("testing");
        let incident = inspect(&paths).unwrap().unwrap();
        assert_eq!(incident.tasks.len(), 1);
        assert_eq!(incident.tasks[0].id, "L4L-BASELINE-001");
        assert_eq!(incident.run_status.as_deref(), Some("failed"));
        assert!(resumable_by_restart(&incident));
    }

    #[test]
    fn a_blocked_root_needs_more_than_a_restart() {
        let (_temp, paths) = failed_run_with("blocked");
        let incident = inspect(&paths).unwrap().unwrap();
        assert_eq!(incident.tasks[0].id, "L4L-BASELINE-001");
        assert!(!resumable_by_restart(&incident));
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
