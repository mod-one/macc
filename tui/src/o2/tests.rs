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
    assert!(panel.pending.is_some());
    let before = execute(root, Action::MemoryConflicts).unwrap();
    assert_eq!(before[0]["status"], "open");
    assert!(panel.mutation(KeyCode::Char('a')));
    panel.key(root, KeyCode::Char('a')).unwrap();
    let after = execute(root, Action::MemoryConflicts).unwrap();
    assert_eq!(after[0]["status"], "resolved");
    assert!(panel.pending.is_none());
}
