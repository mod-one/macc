use super::*;
use crate::{config::CanonicalConfig, ProjectPaths};
use serde_json::json;
pub(super) fn root() -> (tempfile::TempDir, ProjectPaths) {
    let temp = tempfile::tempdir().unwrap();
    let paths = ProjectPaths::from_root(temp.path());
    std::fs::create_dir_all(&paths.macc_dir).unwrap();
    crate::save_canonical_config(&paths, &CanonicalConfig::default()).unwrap();
    (temp, paths)
}
pub(super) fn external() -> Hook {
    serde_json::from_value(json!({"id":"external-warning-filter","name":"Warnings","description":"Keep warnings","version":"1.0.0","event":"tool_output","handler":{"kind":"rules","steps":[{"kind":"retain_lines","pattern":"warning|error"}]},"skills":["fixture-check"],"source":{"repository":"local-reviewed-repository","subpath":"hooks/warnings","distribution":"snapshot"}})).unwrap()
}
pub(super) fn import(paths: &ProjectPaths, definition: Hook) -> Status {
    let view = execute(&paths.root, Action::Status).unwrap();
    execute(
        &paths.root,
        Action::Import {
            sha256: checksum(&definition).unwrap(),
            definition,
            revision: view.revision,
        },
    )
    .unwrap()
}
#[test]
fn defaults_are_offline_tool_neutral_and_not_skills() {
    let config = CanonicalConfig::default();
    let view = status(&config).unwrap();
    assert_eq!(view.hooks.len(), 7);
    assert!(view.hooks.iter().all(|h| h.enabled));
    assert!(view
        .hooks
        .iter()
        .all(|h| h.integration.contains("all adapters")));
    assert!(config.selections.is_none());
    assert!(!selected(&config, "unknown", &[])
        .unwrap()
        .iter()
        .any(|s| s.contains("test")));
}
#[test]
fn feature_claims_mask_old_opt_out_and_release_restores_manual_choice() {
    let mut config = CanonicalConfig::default();
    config.hooks = Some(HooksConfig::default());
    config
        .hooks
        .as_mut()
        .unwrap()
        .overrides
        .insert("performer-log-summary".into(), false);
    config.validate().unwrap();
    let state = status(&config)
        .unwrap()
        .hooks
        .into_iter()
        .find(|h| h.hook.id == "performer-log-summary")
        .unwrap();
    assert!(state.enabled);
    assert!(!state.required_by.is_empty());
    let mut policy = crate::o2::policy::from_config(&config).context_engine;
    policy.output_reduction = false;
    config.context_engine = Some(policy);
    let state = status(&config)
        .unwrap()
        .hooks
        .into_iter()
        .find(|h| h.hook.id == "performer-log-summary")
        .unwrap();
    assert!(!state.enabled);
    assert!(state.required_by.is_empty());
    config
        .hooks
        .as_mut()
        .unwrap()
        .overrides
        .insert("performer-log-summary".into(), true);
    assert!(
        status(&config)
            .unwrap()
            .hooks
            .into_iter()
            .find(|h| h.hook.id == "performer-log-summary")
            .unwrap()
            .enabled
    );
}
#[test]
fn required_disable_is_rejected_atomically_with_owner_explanation() {
    let (_temp, paths) = root();
    let before = std::fs::read(&paths.config_path).unwrap();
    let view = execute(&paths.root, Action::Status).unwrap();
    let error = execute(
        &paths.root,
        Action::Set {
            id: "performer-log-summary".into(),
            enabled: Some(false),
            revision: view.revision,
        },
    )
    .unwrap_err()
    .to_string();
    assert!(error.contains("O2 context engine"));
    assert_eq!(before, std::fs::read(&paths.config_path).unwrap());
}
#[test]
fn stale_clients_cannot_overwrite_activation() {
    let (_temp, paths) = root();
    let view = execute(&paths.root, Action::Status).unwrap();
    execute(
        &paths.root,
        Action::Set {
            id: "lint-errors-only".into(),
            enabled: Some(false),
            revision: view.revision.clone(),
        },
    )
    .unwrap();
    let error = execute(
        &paths.root,
        Action::Set {
            id: "test-output-failures-only".into(),
            enabled: Some(false),
            revision: view.revision,
        },
    )
    .unwrap_err()
    .to_string();
    assert!(error.contains("revision conflict"));
    assert!(
        execute(&paths.root, Action::Status)
            .unwrap()
            .hooks
            .into_iter()
            .find(|h| h.hook.id == "test-output-failures-only")
            .unwrap()
            .enabled
    );
}
#[test]
fn imported_snapshot_stays_disabled_until_enabled_and_runs_only_for_declared_skill() {
    let (_temp, paths) = root();
    let view = import(&paths, external());
    let id = "external-warning-filter";
    assert!(!view.hooks.iter().find(|h| h.hook.id == id).unwrap().enabled);
    execute(
        &paths.root,
        Action::Set {
            id: id.into(),
            enabled: Some(true),
            revision: view.revision,
        },
    )
    .unwrap();
    let config = crate::load_canonical_config(&paths.config_path).unwrap();
    assert!(selected(&config, "other-skill", &[]).unwrap().is_empty());
    let ids = selected(&config, "fixture-check", &[]).unwrap();
    assert_eq!(
        transform(&config, &ids, "progress\nwarning: useful\nsuccess").unwrap(),
        "warning: useful"
    );
    assert_eq!(
        config.hooks.unwrap().external[0].sha256,
        checksum(&external()).unwrap()
    );
}
#[test]
fn import_rejects_tamper_reserved_ids_and_external_defaults() {
    let (_temp, paths) = root();
    let view = execute(&paths.root, Action::Status).unwrap();
    assert!(execute(
        &paths.root,
        Action::Import {
            definition: external(),
            sha256: "tampered".into(),
            revision: view.revision
        }
    )
    .is_err());
    let mut hook = external();
    hook.id = "test-output-failures-only".into();
    assert!(review(hook).is_err());
    let mut hook = external();
    hook.default_enabled = true;
    assert!(review(hook).is_err());
    let mut hook = external();
    hook.handler = Handler::Builtin {
        name: "test-output-failures-only".into(),
    };
    assert!(review(hook).is_err());
    let mut hook = external();
    hook.handler = Handler::Rules {
        steps: vec![Step::RetainLines {
            pattern: "[".into(),
        }],
    };
    assert!(review(hook).is_err());
}
#[test]
fn remove_cannot_break_a_feature_output_contract() {
    let (_temp, paths) = root();
    let view = import(&paths, external());
    crate::config::transaction::update(&paths,Some(&view.revision),|config| {
        config.context = Some(serde_json::from_value(json!({"summarization":{"enabled":true,"default_bundles":["external-warning-filter"]}})).unwrap()); Ok(())
    }).unwrap();
    let view = execute(&paths.root, Action::Status).unwrap();
    let error = execute(
        &paths.root,
        Action::Remove {
            id: "external-warning-filter".into(),
            revision: view.revision,
        },
    )
    .unwrap_err()
    .to_string();
    assert!(error.contains("output contract"));
}
#[test]
fn reset_keeps_feature_claim_and_supervisor_claims_are_derived() {
    let (_temp, paths) = root();
    let view = execute(&paths.root, Action::Status).unwrap();
    let after = execute(
        &paths.root,
        Action::Set {
            id: "performer-log-summary".into(),
            enabled: None,
            revision: view.revision,
        },
    )
    .unwrap();
    assert!(
        after
            .hooks
            .iter()
            .find(|h| h.hook.id == "performer-log-summary")
            .unwrap()
            .enabled
    );
    let mut config = CanonicalConfig::default();
    config.automation.supervisor = Some(Default::default());
    let state = status(&config)
        .unwrap()
        .hooks
        .into_iter()
        .find(|h| h.hook.id == "coordinator-event-summarizer")
        .unwrap();
    assert!(!state.required_by.is_empty());
}
#[test]
fn two_concurrent_clients_have_exactly_one_winner() {
    let (_temp, paths) = root();
    let revision = execute(&paths.root, Action::Status).unwrap().revision;
    let barrier = std::sync::Arc::new(std::sync::Barrier::new(2));
    let handles: Vec<_> = ["test-output-failures-only", "lint-errors-only"]
        .into_iter()
        .map(|id| {
            let root = paths.root.clone();
            let revision = revision.clone();
            let barrier = barrier.clone();
            std::thread::spawn(move || {
                barrier.wait();
                execute(
                    &root,
                    Action::Set {
                        id: id.into(),
                        enabled: Some(false),
                        revision,
                    },
                )
            })
        })
        .collect();
    assert_eq!(
        handles
            .into_iter()
            .map(|h| h.join().unwrap())
            .filter(Result::is_ok)
            .count(),
        1
    );
}
#[test]
fn hook_lifecycle_survives_full_config_roundtrip_without_network() {
    let (_temp, paths) = root();
    let view = import(&paths, external());
    let config = crate::load_canonical_config(&paths.config_path).unwrap();
    let decoded = CanonicalConfig::from_yaml(&config.to_yaml().unwrap()).unwrap();
    assert_eq!(status(&decoded).unwrap().revision, view.revision);
    execute(
        &paths.root,
        Action::Remove {
            id: "external-warning-filter".into(),
            revision: view.revision,
        },
    )
    .unwrap();
    assert_eq!(execute(&paths.root, Action::Status).unwrap().hooks.len(), 7);
}
#[test]
fn unknown_hooks_and_unknown_manifest_fields_are_rejected() {
    let mut config = CanonicalConfig::default();
    config.hooks = Some(HooksConfig {
        overrides: [("typo".into(), true)].into(),
        ..Default::default()
    });
    assert!(config
        .validate()
        .unwrap_err()
        .to_string()
        .contains("Unknown hook"));
    let mut value = serde_json::to_value(external()).unwrap();
    value["mandatory"] = json!(true);
    assert!(serde_json::from_value::<Hook>(value).is_err());
}

