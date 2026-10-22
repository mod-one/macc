use super::*;
use crate::workspace_tests::{fixture, render, text};
#[test]
fn hooks_are_discoverable_and_reviewed_changes_have_visible_commands() {
    let (_temp, mut state) = fixture();
    assert!(text(&render(&state, 120, 36)).contains("[H] Hooks"));
    assert!(Screen::Home
        .help_keybindings()
        .iter()
        .any(|(key, _)| *key == "H"));
    handle_key(&mut state, KeyCode::Char('H'));
    assert_eq!(state.current_screen(), Screen::Hooks);
    for (width, height) in [(80, 24), (120, 36)] {
        let output = text(&render(&state, width, height));
        assert!(output.contains("Hooks"));
        assert!(output.contains("[F2] Commands"));
    }
    handle_key(&mut state, KeyCode::Char(' '));
    assert!(state.hooks.modal());
    let popup = text(&render(&state, 120, 36));
    assert!(popup.contains("Review hook change"));
    assert!(popup.contains("Confirm"));
    handle_key(&mut state, KeyCode::Esc);
    assert!(!state.hooks.modal());
    assert_eq!(state.current_screen(), Screen::Hooks);
}
#[test]
fn hook_input_is_exclusive_and_cannot_navigate_or_mutate_from_typing() {
    let (_temp, mut state) = fixture();
    handle_key(&mut state, KeyCode::Char('H'));
    handle_key(&mut state, KeyCode::Char('i'));
    assert!(state.hooks.input_active());
    for key in "HOs".chars() {
        handle_key(&mut state, KeyCode::Char(key));
    }
    assert_eq!(state.current_screen(), Screen::Hooks);
    assert!(state.hooks.input_active());
    assert!(command_catalog::exclusive(&state));
    handle_key(&mut state, KeyCode::F(2));
    assert!(contextual_items(&state)
        .iter()
        .all(|c| matches!(c.action, CommandPaletteAction::Key(_))));
}
#[test]
fn dirty_config_is_not_discarded_by_immediate_hook_settings() {
    let (_temp, mut state) = fixture();
    state.working_copy.as_mut().unwrap().settings.quiet = true;
    handle_key(&mut state, KeyCode::Char('H'));
    handle_key(&mut state, KeyCode::Char(' '));
    handle_key(&mut state, KeyCode::Enter);
    assert!(text(&render(&state, 120, 36)).contains("Save or discard"));
    assert!(state.working_copy.as_ref().unwrap().settings.quiet);
    assert!(state.hooks.modal());
}
