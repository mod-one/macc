use super::*;
pub(super) fn fixture() -> (tempfile::TempDir, store::LeaseKey, Credentials) {
    let t = crate::o2::tests::root();
    let r = t.path();
    std::fs::write(r.join("worktree.prd.json"),json!({"tasks":[{"id":"A","feature_id":"feature-a","acceptance_criteria":["Keep API"]},{"id":"B","feature_id":"feature-b","acceptance_criteria":["Isolated"]}]}).to_string()).unwrap();
    let p = policy::inspect(r).unwrap();
    let mut v = p.policy;
    v.memory.mcp_enabled = true;
    v.memory.phase_extraction_enabled = true;
    policy::update(r, &p.revision, Some(v)).unwrap();
    let spec = json!({"id":"fixture","performer":{"command":"fake","session":{"enabled":true,"scope":"task_role_tool","create":{"command":"fake","args":["new","{session_id}"]}}}});
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
    let cp = store::checkpoints::prepare(
        r,
        &store::checkpoints::ContextRequest {
            identity: identity.clone(),
            phase: "dev".into(),
        },
    )
    .unwrap();
    let lease = store::acquire(
        r,
        &store::AcquireRequest {
            identity,
            request_id: uuid::Uuid::new_v4().to_string(),
            owner_pid: std::process::id() as i64,
            preferred_provider_id: None,
            generate_provider_id: true,
            checkpoint_id: cp.checkpoint_id,
        },
    )
    .unwrap();
    let env = agent::options(r, &lease.key).unwrap()["env"].clone();
    let c = Credentials {
        session_id: env["MACC_O2_MCP_SESSION"].as_str().unwrap().into(),
        generation: env["MACC_O2_MCP_GENERATION"]
            .as_str()
            .unwrap()
            .parse()
            .unwrap(),
        token: env["MACC_O2_MCP_TOKEN"].as_str().unwrap().into(),
    };
    assert_ne!(c.token, lease.key.lease_token);
    (t, lease.key, c)
}
pub(super) fn initialized(root: &Path, c: Credentials) -> Server {
    let mut s = Server::new(root, c).unwrap();
    assert!(s.handle(json!({"jsonrpc":"2.0","id":0,"method":"initialize","params":{"protocolVersion":"2025-11-25","capabilities":{},"clientInfo":{"name":"fixture","version":"1"}}})).unwrap()["result"].is_object());
    assert!(s
        .handle(json!({"jsonrpc":"2.0","method":"notifications/initialized"}))
        .is_none());
    s
}
pub(super) fn call(s: &mut Server, name: &str, args: Value) -> Value {
    let wire = s.handle(json!({"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":name,"arguments":args}})).unwrap();
    s.acknowledge().unwrap();
    wire
}
pub(super) fn data(wire: &Value) -> Value {
    serde_json::from_str(wire["result"]["content"][0]["text"].as_str().unwrap()).unwrap()
}
pub(super) fn proposal() -> Value {
    json!({"request_id":"one","kind":"constraint","title":"Preserve API","body":"Preserve the documented public contract","confidence":0.9,"evidence_paths":["contract.rs"]})
}
