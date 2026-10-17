use super::*;
#[test]
fn overview_is_bounded_read_only_and_joins_task_evidence_without_delivery_inference() {
    let t = crate::o2::tests::root();
    let r = t.path();
    for task in ["C", "A", "B"] {
        memory::propose(r, &crate::o2::tests::proposal(r, Some(task))).unwrap();
    }
    let before = std::fs::read(r.join(".macc/state/tool-sessions.json")).unwrap();
    let first = inspect(
        r,
        &Query {
            limit: 2,
            ..Default::default()
        },
    )
    .unwrap();
    assert_eq!(
        first
            .tasks
            .iter()
            .map(|t| t.task_id.as_str())
            .collect::<Vec<_>>(),
        ["A", "B"]
    );
    assert_eq!(first.memory_statuses["proposed"], 3);
    assert_eq!(first.tasks[0].proposals, 1);
    assert_eq!(first.next_cursor.as_deref(), Some("B"));
    let next = inspect(
        r,
        &Query {
            after: first.next_cursor.clone(),
            ..Default::default()
        },
    )
    .unwrap();
    assert_eq!(next.tasks[0].task_id, "C");
    assert!(next.next_cursor.is_none());
    let scoped = inspect(
        r,
        &Query {
            task_id: Some("B".into()),
            ..Default::default()
        },
    )
    .unwrap();
    assert_eq!(scoped.tasks.len(), 1);
    assert_eq!(scoped.tasks[0].task_id, "B");
    assert!(first.providers.is_empty()); // Inspect does not detect, install or create receipts.
    assert_eq!(
        before,
        std::fs::read(r.join(".macc/state/tool-sessions.json")).unwrap()
    );
    let (db, _) = store::readonly_database(r).unwrap();
    assert_eq!(
        db.query_row("SELECT count(*) FROM o2_audit", [], |r| r.get::<_, i64>(0))
            .unwrap(),
        3
    );
    assert!(!serde_json::to_string(&first)
        .unwrap()
        .contains("lease_token"));
}
#[test]
fn unavailable_authority_and_bad_input_remain_explicit() {
    let t = tempfile::tempdir().unwrap();
    let v = inspect(t.path(), &Query::default()).unwrap();
    assert!(!v.ready);
    assert!(v.tasks.is_empty());
    assert!(inspect(
        t.path(),
        &Query {
            limit: 0,
            ..Default::default()
        }
    )
    .is_err());
    assert!(inspect(
        t.path(),
        &Query {
            task_id: Some("bad\u{1b}".into()),
            ..Default::default()
        }
    )
    .is_err());
    let action = actions::Action::Overview {
        query: Query::default(),
    };
    assert!(!action.is_mutation());
}
#[test]
fn latest_checkpoint_and_estimated_context_are_joined_without_old_reconciliation_alerts() {
    let temp = crate::o2::tests::root();
    let r = temp.path();
    std::fs::write(
        r.join("worktree.prd.json"),
        serde_json::json!({"tasks":[{"id":"A","acceptance_criteria":["Preserve contract"]}]})
            .to_string(),
    )
    .unwrap();
    let spec = serde_json::json!({"id":"fixture","performer":{"command":"fake","session":{"enabled":true,"scope":"task_role_tool","create":{"command":"fake","args":["new","{session_id}"]}}}});
    let identity = crate::coordinator::session_identity::identity(
        r,
        r,
        "A",
        "performer",
        "fixture",
        &spec,
        &crate::coordinator::session_identity::SessionRouting {
            tier: "".into(),
            mode: "auto".into(),
        },
    )
    .unwrap();
    let request = store::checkpoints::ContextRequest {
        identity,
        phase: "dev".into(),
    };
    let old = store::checkpoints::prepare(r, &request).unwrap();
    let latest = store::checkpoints::prepare(r, &request).unwrap();
    let (db, _) = store::active_database(r).unwrap();
    db.execute(
        "UPDATE task_checkpoints SET validation_state='needs_reconciliation' WHERE id=?1",
        [old.checkpoint_id.unwrap()],
    )
    .unwrap();
    let view = inspect(r, &Query::default()).unwrap();
    let task = &view.tasks[0];
    assert_eq!(task.checkpoints, 2);
    assert_eq!(task.contexts, 2);
    assert_eq!(task.needs_reconciliation, 0);
    assert_eq!(
        task.latest_checkpoint.as_ref().unwrap().id,
        latest.checkpoint_id.unwrap()
    );
    assert!(task.latest_context.as_ref().unwrap().optional_limit_tokens > 0);
    let raw = serde_json::to_string(&view).unwrap();
    assert!(!raw.contains("mandatory_task"));
    assert!(!raw.contains("Preserve contract"));
}

#[test]
fn foreign_project_records_do_not_enter_overview_counts_or_tasks() {
    let temp = crate::o2::tests::root();
    let root = temp.path();
    let m = memory::propose(root, &crate::o2::tests::proposal(root, Some("PRIVATE"))).unwrap();
    let (db, _) = store::active_database(root).unwrap();
    db.execute(
        "UPDATE memories SET project_id='foreign' WHERE id=?1",
        [&m.id],
    )
    .unwrap();
    let v = inspect(root, &Query::default()).unwrap();
    assert!(v.tasks.is_empty());
    assert!(v.memory_statuses.is_empty());
}
