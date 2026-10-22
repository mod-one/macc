use super::*;
#[test]
fn policy_editor_requires_preview_preserves_drafts_and_saves_exact_revision() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path();
    let mut panel = Panel::default();
    panel.refresh(root).unwrap();
    assert_eq!(panel.access, json!({}));
    panel.key(root, KeyCode::Char('5')).unwrap();
    panel.selected = panel
        .view
        .as_ref()
        .unwrap()
        .fields
        .iter()
        .position(|f| f.path == "memory.enabled")
        .unwrap();
    panel.key(root, KeyCode::Enter).unwrap();
    assert_eq!(panel.draft["memory"]["enabled"], true);
    assert!(panel.key(root, KeyCode::Char('s')).is_err());
    panel.refresh(root).unwrap();
    assert_eq!(panel.draft["memory"]["enabled"], true);
    panel.key(root, KeyCode::Char('p')).unwrap();
    assert!(panel.mutation(KeyCode::Char('s')));
    panel.key(root, KeyCode::Char('s')).unwrap();
    assert!(policy::load(root).unwrap().memory.enabled);
    panel.key(root, KeyCode::Enter).unwrap();
    panel.key(root, KeyCode::Char('p')).unwrap();
    let current = policy::inspect(root).unwrap();
    let mut changed = current.policy;
    changed.memory.search_limit += 1;
    policy::update(root, &current.revision, Some(changed)).unwrap();
    assert!(panel.key(root, KeyCode::Char('s')).is_err());
    assert_eq!(panel.draft["memory"]["enabled"], false);
}
#[test]
fn scope_form_has_all_scopes_and_retains_input_on_errors() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path();
    let mut panel = Panel::default();
    panel.refresh(root).unwrap();
    panel.key(root, KeyCode::Char('/')).unwrap();
    assert_eq!(panel.form.as_ref().unwrap().labels.len(), 6);
    assert!(!panel.mutation(KeyCode::Enter));
    for text in ["contract", "A", "fake", "", "feature", "user"] {
        for c in text.chars() {
            panel.key(root, KeyCode::Char(c)).unwrap();
        }
        let _ = panel.key(root, KeyCode::Enter);
    }
    assert_eq!(panel.access["feature"], "feature");
    assert_eq!(panel.access["user_id"], "user");
    assert!(panel.form.is_some()); // JSON authority: action fails, user input retained.
    panel.key(root, KeyCode::Esc).unwrap();
    panel.key(root, KeyCode::Char('3')).unwrap();
    panel.key(root, KeyCode::Char('c')).unwrap();
    assert!(panel.mutation(KeyCode::Enter));
}

#[test]
fn typed_conflict_resolution_previews_then_confirms_the_shared_action() {
    use macc_core::{o2::memory, session_store};
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path();
    std::fs::create_dir_all(root.join(".macc/state")).unwrap();
    let plan = session_store::migrate(root, false, None).unwrap();
    session_store::migrate(root, true, Some(&plan.source_sha256)).unwrap();
    let view = policy::inspect(root).unwrap();
    let mut p = view.policy;
    p.memory.enabled = true;
    policy::update(root, &view.revision, Some(p)).unwrap();
    let proposal = memory::Proposal {
        kind: "constraint".into(),
        title: "Claim".into(),
        body: "A concise claim".into(),
        scope: "project".into(),
        scope_ref: None,
        confidence: 0.9,
        evidence: vec![],
        tags: vec![],
        source_task: None,
        source_revision: None,
    };
    let left = memory::propose(root, &proposal).unwrap();
    let right = memory::propose(root, &proposal).unwrap();
    memory::conflict(root, &left.id, &right.id, "Needs explicit resolution").unwrap();
    let mut panel = Panel {
        conflicts: true,
        ..Default::default()
    };
    panel.refresh(root).unwrap();
    panel.key(root, KeyCode::Char('v')).unwrap();
    assert!(panel.mutation(KeyCode::Enter));
    for c in "Inspected both sources".chars() {
        panel.key(root, KeyCode::Char(c)).unwrap();
    }
    panel.key(root, KeyCode::Enter).unwrap();
    panel.key(root, KeyCode::Enter).unwrap(); // Default conservative both_stale outcome.
    assert!(panel.pending.is_some());
    let before = execute(root, Action::MemoryConflicts).unwrap();
    assert_eq!(before[0]["status"], "open");
    assert!(panel.mutation(KeyCode::Char('a')));
    panel.key(root, KeyCode::Char('a')).unwrap();
    settle(&mut panel, root);
    let after = execute(root, Action::MemoryConflicts).unwrap();
    assert_eq!(after[0]["status"], "resolved");
    assert!(panel.pending.is_none());
}

