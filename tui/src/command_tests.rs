//! Render and dispatch regressions for the shared command surface.
use super::*;
use crate::workspace_tests::{fixture, render, text};

#[test]
fn o2_settings_footer_shows_preview_save_and_discard_at_terminal_widths() {
    let (_t, mut state) = fixture();
    navigation::Destination::Memory.open(&mut state);
    handle_shortcut(&mut state, KeyCode::Char('t'), true);
    for width in [80, 120, 190] {
        let buffer = render(&state, width, 36);
        let footer = text(&buffer)
            .lines()
            .skip(32)
            .collect::<Vec<_>>()
            .join("\n");
        assert!(footer.contains("[p] Preview"), "{width}: {footer}");
        assert!(footer.contains("[s] Save"), "{width}: {footer}");
        assert!(footer.contains("[x] Discard"), "{width}: {footer}");
    }
}

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
        Screen::Sessions,
        Screen::O2,
        Screen::About,
        Screen::Watch,
    ] {
        if matches!(screen, Screen::Sessions | Screen::O2) {
            session_navigation::open(&mut state, screen == Screen::O2, false);
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
            assert!(footer.contains("[:] Commands"), "{screen:?}: {footer}");
            assert!(footer.contains("[?] Help"), "{screen:?}: {footer}");
        }
        handle_key(&mut state, KeyCode::Char(':'));
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
    handle_key(&mut state, KeyCode::Char(':'));
    let items = contextual_items(&state);
    assert!(items.iter().all(|c| matches!(
        c.action,
        command_catalog::Action::Widget(_)
            | command_catalog::Action::Help
            | command_catalog::Action::Palette
            | command_catalog::Action::Quit
    )));
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
        handle_key(&mut state, KeyCode::Char('?'));
        assert!(state.help_open);
        handle_key(&mut state, KeyCode::Esc);
        handle_key(&mut state, KeyCode::Char(':'));
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
    handle_shortcut(&mut state, KeyCode::Char('p'), true);
    assert!(state.command_palette_open);
    assert!(contextual_items(&state).iter().all(|c| matches!(
        c.action,
        command_catalog::Action::Widget(_)
            | command_catalog::Action::Help
            | command_catalog::Action::Palette
            | command_catalog::Action::Quit
    )));
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
        .any(|c| c.shortcut() == "r" && c.label == "Run coordinator" && c.enabled));
    assert!(commands
        .iter()
        .any(|c| c.shortcut() == "t" && c.label.contains("Retry") && !c.enabled));
    assert!(commands
        .iter()
        .any(|c| c.shortcut() == "l" && c.label == "Toggle logs"));
    let mut seen = std::collections::HashSet::new();
    for c in commands.into_iter().filter(|c| c.key.is_some()) {
        assert!(
            seen.insert(c.shortcut()),
            "Duplicate shortcut {}",
            c.shortcut()
        );
    }
}