#[test]
fn releasing_one_of_two_feature_claims_does_not_disable_a_required_hook() {
    let mut config = CanonicalConfig::default();
    config.automation.supervisor = Some(Default::default());
    config.hooks = Some(HooksConfig {
        overrides: [("performer-log-summary".into(), false)].into(),
        ..Default::default()
    });
    let state = |config: &CanonicalConfig| {
        status(config)
            .unwrap()
            .hooks
            .into_iter()
            .find(|h| h.hook.id == "performer-log-summary")
            .unwrap()
    };
    assert_eq!(state(&config).required_by.len(), 2);
    let mut context = crate::o2::policy::from_config(&config).context_engine;
    context.output_reduction = false;
    config.context_engine = Some(context);
    assert_eq!(state(&config).required_by.len(), 1);
    assert!(state(&config).enabled);
    config.automation.supervisor = None;
    assert!(!state(&config).enabled);
}

#[test]
fn output_contract_claims_follow_the_effective_o2_or_legacy_dispatch_condition() {
    let mut config = CanonicalConfig::default();
    config.context = Some(
        serde_json::from_value(
            json!({"summarization":{"enabled":false,"default_bundles":["lint"]}}),
        )
        .unwrap(),
    );
    let state = |config: &CanonicalConfig| {
        status(config)
            .unwrap()
            .hooks
            .into_iter()
            .find(|h| h.hook.id == "lint-errors-only")
            .unwrap()
    };
    assert!(!state(&config).required_by.is_empty());
    let mut context = crate::o2::policy::from_config(&config).context_engine;
    context.output_reduction = false;
    config.context_engine = Some(context);
    assert!(state(&config).required_by.is_empty());
}
