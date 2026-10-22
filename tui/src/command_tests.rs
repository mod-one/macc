//! Render and dispatch regressions for the shared command surface.
use super::*;
use crate::workspace_tests::{fixture, render, text};

#[test]
fn every_page_has_visible_commands_and_opens_the_same_palette() {
    let (_t, mut state) = fixture();
    for screen in [
        Screen::Home,
        Screen::Tools,
        Screen::Automation,
        Screen::CoordinatorLive,
        Screen::Mcp,
        Screen::Logs,
        Screen::Skills,
        Screen::Hooks,
        Screen::Agents,
        Screen::ToolSettings,
        Screen::Preview,
        Screen::Apply,
        Screen::Settings,
        Screen::Sessions,
        Screen::O2,
        Screen::About,
        Screen::Watch,
    ] {
        if matches!(screen, Screen::Sessions | Screen::O2) {
            session_navigation::open(&mut state, screen == Screen::O2);
        } else {
            state.goto_screen(screen);
        }
        for (width, height) in [(80, 24), (120, 36)] {
            let buffer = render(&state, width, height);
            let footer = text(&buffer)
                .lines()
                .skip(height as usize - 4)
                .collect::<Vec<_>>()
                .join("\n");
            assert!(footer.contains("[F2] Commands"), "{screen:?}: {footer}");
            assert!(footer.contains("[F1] Help"), "{screen:?}: {footer}");
        }
        handle_key(&mut state, KeyCode::F(2));
        assert!(state.command_palette_open, "{screen:?}");
        assert!(text(&render(&state, 80, 24)).contains("Command palette"));
        assert!(!contextual_items(&state).is_empty(), "{screen:?}");
        handle_key(&mut state, KeyCode::Esc);
        assert_eq!(state.current_screen(), screen);
    }
}

#[test]
fn modal_palette_controls_the_modal_without_exposing_page_actions() {
    let (_t, mut state) = fixture();
    state.goto_screen(Screen::CoordinatorLive);
    state.coordinator_stop_dialog_open = true;
    handle_key(&mut state, KeyCode::F(2));
    let items = contextual_items(&state);
    assert!(items
        .iter()
        .all(|c| matches!(c.action, CommandPaletteAction::Key(_))));
    assert!(items.iter().any(|c| c.title.contains("Cancel")));
    handle_key(&mut state, KeyCode::Down);
    assert_eq!(state.coordinator_stop_dialog_selection, 0);
    for c in "Cancel".chars() {
        handle_key(&mut state, KeyCode::Char(c));
    }
    handle_key(&mut state, KeyCode::Enter);
    assert!(!state.command_palette_open);
    assert!(!state.coordinator_stop_dialog_open);
    assert!(!state.is_coordinator_running());
}

#[test]
fn nested_windows_keep_their_commands_in_the_bottom_area() {
    let (_t, mut state) = fixture();
    state.goto_screen(Screen::CoordinatorLive);
    for variant in 0..6 {
        match variant {
            0 => state.coordinator_task_diff_popup = Some("Example diff".into()),
            1 => state.coordinator_stop_dialog_open = true,
            2 => state.coordinator_recover_dialog_open = true,
            3 => state.coordinator_stop_task_confirm_id = Some("TASK-1".into()),
            4 => state.coordinator_pause_error = Some("Example failure".into()),
            _ => {
                state.prd_queue_editor =
                    Some(prd_queue::Editor::new(std::path::PathBuf::new(), vec![]))
            }
        }
        let before = text(&render(&state, 80, 24));
        assert!(
            before.lines().rev().take(4).any(|l| l.contains("[Esc]")),
            "{variant}: {before}"
        );
        handle_key(&mut state, KeyCode::F(1));
        assert!(state.help_open);
        handle_key(&mut state, KeyCode::Esc);
        handle_key(&mut state, KeyCode::F(2));
        assert!(state.command_palette_open);
        assert!(text(&render(&state, 80, 24)).contains("[Enter] Run"));
        handle_key(&mut state, KeyCode::Esc);
        state.coordinator_task_diff_popup = None;
        state.coordinator_stop_dialog_open = false;
        state.coordinator_recover_dialog_open = false;
        state.coordinator_stop_task_confirm_id = None;
        state.coordinator_pause_error = None;
        state.prd_queue_editor = None;
    }
}

