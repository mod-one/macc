//! Real registered-server process and public Apply, without native AI clients.
use serde_json::{json, Value};
use std::{
    io::Write,
    path::Path,
    process::{Command, Stdio},
};

fn cli(root: &Path, args: &[&str]) -> Value {
    let out = Command::new(env!("CARGO_BIN_EXE_macc"))
        .arg("--cwd")
        .arg(root)
        .args(args)
        .stdin(Stdio::null())
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "{}\n{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    serde_json::from_slice(&out.stdout).unwrap()
}

#[test]
fn public_apply_verifies_real_stdio_and_run_observations_are_distinct() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path();
    std::fs::create_dir_all(root.join(".macc/tools.d")).unwrap();
    std::fs::create_dir_all(root.join(".macc/catalog")).unwrap();
    std::fs::write(root.join(".macc/macc.yaml"),"tools:\n  enabled: [mcp-fixture]\nmemory:\n  enabled: true\n  mcp_enabled: true\ncode_intelligence:\n  enabled: false\n").unwrap();
    let spec = json!({"api_version":"v1","id":"mcp-fixture","display_name":"MCP Fixture","fields":[],"performer":{"command":"/bin/sh","runner":"fixture","args":[],"session":{"enabled":true,"scope":"task_role_tool"},"scoped_mcp":{"path":".fixture/mcp.json","format":"json","servers_key":"mcpServers"}}});
    std::fs::write(
        root.join(".macc/tools.d/mcp-fixture.tool.json"),
        spec.to_string(),
    )
    .unwrap();
    let mut catalog: Value =
        serde_json::from_str(include_str!("../../catalog/skills.catalog.json")).unwrap();
    for entry in catalog["entries"].as_array_mut().unwrap() {
        entry["mandatory"] = json!(false);
    }
    std::fs::write(
        root.join(".macc/catalog/skills.catalog.json"),
        catalog.to_string(),
    )
    .unwrap();
    for args in [
        vec!["init", "-b", "main"],
        vec![
            "-c",
            "user.name=Test",
            "-c",
            "user.email=test@example.invalid",
            "commit",
            "--allow-empty",
            "-qm",
            "fixture",
        ],
    ] {
        assert!(
            macc_core::git::run_git_output_mapped(root, &args, "Create MCP test repository")
                .unwrap()
                .status
                .success()
        );
    }
    cli(root, &["apply", "--yes", "--json"]);
    let status = cli(root, &["o2", "agent-status"]);
    let row = &status["tool_states"][0];
    assert_eq!(row["configured"], true);
    assert_eq!(row["connection_verified"], true);
    assert_eq!(row["available_in_run"], false);
    assert_eq!(row["consulted"], false);
    assert!(macc_core::session_store::inspect(root)
        .unwrap()
        .sessions
        .iter()
        .all(|s| s.status != "active"));
    let identity = macc_core::coordinator::session_identity::identity(
        root,
        root,
        "A",
        "performer",
        "mcp-fixture",
        &spec,
        &macc_core::coordinator::session_identity::SessionRouting {
            tier: String::new(),
            mode: "auto".into(),
        },
    )
    .unwrap();
    let grant = macc_core::session_store::acquire(
        root,
        &macc_core::session_store::AcquireRequest {
            identity,
            request_id: "run-observation".into(),
            owner_pid: std::process::id() as i64,
            preferred_provider_id: None,
            generate_provider_id: false,
            checkpoint_id: None,
        },
    )
    .unwrap();
    let env = macc_core::o2::agent::options(root, &grant.key).unwrap()["env"].clone();
    let mut command = Command::new(env!("CARGO_BIN_EXE_macc"));
    command
        .arg("o2")
        .arg("mcp")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    for (key, value) in env.as_object().unwrap() {
        command.env(key, value.as_str().unwrap());
    }
    let mut child = command.spawn().unwrap();
    let requests = [
        json!({"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-11-25","capabilities":{},"clientInfo":{"name":"fixture","version":"1"}}}),
        json!({"jsonrpc":"2.0","method":"notifications/initialized"}),
        json!({"jsonrpc":"2.0","id":2,"method":"tools/list"}),
        json!({"jsonrpc":"2.0","id":3,"method":"tools/call","params":{"name":"memory.search","arguments":{}}}),
    ];
    child
        .stdin
        .take()
        .unwrap()
        .write_all(
            requests
                .iter()
                .map(|r| format!("{r}\n"))
                .collect::<String>()
                .as_bytes(),
        )
        .unwrap();
    let out = child.wait_with_output().unwrap();
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let status = cli(root, &["o2", "agent-status"]);
    assert_eq!(status["tool_states"][0]["available_in_run"], true);
    assert_eq!(status["tool_states"][0]["connected_in_run"], true);
    assert_eq!(status["tool_states"][0]["consulted"], true);
    assert!(!status
        .to_string()
        .contains(env["MACC_O2_MCP_TOKEN"].as_str().unwrap()));
    macc_core::session_store::release(root, &grant.key, false).unwrap();
    assert_eq!(
        cli(root, &["o2", "agent-status"])["tool_states"][0]["available_in_run"],
        false
    );
}