#[test]
fn graphify_executable_editor_keeps_typed_paths_and_requires_a_new_preview() {
    let t = tempfile::tempdir().unwrap();
    let root = t.path();
    let mut panel = Panel::default();
    panel.refresh(root).unwrap();
    panel.key(root, KeyCode::Char('5')).unwrap();
    panel.selected = panel
        .view
        .as_ref()
        .unwrap()
        .fields
        .iter()
        .position(|f| f.path == "code_intelligence.graphify_python")
        .unwrap();
    panel.key(root, KeyCode::Enter).unwrap();
    assert_eq!(panel.form.as_ref().unwrap().name, "text_field");
    for c in "/usr/bin/python3".chars() {
        panel.key(root, KeyCode::Char(c)).unwrap();
    }
    panel.key(root, KeyCode::Enter).unwrap();
    assert_eq!(
        panel.draft["code_intelligence"]["graphify_python"],
        "/usr/bin/python3"
    );
    assert!(!panel.preview);
    assert!(panel.key(root, KeyCode::Char('s')).is_err());
    panel.key(root, KeyCode::Char('p')).unwrap();
    panel.key(root, KeyCode::Char('s')).unwrap();
    assert_eq!(
        policy::load(root)
            .unwrap()
            .code_intelligence
            .graphify_python,
        "/usr/bin/python3"
    );
    assert_eq!(
        panel.view.as_ref().unwrap().fields[panel.selected].path,
        "code_intelligence.graphify_python"
    );
}

