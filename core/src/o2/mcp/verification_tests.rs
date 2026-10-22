use super::*;

fn fixture() -> (tempfile::TempDir, ToolSpec) {
    let root = crate::o2::tests::root();
    let view = policy::inspect(root.path()).unwrap();
    let mut p = view.policy;
    p.memory.mcp_enabled = true;
    policy::update(root.path(), &view.revision, Some(p)).unwrap();
    let spec: ToolSpec = serde_json::from_value(json!({"api_version":"v1","id":"fixture","display_name":"Fixture","fields":[],"performer":{"command":"fixture","runner":"fixture","args":[],"scoped_mcp":{"path":".mcp.json","format":"json","servers_key":"mcpServers"},"session":{"enabled":true,"scope":"task_role_tool"}}})).unwrap();
    let mut plan = crate::plan::ActionPlan::new();
    spec.performer
        .as_ref()
        .unwrap()
        .scoped_mcp
        .as_ref()
        .unwrap()
        .configure(&crate::ProjectPaths::from_root(root.path()), &mut plan)
        .unwrap();
    for action in plan.actions {
        if let crate::plan::Action::WriteFile { path, content, .. } = action {
            std::fs::write(root.path().join(path), content).unwrap();
        }
    }
    (root, spec)
}

#[test]
fn control_round_trip_revokes_lease_and_never_counts_as_real_use() {
    let (t, spec) = fixture();
    let root = t.path();
    verify_with(root, &spec, |_, env, input| {
        let c = Credentials {
            session_id: env["MACC_O2_MCP_SESSION"].as_str().unwrap().into(),
            generation: env["MACC_O2_MCP_GENERATION"]
                .as_str()
                .unwrap()
                .parse()
                .unwrap(),
            token: env["MACC_O2_MCP_TOKEN"].as_str().unwrap().into(),
        };
        let mut output = vec![];
        super::super::serve(root, c, std::io::Cursor::new(input), &mut output)?;
        Ok(output)
    })
    .unwrap();
    let (db, _) = database(root).unwrap();
    assert_eq!(
        db.query_row("SELECT count(*) FROM session_leases", [], |r| r
            .get::<_, i64>(0))
            .unwrap(),
        0
    );
    assert_eq!(
        db.query_row(
            "SELECT count(*) FROM o2_audit WHERE kind='mcp_verified' AND json_extract(payload_json,'$.verified')=1",
            [],
            |r| r.get::<_, i64>(0)
        )
        .unwrap(),
        1
    );
    assert_eq!(
        db.query_row(
            "SELECT count(*) FROM o2_audit WHERE kind='mcp_activity'",
            [],
            |r| r.get::<_, i64>(0)
        )
        .unwrap(),
        0
    );
}

#[test]
fn launch_or_protocol_failure_revokes_control_and_cannot_publish_success() {
    for response in [None, Some(b"{}\n".to_vec())] {
        let (t, spec) = fixture();
        assert!(verify_with(t.path(), &spec, |_, _, _| response
            .ok_or_else(|| error("fixture launch failure")))
        .is_err());
        let (db, _) = database(t.path()).unwrap();
        assert_eq!(
            db.query_row("SELECT count(*) FROM session_leases", [], |r| r
                .get::<_, i64>(0))
                .unwrap(),
            0
        );
        assert_eq!(
            db.query_row(
                "SELECT count(*) FROM o2_audit WHERE kind='mcp_verified' AND json_extract(payload_json,'$.verified')=1",
                [],
                |r| r.get::<_, i64>(0)
            )
            .unwrap(),
            0
        );
    }
}

#[test]
fn a_failed_recheck_invalidates_previous_success_and_status_stays_honest() {
    let (t, spec) = fixture();
    let root = t.path();
    let paths = crate::ProjectPaths::from_root(root);
    std::fs::create_dir_all(paths.macc_dir.join("tools.d")).unwrap();
    std::fs::write(
        paths.macc_dir.join("tools.d/fixture.tool.json"),
        serde_json::to_vec(&spec).unwrap(),
    )
    .unwrap();
    let mut config = crate::load_canonical_config(&paths.config_path).unwrap();
    config.tools.enabled = vec![spec.id.clone()];
    std::fs::write(&paths.config_path, serde_yaml::to_string(&config).unwrap()).unwrap();
    verify(root, &spec).unwrap();
    let status = super::super::status::tool_states(root).unwrap();
    assert_eq!(status[0]["configured"], true);
    assert_eq!(status[0]["connection_verified"], true);
    assert_eq!(status[0]["available_in_run"], false);
    assert_eq!(status[0]["consulted"], false);
    assert!(verify_with(root, &spec, |_, _, _| Err(error("fixture timeout"))).is_err());
    let status = super::super::status::tool_states(root).unwrap();
    assert_eq!(status[0]["configured"], true);
    assert_eq!(status[0]["connection_verified"], false);
}
