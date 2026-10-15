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
