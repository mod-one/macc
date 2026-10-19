use super::*;
use crate::session_store::{acquire, bind, migrate, AcquireRequest};
use serde_json::json;
use std::os::unix::fs::PermissionsExt;
fn setup(version: &str, config: Option<Value>) -> (tempfile::TempDir, LeaseKey, String) {
    let t = tempfile::tempdir().unwrap();
    let root = t.path();
    std::fs::create_dir_all(root.join(".macc/state")).unwrap();
    let plan = migrate(root, false, None).unwrap();
    migrate(root, true, Some(&plan.source_sha256)).unwrap();
    let exe = root.join("future-ai");
    script(&exe, version);
    let mut performer = json!({"command":exe,"args":["--model","default-model"],"session":{"enabled":true,"scope":"task_role_tool"},"usage":{"schema":"future-v1","event_match":{"/type":"result"},"fields":{"input_uncached":"/input"},"model":"/model","session_id":"/session_id"},"compaction":{"schema":"future-compaction-v1","version_pattern":"^future-cli [0-9]+\\.[0-9]+\\n?$","documentation":"https://example.test/future/version"}});
    if let Some(config) = config {
        performer["model_config"] = config;
    }
    let spec = json!({"id":"future-tool","performer":performer,"model_tiers":{"heavy":{"model":"unapplied-tier"}}});
    let identity = crate::coordinator::session_identity::execution_identity(
        root,
        root,
        "TASK",
        "performer",
        "future-tool",
        &spec,
        &crate::coordinator::session_identity::SessionRouting {
            tier: "heavy".into(),
            mode: "auto".into(),
        },
    )
    .unwrap();
    let lease = acquire(
        root,
        &AcquireRequest {
            identity,
            request_id: "first".into(),
            owner_pid: std::process::id() as i64,
            preferred_provider_id: None,
            generate_provider_id: false,
            checkpoint_id: None,
        },
    )
    .unwrap();
    bind(root, &lease.key, &lease.key.session_id).unwrap();
    let command = exe.to_string_lossy().into_owned();
    (t, lease.key, command)
}
fn script(path: &Path, version: &str) {
    std::fs::write(
        path,
        format!("#!/bin/sh\n[ \"$1\" = --version ] || exit 44\nprintf '%s\\n' '{version}'\n"),
    )
    .unwrap();
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o700)).unwrap();
}
fn request(key: &LeaseKey, command: &str, args: &[&str]) -> BeginRequest {
    BeginRequest {
        key: key.clone(),
        phase: "dev".into(),
        invocation: Some(Invocation {
            command: command.into(),
            args: args.iter().map(|v| (*v).into()).collect(),
        }),
    }
}
fn finish(root: &Path, key: &LeaseKey, model: Option<&str>) -> UsageRecord {
    let raw = json!({"type":"result","input":17,"model":model}).to_string();
    end(
        root,
        &EndRequest {
            key: key.clone(),
            outcome: "success".into(),
            raw,
            omitted: false,
        },
    )
    .unwrap()
}
#[test]
fn prepared_model_and_verified_version_survive_missing_provider_fields() {
    let (t, key, command) = setup("future-cli 1.2", None);
    let req = request(
        &key,
        &command,
        &[
            "exec",
            "--model",
            "actually-routed",
            "--private-option",
            "not-persisted",
        ],
    );
    begin(t.path(), &req).unwrap();
    let pending = inspect(t.path()).unwrap().records.remove(0);
    assert_eq!(pending.model.as_deref(), Some("actually-routed"));
    assert_eq!(pending.provider_version.as_deref(), Some("future-cli 1.2"));
    // Idempotent begin must not re-probe or overwrite the original identity.
    std::fs::remove_file(&command).unwrap();
    begin(t.path(), &request(&key, &command, &["--model", "changed"])).unwrap();
    let result = finish(t.path(), &key, None);
    assert_eq!(result.model, pending.model);
    assert_eq!(result.provider_version, pending.provider_version);
    assert_eq!(result.incremental.input_uncached, Some(17));
    assert!(result.notes.contains(&"identity_model_from_argv".into()));
    assert!(!serde_json::to_string(&inspect(t.path()).unwrap())
        .unwrap()
        .contains("not-persisted"));
}
#[test]
fn provider_model_wins_without_manufacturing_price_or_version() {
    let (t, key, command) = setup("wrong-version", None);
    begin(
        t.path(),
        &request(&key, &command, &["--model=requested-alias"]),
    )
    .unwrap();
    let result = finish(t.path(), &key, Some("provider-resolved-2"));
    assert_eq!(result.model.as_deref(), Some("provider-resolved-2"));
    assert_eq!(result.provider_version, None);
    assert_eq!(result.incremental.cost, None);
    assert!(result
        .notes
        .contains(&"identity_model_from_provider".into()));
}
#[test]
fn config_file_models_use_actual_json_or_toml_values_not_tier_assumptions() {
    for (format, raw) in [
        ("json", "{\"model\":\"Future Large (Medium)\"}"),
        ("toml", "model = \"Future Large (Medium)\"\n"),
    ] {
        let cfg = json!({"path":".future/settings","format":format,"key":"model"});
        let (t, key, command) = setup("future-cli 1.2", Some(cfg));
        std::fs::create_dir(t.path().join(".future")).unwrap();
        std::fs::write(t.path().join(".future/settings"), raw).unwrap();
        begin(t.path(), &request(&key, &command, &["exec"])).unwrap();
        let result = finish(t.path(), &key, None);
        assert_eq!(result.model.as_deref(), Some("Future Large (Medium)"));
        assert!(result.notes.contains(&"identity_model_from_config".into()));
    }
}
#[test]
fn undeclared_command_and_unsafe_or_missing_model_remain_unknown() {
    for (args, other) in [
        (vec!["exec"], false),
        (vec!["--model", "sk-abcdefghijklmnopqrstuvwxyz"], false),
        (vec!["--model", "model-x"], true),
    ] {
        let (t, key, command) = setup("future-cli 1.2", None);
        let cmd = if other {
            "/not-a-declared-command"
        } else {
            &command
        };
        begin(t.path(), &request(&key, cmd, &args)).unwrap();
        let result = finish(t.path(), &key, None);
        assert_eq!(result.model, None);
        assert!(!serde_json::to_string(&result)
            .unwrap()
            .contains("sk-abcdefghijklmnopqrstuvwxyz"));
        if other {
            assert_eq!(result.provider_version, None);
        }
    }
}
#[test]
fn version_probe_never_holds_the_session_authority_lock() {
    let (t, key, command) = setup("future-cli 1.2", None);
    let lock = t.path().join(".macc/state/tool-sessions.json.lock");
    std::fs::write(&command,format!("#!/bin/sh\n[ \"$1\" = --version ] || exit 44\nexec flock -n '{}' printf 'future-cli 1.2\\n'\n",lock.display())).unwrap();
    begin(t.path(), &request(&key, &command, &["--model", "model-x"])).unwrap();
    assert_eq!(
        finish(t.path(), &key, None).provider_version.as_deref(),
        Some("future-cli 1.2")
    );
}
#[test]
fn version_is_reobserved_for_each_generation_and_unavailable_stays_unknown() {
    let (t, key, command) = setup("future-cli 1.2", None);
    begin(t.path(), &request(&key, &command, &["--model", "model-x"])).unwrap();
    finish(t.path(), &key, None);
    release(t.path(), &key, true).unwrap();
    let identity = {
        let (db, _) = db::active(t.path()).unwrap();
        let raw: String = db
            .query_row(
                "SELECT identity_json FROM logical_sessions WHERE id=?1",
                [&key.session_id],
                |r| r.get(0),
            )
            .unwrap();
        serde_json::from_str(&raw).unwrap()
    };
    let next = acquire(
        t.path(),
        &AcquireRequest {
            identity,
            request_id: "second".into(),
            owner_pid: std::process::id() as i64,
            preferred_provider_id: None,
            generate_provider_id: false,
            checkpoint_id: None,
        },
    )
    .unwrap();
    script(Path::new(&command), "future-cli 1.3");
    begin(
        t.path(),
        &request(&next.key, &command, &["--model", "model-x"]),
    )
    .unwrap();
    assert_eq!(
        finish(t.path(), &next.key, None)
            .provider_version
            .as_deref(),
        Some("future-cli 1.3")
    );
}
#[test]
fn foreign_lease_cannot_launch_version_probe() {
    let (t, mut key, command) = setup("future-cli 1.2", None);
    key.lease_token = "foreign".into();
    std::fs::write(&command, "#!/bin/sh\nexit 99\n").unwrap();
    assert!(begin(t.path(), &request(&key, &command, &["--model", "x"])).is_err());
    assert!(inspect(t.path()).unwrap().records.is_empty());
}

