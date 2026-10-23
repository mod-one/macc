//! Real public CLI lifecycle; no model calls or network access.
use serde_json::{json, Value};
use std::{
    path::Path,
    process::{Command, Output},
};
fn cli(root: &Path, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_macc"))
        .arg("--cwd")
        .arg(root)
        .args(args)
        .output()
        .unwrap()
}
fn value(out: Output) -> Value {
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    serde_json::from_slice(&out.stdout).unwrap()
}
#[test]
fn public_cli_manages_offline_snapshots_required_claims_and_stale_revisions() {
    let root = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(root.path().join(".macc")).unwrap();
    std::fs::write(
        root.path().join(".macc/macc.yaml"),
        "tools:\n  enabled: []\nsettings:\n  offline: true\n",
    )
    .unwrap();
    let before = value(cli(root.path(), &["hooks", "list", "--json"]));
    assert_eq!(before["hooks"].as_array().unwrap().len(), 7);
    let blocked = cli(root.path(), &["hooks", "disable", "performer-log-summary"]);
    assert!(!blocked.status.success());
    assert!(String::from_utf8_lossy(&blocked.stderr).contains("O2 context engine"));
    let manifest = root.path().join("hook.json");
    std::fs::write(&manifest,json!({"id":"external-check","name":"External check","description":"Keep warnings","version":"1.0.0","event":"tool_output","default_enabled":false,"handler":{"kind":"rules","steps":[{"kind":"retain_lines","pattern":"warning"}]},"skills":["fixture-check"],"source":{"repository":"local","subpath":"hooks/external","distribution":"snapshot"}}).to_string()).unwrap();
    let reviewed = value(cli(
        root.path(),
        &["hooks", "inspect", manifest.to_str().unwrap()],
    ));
    let installed = cli(
        root.path(),
        &[
            "hooks",
            "import",
            manifest.to_str().unwrap(),
            "--sha256",
            reviewed["sha256"].as_str().unwrap(),
        ],
    );
    assert!(
        installed.status.success(),
        "{}",
        String::from_utf8_lossy(&installed.stderr)
    );
    // Changing/deleting the original checkout does not affect the installed snapshot.
    std::fs::remove_file(&manifest).unwrap();
    assert!(cli(root.path(), &["hooks", "enable", "external-check"])
        .status
        .success());
    let current = value(cli(root.path(), &["hooks", "list", "--json"]));
    assert!(current["hooks"]
        .as_array()
        .unwrap()
        .iter()
        .any(|h| h["hook"]["id"] == "external-check" && h["enabled"] == true));
    let stale = cli(
        root.path(),
        &[
            "hooks",
            "disable",
            "external-check",
            "--revision",
            before["revision"].as_str().unwrap(),
        ],
    );
    assert!(!stale.status.success());
    assert!(String::from_utf8_lossy(&stale.stderr).contains("revision conflict"));
    assert!(cli(root.path(), &["hooks", "remove", "external-check"])
        .status
        .success());
    assert_eq!(
        value(cli(root.path(), &["hooks", "list", "--json"]))["hooks"]
            .as_array()
            .unwrap()
            .len(),
        7
    );
}

