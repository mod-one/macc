use super::*;
use crate::plan::ActionPlan;
use std::process::Command;

fn fixture(native: bool) -> (tempfile::TempDir, ProjectPaths) {
    let temp = tempfile::tempdir().unwrap();
    let paths = ProjectPaths::from_root(temp.path());
    std::fs::create_dir_all(&paths.macc_dir).unwrap();
    std::fs::write(&paths.config_path,format!("tools:\n  enabled: []\nmemory:\n  enabled: true\ncode_intelligence:\n  enabled: {native}\n  native_map_enabled: {native}\n")).unwrap();
    std::fs::write(paths.root.join(".gitignore"), ".macc/\n").unwrap();
    std::fs::write(
        paths.root.join("contract.rs"),
        "pub fn current_contract() {}\n",
    )
    .unwrap();
    for args in [
        vec!["init", "-b", "main"],
        vec!["config", "user.name", "Test"],
        vec!["config", "user.email", "test@example.invalid"],
        vec!["add", "."],
        vec!["commit", "-qm", "fixture"],
    ] {
        assert!(Command::new("git")
            .arg("-C")
            .arg(&paths.root)
            .args(args)
            .output()
            .unwrap()
            .status
            .success());
    }
    (temp, paths)
}
fn plan(paths: &ProjectPaths) -> ActionPlan {
    let config = crate::load_canonical_config(&paths.config_path).unwrap();
    crate::build_plan(
        paths,
        &crate::resolve::resolve(&config, &Default::default()),
        &[],
        &crate::ToolRegistry::new(),
    )
    .unwrap()
}
#[test]
fn disabled_configuration_has_no_preparation() {
    let tmp = tempfile::tempdir().unwrap();
    let paths = ProjectPaths::from_root(tmp.path());
    std::fs::create_dir_all(&paths.macc_dir).unwrap();
    std::fs::write(&paths.config_path, "tools:\n  enabled: []\n").unwrap();
    assert!(preview(&paths).unwrap().is_none());
}

#[test]
fn apply_prepares_native_map_and_run_refreshes_current_sources_without_extra_commands() {
    let (_temp, paths) = fixture(true);
    let mut reviewed = plan(&paths);
    assert!(!paths.macc_dir.join("memory.db").exists());
    assert!(reviewed
        .preparation
        .as_ref()
        .unwrap()
        .steps
        .iter()
        .any(|s| s.contains("native")));
    crate::apply_plan(&paths, &mut reviewed, false).unwrap();
    assert_eq!(status(&paths.root).unwrap().unwrap().status, "ready");
    let original = crate::o2::providers::inspect(&paths.root)
        .unwrap()
        .receipts
        .into_iter()
        .find(|r| r.provider_id == "native")
        .unwrap();
    assert_eq!(original.state, "healthy");
    assert!(original.metadata.as_ref().unwrap()["native_map"] == true);
    let mut second = plan(&paths);
    crate::apply_plan(&paths, &mut second, false).unwrap();
    let unchanged = crate::o2::providers::inspect(&paths.root)
        .unwrap()
        .receipts
        .into_iter()
        .find(|r| r.provider_id == "native")
        .unwrap();
    assert_eq!(unchanged.index, original.index);
    std::fs::write(
        paths.root.join("contract.rs"),
        "pub fn repaired_contract() {}\n",
    )
    .unwrap();
    require_ready(&paths.root).unwrap();
    let refreshed = crate::o2::providers::inspect(&paths.root)
        .unwrap()
        .receipts
        .into_iter()
        .find(|r| r.provider_id == "native")
        .unwrap();
    assert_ne!(refreshed.index, original.index);
    assert_eq!(
        std::fs::read_to_string(paths.root.join("contract.rs")).unwrap(),
        "pub fn repaired_contract() {}\n"
    );
    let query = crate::o2::providers::Query {
        text: "repaired_contract".into(),
        path: None,
        worktree: None,
        kind: "symbol_definition".into(),
    };
    let crate::o2::providers::Capability::Supported(result) =
        crate::o2::providers::query(&paths.root, &query).unwrap()
    else {
        panic!()
    };
    assert!(!result.hits.is_empty());
}

