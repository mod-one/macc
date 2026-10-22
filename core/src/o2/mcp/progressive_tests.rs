use super::*;

fn active(root: &Path) -> String {
    let memory = memory::propose(root, &crate::o2::tests::proposal(root, Some("A"))).unwrap();
    crate::o2::tests::activate(root, &memory, Some("A"));
    memory.id
}

#[test]
fn summaries_exclude_full_records_and_reads_skip_only_acknowledged_ranges() {
    let (t, key, c) = fixture();
    let root = t.path();
    let id = active(root);
    let mut server = initialized(root, c.clone());
    let summary = data(&call(&mut server, "memory.search", json!({})));
    assert_eq!(summary["summary_only"], true);
    assert!(summary["records"][0]["memory"]["proposal"]
        .get("body")
        .is_none());
    assert!(
        summary["records"][0]["preview"]
            .as_str()
            .unwrap()
            .chars()
            .count()
            <= 160
    );
    let request = json!({"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":"memory.get","arguments":{"id":id,"max_chars":64}}});
    let unsent = data(&server.handle(request).unwrap());
    assert!(unsent["fragment"].is_string());
    // An unacknowledged response cannot suppress the next attempt.
    let sent = data(&call(
        &mut server,
        "memory.get",
        json!({"id":id,"max_chars":64}),
    ));
    assert_eq!(sent["fragment"], unsent["fragment"]);
    let repeat = data(&call(
        &mut server,
        "memory.get",
        json!({"id":id,"max_chars":64}),
    ));
    assert_eq!(repeat["already_presented"], true);
    assert_eq!(repeat["next_offset"], sent["next_offset"]);
    let forced = data(&call(
        &mut server,
        "memory.get",
        json!({"id":id,"max_chars":64,"force":true}),
    ));
    assert_eq!(forced["fragment"], sent["fragment"]);
    let next = data(&call(
        &mut server,
        "memory.get",
        json!({"id":id,"offset":sent["next_offset"],"max_chars":64}),
    ));
    assert!(next["fragment"].is_string());
    let observations = activity::observations(&database(root).unwrap().0, "fixture").unwrap();
    assert_eq!(observations["available_in_run"], true);
    assert_eq!(observations["consulted"], true);
    store::release(root, &key, false).unwrap();
    let observations = activity::observations(&database(root).unwrap().0, "fixture").unwrap();
    assert_eq!(observations["available_in_run"], false);
}

#[test]
fn current_push_versions_are_referenced_but_changed_versions_remain_readable() {
    let (t, _, c) = fixture();
    let root = t.path();
    let id = active(root);
    let hit = memory::get(
        root,
        &id,
        &memory::Access {
            task_id: Some("A".into()),
            ..Default::default()
        },
    )
    .unwrap();
    // Use the verified checkpoint's normal manifest schema, not client-supplied receipts.
    let (db, _) = database(root).unwrap();
    let checkpoint: String = db
        .query_row(
            "SELECT id FROM task_checkpoints WHERE task_id='A' ORDER BY rowid DESC LIMIT 1",
            [],
            |r| r.get(0),
        )
        .unwrap();
    let raw: String = db
        .query_row(
            "SELECT manifest_json FROM context_bundles WHERE checkpoint_id=?1",
            [&checkpoint],
            |r| r.get(0),
        )
        .unwrap();
    let mut bundle: context::Bundle = serde_json::from_str(&raw).unwrap();
    bundle.selected.push(context::Selection {
        id: id.clone(),
        checksum: hit.checksum.clone(),
        source_refs: vec![],
        reason: "test push".into(),
        estimated_tokens: 100,
        title: hit.memory.proposal.title.clone(),
        body: hit.memory.proposal.body.clone(),
    });
    db.execute(
        "UPDATE context_bundles SET manifest_json=?1 WHERE checkpoint_id=?2",
        rusqlite::params![serde_json::to_string(&bundle).unwrap(), checkpoint],
    )
    .unwrap();
    drop(db);
    let mut server = initialized(root, c);
    let reply = data(&call(&mut server, "memory.get", json!({"id":id})));
    assert_eq!(reply["already_presented"], true);
    assert_eq!(reply["source"], "push");
    let (db, _) = database(root).unwrap();
    bundle.selected[0].checksum = "old-version".into();
    db.execute(
        "UPDATE context_bundles SET manifest_json=?1 WHERE checkpoint_id=?2",
        rusqlite::params![serde_json::to_string(&bundle).unwrap(), checkpoint],
    )
    .unwrap();
    drop(db);
    let reply = data(&call(&mut server, "memory.get", json!({"id":id})));
    assert!(reply["fragment"].is_string());
}
