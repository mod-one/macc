//! Recovery means observed coordinator progress, not successful launcher creation.
use macc_core::coordinator::model::TaskRegistry;
use macc_core::coordinator_storage::{CoordinatorRun, CoordinatorStoragePaths, SqliteStorage};
use macc_core::{MaccError, ProjectPaths, Result};
use std::time::{Duration, Instant};

fn resumed(
    run: &CoordinatorRun,
    old_run: &str,
    registry: &TaskRegistry,
    ids: &[String],
) -> Result<bool> {
    if run.run_id == old_run {
        return Ok(false);
    }
    if matches!(
        run.status.as_str(),
        "blocked" | "failed" | "crashed" | "stopped_by_user"
    ) {
        return Err(MaccError::Validation(format!(
            "Coordinator restart {} stopped ({}): {}",
            run.run_id,
            run.status,
            run.stop_reason
                .as_deref()
                .unwrap_or("inspect coordinator logs")
        )));
    }
    if let Some(task) = ids
        .iter()
        .filter_map(|id| registry.find_task(id))
        .find(|t| t.state == "blocked")
    {
        return Err(MaccError::Validation(format!(
            "Coordinator restart {} reblocked {}: {}",
            run.run_id,
            task.id,
            task.task_runtime
                .last_error
                .as_deref()
                .unwrap_or("inspect task evidence")
        )));
    }
    if matches!(run.status.as_str(), "success" | "completed") {
        return Ok(true);
    }
    // At least one root must progress. Other roots may legitimately remain
    // queued behind the concurrency limit; any blocked root was rejected above.
    Ok(!ids.is_empty()
        && ids.iter().any(|id| {
            registry.find_task(id).is_some_and(|task| {
                matches!(
                    task.state.as_str(),
                    "claimed"
                        | "in_progress"
                        | "implemented"
                        | "testing"
                        | "reviewing"
                        | "changes_requested"
                        | "pr_open"
                        | "queued"
                        | "merged"
                )
            })
        }))
}

pub async fn wait(
    paths: &ProjectPaths,
    old_run: &str,
    ids: &[String],
    timeout: Duration,
) -> Result<()> {
    let storage = SqliteStorage::new(CoordinatorStoragePaths::from_project_paths(paths));
    let deadline = Instant::now() + timeout;
    let mut observed_since = None;
    loop {
        if super::shutdown::requested(paths)? {
            return Err(MaccError::Validation(
                "Operator stopped coordinator during restart observation".into(),
            ));
        }
        if let Some(run) = storage.get_latest_coordinator_run()? {
            let registry = storage.load_registry()?;
            if resumed(&run, old_run, &registry, ids)? {
                let since = observed_since.get_or_insert_with(Instant::now);
                if matches!(run.status.as_str(), "success" | "completed")
                    || since.elapsed() >= Duration::from_secs(1)
                {
                    return Ok(());
                }
            } else {
                observed_since = None;
            }
        }
        if Instant::now() >= deadline {
            return Err(MaccError::Validation("Coordinator restart requested, but task resumption was not observed; inspect the new run and retained worktrees before retrying".into()));
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn running_launcher_is_insufficient_and_reblocked_roots_are_reported() {
        let mut run = CoordinatorRun {
            run_id: "new".into(),
            pid: 1,
            hostname: "test".into(),
            started_at: "now".into(),
            last_tick_at: None,
            stopped_at: None,
            status: "running".into(),
            epoch: 2,
            version: "test".into(),
            stop_reason: None,
        };
        let mut registry = TaskRegistry::default();
        registry.tasks.push(macc_core::coordinator::model::Task {
            id: "ROOT".into(),
            state: "todo".into(),
            ..Default::default()
        });
        let ids = vec!["ROOT".into()];
        assert!(!resumed(&run, "old", &registry, &ids).unwrap());
        registry.tasks[0].state = "blocked".into();
        registry.tasks[0].task_runtime.last_error =
            Some("E902: retained worker cannot resume".into());
        assert!(resumed(&run, "old", &registry, &ids)
            .unwrap_err()
            .to_string()
            .contains("E902"));
        registry.tasks[0].state = "in_progress".into();
        assert!(resumed(&run, "old", &registry, &ids).unwrap());
        assert!(!resumed(&run, "new", &registry, &ids).unwrap());
        registry.tasks.push(macc_core::coordinator::model::Task {
            id: "QUEUED".into(),
            state: "todo".into(),
            ..Default::default()
        });
        let parallel_ids = vec!["ROOT".into(), "QUEUED".into()];
        assert!(resumed(&run, "old", &registry, &parallel_ids).unwrap());
        registry.tasks[0].state = "todo".into();
        assert!(!resumed(&run, "old", &registry, &parallel_ids).unwrap());
        registry.tasks[0].state = "in_progress".into();

        run.status = "blocked".into();
        assert!(resumed(&run, "old", &registry, &ids).is_err());
        run.status = "success".into();
        assert!(resumed(&run, "old", &registry, &ids).unwrap());
    }
}
