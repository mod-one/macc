use super::*;
use crate::coordinator_storage::{
    CoordinatorSnapshot, CoordinatorStorage, CoordinatorStoragePaths, SqliteStorage,
};
fn fixture(state: &str) -> (tempfile::TempDir, Task) {
    let root = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(root.path().join(".macc/state")).unwrap();
    let task = Task {
        id: "T".into(),
        state: state.into(),
        ..Default::default()
    };
    save(root.path(), task.clone());
    (root, task)
}
fn save(root: &Path, task: Task) {
    let db = SqliteStorage::new(CoordinatorStoragePaths::from_project_paths(
        &ProjectPaths::from_root(root),
    ));
    let mut snapshot = CoordinatorSnapshot::empty();
    snapshot.registry.tasks.push(task);
    db.save_snapshot(&snapshot).unwrap();
}
fn task(root: &Path) -> Task {
    SqliteStorage::new(CoordinatorStoragePaths::from_project_paths(
        &ProjectPaths::from_root(root),
    ))
    .load_snapshot()
    .unwrap()
    .registry
    .tasks[0]
        .clone()
}
fn apply(root: &Path, request: &Request) -> Result<()> {
    crate::service::coordinator_workflow::unblocking::apply(
        &ProjectPaths::from_root(root),
        None,
        &CoordinatorEnvConfig::default(),
        request,
    )
}
#[test]
fn duplicate_late_and_reused_requests_cannot_reset_a_phase() {
    let (root, before) = fixture("blocked");
    let request = Request::new(&before, "verified repair").unwrap();
    apply(root.path(), &request).unwrap();
    let unblocked = task(root.path());
    assert_eq!(unblocked.state, "todo");
    apply(root.path(), &request).unwrap();
    assert_eq!(
        task_revision(&task(root.path())).unwrap(),
        task_revision(&unblocked).unwrap()
    );
    for state in ["in_progress", "merged"] {
        let mut advanced = unblocked.clone();
        advanced.state = state.into();
        advanced.task_runtime.pid = Some(1234);
        save(root.path(), advanced.clone());
        apply(root.path(), &request).unwrap();
        assert_eq!(
            task_revision(&task(root.path())).unwrap(),
            task_revision(&advanced).unwrap()
        );
        let late = Request {
            request_id: uuid::Uuid::new_v4().to_string(),
            ..request.clone()
        };
        assert!(apply(root.path(), &late).is_err());
        assert_eq!(task(root.path()).state, state);
    }
    let reused = Request {
        evidence: "changed payload".into(),
        ..request
    };
    assert!(apply(root.path(), &reused).is_err());
}
#[test]
fn historical_unblocks_are_informational_and_do_not_erase_later_failures() {
    for state in ["in_progress", "merged", "blocked"] {
        let (root, before) = fixture(state);
        let record = crate::coordinator::CoordinatorEventRecord {
            schema_version: "1".into(),
            event_id: "old-unblock".into(),
            seq: 1,
            ts: "2020-01-01T00:00:00Z".into(),
            source: "coordinator:cli".into(),
            task_id: Some("T".into()),
            event_type: "task_unblocked".into(),
            phase: Some("dispatch".into()),
            status: "todo".into(),
            detail: Some("old proof".into()),
            msg: None,
            payload: serde_json::Value::Null,
            run_id: None,
            coordinator_epoch: None,
            claim_id: None,
            extra: Default::default(),
        };
        crate::coordinator_storage::append_event_record_sqlite(
            &ProjectPaths::from_root(root.path()),
            &record,
        )
        .unwrap();
        assert!(crate::coordinator::runtime::raw_event_to_runtime_event(&record).is_none());
        let mut runtime = CoordinatorRunState::new();
        crate::coordinator::control_plane::consume_runtime_events(root.path(), &mut runtime, None)
            .unwrap();
        crate::coordinator::control_plane::apply_runtime_event_bus_updates(
            root.path(),
            &CoordinatorEnvConfig::default(),
            None,
            &mut runtime,
            None,
        )
        .unwrap();
        assert_eq!(
            task_revision(&task(root.path())).unwrap(),
            task_revision(&before).unwrap()
        );
    }
}
#[tokio::test]
async fn acknowledgement_follows_owner_application_on_single_thread_runtime() {
    let (root, before) = fixture("blocked");
    let request = Request::new(&before, "repair checked").unwrap();
    let mut runtime = CoordinatorRunState::new();
    crate::coordinator::ipc::ensure_performer_ipc_listener(root.path(), &mut runtime, None)
        .await
        .unwrap();
    let result = tokio::spawn({
        let root = root.path().to_path_buf();
        let request = request.clone();
        async move { send(&root, &request).await }
    });
    for _ in 0..100 {
        if !runtime.control_rx.is_empty() {
            break;
        }
        tokio::time::sleep(Duration::from_millis(5)).await;
    }
    assert!(
        !result.is_finished(),
        "transport ACK must await application"
    );
    assert_eq!(task(root.path()).state, "blocked");
    drain(
        root.path(),
        &CoordinatorEnvConfig::default(),
        None,
        &mut runtime,
    );
    result.await.unwrap().unwrap();
    assert_eq!(task(root.path()).state, "todo");
    let duplicate = tokio::task::spawn_blocking({
        let root = root.path().to_path_buf();
        let request = request.clone();
        move || send_sync(&root, &request)
    });
    for _ in 0..100 {
        if duplicate.is_finished() {
            break;
        }
        drain(
            root.path(),
            &CoordinatorEnvConfig::default(),
            None,
            &mut runtime,
        );
        tokio::time::sleep(Duration::from_millis(5)).await;
    }
    duplicate.await.unwrap().unwrap();
    assert_eq!(task(root.path()).state, "todo");
}
#[tokio::test]
async fn cancelled_queue_requests_and_stale_revision_have_no_side_effects() {
    let (root, before) = fixture("blocked");
    let request = Request::new(&before, "checked").unwrap();
    let mut state = CoordinatorRunState::new();
    let (tx, rx) = tokio::sync::oneshot::channel();
    drop(rx);
    state
        .control_tx
        .try_send(Pending {
            request: request.clone(),
            deadline: Instant::now() + BUDGET,
            reply: tx,
        })
        .unwrap();
    drain(
        root.path(),
        &CoordinatorEnvConfig::default(),
        None,
        &mut state,
    );
    assert_eq!(task(root.path()).state, "blocked");
    let mut changed = before;
    changed.task_runtime.last_error = Some("a new blocker".into());
    save(root.path(), changed.clone());
    assert!(apply(root.path(), &request).is_err());
    assert_eq!(
        task_revision(&task(root.path())).unwrap(),
        task_revision(&changed).unwrap()
    );
}

