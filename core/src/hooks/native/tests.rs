use super::*;
use crate::hooks::tests::root;
use serde_json::json;
fn spec(protocol: Protocol) -> Spec {
    Spec {
        tool: "codex".into(),
        config: ".codex/hooks.json".into(),
        launcher: ".codex/hooks/macc-output.sh".into(),
        protocol,
    }
}
#[test]
fn native_registration_preserves_other_handlers_and_is_idempotent() {
    let (_tmp, paths) = root();
    let spec = spec(Protocol::Feedback);
    let original = json!({"permissions":{"allow":["Read"]},"hooks":{"PostToolUse":[{"matcher":"Bash","hooks":[{"type":"command","command":"user-owned"}]}],"Stop":[{"hooks":[{"type":"command","command":"stop-owned"}]}]}});
    let added = deployment::document(original.clone(), &spec, &paths.root, true).unwrap();
    assert_eq!(added["hooks"]["PostToolUse"].as_array().unwrap().len(), 2);
    assert_eq!(
        deployment::document(added.clone(), &spec, &paths.root, true).unwrap(),
        added
    );
    let removed = deployment::document(added, &spec, &paths.root, false).unwrap();
    assert_eq!(removed["permissions"], original["permissions"]);
    assert_eq!(
        removed["hooks"]["PostToolUse"],
        original["hooks"]["PostToolUse"]
    );
    assert_eq!(removed["hooks"]["Stop"], original["hooks"]["Stop"]);
}
#[test]
fn malformed_configuration_is_rejected() {
    let spec = spec(Protocol::Feedback);
    for value in [
        json!([]),
        json!({"hooks":[]}),
        json!({"hooks":{"PostToolUse":{}}}),
    ] {
        assert!(deployment::document(value, &spec, std::path::Path::new("/tmp"), true).is_err());
    }
}
fn event(stdout: &str) -> serde_json::Value {
    json!({"hook_event_name":"PostToolUse","tool_name":"Bash","tool_input":{"command":"# macc-hook: log-grep-error-first\ncat fixture.log"},"tool_response":{"stdout":stdout,"stderr":"","exit_code":0,"interrupted":false,"isImage":false}})
}
#[test]
fn native_dispatch_preserves_failures_unknown_contracts_and_protocols() {
    let (_tmp, paths) = root();
    let spec = spec(Protocol::Feedback);
    for raw in [
        "MACC_TASK_RESULT: success\nnoise",
        "{\"jsonrpc\":\"2.0\",\"result\":42}",
        "{\n  \"jsonrpc\": \"2.0\",\n  \"result\": 42\n}",
    ] {
        assert_eq!(
            dispatch(&paths.root, &spec, &event(raw)).unwrap(),
            json!({})
        );
    }
    let mut input = event("noise\nerror: required evidence");
    input["tool_response"]["exit_code"] = json!(1);
    assert_eq!(dispatch(&paths.root, &spec, &input).unwrap(), json!({}));
    input["tool_response"]
        .as_object_mut()
        .unwrap()
        .remove("exit_code");
    assert_eq!(dispatch(&paths.root, &spec, &input).unwrap(), json!({}));
    input["tool_input"]["command"] = json!("cat fixture.log");
    assert_eq!(dispatch(&paths.root, &spec, &input).unwrap(), json!({}));
}
#[test]
fn both_protocols_keep_raw_evidence_and_preserve_typed_fields() {
    for protocol in [Protocol::Replacement, Protocol::Feedback] {
        let (_tmp, paths) = root();
        let raw = format!("{}\nerror: retain this", "noise\n".repeat(1000));
        let input = event(&raw);
        let output = dispatch(&paths.root, &spec(protocol), &input).unwrap();
        match protocol {
            Protocol::Replacement => {
                let response = &output["hookSpecificOutput"]["updatedToolOutput"];
                assert_eq!(response["exit_code"], 0);
                assert_eq!(response["interrupted"], false);
                assert!(response["stdout"]
                    .as_str()
                    .unwrap()
                    .contains("error: retain this"));
            }
            Protocol::Feedback => {
                assert_eq!(output["continue"], false);
                assert!(output["stopReason"]
                    .as_str()
                    .unwrap()
                    .contains("error: retain this"));
            }
        }
        let artifacts: Vec<_> = std::fs::read_dir(paths.root.join(".macc/log/hooks"))
            .unwrap()
            .map(|f| f.unwrap().path())
            .collect();
        assert_eq!(artifacts.len(), 1);
        assert_eq!(
            serde_json::from_slice::<serde_json::Value>(&std::fs::read(&artifacts[0]).unwrap())
                .unwrap(),
            input
        );
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(
                std::fs::metadata(&artifacts[0])
                    .unwrap()
                    .permissions()
                    .mode()
                    & 0o777,
                0o600
            );
        }
    }
}
#[test]
fn session_advertises_contracts_and_does_not_run_user_command() {
    let (_tmp, paths) = root();
    let input =
        json!({"hook_event_name":"SessionStart","tool_input":{"command":"touch SHOULD_NOT_EXIST"}});
    let output = dispatch(&paths.root, &spec(Protocol::Feedback), &input).unwrap();
    assert!(output["hookSpecificOutput"]["additionalContext"]
        .as_str()
        .unwrap()
        .contains("# macc-hook:"));
    assert!(!paths.root.join("SHOULD_NOT_EXIST").exists());
}
#[test]
fn planning_preserves_existing_settings_and_registers_only_selected_tools() {
    let (_tmp, paths) = root();
    let mut config = crate::load_canonical_config(&paths.config_path).unwrap();
    config.tools.enabled = vec!["codex".into()];
    crate::save_canonical_config(&paths, &config).unwrap();
    let spec = spec(Protocol::Feedback);
    std::fs::create_dir_all(paths.root.join(".codex")).unwrap();
    std::fs::write(
        paths.root.join(&spec.config),
        r#"{"custom":true,"hooks":{"Stop":[{"hooks":[{"command":"user"}]}]}}"#,
    )
    .unwrap();
    let mut plan = crate::plan::ActionPlan::new();
    super::plan(&paths, &mut plan, &spec).unwrap();
    let ops = crate::plan::collect_plan_operations(&paths, &plan);
    let content = ops
        .iter()
        .find(|o| o.path == spec.config)
        .unwrap()
        .after
        .as_ref()
        .unwrap();
    let value: serde_json::Value = serde_json::from_slice(content).unwrap();
    assert_eq!(value["custom"], true);
    assert_eq!(value["hooks"]["Stop"][0]["hooks"][0]["command"], "user");
    assert!(value["hooks"]["PostToolUse"].is_array());
}

