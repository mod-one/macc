use crate::sessions::SessionsScreen;
use crossterm::event::KeyCode;
use macc_core::session_store as store;
fn root() -> tempfile::TempDir {
    let t = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(t.path().join(".macc")).unwrap();
    t
}
#[test]
fn stale_draft_is_not_rebased_silently_by_live_refresh() {
    let t = root();
    let mut screen = SessionsScreen::default();
    screen.refresh(t.path()).unwrap();
    screen.key(t.path(), KeyCode::Enter).unwrap();
    for _ in 0..4 {
        screen.key(t.path(), KeyCode::Backspace).unwrap();
    }
    for c in "900".chars() {
        screen.key(t.path(), KeyCode::Char(c)).unwrap();
    }
    screen.key(t.path(), KeyCode::Enter).unwrap();
    let view = store::inspect(t.path()).unwrap();
    let mut changed = view.policy.clone();
    changed.lifecycle.max_idle_hours = 48;
    store::update_policy(t.path(), &view.revision, &changed).unwrap();
    screen.refresh(t.path()).unwrap();
    assert!(screen
        .key(t.path(), KeyCode::Char('s'))
        .unwrap_err()
        .to_string()
        .contains("revision conflict"));
    assert!(!screen.key(t.path(), KeyCode::Esc).unwrap()); // draft is still present
    screen.key(t.path(), KeyCode::Char('x')).unwrap();
    assert!(screen.key(t.path(), KeyCode::Esc).unwrap());
}
#[test]
fn every_catalog_field_can_be_edited_and_saved_from_tui() {
    let t = root();
    let mut screen = SessionsScreen::default();
    screen.refresh(t.path()).unwrap();
    for _ in 0..store::fields().len() {
        screen.key(t.path(), KeyCode::Enter).unwrap();
        screen.key(t.path(), KeyCode::Enter).unwrap();
        screen.key(t.path(), KeyCode::Down).unwrap();
    }
    screen.key(t.path(), KeyCode::Char('p')).unwrap();
    screen.key(t.path(), KeyCode::Char('s')).unwrap();
    assert_eq!(store::inspect(t.path()).unwrap().policy_source, "project");
    screen.key(t.path(), KeyCode::Char('u')).unwrap();
    screen.key(t.path(), KeyCode::Char('s')).unwrap();
    assert_eq!(store::inspect(t.path()).unwrap().policy_source, "defaults");
}