#[test]
fn missing_semantic_evidence_opens_a_question_and_never_reports_ready() {
    let (_temp, paths) = fixture(false);
    let mut config = crate::load_canonical_config(&paths.config_path).unwrap();
    config.memory.as_mut().unwrap().semantic_enabled = true;
    std::fs::write(&paths.config_path, serde_yaml::to_string(&config).unwrap()).unwrap();
    let mut reviewed = plan(&paths);
    assert!(crate::apply_plan(&paths, &mut reviewed, false).is_err());
    let report = status(&paths.root).unwrap().unwrap();
    assert_eq!(report.status, "waiting_for_input");
    assert_eq!(report.question.unwrap().kind, "semantic_suite");
    assert!(require_ready(&paths.root).is_err());
}

#[test]
fn changed_preparation_is_rejected_before_generated_files_are_written() {
    let (_temp, paths) = fixture(false);
    let mut reviewed = plan(&paths);
    reviewed.add_action(crate::plan::Action::WriteFile {
        path: "must-not-write".into(),
        content: vec![1],
        scope: crate::plan::Scope::Project,
    });
    let mut config = crate::load_canonical_config(&paths.config_path).unwrap();
    config.memory.as_mut().unwrap().enabled = false;
    std::fs::write(&paths.config_path, serde_yaml::to_string(&config).unwrap()).unwrap();
    assert!(crate::apply_plan(&paths, &mut reviewed, false).is_err());
    assert!(!paths.root.join("must-not-write").exists());
}

#[test]
fn declarative_tool_setup_runs_account_flow_and_verifies_its_result() {
    let (_temp, paths) = fixture(false);
    let folder = paths.macc_dir.join("tools.d");
    std::fs::create_dir_all(&folder).unwrap();
    let account = paths.macc_dir.join("account-ready");
    let calls = paths.macc_dir.join("setup-calls");
    let spec = serde_json::json!({"api_version":"v1","id":"future-ai","display_name":"Future AI","fields":[],
        "performer":{"command":"/bin/sh","runner":"future","args":[],"preparation":{
            "verify":{"command":"/bin/sh","args":["-c",format!("if test -f '{}'; then printf '{{\"ready\":true}}'; else printf '{{\"ready\":false}}'; fi",account.display())]},
            "expected_json":{"/ready":true},"authentication_json":{"/ready":false},"setup":{"command":"/bin/sh","args":["-c",format!("printf x >> '{}'; touch '{}'",calls.display(),account.display())]}}}});
    std::fs::write(folder.join("future-ai.tool.json"), spec.to_string()).unwrap();
    let mut config = crate::load_canonical_config(&paths.config_path).unwrap();
    config.tools.enabled = vec!["future-ai".into()];
    std::fs::write(&paths.config_path, serde_yaml::to_string(&config).unwrap()).unwrap();
    let mut reviewed = plan(&paths);
    crate::apply_plan(&paths, &mut reviewed, false).unwrap();
    require_ready(&paths.root).unwrap();
    assert_eq!(std::fs::read_to_string(calls).unwrap(), "x");
    assert_eq!(status(&paths.root).unwrap().unwrap().status, "ready");
}

