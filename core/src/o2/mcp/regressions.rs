use super::super::*;
use super::fixture::*;
#[test]
fn capabilities_expire_and_policy_disables_live_servers() {
    let (t, key, c) = fixture();
    let r = t.path();
    let mut server = initialized(r, c.clone());
    let p = policy::inspect(r).unwrap();
    let mut v = p.policy;
    v.memory.agent_proposals_allowed = false;
    policy::update(r, &p.revision, Some(v)).unwrap();
    assert_eq!(
        call(&mut server, "memory.propose", proposal())["error"]["code"],
        -32602
    );
    let (db, _) = store::active_database(r).unwrap();
    db.execute(
        "UPDATE session_leases SET expires_at=0 WHERE logical_session_id=?1",
        [&key.session_id],
    )
    .unwrap();
    assert_eq!(
        call(&mut server, "memory.search", json!({}))["error"]["code"],
        -32001
    );
    assert!(Server::new(r, c).is_err());
    let (t, _, c) = fixture();
    let r = t.path();
    let mut server = initialized(r, c);
    let p = policy::inspect(r).unwrap();
    let mut v = p.policy;
    v.memory.mcp_enabled = false;
    policy::update(r, &p.revision, Some(v)).unwrap();
    assert_eq!(
        call(&mut server, "memory.search", json!({}))["error"]["code"],
        -32001
    );
}
#[test]
fn feature_scope_is_pinned_and_foreign_project_cannot_use_capability() {
    let (t, _, c) = fixture();
    let r = t.path();
    let mut p = crate::o2::tests::proposal(r, None);
    p.scope = "feature".into();
    p.scope_ref = Some("feature-b".into());
    let m = memory::propose(r, &p).unwrap();
    let h = memory::get(
        r,
        &m.id,
        &memory::Access {
            feature: Some("feature-b".into()),
            ..Default::default()
        },
    )
    .unwrap();
    memory::review(
        r,
        &memory::Review {
            id: m.id.clone(),
            status: "active".into(),
            reason: "Checked source".into(),
            confirmed: true,
            expected_checksum: h.checksum,
            evidence: None,
        },
    )
    .unwrap();
    let mut s = initialized(r, c.clone());
    std::fs::write(
        r.join("worktree.prd.json"),
        json!({"tasks":[{"id":"A","feature_id":"feature-b"}]}).to_string(),
    )
    .unwrap();
    assert_eq!(
        call(&mut s, "memory.get", json!({"id":m.id}))["result"]["isError"],
        true
    );
    let (other, _, _) = fixture();
    assert!(Server::new(other.path(), c).is_err());
}
#[test]
fn disabled_defaults_and_proposal_controls_are_explicit() {
    let p = policy::Policy::default();
    assert!(!p.memory.mcp_enabled);
    assert!(!p.memory.phase_extraction_enabled);
    let (t, _, c) = fixture();
    let r = t.path();
    let mut s = initialized(r, c);
    for (field, value) in [
        ("body", json!("unsafe\u{001b}[31m")),
        ("tags", json!(["unsafe\u{202e}"])),
    ] {
        let mut v = proposal();
        v[field] = value;
        assert_eq!(call(&mut s, "memory.propose", v)["result"]["isError"], true);
    }
    let status = agent::status(r).unwrap().to_string();
    assert!(!status.contains("lease_token"));
    assert!(!status.contains("provider_session_id"));
}
#[test]
fn code_queries_use_existing_provider_and_never_install() {
    let (t, _, c) = fixture();
    let r = t.path();
    let mut s = initialized(r, c);
    let response = call(
        &mut s,
        "context.search_sources",
        json!({"text":"preserve_contract"}),
    );
    assert_eq!(response["result"]["isError"], false, "{response}");
    let response = call(
        &mut s,
        "code.query",
        json!({"text":"preserve_contract","worktree":"/tmp"}),
    );
    assert_eq!(response["result"]["isError"], true);
    let response = call(&mut s, "code.path", json!({"text":"preserve_contract"}));
    assert_eq!(response["result"]["isError"], false, "{response}");
    assert!(!r.join(".macc/providers/serena").exists());
}