#[test]
fn palette_scroll_keeps_the_selected_command_visible() {
    let (_t, mut state) = fixture();
    state.goto_screen(Screen::CoordinatorLive);
    handle_key(&mut state, KeyCode::Char(':'));
    for _ in 0..100 {
        handle_key(&mut state, KeyCode::Down);
    }
    let items = contextual_items(&state);
    let label = &items[state.command_palette_selection].title;
    assert!(text(&render(&state, 120, 36)).contains(label));
    handle_key(&mut state, KeyCode::Char(':'));
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
    handle_key(&mut state, KeyCode::Char(':'));
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
    handle_key(&mut state, KeyCode::Char(':'));
    assert!(contextual_items(&state).iter().all(|c| matches!(
        c.action,
        command_catalog::Action::Widget(_)
            | command_catalog::Action::Help
            | command_catalog::Action::Palette
            | command_catalog::Action::Quit
    )));
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

fn audit_bindings(state: &AppState) {
    let commands = command_catalog::active(state);
    let palette = contextual_items(state);
    assert_eq!(commands.len(), palette.len());
    let mut seen = std::collections::HashSet::new();
    for (c, item) in commands.iter().zip(palette) {
        assert_eq!((c.action, c.enabled), (item.action, item.enabled));
        assert!(item.title.contains(&c.label));
        if let Some(key) = c.key {
            assert!(
                seen.insert((key, c.control)),
                "Duplicate {} in {:?}/{}: {}",
                c.shortcut(),
                state.current_screen(),
                state.sessions.section_name(),
                c.label
            );
            let binding = command_catalog::resolve(state, key, c.control).unwrap();
            assert_eq!((binding.action, binding.enabled), (c.action, c.enabled));
        }
    }
}

#[test]
fn each_destination_and_workspace_section_has_an_unambiguous_keymap() {
    let (_dir, mut s) = fixture();
    for &dest in navigation::ALL {
        dest.open(&mut s);
        audit_bindings(&s);
    }
    for (dest, count) in [
        (navigation::Destination::Memory, 4),
        (navigation::Destination::MemorySettings, 3),
    ] {
        dest.open(&mut s);
        for i in 0..count {
            handle_key(&mut s, KeyCode::Char((b'1' + i) as char));
            assert_eq!(navigation::mode(&s), dest.mode());
            audit_bindings(&s);
        }
        handle_key(&mut s, KeyCode::Tab);
        assert_eq!(navigation::mode(&s), dest.mode());
    }
    navigation::Destination::Coordinator.open(&mut s);
    for key in '1'..='7' {
        handle_key(&mut s, KeyCode::Char(key));
        audit_bindings(&s);
    }
    s.goto_screen(Screen::ToolSettings);
    audit_bindings(&s);
    s.goto_screen(Screen::Apply);
    audit_bindings(&s);
    s.goto_screen(Screen::Preview);
    audit_bindings(&s);
}

#[test]
fn help_and_palette_expose_the_same_unavailable_action_as_the_keyboard() {
    let (_dir, mut s) = fixture();
    navigation::Destination::Live.open(&mut s);
    let disabled = command_catalog::resolve(&s, KeyCode::Char('x'), false).unwrap();
    assert!(!disabled.enabled);
    handle_key(&mut s, KeyCode::Char('?'));
    let help = text(&render(&s, 160, 50));
    assert!(help.contains(&format!("[x] {} [unavailable]", disabled.label)));
    handle_key(&mut s, KeyCode::Esc);
    handle_key(&mut s, KeyCode::Char(':'));
    s.command_palette_query = disabled.label.clone();
    assert_eq!(contextual_items(&s)[0].action, disabled.action);
    handle_key(&mut s, KeyCode::Enter);
    assert!(s.command_palette_open);
    handle_key(&mut s, KeyCode::Esc);
    handle_key(&mut s, KeyCode::Char('x'));
    assert!(s.coordinator_stop_task_confirm_id.is_none());
}

#[test]
fn mode_twins_preserve_drafts_and_changes_require_save_before_apply() {
    let (_dir, mut s) = fixture();
    for _ in 0..4 {
        handle_shortcut(&mut s, KeyCode::Char('t'), true);
        assert_eq!(s.current_screen(), Screen::Changes);
        handle_shortcut(&mut s, KeyCode::Char('t'), true);
        assert_eq!(s.screen_stack, vec![Screen::Home]);
    }
    s.working_copy.as_mut().unwrap().settings.quiet = true;
    handle_shortcut(&mut s, KeyCode::Char('t'), true);
    assert!(text(&render(&s, 120, 32)).contains("settings.quiet"));
    handle_key(&mut s, KeyCode::Char('a'));
    assert_eq!(s.current_screen(), Screen::Changes);
    assert!(!s.apply_preparing());
    handle_key(&mut s, KeyCode::Enter);
    assert_eq!(s.current_screen(), Screen::Automation);
    assert!(s.working_copy.as_ref().unwrap().settings.quiet);
}

#[test]
fn recorded_preconditions_expose_only_relevant_pause_actions() {
    let (_dir, mut s) = fixture();
    s.goto_screen(Screen::CoordinatorLive);
    s.coordinator_pause_error = Some("[E903] External runner unavailable".into());
    for key in [KeyCode::Char('r'), KeyCode::Char('s'), KeyCode::Enter] {
        assert!(command_catalog::resolve(&s, key, false).is_none());
        handle_key(&mut s, key);
        assert!(s.has_coordinator_pause_prompt());
    }
    audit_bindings(&s);
    handle_key(&mut s, KeyCode::Esc);
    assert!(!s.has_coordinator_pause_prompt());
}

#[test]
fn a_visible_decision_takes_focus_above_a_background_task_detail() {
    let (_dir, mut s) = fixture();
    s.goto_screen(Screen::CoordinatorLive);
    s.coordinator_task_diff_popup = Some("retained detail".into());
    s.decision_center.show();
    let cancel = command_catalog::resolve(&s, KeyCode::Esc, false).unwrap();
    assert!(cancel.label.contains("draft"));
    handle_key(&mut s, KeyCode::Esc);
    assert!(!s.decision_center.open);
    assert!(s.coordinator_task_diff_popup.is_some());
    handle_key(&mut s, KeyCode::Esc);
    assert!(s.coordinator_task_diff_popup.is_none());
}

#[test]
fn evidence_and_filters_remain_available_after_switching_modes() {
    let (_dir, mut s) = fixture();
    handle_key(&mut s, KeyCode::Char('I'));
    handle_key(&mut s, KeyCode::Char('3'));
    assert!(text(&render(&s, 80, 24)).contains("No recorded decisions"));
    handle_shortcut(&mut s, KeyCode::Char('t'), true);
    assert_eq!(s.config_tab_index, 7);
    handle_shortcut(&mut s, KeyCode::Char('t'), true);
    assert_eq!(s.current_screen(), Screen::Inbox);
    assert_eq!(s.inbox_section, crate::inbox::Section::History);
    handle_key(&mut s, KeyCode::Char('C'));
    handle_key(&mut s, KeyCode::Char('/'));
    assert!(s.search_editing);
    for c in "Task:O?".chars() {
        handle_key(&mut s, KeyCode::Char(c));
    }
    assert_eq!(s.search_query, "Task:O?");
    assert_eq!(s.current_screen(), Screen::CoordinatorLive);
}

#[test]
fn doctor_offers_reset_only_for_the_displayed_throttle() {
    let (_dir, mut s) = fixture();
    s.goto_screen(Screen::Doctor);
    assert!(command_catalog::resolve(&s, KeyCode::Enter, false).is_none());
    s.coordinator_throttled_tools
        .push(state::ThrottledToolInfo {
            tool_id: "fixture-tool".into(),
            throttled_until: "2099-01-01T00:00:00Z".into(),
            display_until: "2099-01-01 UTC".into(),
            backoff_seconds: 300,
            consecutive_count: 1,
        });
    let reset = command_catalog::resolve(&s, KeyCode::Enter, false).unwrap();
    assert_eq!(reset.action, command_catalog::Action::Cooldown);
    assert!(text(&render(&s, 120, 32)).contains("fixture-tool cooldown"));
    s.goto_screen(Screen::Tools);
    assert!(!command_catalog::active(&s)
        .iter()
        .any(|c| c.action == command_catalog::Action::Cooldown));
}

#[test]
fn session_operations_cannot_edit_the_policy_from_an_empty_inventory() {
    let (_dir, mut s) = fixture();
    navigation::Destination::Sessions.open(&mut s);
    for key in [KeyCode::Down, KeyCode::Enter, KeyCode::Char('s')] {
        handle_key(&mut s, key);
    }
    assert!(!s.sessions.input_active());
    assert_eq!(navigation::mode(&s), navigation::Mode::Operate);
    handle_shortcut(&mut s, KeyCode::Char('t'), true);
    assert_eq!(navigation::mode(&s), navigation::Mode::Configure);
    assert!(command_catalog::resolve(&s, KeyCode::Char('s'), false).is_some());
}

#[test]
fn redo_and_evidence_use_distinct_control_keys() {
    let (_root, mut state) = fixture();
    assert!(text(&render(&state, 120, 32)).contains("C  Coordinator live"));
    handle_shortcut(&mut state, KeyCode::Char('t'), true);
    let before = state.working_copy.as_ref().unwrap().settings.quiet;
    state.snapshot_before_config_change();
    state.working_copy.as_mut().unwrap().settings.quiet = !before;
    handle_key(&mut state, KeyCode::Char('u'));
    assert_eq!(state.working_copy.as_ref().unwrap().settings.quiet, before);
    handle_shortcut(&mut state, KeyCode::Char('r'), true);
    assert_eq!(state.working_copy.as_ref().unwrap().settings.quiet, before);
    handle_shortcut(&mut state, KeyCode::Char('z'), true);
    assert_eq!(state.working_copy.as_ref().unwrap().settings.quiet, !before);
    assert!(state.redo_stack.is_empty());

    navigation::Destination::Doctor.open(&mut state);
    let initial = state.doctor_raw;
    handle_shortcut(&mut state, KeyCode::Char('r'), true);
    assert_ne!(state.doctor_raw, initial);
    for n in 1..=12 {
        let key = KeyCode::F(n);
        assert!(command_catalog::resolve(&state, key, false).is_none());
        handle_key(&mut state, key);
    }
    assert!(!state.command_palette_open && !state.help_open);
    assert_eq!(state.current_screen(), Screen::Doctor);
    assert_ne!(state.doctor_raw, initial);
    handle_shortcut(&mut state, KeyCode::Char('p'), true);
    assert!(state.command_palette_open);
    handle_shortcut(&mut state, KeyCode::Char('p'), true);
    assert!(!state.command_palette_open);
}

#[test]
fn ctrl_c_quits_from_pages_editors_and_overlays() {
    let (_dir, mut s) = fixture();
    let ctrl_c = KeyCode::Char('c');
    for context in 0..5 {
        match context {
            0 => navigation::Destination::Live.open(&mut s),
            1 => s.coordinator_stop_dialog_open = true,
            2 => {
                s.coordinator_stop_dialog_open = false;
                s.goto_screen(Screen::Automation);
                s.automation_field_editing = true;
                handle_key(&mut s, ctrl_c);
                assert!(s.automation_field_input.ends_with('c'));
            }
            3 => open_command_palette(&mut s),
            _ => {
                close_command_palette(&mut s);
                s.toggle_help();
            }
        }
        assert!(!s.should_quit);
        assert_eq!(
            command_catalog::resolve(&s, ctrl_c, true).unwrap().action,
            command_catalog::Action::Quit
        );
        handle_shortcut(&mut s, ctrl_c, true);
        assert!(s.should_quit);
        s.should_quit = false;
    }
}

#[test]
fn manual_approval_inbox_shows_count_and_review_requirements_without_a_popup() {
    use macc_core::coordinator::approval::PendingApproval;
    let (_dir, mut state) = fixture();
    let gate = |task: &str, awaiting| PendingApproval {
        task_id: task.into(),
        awaiting_decision: awaiting,
        auto_prompt: false,
        subject_task: "DELIVERY".into(),
        subject_revision: Some("abc123456789".into()),
        missing_roles: vec!["AUDITOR".into()],
        evidence_type: Some("pull_request_review".into()),
        evidence_required: true,
        ..Default::default()
    };
    state.coordinator_pending_approvals = vec![
        gate("PAST", false),
        gate("APP-1", true),
        gate("APP-2", true),
    ];
    state.sync_approval_window();
    assert!(state.approval_window.is_none());
    handle_key(&mut state, KeyCode::Char('I'));
    handle_key(&mut state, KeyCode::Char('2'));
    let display = text(&render(&state, 140, 36));
    for phrase in [
        "Inbox ▲ 2",
        "Approvals (2)",
        "Revision: abc123456789",
        "Missing roles: AUDITOR",
        "Evidence: pull_request_review",
    ] {
        assert!(display.contains(phrase), "Missing {phrase} in {display}");
    }
    handle_key(&mut state, KeyCode::Down);
    handle_key(&mut state, KeyCode::Enter);
    assert_eq!(
        state.approval_window.as_ref().unwrap().gate.task_id,
        "APP-2"
    );
    state.sync_approval_window();
    assert!(state.approval_window.as_ref().unwrap().opened_manually);
    state.coordinator_pending_approvals.pop();
    state.sync_approval_window();
    assert!(state.approval_window.is_none());
    assert_eq!(state.inspector_index, 0);
    assert!(text(&render(&state, 140, 36)).contains("Approvals (1)"));
}