#[test]
fn scoped_mcp_is_registered_and_usable_by_the_next_leased_runner() {
    let (_temp, paths) = fixture(false);
    let folder = paths.macc_dir.join("tools.d");
    std::fs::create_dir_all(&folder).unwrap();
    let spec = serde_json::json!({"api_version":"v1","id":"future-mcp","display_name":"Future MCP","fields":[],"performer":{
        "command":"/bin/sh","runner":"future","args":[],
        "session":{"enabled":true,"scope":"task_role_tool","create":{"command":"/bin/sh","args":["new","{session_id}"]}},
        "scoped_mcp":{"path":".future/mcp.json","format":"json","servers_key":"mcpServers"}}});
    std::fs::write(folder.join("future-mcp.tool.json"), spec.to_string()).unwrap();
    let mut config = crate::load_canonical_config(&paths.config_path).unwrap();
    config.tools.enabled = vec!["future-mcp".into()];
    config.memory.as_mut().unwrap().mcp_enabled = true;
    std::fs::write(&paths.config_path, serde_yaml::to_string(&config).unwrap()).unwrap();
    let mut reviewed = plan(&paths);
    crate::apply_plan(&paths, &mut reviewed, false).unwrap();
    require_ready(&paths.root).unwrap();
    let registration = std::fs::read_to_string(paths.root.join(".future/mcp.json")).unwrap();
    assert!(registration.contains("macc-o2"));
    assert!(!registration.contains("lease_token"));
    std::fs::write(
        paths.root.join("worktree.prd.json"),
        r#"{"tasks":[{"id":"CHECK","acceptance_criteria":["Preserve API"]}]}"#,
    )
    .unwrap();
    let identity = crate::coordinator::session_identity::identity(
        &paths.root,
        &paths.root,
        "CHECK",
        "performer",
        "future-mcp",
        &spec,
        &crate::coordinator::session_identity::SessionRouting {
            tier: String::new(),
            mode: "auto".into(),
        },
    )
    .unwrap();
    let checkpoint = crate::session_store::checkpoints::prepare(
        &paths.root,
        &crate::session_store::checkpoints::ContextRequest {
            identity: identity.clone(),
            phase: "dev".into(),
        },
    )
    .unwrap();
    let grant = crate::session_store::acquire(
        &paths.root,
        &crate::session_store::AcquireRequest {
            identity,
            request_id: uuid::Uuid::new_v4().to_string(),
            owner_pid: std::process::id() as i64,
            preferred_provider_id: None,
            generate_provider_id: true,
            checkpoint_id: checkpoint.checkpoint_id,
        },
    )
    .unwrap();
    let env = crate::o2::agent::options(&paths.root, &grant.key).unwrap()["env"].clone();
    let credentials = crate::o2::agent::Credentials {
        session_id: grant.key.session_id.clone(),
        generation: grant.key.generation,
        token: env["MACC_O2_MCP_TOKEN"].as_str().unwrap().into(),
    };
    let mut server = crate::o2::mcp::Server::new(&paths.root, credentials).unwrap();
    assert!(server.handle(serde_json::json!({"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-11-25","capabilities":{},"clientInfo":{"name":"test","version":"1"}}})).unwrap()["result"].is_object());
    server.handle(serde_json::json!({"jsonrpc":"2.0","method":"notifications/initialized"}));
    assert!(!server
        .handle(serde_json::json!({"jsonrpc":"2.0","id":2,"method":"tools/list"}))
        .unwrap()["result"]["tools"]
        .as_array()
        .unwrap()
        .is_empty());
    crate::session_store::release(&paths.root, &grant.key, true).unwrap();
    assert!(server
        .handle(serde_json::json!({"jsonrpc":"2.0","id":3,"method":"tools/list"}))
        .unwrap()["error"]
        .is_object());
}

#[test]
fn offline_setup_requires_explicit_revision_checked_permission_inside_apply() {
    let (_temp, paths) = fixture(false);
    let folder = paths.macc_dir.join("tools.d");
    std::fs::create_dir_all(&folder).unwrap();
    let binary = paths.macc_dir.join("missing-tool");
    let spec = serde_json::json!({"api_version":"v1","id":"future-install","display_name":"Future install","fields":[],
        "performer":{"command":binary,"runner":"future","args":[]},"version_check":{"current":{"command":binary,"args":[]}},"install":{"commands":[{"command":"/bin/sh","args":["-c",format!("printf '#!/bin/sh\nexit 0\n' > '{}'; chmod +x '{}'",binary.display(),binary.display())]}]}});
    std::fs::write(folder.join("future-install.tool.json"), spec.to_string()).unwrap();
    let mut config = crate::load_canonical_config(&paths.config_path).unwrap();
    config.tools.enabled = vec!["future-install".into()];
    config.settings.offline = true;
    std::fs::write(&paths.config_path, serde_yaml::to_string(&config).unwrap()).unwrap();
    let mut reviewed = plan(&paths);
    assert!(crate::apply_plan(&paths, &mut reviewed, false).is_err());
    let report = status(&paths.root).unwrap().unwrap();
    assert_eq!(report.question.unwrap().kind, "network_access");
    assert!(allow_network(&paths.root, &report.revision, false).is_err());
    assert!(allow_network(&paths.root, "stale", true).is_err());
    allow_network(&paths.root, &report.revision, true).unwrap();
    let mut reviewed = plan(&paths);
    crate::apply_plan(&paths, &mut reviewed, false).unwrap();
    assert!(binary.exists());
    assert_eq!(status(&paths.root).unwrap().unwrap().status, "ready");
}