#[test]
fn ordered_failure_events_remain_effective_around_old_unblock_notifications() {
    use crate::coordinator::runtime::{CoordinatorRuntimeEvent, CoordinatorRuntimeEventKind};
    for replay_first in [true, false] {
        let (root, _) = fixture("in_progress");
        let old = crate::coordinator::CoordinatorEventRecord {
            schema_version: "1".into(),
            event_id: "old-unblock".into(),
            seq: 1,
            ts: "2020-01-01T00:00:00Z".into(),
            source: "coordinator:cli".into(),
            task_id: Some("T".into()),
            event_type: "task_unblocked".into(),
            phase: Some("dispatch".into()),
            status: "todo".into(),
            detail: Some("old proof".into()),
            msg: None,
            payload: serde_json::Value::Null,
            run_id: None,
            coordinator_epoch: None,
            claim_id: None,
            extra: Default::default(),
        };
        crate::coordinator_storage::append_event_record_sqlite(
            &ProjectPaths::from_root(root.path()),
            &old,
        )
        .unwrap();
        let mut state = CoordinatorRunState::new();
        if replay_first {
            crate::coordinator::control_plane::consume_runtime_events(
                root.path(),
                &mut state,
                None,
            )
            .unwrap();
        }
        state
            .runtime_event_bus_tx
            .send(CoordinatorRuntimeEvent {
                task_id: "T".into(),
                ts: "2026-10-28T00:00:00Z".into(),
                source: "performer".into(),
                kind: CoordinatorRuntimeEventKind::Failed {
                    phase: Some("dev".into()),
                    message: Some("new failure".into()),
                },
            })
            .unwrap();
        if !replay_first {
            crate::coordinator::control_plane::consume_runtime_events(
                root.path(),
                &mut state,
                None,
            )
            .unwrap();
        }
        crate::coordinator::control_plane::apply_runtime_event_bus_updates(
            root.path(),
            &CoordinatorEnvConfig::default(),
            None,
            &mut state,
            None,
        )
        .unwrap();
        let current = task(root.path());
        assert_eq!(current.state, "in_progress");
        assert_eq!(
            current.task_runtime.last_error.as_deref(),
            Some("new failure")
        );
        assert_eq!(current.task_runtime.status.as_deref(), Some("failed"));
    }
}

#[tokio::test]
async fn expired_commands_and_nonloopback_endpoints_are_rejected() {
    let (root, before) = fixture("blocked");
    let request = Request::new(&before, "checked").unwrap();
    let mut state = CoordinatorRunState::new();
    let (tx, rx) = tokio::sync::oneshot::channel();
    state
        .control_tx
        .try_send(Pending {
            request: request.clone(),
            deadline: Instant::now() - Duration::from_secs(1),
            reply: tx,
        })
        .unwrap();
    drain(
        root.path(),
        &CoordinatorEnvConfig::default(),
        None,
        &mut state,
    );
    assert!(rx.await.is_err());
    assert_eq!(task(root.path()).state, "blocked");
    std::fs::write(
        root.path()
            .join(crate::coordinator::ipc::COORDINATOR_IPC_ADDR_REL_PATH),
        "192.0.2.1:1234",
    )
    .unwrap();
    assert!(send(root.path(), &request)
        .await
        .unwrap_err()
        .to_string()
        .contains("loopback"));
}