#[test]
fn inherited_managed_registration_rebinds_to_worker_without_changing_user_hooks() {
    let spec = spec(Protocol::Feedback);
    let original =
        deployment::document(json!({}), &spec, std::path::Path::new("/main"), true).unwrap();
    let rebound =
        deployment::document(original, &spec, std::path::Path::new("/worker"), true).unwrap();
    assert_eq!(rebound["hooks"]["PostToolUse"].as_array().unwrap().len(), 1);
    assert_eq!(
        rebound["hooks"]["PostToolUse"][0]["hooks"][0]["command"],
        "sh '/worker/.codex/hooks/macc-output.sh'"
    );
}
#[test]
fn claude_successful_bash_shape_without_exit_code_is_supported() {
    let (_tmp, paths) = root();
    let mut input = event(&format!("{}\nerror: evidence", "noise\n".repeat(1000)));
    input["tool_response"]
        .as_object_mut()
        .unwrap()
        .remove("exit_code");
    let spec = spec(Protocol::Replacement);
    assert!(dispatch(&paths.root, &spec, &input).unwrap()["hookSpecificOutput"].is_object());
    input["tool_response"]["is_error"] = json!(true);
    assert_eq!(dispatch(&paths.root, &spec, &input).unwrap(), json!({}));
}

#[test]
fn inactive_adapter_does_not_touch_unmanaged_or_malformed_configuration() {
    let (_tmp, paths) = root();
    let mut config = crate::load_canonical_config(&paths.config_path).unwrap();
    config.tools.enabled = vec![];
    crate::save_canonical_config(&paths, &config).unwrap();
    std::fs::create_dir_all(paths.root.join(".codex")).unwrap();
    std::fs::write(
        paths.root.join(".codex/hooks.json"),
        "user-owned invalid JSON",
    )
    .unwrap();
    let mut plan = crate::plan::ActionPlan::new();
    super::plan(&paths, &mut plan, &spec(Protocol::Feedback)).unwrap();
    assert!(plan.actions.is_empty());
}
