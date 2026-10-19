use super::*;
fn fixture() -> tempfile::TempDir {
    let root = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(root.path().join(".macc/state")).unwrap();
    std::fs::write(
        root.path().join(".macc/macc.yaml"),
        "tools:\n  enabled: [future-ai]\nsettings:\n  offline: true\n",
    )
    .unwrap();
    root
}
#[test]
fn preview_does_not_write_and_apply_preserves_unrelated_inheritance() {
    let root = fixture();
    let path = root.path().join(".macc/macc.yaml");
    let before = std::fs::read(&path).unwrap();
    let p = preview(root.path(), "memory.enabled", "true", None).unwrap();
    assert_eq!(p.before, false);
    assert_eq!(p.after, true);
    assert_eq!(std::fs::read(&path).unwrap(), before);
    let result = apply(root.path(), "memory.enabled", "true", &p.revision, true).unwrap();
    assert!(result.policy.memory.enabled);
    let config = crate::load_canonical_config(&path).unwrap();
    assert!(config.context_engine.is_none());
    assert!(config.adaptive_compaction.is_none());
    assert!(config.code_intelligence.is_none());
    assert!(config.settings.offline);
    assert_eq!(config.tools.enabled, vec!["future-ai"]);
    let field = get(root.path(), "memory.enabled").unwrap();
    assert_eq!(field.value, true);
    assert_eq!(field.field.kind, "bool");
}
#[test]
fn invalid_types_fields_ranges_and_dependencies_never_write() {
    let root = fixture();
    let path = root.path().join(".macc/macc.yaml");
    let before = std::fs::read(&path).unwrap();
    for (field, value) in [
        ("memory.enabled", "yes"),
        ("memory.enabled", "1"),
        ("memory.search_limit", "1.5"),
        ("memory.search_limit", "0"),
        ("memory.semantic_backend", "unknown-provider"),
        ("context_engine.enabled", "true"),
        ("adaptive_compaction.warning_ratio", "0.99"),
        ("code_intelligence.serena_python", ""),
        ("memory.typo", "true"),
        ("memory", "{}"),
    ] {
        assert!(
            preview(root.path(), field, value, None).is_err(),
            "{field}={value}"
        );
        assert_eq!(std::fs::read(&path).unwrap(), before);
    }
    assert_eq!(
        preview(root.path(), "memory.search_limit", "7", None)
            .unwrap()
            .after,
        7
    );
    assert_eq!(
        preview(
            root.path(),
            "code_intelligence.serena_python",
            "/path with spaces/python",
            None
        )
        .unwrap()
        .after,
        "/path with spaces/python"
    );
}
#[test]
fn confirmation_and_stale_revision_cannot_overwrite_newer_settings() {
    let root = fixture();
    let p = preview(root.path(), "memory.enabled", "true", None).unwrap();
    assert!(apply(root.path(), "memory.enabled", "true", &p.revision, false).is_err());
    apply(root.path(), "memory.search_limit", "7", &p.revision, true).unwrap();
    assert!(
        apply(root.path(), "memory.enabled", "true", &p.revision, true)
            .unwrap_err()
            .to_string()
            .contains("revision conflict")
    );
    assert!(preview(root.path(), "memory.enabled", "true", Some(&p.revision)).is_err());
    let current = policy::inspect(root.path()).unwrap();
    assert!(!current.policy.memory.enabled);
    assert_eq!(current.policy.memory.search_limit, 7);
}

#[test]
fn shared_actions_preserve_read_only_diagnostics_and_confirmed_writes() {
    use crate::o2::actions::{execute, Action};
    let root = fixture();
    let preview = Action::PolicyFieldPreview {
        path: "memory.enabled".into(),
        value: "true".into(),
        revision: None,
    };
    assert!(!preview.is_mutation());
    assert!(!Action::MemoryExplain {
        search: crate::o2::memory::Search {
            query: "".into(),
            access: Default::default(),
            include_inactive: false
        }
    }
    .is_mutation());
    let value = execute(root.path(), preview).unwrap();
    let write = Action::PolicyFieldApply {
        path: "memory.enabled".into(),
        value: "true".into(),
        revision: value["revision"].as_str().unwrap().into(),
        confirmed: false,
    };
    assert!(write.is_mutation());
    assert!(execute(root.path(), write).is_err());
    assert_eq!(get(root.path(), "memory.enabled").unwrap().value, false);
}
