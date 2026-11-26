//! A drain stops the queue between tasks without reporting a failed command to clients.
use macc_core::{
    coordinator::{
        control_plane::CoordinatorLog,
        managed_command_registry::{
            managed_command_result_dir, managed_command_result_path,
            upsert_managed_command_with_result,
        },
    },
    coordinator_storage::{CoordinatorStoragePaths, SqliteStorage},
    prd_queue,
    service::coordinator::{coordinator_managed_command_state, CoordinatorManagedCommandState},
    ProjectPaths, Result,
};
use std::{fs, process::Command, sync::Mutex};

struct DrainOnStartup {
    storage: SqliteStorage,
    lines: Mutex<Vec<String>>,
}

impl CoordinatorLog for DrainOnStartup {
    fn note(&self, line: String) -> Result<()> {
        if let Some(mut control) = self.storage.get_coordinator_control()? {
            if control.mode == "running" {
                control.mode = "draining".into();
                control.reason = Some("tui drain".into());
                control.drain_snapshot_json = Some("[]".into());
                self.storage.set_coordinator_control(&control)?;
            }
        }
        self.lines.lock().unwrap().push(line);
        Ok(())
    }
}

#[tokio::test]
async fn operator_drain_exits_successfully_and_preserves_the_active_prd() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();
    let files = ["first.json", "second.json"];
    for (path, id) in files.iter().zip(["FIRST", "SECOND"]) {
        fs::write(
            root.join(path),
            format!(r#"{{"tasks":[{{"id":"{id}","title":"Work","priority":"1"}}]}}"#),
        )
        .unwrap();
    }
    for args in [
        vec!["init", "-q", "-b", "main"],
        vec!["add", "."],
        vec![
            "-c",
            "user.name=Test",
            "-c",
            "user.email=test@example.invalid",
            "commit",
            "-qm",
            "fixture",
        ],
    ] {
        assert!(Command::new("git")
            .current_dir(root)
            .args(args)
            .status()
            .unwrap()
            .success());
    }
    let paths = ProjectPaths::from_root(root);
    let pid = i32::MAX;
    upsert_managed_command_with_result(&paths, "run", pid).unwrap();
    let log = DrainOnStartup {
        storage: SqliteStorage::new(CoordinatorStoragePaths::from_project_paths(&paths)),
        lines: Mutex::default(),
    };
    let config = macc_core::config::CoordinatorConfig {
        prd_files: files.map(String::from).to_vec(),
        reference_branch: Some("main".into()),
        ..Default::default()
    };
    let result = prd_queue::run(
        root,
        &Default::default(),
        &config,
        &Default::default(),
        Some(&log),
    )
    .await;
    fs::create_dir_all(managed_command_result_dir(root)).unwrap();
    fs::write(
        managed_command_result_path(root, "run", pid),
        if result.is_ok() { "0" } else { "1" },
    )
    .unwrap();
    result.unwrap();
    assert!(matches!(coordinator_managed_command_state(&paths).unwrap(),
        CoordinatorManagedCommandState::Succeeded { finish_reason: Some(reason), .. }
            if reason.contains("Drain complete")));

    let progress = prd_queue::load(root).unwrap();
    assert_eq!(progress.next, 0);
    assert_eq!(progress.status, "stopped_by_user");
    let run = log.storage.get_latest_coordinator_run().unwrap().unwrap();
    assert_eq!(run.status, "stopped_by_user");
    assert_eq!(progress.reason, run.stop_reason);
    let text = log.lines.lock().unwrap().join("\n");
    assert!(
        text.contains("- result: stopped_by_user\n- exit_reason: Drain complete."),
        "{text}"
    );
    let events = log.storage.load_recent_events(100).unwrap();
    let stop = events
        .iter()
        .find(|e| e.event_type == "prd_queue_stopped")
        .unwrap();
    assert_eq!(stop.severity(), Some("info"));
    assert_eq!(
        events
            .iter()
            .filter(|e| e.event_type == "prd_queue_started")
            .count(),
        1
    );
}
