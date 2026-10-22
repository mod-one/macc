use super::*;
#[path = "fixture.rs"]
mod fixture;
use fixture::*;
#[test]
fn protocol_lifecycle_catalog_scope_and_revocation() {
    let (t, key, c) = fixture();
    let r = t.path();
    let mut s = Server::new(r, c.clone()).unwrap();
    assert!(call(&mut s, "memory.search", json!({}))["error"].is_object());
    let mut s = initialized(r, c.clone());
    let list = s
        .handle(json!({"jsonrpc":"2.0","id":"list","method":"tools/list"}))
        .unwrap();
    assert_eq!(list["result"]["tools"].as_array().unwrap().len(), 9);
    for forbidden in [
        "memory.review",
        "memory.export",
        "provider.apply",
        "checkpoint.finish",
        "command.exec",
    ] {
        assert_eq!(call(&mut s, forbidden, json!({}))["error"]["code"], -32602);
    }
    for arg in [
        json!({"task_id":"B"}),
        json!({"worktree":"/tmp"}),
        json!({"include_inactive":true}),
        json!({"project_id":"other"}),
    ] {
        assert_eq!(
            call(&mut s, "memory.search", arg)["result"]["isError"],
            true
        );
    }
    assert!(s.handle(json!({"jsonrpc":"2.0","method":"tools/call","params":{"name":"memory.propose","arguments":proposal()}})).is_none());
    assert!(data(&call(&mut s, "memory.search", json!({})))["records"]
        .as_array()
        .unwrap()
        .is_empty());
    let mut bad = c.clone();
    bad.generation += 1;
    assert!(Server::new(r, bad).is_err());
    let mut bad = c;
    bad.token = "f".repeat(64);
    assert!(Server::new(r, bad).is_err());
    store::release(r, &key, false).unwrap();
    assert_eq!(
        call(&mut s, "memory.search", json!({}))["error"]["code"],
        -32001
    );
}
#[test]
fn proposals_are_fenced_idempotent_bounded_and_never_active() {
    let (t, _, c) = fixture();
    let r = t.path();
    let mut s = initialized(r, c);
    let first = data(&call(&mut s, "memory.propose", proposal()));
    assert_eq!(first["status"], "proposed");
    assert_eq!(
        data(&call(&mut s, "memory.propose", proposal()))["replayed"],
        true
    );
    let mut different = proposal();
    different["body"] = json!("Changed assertion");
    assert_eq!(
        call(&mut s, "memory.propose", different)["result"]["isError"],
        true
    );
    let mut escalation = proposal();
    escalation["scope"] = json!("project");
    assert_eq!(
        call(&mut s, "memory.propose", escalation)["result"]["isError"],
        true
    );
    let mut traversal = proposal();
    traversal["evidence_paths"] = json!(["../secret"]);
    assert_eq!(
        call(&mut s, "memory.propose", traversal)["result"]["isError"],
        true
    );
    assert_eq!(
        call(&mut s, "memory.get", json!({"id":first["id"]}))["result"]["isError"],
        true
    );
    assert!(data(&call(&mut s, "memory.search", json!({})))["records"]
        .as_array()
        .unwrap()
        .is_empty());
    let p = policy::inspect(r).unwrap();
    let mut v = p.policy;
    v.memory.mcp_max_proposals_per_session = 1;
    policy::update(r, &p.revision, Some(v)).unwrap();
    let mut second = proposal();
    second["request_id"] = json!("two");
    assert_eq!(
        call(&mut s, "memory.propose", second)["result"]["isError"],
        true
    );
    assert_eq!(
        data(&call(&mut s, "memory.propose", proposal()))["replayed"],
        true
    );
    let (db, _) = store::readonly_database(r).unwrap();
    assert_eq!(
        db.query_row(
            "SELECT status FROM memories WHERE id=?1",
            [first["id"].as_str().unwrap()],
            |row| row.get::<_, String>(0)
        )
        .unwrap(),
        "proposed"
    );
    assert!(db.execute("DELETE FROM memories", []).is_err());
}
#[test]
fn reads_filter_scope_staleness_and_challenge_does_not_suppress_fact() {
    let (t, _, c) = fixture();
    let r = t.path();
    let own = memory::propose(r, &crate::o2::tests::proposal(r, Some("A"))).unwrap();
    let other = memory::propose(r, &crate::o2::tests::proposal(r, Some("B"))).unwrap();
    for (m, task) in [(&own, "A"), (&other, "B")] {
        crate::o2::tests::activate(r, m, Some(task));
    }
    let mut s = initialized(r, c);
    let result = data(&call(&mut s, "memory.search", json!({})));
    assert_eq!(result["records"].as_array().unwrap().len(), 1);
    assert_eq!(result["records"][0]["memory"]["id"], own.id);
    assert_eq!(
        call(&mut s, "memory.get", json!({"id":other.id}))["result"]["isError"],
        true
    );
    let challenge = data(&call(
        &mut s,
        "memory.challenge",
        json!({"request_id":"challenge","id":own.id,"reason":"Please recheck the assumption"}),
    ));
    assert_eq!(challenge["status"], "proposed");
    assert_eq!(
        data(&call(&mut s, "memory.search", json!({})))["records"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
    assert_eq!(
        data(&call(&mut s, "context.get_checkpoint", json!({})))["task_id"],
        "A"
    );
    std::fs::write(r.join("contract.rs"), "changed").unwrap();
    assert!(data(&call(&mut s, "memory.search", json!({})))["records"]
        .as_array()
        .unwrap()
        .is_empty());
    assert_eq!(
        call(&mut s, "memory.get", json!({"id":own.id}))["result"]["isError"],
        true
    );
}
#[test]
fn phase_extraction_is_opt_in_deduplicated_and_never_activates() {
    let (t, key, _) = fixture();
    let r = t.path();
    for outcome in ["success", "quota", "interrupted", "precondition_unmet"] {
        store::checkpoints::finish(
            r,
            &store::checkpoints::FinishRequest {
                key: key.clone(),
                phase: "dev".into(),
                result: outcome.into(),
                acknowledged: true,
            },
        )
        .unwrap();
    }
    let count = || {
        let (db, _) = store::readonly_database(r).unwrap();
        db.query_row("SELECT count(*) FROM memories", [], |row| {
            row.get::<_, i64>(0)
        })
        .unwrap()
    };
    assert_eq!(count(), 0);
    let finish = || {
        store::checkpoints::finish(
            r,
            &store::checkpoints::FinishRequest {
                key: key.clone(),
                phase: "dev".into(),
                result: "failure".into(),
                acknowledged: true,
            },
        )
        .unwrap()
    };
    let cp = finish();
    assert_eq!(count(), 1);
    finish();
    assert_eq!(count(), 1);
    assert!(extraction::from_checkpoint(r, &cp.id).unwrap()["candidate_id"].is_string());
    assert_eq!(count(), 1);
    let (db, _) = store::readonly_database(r).unwrap();
    assert_eq!(
        db.query_row("SELECT status FROM memories", [], |row| row
            .get::<_, String>(0))
            .unwrap(),
        "proposed"
    );
    let p = policy::inspect(r).unwrap();
    let mut v = p.policy;
    v.memory.phase_extraction_enabled = false;
    policy::update(r, &p.revision, Some(v)).unwrap();
    assert!(extraction::from_checkpoint(r, &cp.id).unwrap()["candidate_id"].is_null());
}
#[test]
fn stdio_parse_errors_notifications_and_frame_limit() {
    let (t, _, c) = fixture();
    let r = t.path();
    let input=b"broken\n{\"jsonrpc\":\"2.0\",\"method\":\"notifications/initialized\"}\n{\"jsonrpc\":\"2.0\",\"id\":2,\"method\":\"ping\"}\n";
    let mut out = Vec::new();
    serve(r, c.clone(), std::io::Cursor::new(input), &mut out).unwrap();
    let replies = String::from_utf8(out)
        .unwrap()
        .lines()
        .map(|line| serde_json::from_str::<Value>(line).unwrap())
        .collect::<Vec<_>>();
    assert_eq!(replies.len(), 2);
    assert_eq!(replies[0]["error"]["code"], -32700);
    assert_eq!(replies[1]["id"], 2);
    let mut out = Vec::new();
    assert!(serve(r, c, std::io::Cursor::new(vec![b'x'; 65537]), &mut out).is_err());
    assert!(out.is_empty());
}
#[test]
fn budget_includes_wire_escaping_unicode_and_fragments() {
    let id = json!("i".repeat(64));
    let raw = "é\\\"\n".repeat(2000);
    let mut offset = 0;
    let mut restored = String::new();
    while offset < raw.chars().count() {
        let remaining = raw.chars().skip(offset).collect::<String>();
        let wire = budget::bound(
            &id,
            json!({"fragment":remaining,"offset":offset,"next_offset":null}),
            false,
            256,
        );
        assert!(crate::context::payload::json_tokens(&wire).unwrap() <= 256);
        let body = data(&wire);
        let piece = body["fragment"].as_str().unwrap();
        assert!(!piece.is_empty());
        restored.push_str(piece);
        offset += piece.chars().count();
    }
    assert_eq!(restored, raw);
    let mut record = crate::o2::tests::proposal(crate::o2::tests::root().path(), None);
    record.body = raw;
    let wire = budget::bound(
        &id,
        json!({"records":[{"memory":{"id":"record","proposal":record},"checksum":"hash"}],"cursor":0}),
        false,
        256,
    );
    assert!(crate::context::payload::json_tokens(&wire).unwrap() <= 256);
    assert_eq!(data(&wire)["references"][0]["id"], "record");
}
#[path = "regressions.rs"]
mod regressions;

#[path = "progressive_tests.rs"]
mod progressive;
