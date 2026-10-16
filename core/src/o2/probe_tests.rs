use super::*;
use serde_json::json;
#[cfg(unix)]
pub(super) fn spec(root: &Path, format: &str) -> crate::tool::ToolPerformerSpec {
    use std::os::unix::fs::PermissionsExt;
    let executable = root.join(".fake-tool");
    std::fs::write(&executable,"#!/bin/sh\ncase \"$1\" in --version) echo 'future-cli 1.0.0';; --help) echo 'versioned help';; *) exit 42;; esac\n").unwrap();
    std::fs::set_permissions(&executable, std::fs::Permissions::from_mode(0o700)).unwrap();
    serde_json::from_value(json!({"runner":"fake.sh","command":executable,"args":["--model","keep-model","--effort","high","--yolo"],"compaction":{"schema":"fake-native-v1","documentation":"https://example.invalid/verified-fixture","version_pattern":"^future-cli 1\\.0\\.0","native_auto":{"format":format,"path":if format=="env"{None}else{Some(format!(".native/settings.{format}"))},"key":if format=="env"{"FUTURE_COMPACT_PERCENT"}else if format=="json"{"model.compactionThreshold"}else{"native_compact_tokens"},"unit":if format=="env"{"percent"}else if format=="json"{"ratio"}else{"tokens"},"disabled_env":[]}},"session":{"enabled":true,"scope":"task_role_tool","create":{"command":"fake","args":["new","{session_id}"]}}})).unwrap()
}
#[cfg(unix)]
pub(super) fn grant(root: &Path, p: &crate::tool::ToolPerformerSpec) -> store::LeaseGrant {
    let spec = json!({"id":"future-tool","performer":p});
    let identity = crate::coordinator::session_identity::identity(
        root,
        root,
        "TASK-A",
        "performer",
        "future-tool",
        &spec,
        &crate::coordinator::session_identity::SessionRouting {
            tier: "".into(),
            mode: "auto".into(),
        },
    )
    .unwrap();
    store::acquire(
        root,
        &store::AcquireRequest {
            identity,
            request_id: uuid::Uuid::new_v4().to_string(),
            owner_pid: std::process::id() as i64,
            preferred_provider_id: None,
            generate_provider_id: true,
            checkpoint_id: None,
        },
    )
    .unwrap()
}
#[cfg(unix)]
pub(super) fn enable(root: &Path) {
    let view = policy::inspect(root).unwrap();
    let mut policy = view.policy;
    policy.adaptive_compaction.enabled = true;
    policy.adaptive_compaction.configure_native_auto = true;
    policy.adaptive_compaction.native_token_threshold = 5000;
    policy::update(root, &view.revision, Some(policy)).unwrap();
}
#[test]
#[cfg(unix)]
fn declared_native_configuration_requires_probe_and_preserves_existing_settings() {
    for format in ["env", "json", "toml"] {
        let temp = tests::root();
        let r = temp.path();
        enable(r);
        let p = spec(r, format);
        let lease = grant(r, &p);
        assert!(
            !compaction::execution_options(r, &lease.key)
                .unwrap()
                .applied
        );
        let observed = compaction::probe_spec(r, "future-tool", &p).unwrap();
        assert_eq!(observed.state, "version_help_observed");
        assert_eq!(
            observed.explicit_headless,
            "unsupported; checkpoint rotation"
        );
        let options = compaction::execution_options(r, &lease.key).unwrap();
        assert!(options.applied, "{format}: {}", options.reason);
        match format {
            "env" => assert_eq!(options.env["FUTURE_COMPACT_PERCENT"], "70"),
            "json" => {
                let config = r.join(".native/settings.json");
                let mut value: serde_json::Value =
                    serde_json::from_slice(&std::fs::read(&config).unwrap()).unwrap();
                assert_eq!(value["model"]["compactionThreshold"], 0.7);
                value["model"]["compactionThreshold"] = json!(0.42);
                std::fs::write(&config, value.to_string()).unwrap();
                assert!(
                    !compaction::execution_options(r, &lease.key)
                        .unwrap()
                        .applied
                );
                let value: serde_json::Value =
                    serde_json::from_slice(&std::fs::read(&config).unwrap()).unwrap();
                assert_eq!(value["model"]["compactionThreshold"], 0.42);
            }
            _ => {
                let value: toml::Value = toml::from_str(
                    &std::fs::read_to_string(r.join(".native/settings.toml")).unwrap(),
                )
                .unwrap();
                assert_eq!(value["native_compact_tokens"].as_integer(), Some(5000));
            }
        }
        assert_eq!(
            p.args,
            vec!["--model", "keep-model", "--effort", "high", "--yolo"]
        );
        std::fs::write(r.join(".fake-tool"), "#!/bin/sh\necho changed\n").unwrap();
        assert!(
            !compaction::execution_options(r, &lease.key)
                .unwrap()
                .applied
        );
        store::release(r, &lease.key, true).unwrap();
    }
}
#[test]
#[cfg(unix)]
fn only_declared_context_gauges_drive_runtime_rotation() {
    let temp = tests::root();
    let r = temp.path();
    enable(r);
    let mut p = spec(r, "env");
    p.compaction.as_mut().unwrap().context_tokens_path = Some("/window/used".into());
    p.compaction.as_mut().unwrap().context_window_path = Some("/window/size".into());
    p.compaction.as_mut().unwrap().compaction_count_path = Some("/window/compactions".into());
    let lease = grant(r, &p);
    let (db, _) = database(r).unwrap();
    compaction::observe_pressure(&db, r, &lease.key, r#"{"usage":{"input_tokens":999999}}"#)
        .unwrap();
    let (decision, _) =
        compaction::evaluate_pressure(&db, r, &lease.key.session_id, 4, 20).unwrap();
    assert_eq!(decision.pressure, None);
    compaction::observe_pressure(
        &db,
        r,
        &lease.key,
        r#"{"window":{"used":90,"size":100,"compactions":1}}"#,
    )
    .unwrap();
    let (decision, count) =
        compaction::evaluate_pressure(&db, r, &lease.key.session_id, 4, 20).unwrap();
    assert_eq!(decision.action, "defer");
    assert_eq!(decision.pressure, Some(0.9));
    assert_eq!(count, Some(1));
    drop(db);
    store::release(r, &lease.key, true).unwrap();
    let (db, _) = database(r).unwrap();
    assert_eq!(
        compaction::evaluate_pressure(&db, r, &lease.key.session_id, 4, 20)
            .unwrap()
            .0
            .action,
        "checkpoint_rotate"
    );
}
#[test]
fn shipped_adapter_matrix_validates_without_claiming_native_execution() {
    for body in [
        include_str!("../../../registry/tools.d/codex.tool.yaml"),
        include_str!("../../../registry/tools.d/claude.tool.yaml"),
        include_str!("../../../registry/tools.d/gemini.tool.yaml"),
        include_str!("../../../registry/tools.d/vibe.tool.yaml"),
    ] {
        let spec: crate::tool::ToolSpec = serde_yaml::from_str(body).unwrap();
        spec.validate().unwrap();
        let profile = spec.performer.unwrap().compaction.unwrap();
        profile.validate().unwrap();
    }
}

#[test]
#[cfg(unix)]
fn owned_native_settings_follow_policy_changes_disable_and_preserve_user_edits() {
    for format in ["toml", "json"] {
        let temp = tests::root();
        let root = temp.path();
        enable(root);
        let p = spec(root, format);
        let lease = grant(root, &p);
        compaction::probe_spec(root, "future-tool", &p).unwrap();
        assert!(
            compaction::execution_options(root, &lease.key)
                .unwrap()
                .applied
        );
        let view = policy::inspect(root).unwrap();
        let mut policy = view.policy;
        policy.adaptive_compaction.native_token_threshold = 9000;
        policy.adaptive_compaction.action_ratio = 0.65;
        super::policy::update(root, &view.revision, Some(policy.clone())).unwrap();
        assert!(
            compaction::execution_options(root, &lease.key)
                .unwrap()
                .applied
        );
        let path = root.join(format!(".native/settings.{format}"));
        let read = || -> serde_json::Value {
            let text = std::fs::read_to_string(&path).unwrap();
            if format == "toml" {
                serde_json::to_value(toml::from_str::<toml::Value>(&text).unwrap()).unwrap()
            } else {
                serde_json::from_str(&text).unwrap()
            }
        };
        if format == "toml" {
            assert_eq!(read()["native_compact_tokens"], 9000);
        } else {
            assert_eq!(read()["model"]["compactionThreshold"], 0.65);
        }
        let view = super::policy::inspect(root).unwrap();
        policy.adaptive_compaction.enabled = false;
        super::policy::update(root, &view.revision, Some(policy.clone())).unwrap();
        assert!(
            compaction::execution_options(root, &lease.key)
                .unwrap()
                .applied
        );
        assert!(if format == "toml" {
            read().get("native_compact_tokens").is_none()
        } else {
            read()["model"].get("compactionThreshold").is_none()
        });
        let view = super::policy::inspect(root).unwrap();
        policy.adaptive_compaction.enabled = true;
        super::policy::update(root, &view.revision, Some(policy.clone())).unwrap();
        assert!(
            compaction::execution_options(root, &lease.key)
                .unwrap()
                .applied
        );
        let raw = if format == "toml" {
            "native_compact_tokens = 42\nmodel = 'preserve'\n"
        } else {
            r#"{"model":{"compactionThreshold":0.42,"name":"preserve"}}"#
        };
        std::fs::write(&path, raw).unwrap();
        let view = super::policy::inspect(root).unwrap();
        policy.adaptive_compaction.enabled = false;
        super::policy::update(root, &view.revision, Some(policy)).unwrap();
        assert!(
            !compaction::execution_options(root, &lease.key)
                .unwrap()
                .applied
        );
        assert_eq!(std::fs::read_to_string(path).unwrap(), raw);
        store::release(root, &lease.key, true).unwrap();
    }
}

#[test]
#[cfg(unix)]
fn legacy_native_override_needs_reviewed_idle_adoption_and_keeps_toml_types() {
    let temp = tests::root();
    let root = temp.path();
    enable(root);
    let p = spec(root, "toml");
    let lease = grant(root, &p);
    compaction::probe_spec(root, "future-tool", &p).unwrap();
    let path = root.join(".native/settings.toml");
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(
        &path,
        "native_compact_tokens = 42\nmodel = 'kept'\nwhen = 1979-05-27T07:32:00Z\n",
    )
    .unwrap();
    assert!(
        !compaction::execution_options(root, &lease.key)
            .unwrap()
            .applied
    );
    assert!(compaction::adoption::preview(root, &lease.key.session_id).is_err());
    store::release(root, &lease.key, true).unwrap();
    let reviewed = compaction::adoption::preview(root, &lease.key.session_id).unwrap();
    std::fs::write(
        &path,
        "native_compact_tokens = 43\nmodel = 'kept'\nwhen = 1979-05-27T07:32:00Z\n",
    )
    .unwrap();
    assert!(compaction::adoption::apply(
        root,
        &lease.key.session_id,
        reviewed["checksum"].as_str().unwrap()
    )
    .is_err());
    let reviewed = compaction::adoption::preview(root, &lease.key.session_id).unwrap();
    compaction::adoption::apply(
        root,
        &lease.key.session_id,
        reviewed["checksum"].as_str().unwrap(),
    )
    .unwrap();
    let next = grant(root, &p);
    assert!(
        compaction::execution_options(root, &next.key)
            .unwrap()
            .applied
    );
    let actual: toml::Value = toml::from_str(&std::fs::read_to_string(path).unwrap()).unwrap();
    assert_eq!(actual["native_compact_tokens"].as_integer(), Some(5000));
    assert_eq!(actual["model"].as_str(), Some("kept"));
    assert!(actual["when"].is_datetime());
    store::release(root, &next.key, true).unwrap();
}