#[test]
fn model_config_cannot_escape_follow_symlinks_or_exceed_read_bound() {
    for variant in ["escape", "symlink", "oversized"] {
        let path = if variant == "escape" {
            "../outside"
        } else {
            ".future/settings"
        };
        let (t, key, command) = setup(
            "future-cli 1.2",
            Some(json!({"path":path,"format":"json","key":"model"})),
        );
        std::fs::create_dir(t.path().join(".future")).unwrap();
        let config = t.path().join(".future/settings");
        match variant {
            "symlink" => {
                let elsewhere = t.path().join("actual.json");
                std::fs::write(&elsewhere, r#"{"model":"not-authorized"}"#).unwrap();
                std::os::unix::fs::symlink(elsewhere, config).unwrap();
            }
            "oversized" => std::fs::write(
                config,
                format!(
                    "{{\"model\":\"hidden\",\"padding\":\"{}\"}}",
                    "x".repeat(65536)
                ),
            )
            .unwrap(),
            _ => {}
        }
        begin(t.path(), &request(&key, &command, &["exec"])).unwrap();
        assert_eq!(finish(t.path(), &key, None).model, None, "{variant}");
    }
}

#[test]
fn mismatched_provider_output_cannot_override_prepared_identity() {
    let (t, key, command) = setup("future-cli 1.2", None);
    begin(
        t.path(),
        &request(&key, &command, &["-m", "prepared-model"]),
    )
    .unwrap();
    let record = end(t.path(), &EndRequest {
        key, outcome:"success".into(), omitted:false,
        raw:json!({"type":"result","input":17,"model":"foreign-model","session_id":"foreign-session"}).to_string()
    }).unwrap();
    assert_eq!(record.model.as_deref(), Some("prepared-model"));
    assert!(record.incremental.input_uncached.is_none());
    assert!(record.notes.contains(&"provider_session_mismatch".into()));
    assert!(!record
        .notes
        .contains(&"identity_model_from_provider".into()));
}