#[test]
fn read_only_background_completion_preserves_a_pending_reviewed_action() {
    let t = tempfile::tempdir().unwrap();
    let root = t.path();
    let mut panel = Panel {
        pending: Some(Action::ProviderApply {
            plan_id: "reviewed-plan".into(),
            checksum: "reviewed-sha".into(),
        }),
        ..Default::default()
    };
    panel.launch(root, Action::Status).unwrap();
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
    while panel.job.is_some() && std::time::Instant::now() < deadline {
        panel.poll(root);
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    assert!(panel.job.is_none());
    assert!(panel.pending.is_some());
}

#[test]
fn provider_health_refresh_is_background_and_preserves_pending_review() {
    let temp = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(temp.path().join(".macc/state")).unwrap();
    let migration = macc_core::session_store::migrate(temp.path(), false, None).unwrap();
    macc_core::session_store::migrate(temp.path(), true, Some(&migration.source_sha256)).unwrap();
    let mut panel = Panel {
        tab: 3,
        pending: Some(Action::ProviderApply {
            plan_id: "reviewed-plan".into(),
            checksum: "reviewed-hash".into(),
        }),
        ..Default::default()
    };
    panel.refresh(temp.path()).unwrap();
    assert!(panel.job.is_some());
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
    while panel.job.is_some() && std::time::Instant::now() < deadline {
        panel.poll(temp.path());
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
    assert!(
        panel.job.is_none(),
        "Health refresh must not schedule itself again"
    );
    assert!(panel.rows.iter().any(|r| r["provider"]["id"] == "serena"));
    assert!(matches!(panel.pending, Some(Action::ProviderApply { .. })));
}

#[test]
fn agent_setup_is_read_only_and_extraction_requires_explicit_apply() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path();
    let mut panel = Panel::default();
    panel.refresh(root).unwrap();
    assert!(!panel.mutation(KeyCode::Char('h')));
    panel.key(root, KeyCode::Char('h')).unwrap();
    assert!(panel.details.contains("required_inherited_environment"));
    panel.key(root, KeyCode::Char('e')).unwrap();
    assert!(panel.mutation(KeyCode::Enter));
    for c in "checkpoint-id".chars() {
        panel.key(root, KeyCode::Char(c)).unwrap();
    }
    panel.key(root, KeyCode::Enter).unwrap();
    assert!(matches!(panel.pending, Some(Action::MemoryExtract { .. })));
    assert!(panel.mutation(KeyCode::Char('a')));
    assert!(!root.join(".macc/memory.db").exists());
}

#[test]
fn overview_navigation_preserves_scope_selection_and_policy_draft() {
    let temp = tempfile::tempdir().unwrap();
    let r = temp.path();
    let mut panel = Panel::default();
    panel.refresh(r).unwrap();
    panel.draft["memory"]["enabled"] = json!(true);
    panel.tab = 5;
    panel.rows = vec![json!({"task_id":"A"}), json!({"task_id":"B"})];
    panel.selected = 1;
    panel.replace_rows(vec![json!({"task_id":"B"}), json!({"task_id":"A"})]);
    assert_eq!(panel.selected, 0);
    assert!(!panel.mutation(KeyCode::Char('s')));
    panel.key(r, KeyCode::Char('s')).unwrap();
    assert_eq!(panel.session_task.as_deref(), Some("B"));
    panel.key(r, KeyCode::Char('m')).unwrap();
    assert_eq!(panel.access["task_id"], "B");
    assert_eq!(panel.tab, 0);
    assert_eq!(panel.draft["memory"]["enabled"], true);
    let mut terminal = ratatui::Terminal::new(ratatui::backend::TestBackend::new(120, 25)).unwrap();
    panel.tab = 5;
    terminal.draw(|f| panel.draw(f, f.size(), "")).unwrap();
    let contents = terminal
        .backend()
        .buffer()
        .content
        .iter()
        .map(|c| c.symbol())
        .collect::<String>();
    assert!(contents.contains("Overview"));
    assert!(contents.contains("F2"));
    assert!(contents.contains("Get started"));
    panel.tab = 2;
    panel.rows = vec![json!({"tool_id":"a"}), json!({"tool_id":"b"})];
    panel.selected = 1;
    panel.replace_rows(vec![json!({"tool_id":"b"}), json!({"tool_id":"a"})]);
    assert_eq!(panel.selected, 0);
    panel.tab = 0;
    panel.conflicts = true;
    panel.rows = vec![json!({"id":"a"}), json!({"id":"b"})];
    panel.selected = 1;
    panel.replace_rows(vec![json!({"id":"b"}), json!({"id":"a"})]);
    assert_eq!(panel.selected, 0);
}

#[test]
fn overview_async_refresh_does_not_replace_another_domains_rows() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path();
    std::fs::create_dir_all(root.join(".macc")).unwrap();
    let migration = macc_core::session_store::migrate(root, false, None).unwrap();
    macc_core::session_store::migrate(root, true, Some(&migration.source_sha256)).unwrap();
    let mut panel = Panel::default();
    panel.refresh(root).unwrap();
    panel.open = true;
    panel.key(root, KeyCode::Char('6')).unwrap();
    panel.tab = 0;
    panel.rows = vec![json!({"memory":{"id":"keep-selected"}})];
    for _ in 0..100 {
        panel.poll(root);
        if panel.job.is_none() {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    assert!(panel.job.is_none());
    assert_eq!(panel.rows[0]["memory"]["id"], "keep-selected");
}

fn settle(panel: &mut Panel, root: &Path) {
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
    while panel.job.is_some() && std::time::Instant::now() < deadline {
        panel.poll(root);
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
    assert!(panel.job.is_none(), "{}", panel.message);
}
#[test]
fn async_memory_search_retains_failed_form_and_ignores_obsolete_scope() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path();
    std::fs::create_dir_all(root.join(".macc/state")).unwrap();
    let plan = macc_core::session_store::migrate(root, false, None).unwrap();
    macc_core::session_store::migrate(root, true, Some(&plan.source_sha256)).unwrap();
    let mut panel = Panel::default();
    panel.refresh(root).unwrap();
    settle(&mut panel, root);
    panel.key(root, KeyCode::Char('/')).unwrap();
    for text in [
        "q".repeat(513),
        "A".into(),
        "".into(),
        "".into(),
        "".into(),
        "user".into(),
    ] {
        for c in text.chars() {
            panel.key(root, KeyCode::Char(c)).unwrap();
        }
        panel.key(root, KeyCode::Enter).unwrap();
    }
    assert!(panel.job.is_some());
    settle(&mut panel, root);
    assert!(panel.form.is_some());
    assert_eq!(panel.form.as_ref().unwrap().values[0].len(), 513);
    panel.form = None;
    panel.query = String::new();
    panel.refresh(root).unwrap();
    panel.access = json!({"task_id":"B"});
    panel.tab = 1;
    panel.rows = vec![json!({"id":"keep-context"})];
    settle(&mut panel, root);
    assert_eq!(panel.rows[0]["id"], "keep-context");
}

#[test]
fn symbol_capture_proposal_and_captured_revalidation_use_the_shared_services() {
    let t = tempfile::tempdir().unwrap();
    let root = t.path();
    std::fs::write(root.join("contract.rs"), "pub fn contract(){}\n").unwrap();
    std::fs::write(root.join(".gitignore"), ".macc/\n").unwrap();
    for args in [
        vec!["init", "-b", "main"],
        vec!["config", "user.name", "Test"],
        vec!["config", "user.email", "test@example.invalid"],
        vec!["add", "."],
        vec!["commit", "-m", "fixture"],
    ] {
        assert!(macc_core::git::run_git_output_mapped(
            root,
            &args,
            "Initialize symbol capture fixture"
        )
        .unwrap()
        .status
        .success());
    }
    std::fs::create_dir_all(root.join(".macc/state")).unwrap();
    let plan = macc_core::session_store::migrate(root, false, None).unwrap();
    macc_core::session_store::migrate(root, true, Some(&plan.source_sha256)).unwrap();
    let view = policy::inspect(root).unwrap();
    let mut p = view.policy;
    p.memory.enabled = true;
    p.code_intelligence.enabled = true;
    policy::update(root, &view.revision, Some(p)).unwrap();
    let mut panel = Panel::default();
    panel.refresh(root).unwrap();
    settle(&mut panel, root);
    fn fill(panel: &mut Panel, root: &Path, key: char, fields: &[&str]) {
        panel.key(root, KeyCode::Char(key)).unwrap();
        for value in fields {
            for c in value.chars() {
                panel.key(root, KeyCode::Char(c)).unwrap();
            }
            panel.key(root, KeyCode::Enter).unwrap();
        }
    }
    fill(
        &mut panel,
        root,
        'y',
        &["contract.rs", "contract", "native", ""],
    );
    settle(&mut panel, root);
    assert_eq!(panel.captured_evidence.as_ref().unwrap()["kind"], "symbol");
    fill(
        &mut panel,
        root,
        'n',
        &[
            "decision",
            "Contract",
            "Reviewed contract",
            "project",
            "",
            "",
            "0.9",
        ],
    );
    settle(&mut panel, root);
    assert_eq!(panel.rows[0]["memory"]["status"], "proposed");
    fill(
        &mut panel,
        root,
        'p',
        &["active", "Reviewed symbol source", ""],
    );
    panel.key(root, KeyCode::Char('a')).unwrap();
    settle(&mut panel, root);
    assert_eq!(panel.rows[0]["memory"]["status"], "active");
    std::fs::write(
        root.join("contract.rs"),
        "pub fn contract(){let changed=1;}\n",
    )
    .unwrap();
    fill(&mut panel, root, 'p', &["stale", "Source changed", ""]);
    panel.key(root, KeyCode::Char('a')).unwrap();
    settle(&mut panel, root);
    fill(
        &mut panel,
        root,
        'y',
        &["contract.rs", "contract", "native", ""],
    );
    settle(&mut panel, root);
    fill(
        &mut panel,
        root,
        'R',
        &["active", "Reviewed updated symbol"],
    );
    assert!(
        matches!(&panel.pending,Some(Action::MemoryReview{review}) if review.evidence.is_some())
    );
    panel.key(root, KeyCode::Char('a')).unwrap();
    settle(&mut panel, root);
    assert_eq!(panel.rows[0]["memory"]["status"], "active");
    panel.access = json!({"task_id":"B"});
    panel.refresh(root).unwrap();
    settle(&mut panel, root);
    assert!(panel.captured_evidence.is_none());
}

#[test]
fn native_map_default_opt_out_is_previewed_and_applied_through_the_shared_catalog() {
    let t = tempfile::tempdir().unwrap();
    let root = t.path();
    let mut panel = Panel::default();
    panel.refresh(root).unwrap();
    panel.key(root, KeyCode::Char('5')).unwrap();
    panel.selected = panel
        .view
        .as_ref()
        .unwrap()
        .fields
        .iter()
        .position(|f| f.path == "code_intelligence.native_map_enabled")
        .unwrap();
    panel.key(root, KeyCode::Enter).unwrap();
    assert_eq!(
        panel.draft["code_intelligence"]["native_map_enabled"],
        false
    );
    assert!(
        policy::load(root)
            .unwrap()
            .code_intelligence
            .native_map_enabled
    );
    assert!(panel.key(root, KeyCode::Char('s')).is_err());
    panel.key(root, KeyCode::Char('p')).unwrap();
    panel.key(root, KeyCode::Char('s')).unwrap();
    assert!(
        !policy::load(root)
            .unwrap()
            .code_intelligence
            .native_map_enabled
    );
    assert_eq!(
        panel.view.as_ref().unwrap().fields[panel.selected].path,
        "code_intelligence.native_map_enabled"
    );
}