#[test]
fn disabling_mcp_removes_only_owned_registration_and_keeps_a_backup() {
    let (_temp, paths) = fixture(false);
    let folder = paths.macc_dir.join("tools.d");
    std::fs::create_dir_all(&folder).unwrap();
    let spec = serde_json::json!({"api_version":"v1","id":"future-cleanup","display_name":"Future cleanup","fields":[],"performer":{
        "command":"/bin/sh","runner":"future","args":[],
        "scoped_mcp":{"path":".future/mcp.json","format":"json","servers_key":"mcpServers"}}});
    std::fs::write(folder.join("future-cleanup.tool.json"), spec.to_string()).unwrap();
    let mut config = crate::load_canonical_config(&paths.config_path).unwrap();
    config.tools.enabled = vec!["future-cleanup".into()];
    config.memory.as_mut().unwrap().mcp_enabled = true;
    std::fs::write(&paths.config_path, serde_yaml::to_string(&config).unwrap()).unwrap();
    crate::apply_plan(&paths, &mut plan(&paths), false).unwrap();
    let path = paths.root.join(".future/mcp.json");
    let mut registration: serde_json::Value =
        serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
    registration["mcpServers"]["other"] = serde_json::json!({"command":"other-server"});
    std::fs::write(&path, serde_json::to_vec(&registration).unwrap()).unwrap();
    config.memory.as_mut().unwrap().mcp_enabled = false;
    std::fs::write(&paths.config_path, serde_yaml::to_string(&config).unwrap()).unwrap();
    let mut reviewed = plan(&paths);
    assert_eq!(reviewed.preparation.as_ref().unwrap().mcp_cleanup.len(), 1);
    crate::apply_plan(&paths, &mut reviewed, false).unwrap();
    let after: serde_json::Value = serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
    assert!(after["mcpServers"]["macc-o2"].is_null());
    assert_eq!(
        after["mcpServers"]["other"],
        registration["mcpServers"]["other"]
    );
    let backed_up = std::fs::read_dir(&paths.backups_dir)
        .unwrap()
        .flatten()
        .any(|entry| {
            let copy = entry.path().join(".future/mcp.json");
            copy.exists()
                && serde_json::from_slice::<serde_json::Value>(&std::fs::read(copy).unwrap())
                    .unwrap()
                    == registration
        });
    assert!(backed_up);
    assert!(plan(&paths).preparation.unwrap().mcp_cleanup.is_empty());
}

#[test]
fn concurrent_apply_cannot_write_while_preparation_is_owned() {
    let (_temp, paths) = fixture(false);
    let guard = lock(&paths).unwrap();
    let mut reviewed = plan(&paths);
    reviewed.add_action(crate::plan::Action::WriteFile {
        path: "concurrent.txt".into(),
        content: vec![1],
        scope: crate::plan::Scope::Project,
    });
    assert!(crate::apply_plan(&paths, &mut reviewed, false).is_err());
    assert!(!paths.root.join("concurrent.txt").exists());
    drop(guard);
    crate::apply_plan(&paths, &mut reviewed, false).unwrap();
    assert!(paths.root.join("concurrent.txt").exists());
}

#[test]
fn parse_failures_are_visible_in_apply_and_its_saved_result() {
    let (_temp, paths) = fixture(true);
    std::fs::write(paths.root.join("contract.rs"), "pub fn broken( {").unwrap();
    let result = crate::apply_plan(&paths, &mut plan(&paths), false).unwrap();
    assert!(result
        .warnings
        .iter()
        .any(|w| w.contains("could not be parsed")));
    assert!(!status(&paths.root).unwrap().unwrap().warnings.is_empty());
    let details: serde_json::Value =
        serde_json::from_slice(&std::fs::read(result.details_path.unwrap()).unwrap()).unwrap();
    assert_eq!(details["status"], "completed_with_warnings");
    assert!(details["warnings"].as_array().unwrap().len() > 0);
}