#[test]
fn native_cli_dispatch_and_status_keep_selection_separate_from_execution() {
    use std::io::Write;
    use std::process::Stdio;
    let root = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(root.path().join(".macc")).unwrap();
    std::fs::write(
        root.path().join(".macc/macc.yaml"),
        "tools:\n  enabled: [claude, codex, agy]\nsettings:\n  offline: true\n",
    )
    .unwrap();
    let initial = value(cli(root.path(), &["hooks", "list", "--json"]));
    let states = initial["native"].as_array().unwrap();
    assert_eq!(states.len(), 3);
    assert!(states
        .iter()
        .filter(|s| s["tool"] != "agy")
        .all(|s| s["supported"] == true
            && s["execution_observed"] == false
            && s["installed"] == false));
    assert!(states
        .iter()
        .any(|s| s["tool"] == "agy" && s["supported"] == false));
    let mut child = Command::new(env!("CARGO_BIN_EXE_macc"))
        .arg("--cwd")
        .arg(root.path())
        .args(["hooks", "dispatch", "--tool", "claude"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    child
        .stdin
        .take()
        .unwrap()
        .write_all(br#"{"hook_event_name":"SessionStart"}"#)
        .unwrap();
    let response = value(child.wait_with_output().unwrap());
    assert!(response["hookSpecificOutput"]["additionalContext"]
        .as_str()
        .unwrap()
        .contains("# macc-hook:"));
    // A handler invocation alone cannot claim that a native registration was installed.
    let after = value(cli(root.path(), &["hooks", "list", "--json"]));
    assert!(after["native"]
        .as_array()
        .unwrap()
        .iter()
        .all(|s| s["execution_observed"] == false));
}

#[cfg(unix)]
#[test]
fn deployed_launcher_runs_from_another_directory_and_observation_expires_on_config_change() {
    use std::io::Write;
    use std::process::Stdio;
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().join("project ' with spaces");
    std::fs::create_dir_all(root.join(".macc")).unwrap();
    std::fs::write(
        root.join(".macc/macc.yaml"),
        "tools:\n  enabled: [claude]\nsettings:\n  offline: true\n",
    )
    .unwrap();
    let paths = macc_core::ProjectPaths::from_root(&root);
    let spec = macc_core::hooks::native::Spec {
        tool: "claude".into(),
        config: ".claude/settings.json".into(),
        launcher: ".claude/hooks/macc-output.sh".into(),
        protocol: macc_core::hooks::native::Protocol::Replacement,
    };
    let mut plan = macc_core::plan::ActionPlan::new();
    macc_core::hooks::native::plan(&paths, &mut plan, &spec).unwrap();
    for action in plan.actions {
        if let macc_core::plan::Action::WriteFile { path, content, .. } = action {
            let destination = root.join(&path);
            std::fs::create_dir_all(destination.parent().unwrap()).unwrap();
            // The test executable is not MACC. Bind the fixture launcher to the
            // actual CLI, just as planning inside that binary does in production.
            let content = String::from_utf8(content).unwrap().replace(
                "exec 'macc'",
                &format!(
                    "exec '{}'",
                    env!("CARGO_BIN_EXE_macc").replace('\'', "'\\''")
                ),
            );
            std::fs::write(destination, content).unwrap();
        }
    }
    let configured = value(cli(&root, &["hooks", "list", "--json"]));
    assert_eq!(configured["native"][0]["locally_verified"], true);
    assert_eq!(configured["native"][0]["execution_observed"], false);
    let mut child = Command::new("sh")
        .arg(root.join(&spec.launcher))
        .current_dir(temp.path())
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    child
        .stdin
        .take()
        .unwrap()
        .write_all(br#"{"hook_event_name":"SessionStart","session_id":"fixture"}"#)
        .unwrap();
    let output = value(child.wait_with_output().unwrap());
    assert!(output["hookSpecificOutput"]["additionalContext"]
        .as_str()
        .unwrap()
        .contains("MACC output hooks"));
    let observed = value(cli(&root, &["hooks", "list", "--json"]));
    assert_eq!(observed["native"][0]["execution_observed"], true);
    std::fs::write(
        root.join(".macc/macc.yaml"),
        "tools:\n  enabled: [claude]\nsettings:\n  offline: false\n",
    )
    .unwrap();
    let changed = value(cli(&root, &["hooks", "list", "--json"]));
    assert_eq!(changed["native"][0]["execution_observed"], false);
}

#[test]
fn ordinary_apply_plan_deploys_selected_adapters_and_preserves_user_settings() {
    let temp = tempfile::tempdir().unwrap();
    let paths = macc_core::ProjectPaths::from_root(temp.path());
    std::fs::create_dir_all(&paths.macc_dir).unwrap();
    let mut config = macc_core::config::CanonicalConfig::default();
    config.tools.enabled = vec!["claude".into(), "codex".into()];
    config.settings.offline = true;
    macc_core::save_canonical_config(&paths, &config).unwrap();
    std::fs::create_dir_all(temp.path().join(".claude")).unwrap();
    std::fs::write(temp.path().join(".claude/settings.json"),r#"{"custom_marker":"user-owned","hooks":{"Stop":[{"hooks":[{"type":"command","command":"user-stop"}]}]}}"#).unwrap();
    let registry = macc_registry::default_registry();
    let resolved = macc_core::resolve::resolve(&config, &Default::default());
    let plan = macc_core::build_plan(&paths, &resolved, &[], &registry).unwrap();
    let writes: std::collections::BTreeMap<_, _> = plan
        .actions
        .iter()
        .filter_map(|action| match action {
            macc_core::plan::Action::WriteFile { path, content, .. } => {
                Some((path.as_str(), content))
            }
            _ => None,
        })
        .collect();
    for path in [
        ".claude/hooks/macc-output.sh",
        ".codex/hooks/macc-output.sh",
        ".codex/hooks.json",
    ] {
        assert!(
            writes.contains_key(path),
            "missing native Apply operation: {path}"
        );
    }
    let settings: Value = serde_json::from_slice(writes[".claude/settings.json"]).unwrap();
    assert_eq!(settings["custom_marker"], "user-owned");
    assert_eq!(
        settings["hooks"]["Stop"][0]["hooks"][0]["command"],
        "user-stop"
    );
    assert!(settings["hooks"]["PostToolUse"].is_array());
}
