use super::*;

fn project() -> (tempfile::TempDir, crate::ProjectPaths) {
    let tmp = tempfile::tempdir().unwrap();
    let paths = crate::ProjectPaths::from_root(tmp.path());
    std::fs::create_dir_all(&paths.macc_dir).unwrap();
    std::fs::write(
        &paths.config_path,
        "tools:\n  enabled: []\nmemory:\n  enabled: true\n",
    )
    .unwrap();
    (tmp, paths)
}

#[test]
fn reviewed_apply_activates_o2_and_preserves_it_across_clear() {
    let (_tmp, paths) = project();
    assert!(require_ready(&paths.root)
        .unwrap_err()
        .to_string()
        .contains("macc apply"));
    let findings = super::super::health::findings(&paths.root);
    assert_eq!(
        findings[0].severity,
        crate::doctor::DiagnosticSeverity::Error
    );
    let activation = preview(&paths.root).unwrap().unwrap();
    assert!(!paths.root.join(".macc/memory.db").exists());
    let config = crate::load_canonical_config(&paths.config_path).unwrap();
    let resolved = crate::resolve::resolve(&config, &Default::default());
    let mut plan = crate::build_plan(
        &paths,
        &resolved,
        &[],
        &crate::ToolRegistry::default_registry(),
    )
    .unwrap();
    assert_eq!(plan.o2_activation, Some(activation));
    let ops = crate::plan::collect_plan_operations(&paths, &plan);
    assert!(ops.iter().any(|op| op.path.contains("SQLite")));
    crate::apply_plan(&paths, &mut plan, false).unwrap();
    require_ready(&paths.root).unwrap();
    let (db, _) = super::super::database(&paths.root).unwrap();
    db.execute(
        "INSERT INTO o2_audit VALUES('retained','test',NULL,0,'{}')",
        [],
    )
    .unwrap();
    drop(db);
    crate::record_managed_path(&paths, ".macc/memory.db").unwrap();
    crate::record_managed_path(&paths, ".macc/state/tool-sessions.json").unwrap();
    crate::clear(&paths).unwrap();
    require_ready(&paths.root).unwrap();
    let (db, _) = super::super::readonly_database(&paths.root).unwrap();
    assert_eq!(
        db.query_row(
            "SELECT count(*) FROM o2_audit WHERE id='retained'",
            [],
            |r| r.get::<_, i64>(0)
        )
        .unwrap(),
        1
    );
    let mut again = crate::plan::ActionPlan::new();
    again.o2_activation = preview(&paths.root).unwrap();
    again.preparation = crate::service::preparation::preview(&paths).unwrap();
    crate::apply_plan(&paths, &mut again, false).unwrap();
}

#[test]
fn changed_config_or_sessions_require_new_review_before_writing() {
    let (_tmp, paths) = project();
    let mut plan = crate::plan::ActionPlan::new();
    plan.o2_activation = preview(&paths.root).unwrap();
    plan.add_action(crate::plan::Action::WriteFile {
        path: "must-not-write".into(),
        content: vec![1],
        scope: crate::plan::Scope::Project,
    });
    std::fs::create_dir_all(paths.macc_dir.join("state")).unwrap();
    std::fs::write(
        paths.macc_dir.join("state/tool-sessions.json"),
        r#"{"tools":{},"changed":true}"#,
    )
    .unwrap();
    assert!(crate::apply_plan(&paths, &mut plan, false).is_err());
    assert!(!paths.root.join("must-not-write").exists());
    plan.o2_activation = preview(&paths.root).unwrap();
    std::fs::write(
        &paths.config_path,
        "tools:\n  enabled: []\nmemory:\n  enabled: false\n",
    )
    .unwrap();
    assert!(crate::apply_plan(&paths, &mut plan, false).is_err());
    assert!(!paths.root.join(".macc/memory.db").exists());
}

#[test]
fn existing_schema_upgrade_is_backed_up_and_disabled_features_are_untouched() {
    let (_tmp, paths) = project();
    apply(&paths.root, &preview(&paths.root).unwrap().unwrap()).unwrap();
    let (db, _) = super::super::database(&paths.root).unwrap();
    db.execute("DELETE FROM session_meta WHERE key='o2_schema_version'", [])
        .unwrap();
    drop(db);
    let reviewed = preview(&paths.root).unwrap().unwrap();
    assert!(reviewed.operation.contains("upgrade"));
    apply(&paths.root, &reviewed).unwrap();
    assert!(std::fs::read_dir(paths.macc_dir.join("state"))
        .unwrap()
        .any(|e| e
            .unwrap()
            .file_name()
            .to_string_lossy()
            .starts_with("pre-o2-")));
    std::fs::write(&paths.config_path, "tools:\n  enabled: []\n").unwrap();
    assert!(preview(&paths.root).unwrap().is_none());
}

#[test]
fn orphaned_database_requires_explicit_recovery() {
    let (_tmp, paths) = project();
    std::fs::write(paths.macc_dir.join("memory.db"), b"existing evidence").unwrap();
    let findings = super::super::health::findings(&paths.root);
    assert_eq!(
        findings[0].severity,
        crate::doctor::DiagnosticSeverity::Error
    );
    assert!(preview(&paths.root)
        .unwrap_err()
        .to_string()
        .contains("Authority marker missing"));
    assert_eq!(
        std::fs::read(paths.macc_dir.join("memory.db")).unwrap(),
        b"existing evidence"
    );
}

#[test]
fn usage_capture_without_other_o2_features_also_requires_sqlite() {
    let (_tmp, paths) = project();
    std::fs::write(
        &paths.config_path,
        "tools:\n  enabled: []\nsession_management:\n  usage:\n    capture_mode: 1\n",
    )
    .unwrap();
    assert!(require_ready(&paths.root).is_err());
    apply(&paths.root, &preview(&paths.root).unwrap().unwrap()).unwrap();
    require_ready(&paths.root).unwrap();
}

#[test]
fn managed_worker_apply_does_not_initialize_an_independent_session_store() {
    let (_tmp, paths) = project();
    apply(&paths.root, &preview(&paths.root).unwrap().unwrap()).unwrap();
    let worker = paths.root.join(".macc/worktree/worker-01");
    let worker_paths = crate::ProjectPaths::from_root(&worker);
    std::fs::create_dir_all(&worker_paths.macc_dir).unwrap();
    std::fs::copy(&paths.config_path, &worker_paths.config_path).unwrap();
    std::fs::write(worker_paths.macc_dir.join("worktree.json"), "{}").unwrap();
    let config = crate::load_canonical_config(&worker_paths.config_path).unwrap();
    let resolved = crate::resolve::resolve(&config, &Default::default());
    let mut plan = crate::build_plan(
        &worker_paths,
        &resolved,
        &[],
        &crate::ToolRegistry::default_registry(),
    )
    .unwrap();
    assert!(plan.o2_activation.is_none());
    crate::apply_plan(&worker_paths, &mut plan, false).unwrap();
    assert!(!worker_paths.macc_dir.join("memory.db").exists());
    require_ready(&paths.root).unwrap();
}