#[test]
fn colon_and_navigation_letters_are_literal_in_configuration_input() {
    let (_t, mut state) = fixture();
    state.goto_screen(Screen::Automation);
    state.automation_field_editing = true;
    state.automation_field_input.clear();
    for c in "https://host/path?x=y".chars() {
        handle_key(&mut state, KeyCode::Char(c));
    }
    assert_eq!(state.automation_field_input, "https://host/path?x=y");
    assert!(!state.command_palette_open);
    assert!(!state.help_open);
    handle_key(&mut state, KeyCode::F(2));
    assert!(contextual_items(&state)
        .iter()
        .all(|c| matches!(c.action, CommandPaletteAction::Key(_))));
    handle_key(&mut state, KeyCode::Esc);
    assert!(state.automation_field_editing);
    assert_eq!(state.automation_field_input, "https://host/path?x=y");
}

#[test]
fn live_commands_distinguish_run_from_requeue_and_match_the_log_action() {
    let (_t, mut state) = fixture();
    state.goto_screen(Screen::CoordinatorLive);
    let commands = command_catalog::active(&state);
    assert!(commands
        .iter()
        .any(|c| c.shortcut == "R" && c.label == "Run coordinator" && c.enabled));
    assert!(commands
        .iter()
        .any(|c| c.shortcut == "r" && c.label.contains("Requeue") && !c.enabled));
    assert!(commands
        .iter()
        .any(|c| c.shortcut == "l" && c.label == "Toggle logs"));
    let mut seen = std::collections::HashSet::new();
    for c in commands {
        assert!(
            seen.insert(c.shortcut.clone()),
            "Duplicate shortcut {}",
            c.shortcut
        );
    }
}

#[test]
fn palette_scroll_keeps_the_selected_command_visible() {
    let (_t, mut state) = fixture();
    state.goto_screen(Screen::CoordinatorLive);
    handle_key(&mut state, KeyCode::F(2));
    for _ in 0..100 {
        handle_key(&mut state, KeyCode::Down);
    }
    let items = contextual_items(&state);
    let label = &items[state.command_palette_selection].title;
    assert!(text(&render(&state, 120, 36)).contains(label));
    handle_key(&mut state, KeyCode::F(2));
    assert!(!state.command_palette_open);
}

#[test]
fn approval_and_installation_keep_their_confirmation_steps() {
    let (_t, mut state) = fixture();
    state.goto_screen(Screen::CoordinatorLive);
    state.approval_window = Some(approval::Window::new(
        macc_core::coordinator::approval::PendingApproval {
            task_id: "APPROVE-1".into(),
            subject_task: "TASK-1".into(),
            required_roles: vec!["OWNER".into(), "REVIEWER".into()],
            missing_roles: vec!["OWNER".into(), "REVIEWER".into()],
            awaiting_decision: true,
            ..Default::default()
        },
        1,
    ));
    handle_key(&mut state, KeyCode::F(2));
    for c in "Approve".chars() {
        handle_key(&mut state, KeyCode::Char(c));
    }
    handle_key(&mut state, KeyCode::Enter);
    assert!(state.approval_window.as_ref().unwrap().is_editing());
    assert!(!state.is_coordinator_running());
    assert!(command_catalog::active(&state)
        .iter()
        .any(|c| c.label == "Choose role"));
    assert!(text(&render(&state, 80, 24)).contains("[Esc] Cancel decision"));
    state.approval_window = None;
    state.goto_screen(Screen::Tools);
    state.tool_install_confirm_id = Some("future-ai".into());
    handle_key(&mut state, KeyCode::F(2));
    assert!(contextual_items(&state)
        .iter()
        .all(|c| matches!(c.action, CommandPaletteAction::Key(_))));
    for c in "Cancel".chars() {
        handle_key(&mut state, KeyCode::Char(c));
    }
    handle_key(&mut state, KeyCode::Enter);
    assert!(state.tool_install_confirm_id.is_none());
    assert_eq!(state.current_screen(), Screen::Tools);
}

#[test]
fn takeover_modal_blocks_page_keys_and_stop_dialog_keeps_escape_priority() {
    use macc_core::process_ownership::{ClientIdentity, ClientKind, TakeoverRequest};
    let (_t, mut state) = fixture();
    state.goto_screen(Screen::CoordinatorLive);
    state.handle_takeover_request_received(TakeoverRequest {
        request_id: "REQUEST-1".into(),
        requested_at: String::new(),
        requester: ClientIdentity {
            client_id: "other-viewer".into(),
            kind: ClientKind::Tui,
            connected_at: String::new(),
            last_heartbeat: String::new(),
        },
    });
    handle_key(&mut state, KeyCode::Char('h'));
    assert_eq!(state.current_screen(), Screen::CoordinatorLive);
    state.coordinator_stop_dialog_open = true;
    handle_key(&mut state, KeyCode::Esc);
    assert!(!state.coordinator_stop_dialog_open);
    assert!(state
        .coordinator_ownership
        .pending_incoming_request
        .is_some());
    handle_key(&mut state, KeyCode::Esc);
    assert!(state
        .coordinator_ownership
        .pending_incoming_request
        .is_none());
}
