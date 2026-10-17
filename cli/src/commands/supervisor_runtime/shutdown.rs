//! Operator stop must cancel attached monitoring, including in-flight AI execution.
use macc_core::{
    coordinator_storage::{CoordinatorStoragePaths, SqliteStorage},
    ProjectPaths, Result,
};
use std::time::Duration;

pub fn requested(paths: &ProjectPaths) -> Result<bool> {
    let storage = CoordinatorStoragePaths::from_project_paths(paths);
    if !storage.sqlite_path.exists() {
        return Ok(false);
    }
    let db = SqliteStorage::new(storage);
    let run = db.get_latest_coordinator_run()?;
    let control = db.get_coordinator_control()?;
    let explicit_control = control.is_some_and(|control| {
        matches!(
            control.mode.as_str(),
            "force_stopping" | "graceful_stopping"
        ) && control.requested_at.as_ref().is_some_and(|request| {
            chrono::DateTime::parse_from_rfc3339(request)
                .ok()
                .is_some_and(|requested| {
                    run.as_ref().is_none_or(|run| {
                        chrono::DateTime::parse_from_rfc3339(&run.started_at)
                            .ok()
                            .is_some_and(|started| requested >= started)
                    })
                })
        })
    });
    if explicit_control {
        return Ok(true);
    }
    let stall_run = std::fs::read_to_string(paths.root.join(".macc/log/supervisor/stall.json"))
        .ok()
        .and_then(|raw| serde_json::from_str::<serde_json::Value>(&raw).ok())
        .and_then(|value| value["run_id"].as_str().map(str::to_owned));
    Ok(run.as_ref().is_some_and(|run| {
        matches!(run.status.as_str(), "stopped" | "stopped_by_user")
            && stall_run.as_deref() != Some(run.run_id.as_str())
    }))
}
pub async fn wait_for_operator_stop(paths: &ProjectPaths, attached: bool) -> Result<()> {
    loop {
        // A storage problem does not authorize shutdown; normal monitoring reports it.
        let stop = std::fs::read_to_string(paths.root.join(".macc/state/supervisor-stop.json"))
            .ok()
            .and_then(|raw| serde_json::from_str::<serde_json::Value>(&raw).ok())
            .and_then(|value| value["pid"].as_u64())
            == Some(std::process::id() as u64);
        if stop || (attached && requested(paths).unwrap_or(false)) {
            return Ok(());
        }
        tokio::time::sleep(Duration::from_millis(250)).await;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use macc_core::coordinator_storage::{CoordinatorControl, CoordinatorRun};
    #[test]
    fn explicit_force_stop_is_detected_but_old_requests_and_crashes_are_not() {
        let temp = tempfile::tempdir().unwrap();
        let paths = ProjectPaths::from_root(temp.path());
        let db = SqliteStorage::new(CoordinatorStoragePaths::from_project_paths(&paths));
        let mut run = CoordinatorRun {
            run_id: "run".into(),
            pid: 9999999,
            hostname: "test".into(),
            started_at: "2026-10-04T10:00:00Z".into(),
            last_tick_at: None,
            stopped_at: None,
            status: "crashed".into(),
            epoch: 1,
            version: "test".into(),
            stop_reason: None,
        };
        db.upsert_coordinator_run(&run).unwrap();
        assert!(!requested(&paths).unwrap());
        let mut control = CoordinatorControl {
            mode: "force_stopping".into(),
            requested_at: Some("2026-10-04T09:00:00Z".into()),
            requested_by: Some("tui".into()),
            drain_snapshot_json: None,
            force_grace_seconds: None,
            cleanup_after_force: None,
            reason: Some("operator stop".into()),
        };
        db.set_coordinator_control(&control).unwrap();
        assert!(!requested(&paths).unwrap());
        control.requested_at = Some("2026-10-04T11:00:00Z".into());
        db.set_coordinator_control(&control).unwrap();
        assert!(requested(&paths).unwrap());
        run.status = "stopped_by_user".into();
        db.upsert_coordinator_run(&run).unwrap();
        assert!(requested(&paths).unwrap());
        control.requested_at = Some("2026-10-04T09:00:00Z".into());
        db.set_coordinator_control(&control).unwrap();
        std::fs::create_dir_all(paths.root.join(".macc/log/supervisor")).unwrap();
        std::fs::write(
            paths.root.join(".macc/log/supervisor/stall.json"),
            r#"{"run_id":"run"}"#,
        )
        .unwrap();
        assert!(
            !requested(&paths).unwrap(),
            "supervisor stall recovery must keep monitoring"
        );
    }
}

pub fn request_stop(paths: &ProjectPaths, pid: u32) -> Result<()> {
    super::report::write_json(
        &paths.root.join(".macc/state/supervisor-stop.json"),
        &serde_json::json!({"pid":pid,"requested_at":chrono::Utc::now().to_rfc3339()}),
    )
}