#[test]
fn reviewed_quality_suggestion_rebuilds_a_partial_receipt_and_preserves_explicit_budgets() {
    let (_temp, paths) = fixture(true);
    crate::apply_plan(&paths, &mut plan(&paths), false).unwrap();
    let mut config = crate::load_canonical_config(&paths.config_path).unwrap();
    config
        .code_intelligence
        .as_mut()
        .unwrap()
        .native_build_timeout_ms = 5000;
    std::fs::write(&paths.config_path, serde_yaml::to_string(&config).unwrap()).unwrap();
    let (db, _) = crate::o2::database(&paths.root).unwrap();
    let raw: String = db
        .query_row(
            "SELECT receipt_json FROM provider_receipts WHERE provider_id='native'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    let mut receipt: serde_json::Value = serde_json::from_str(&raw).unwrap();
    receipt["metadata"]["truncated"] = serde_json::json!(true);
    receipt["metadata"]["stats"]["elapsed_ms"] = serde_json::json!(5020);
    db.execute(
        "UPDATE provider_receipts SET receipt_json=?1 WHERE provider_id='native'",
        [receipt.to_string()],
    )
    .unwrap();
    drop(db);
    // Simulate the recorded partial index decision without depending on CPU speed.
    let mut partial = status(&paths.root).unwrap().unwrap();
    partial.revision = crate::config::transaction::revision(&config).unwrap();
    partial.question = Some(Question {
        kind: "provider_quality".into(),
        message: "Increase budget".into(),
        data: serde_json::json!({"path":"code_intelligence.native_build_timeout_ms","value":"15000"}),
    });
    assert!(improve_quality(&paths.root, &partial, false).is_err());
    assert_eq!(
        crate::load_canonical_config(&paths.config_path)
            .unwrap()
            .code_intelligence
            .unwrap()
            .native_build_timeout_ms,
        5000
    );
    improve_quality(&paths.root, &partial, true).unwrap();
    assert!(
        improve_quality(&paths.root, &partial, true).is_err(),
        "stale suggestions cannot overwrite a newer config"
    );
    let result = crate::apply_plan(&paths, &mut plan(&paths), false).unwrap();
    assert!(result.warnings.is_empty());
    let after = crate::o2::providers::inspect(&paths.root)
        .unwrap()
        .receipts
        .into_iter()
        .find(|r| r.provider_id == "native")
        .unwrap();
    assert_ne!(after.index.as_deref(), receipt["index"].as_str());
}

#[test]
fn startup_budget_includes_declared_checks_and_unknown_failure_does_not_login() {
    let (_temp, paths) = fixture(false);
    let folder = paths.macc_dir.join("tools.d");
    std::fs::create_dir_all(&folder).unwrap();
    let spec = serde_json::json!({"api_version":"v1","id":"future-ai","display_name":"Future AI","fields":[],
        "performer":{"command":"/bin/sh","runner":"future","args":[],"preparation":{
            "timeout_seconds":41,
            "verify":{"command":"/bin/sh","args":["-c","exit 17"]},
            "setup":{"command":"/bin/sh","args":["-c","touch must-not-login"]}}}});
    std::fs::write(folder.join("future-ai.tool.json"), spec.to_string()).unwrap();
    let mut config = crate::load_canonical_config(&paths.config_path).unwrap();
    config.tools.enabled = vec!["future-ai".into()];
    std::fs::write(&paths.config_path, serde_yaml::to_string(&config).unwrap()).unwrap();
    assert_eq!(startup_timeout(&paths.root).unwrap().as_secs(), 71);
    let error = crate::apply_plan(&paths, &mut plan(&paths), false)
        .unwrap_err()
        .to_string();
    assert!(error.contains("Exit"));
    assert!(error.contains("17"));
    assert!(error.contains("evidence:"));
    assert!(!paths.root.join("must-not-login").exists());
    assert!(!error.contains("requires account setup"));
}
