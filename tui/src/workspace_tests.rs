//! Test actual application routing and terminal buffers, not duplicated UI decisions.
use super::*;
use macc_core::{tool::ToolRegistry, MaccEngine, ProjectPaths};
use ratatui::{backend::TestBackend, buffer::Buffer};
use std::sync::Arc;
pub(super) fn fixture() -> (tempfile::TempDir, AppState) {
    let t = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(t.path().join(".macc")).unwrap();
    std::fs::write(t.path().join(".macc/macc.yaml"), "tools:\n  enabled: []\n").unwrap();
    let mut state = AppState::with_engine(Arc::new(MaccEngine::new(ToolRegistry::new())));
    state.project_paths = Some(ProjectPaths::from_root(t.path()));
    state.config = Some(Default::default());
    state.working_copy = Some(Default::default());
    (t, state)
}
pub(super) fn render(state: &AppState, width: u16, height: u16) -> Buffer {
    let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
    terminal.draw(|f| ui(f, state, true)).unwrap();
    terminal.backend().buffer().clone()
}
pub(super) fn text(b: &Buffer) -> String {
    (0..b.area.height)
        .map(|y| {
            (0..b.area.width)
                .map(|x| b.get(x, y).symbol())
                .collect::<String>()
        })
        .collect::<Vec<_>>()
        .join("\n")
}
pub(super) fn export_buffer(name: &str, state: &AppState, width: u16, height: u16) {
    let Ok(directory) = std::env::var("MACC_TUI_SNAPSHOT_DIR") else {
        return;
    };
    let directory = std::path::Path::new(&directory);
    std::fs::create_dir_all(directory).unwrap();
    let b = render(state, width, height);
    std::fs::write(directory.join(format!("{name}-{width}.txt")), text(&b)).unwrap();
    let cells: Vec<_> = b
        .content
        .iter()
        .map(|cell| {
            serde_json::json!({
                "text": cell.symbol(),
                "fg": format!("{:?}", cell.fg),
                "bg": format!("{:?}", cell.bg),
                "bold": cell.modifier.contains(Modifier::BOLD),
                "underline": cell.modifier.contains(Modifier::UNDERLINED),
            })
        })
        .collect();
    std::fs::write(
        directory.join(format!("{name}-{width}.json")),
        serde_json::json!({"width":width,"height":height,"cells":cells}).to_string(),
    )
    .unwrap();
}
#[test]
fn narrow_section_bar_keeps_the_selected_tab_visible() {
    let mut terminal = Terminal::new(TestBackend::new(22, 3)).unwrap();
    terminal
        .draw(|f| {
            workspace_ui::tab_line(
                f,
                f.size(),
                &["Settings", "Conversations", "History"],
                2,
                theme().accent,
            );
        })
        .unwrap();
    assert!(text(terminal.backend().buffer()).contains("3 History"));
}
#[test]
fn o2_is_a_direct_destination_in_home_palette_help_and_breadcrumbs() {
    let (_t, mut state) = fixture();
    let home = text(&render(&state, 120, 32));
    assert!(home.contains("O  O2 Memory"));
    assert!(home.contains("S  Sessions"));
    state.command_palette_query = "memory".into();
    assert!(contextual_items(&state)
        .iter()
        .any(|v| v.action == command_catalog::Action::Go(navigation::Destination::Memory)));
    state.command_palette_query.clear();
    assert!(command_catalog::active(&state)
        .iter()
        .any(|c| c.key == Some(KeyCode::Char('O')) && c.label.contains("O2")));
    handle_key(&mut state, KeyCode::Char('O'));
    assert_eq!(state.current_screen(), Screen::O2);
    let screen = text(&render(&state, 120, 32));
    assert!(screen.contains("Home > O2 Memory & Context"));
    assert!(screen.contains("Get started"));
    assert!(screen.contains("SQLite"));
    assert!(!screen.contains("PRD/task missing"));
    handle_key(&mut state, KeyCode::Char('?'));
    assert!(state.help_open);
    let help = text(&render(&state, 120, 32));
    assert!(help.contains("Help for the current page or window"));
    handle_key(&mut state, KeyCode::PageDown);
    assert!(state.help_scroll > 0);
    handle_key(&mut state, KeyCode::Esc);
    assert!(!state.help_open);
    assert_eq!(state.current_screen(), Screen::O2);
}
#[test]
fn standard_terminal_shows_selected_setting_actions_and_empty_conversations() {
    let (_t, mut state) = fixture();
    handle_shortcut(&mut state, KeyCode::Char('t'), true);
    handle_key(&mut state, KeyCode::Char('S'));
    let small = text(&render(&state, 80, 24));
    assert!(small.contains("[:] Commands"));
    assert!(small.contains("Lease TTL"));
    assert!(small.contains("Details"));
    assert!(small.contains("Settings"));
    handle_shortcut(&mut state, KeyCode::Char('t'), true);
    let empty = text(&render(&state, 120, 32));
    assert!(empty.contains("No recorded conversations"));
    assert!(empty.contains("Run a coordinator task"));
    handle_key(&mut state, KeyCode::Char('?'));
    assert!(state.help_open);
}
#[test]
fn selected_menu_mutations_use_the_same_project_owner_gate_as_shortcuts() {
    use macc_core::{
        process_ownership::{ClientIdentity, ClientKind, ProcessHandle, ProcessKind},
        service::{process_ownership::claim_owner, process_ownership_gate::ClientContext},
    };
    let (t, mut state) = fixture();
    let handle = ProcessHandle {
        kind: ProcessKind::Project,
        project_root: t.path().into(),
        pid: None,
    };
    let now = chrono::Utc::now().to_rfc3339();
    claim_owner(
        t.path(),
        &handle,
        ClientIdentity {
            client_id: "other-owner".into(),
            kind: ClientKind::Cli,
            connected_at: now.clone(),
            last_heartbeat: now,
        },
    )
    .unwrap();
    state.client_context = ClientContext {
        client_id: "viewer".into(),
        project_root: t.path().into(),
    };
    handle_shortcut(&mut state, KeyCode::Char('t'), true);
    handle_key(&mut state, KeyCode::Char('S'));
    handle_key(&mut state, KeyCode::Char(':'));
    let menu = text(&render(&state, 120, 32));
    assert!(menu.contains("Save session settings"));
    // A searched action uses the same project owner gate as its shortcut.
    for c in "Save session settings".chars() {
        handle_key(&mut state, KeyCode::Char(c));
    }
    handle_key(&mut state, KeyCode::Enter);
    assert!(state.sessions.message.contains("not the owner"));
    assert_eq!(
        macc_core::session_store::inspect(t.path())
            .unwrap()
            .policy_source,
        "defaults"
    );
    // Disabled confirmation does not execute or consume any plan.
    handle_key(&mut state, KeyCode::Char(':'));
    for c in "Confirm reviewed operation".chars() {
        handle_key(&mut state, KeyCode::Char(c));
    }
    handle_key(&mut state, KeyCode::Enter);
    assert!(state.command_palette_open);
    handle_key(&mut state, KeyCode::Esc);
    assert_eq!(
        macc_core::session_store::inspect(t.path()).unwrap().backend,
        "json"
    );
}
#[test]
fn settings_groups_forms_and_workspace_switches_preserve_drafts() {
    let (_t, mut state) = fixture();
    handle_key(&mut state, KeyCode::Char('O'));
    handle_shortcut(&mut state, KeyCode::Char('t'), true);
    let before = text(&render(&state, 120, 32));
    assert!(before.contains("Memory settings"));
    handle_key(&mut state, KeyCode::Left);
    assert!(text(&render(&state, 120, 32)).contains("▸ Ledger"));
    handle_key(&mut state, KeyCode::Right);
    assert!(text(&render(&state, 120, 32)).contains("▾ Ledger"));
    handle_key(&mut state, KeyCode::Char(']'));
    assert!(text(&render(&state, 120, 32)).contains("Context settings"));
    handle_key(&mut state, KeyCode::Char('/'));
    for c in "memory.enabled".chars() {
        handle_key(&mut state, KeyCode::Char(c));
    }
    handle_key(&mut state, KeyCode::Enter);
    handle_key(&mut state, KeyCode::Enter);
    handle_key(&mut state, KeyCode::Char('S'));
    assert_eq!(state.current_screen(), Screen::Sessions);
    handle_key(&mut state, KeyCode::Char('O'));
    assert_eq!(state.current_screen(), Screen::O2);
    let draft = text(&render(&state, 120, 32));
    assert!(draft.contains("Edited"));
    handle_key(&mut state, KeyCode::Esc);
    assert_eq!(state.current_screen(), Screen::O2);
    assert!(text(&render(&state, 120, 32)).contains("Unsaved policy draft"));
}
#[test]
fn export_terminal_evidence_when_requested() {
    let Ok(directory) = std::env::var("MACC_TUI_SNAPSHOT_DIR") else {
        return;
    };
    let directory = std::path::Path::new(&directory);
    std::fs::create_dir_all(directory).unwrap();
    let (_t, mut state) = fixture();
    use navigation::Destination;
    for (name, destination) in [
        ("sessions", Destination::Sessions),
        ("o2-overview", Destination::Memory),
        ("o2-settings", Destination::MemorySettings),
        ("coordinator-live", Destination::Live),
        ("hooks", Destination::Hooks),
        ("coordinator-phases", Destination::Coordinator),
        ("changes", Destination::Changes),
    ] {
        destination.open(&mut state);
        if name == "coordinator-phases" {
            handle_key(&mut state, KeyCode::Char('3'));
        }
        if name == "changes" {
            let phases = &mut state
                .working_copy
                .as_mut()
                .unwrap()
                .automation
                .coordinator
                .get_or_insert_with(Default::default)
                .phases;
            phases.testing.enabled = true;
            phases.testing.mode = "required".into();
        }
        if name == "changes" {
            export_buffer(name, &state, 160, 40);
        }
        for (width, height) in [(120, 36), (80, 24)] {
            export_buffer(name, &state, width, height);
        }
    }
    handle_key(&mut state, KeyCode::Char(':'));
    std::fs::write(
        directory.join("actions-menu.txt"),
        text(&render(&state, 120, 36)),
    )
    .unwrap();
}

#[test]
fn storage_review_cancels_without_migration_and_form_text_keeps_navigation_letters() {
    let (t, mut state) = fixture();
    handle_shortcut(&mut state, KeyCode::Char('t'), true);
    handle_key(&mut state, KeyCode::Char('S'));
    handle_key(&mut state, KeyCode::Char('m'));
    assert!(text(&render(&state, 120, 32)).contains("REVIEW REQUIRED"));
    handle_key(&mut state, KeyCode::Esc);
    assert_eq!(state.current_screen(), Screen::Sessions);
    assert_eq!(
        macc_core::session_store::inspect(t.path()).unwrap().backend,
        "json"
    );
    handle_key(&mut state, KeyCode::Char('O'));
    handle_key(&mut state, KeyCode::Char('/'));
    for c in "JOB".chars() {
        handle_key(&mut state, KeyCode::Char(c));
    }
    assert_eq!(state.current_screen(), Screen::O2);
    assert!(state.sessions.input_active());
    assert!(text(&render(&state, 80, 24)).contains("JOB"));
    handle_key(&mut state, KeyCode::Esc);
    assert!(!state.sessions.input_active());
}

#[test]
fn conversation_table_uses_the_session_inventory_and_inspects_the_selected_identity() {
    let (_t, mut state) = fixture();
    handle_key(&mut state, KeyCode::Char('S'));
    state
        .sessions
        .view
        .as_mut()
        .unwrap()
        .sessions
        .push(macc_core::session_store::SessionView {
            id: "session-example".into(),
            project_id: "project-example".into(),
            task_id: "TASK-42".into(),
            role: "reviewer".into(),
            tool_id: "future-ai".into(),
            provider_session_id: None,
            lineage_id: "lineage".into(),
            status: "closed".into(),
            generation: 2,
            use_count: 4,
            owner_pid: None,
            expires_at: None,
            updated_at: 1,
        });
    let selected = text(&render(&state, 120, 36));
    assert!(selected.contains("TASK-42"));
    assert!(selected.contains("future-ai"));
    assert!(selected.contains("reviewer"));
    handle_key(&mut state, KeyCode::Enter);
    assert!(text(&render(&state, 120, 36)).contains("session-example"));
}

#[test]
fn literal_colon_stays_in_session_input_and_palette_preserves_the_draft() {
    let (_t, mut state) = fixture();
    handle_shortcut(&mut state, KeyCode::Char('t'), true);
    handle_key(&mut state, KeyCode::Char('S'));
    handle_key(&mut state, KeyCode::Enter);
    assert!(state.sessions.input_active());
    handle_key(&mut state, KeyCode::Char(':'));
    assert!(!state.command_palette_open);
    assert!(state.sessions.input_active());
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
    assert!(state.sessions.input_active());
    handle_key(&mut state, KeyCode::Esc);
    assert!(!state.sessions.input_active());
    assert!(text(&render(&state, 120, 36)).contains("1800 s"));
}

#[test]
fn decisions_configuration_has_a_tab_and_inbox_keeps_only_operational_actions() {
    let (_tmp, mut state) = fixture();
    navigation::Destination::Coordinator.open(&mut state);
    handle_key(&mut state, KeyCode::Char('6'));
    assert_eq!(state.config_tab_index, 7);
    assert_eq!(state.current_config_field(), Some((3, 0)));
    let before = state.working_copy.clone();
    handle_key(&mut state, KeyCode::Down);
    handle_key(&mut state, KeyCode::Enter);
    assert_ne!(state.working_copy, before);
    handle_key(&mut state, KeyCode::Down);
    handle_key(&mut state, KeyCode::Char('e'));
    handle_key(&mut state, KeyCode::Char('9'));
    handle_key(&mut state, KeyCode::Esc);
    assert_eq!(state.config_tab_index, 7);
    for width in [80, 120] {
        let output = text(&render(&state, width, 32));
        assert!(output.contains("6 Decisions"), "{output}");
        assert!(output.contains("Decisions settings"));
        assert!(output.contains("[s] Save"));
    }
    handle_key(&mut state, KeyCode::Char('s'));
    let saved =
        macc_core::load_canonical_config(&state.project_paths.as_ref().unwrap().config_path)
            .unwrap();
    assert_eq!(
        saved
            .automation
            .coordinator
            .unwrap()
            .decisions
            .unwrap()
            .human_question_min_risk,
        macc_core::coordinator::decisions::Consultation::Critical
    );
    let mut center = crate::decision_center::Center::default();
    // No hidden configuration action may remain on the former inbox shortcuts.
    assert!(center.key_event(KeyCode::F(6)).is_none());
    assert!(center.key_event(KeyCode::F(7)).is_none());
}

#[test]
fn preflight_switches_show_their_effect_and_accept_a_single_toggle() {
    let (_project, mut state) = fixture();
    navigation::Destination::Coordinator.open(&mut state);
    handle_key(&mut state, KeyCode::Char('3'));
    for _ in 0..5 {
        handle_key(&mut state, KeyCode::Down);
    }
    assert_eq!(state.current_config_field(), Some((1, 38)));
    assert!(text(&render(&state, 120, 36)).contains("On · blocks"));
    handle_key(&mut state, KeyCode::Char(' '));
    assert!(text(&render(&state, 120, 36)).contains("Off · warns"));
    handle_key(&mut state, KeyCode::Down);
    assert_eq!(state.current_config_field(), Some((1, 39)));
    handle_key(&mut state, KeyCode::Char(' '));
    assert_eq!(state.config_field_value(1, 39), "false");
}

#[test]
fn changes_tracks_each_phase_edit_through_navigation_and_save() {
    let (_root, mut state) = fixture();
    navigation::Destination::Coordinator.open(&mut state);
    handle_key(&mut state, KeyCode::Char('3'));
    handle_key(&mut state, KeyCode::Char(' '));
    handle_key(&mut state, KeyCode::Char('H'));
    let rows = workbench::changes::draft(&state);
    assert_eq!(rows.len(), 2, "{rows:?}");
    assert_eq!(rows[0].1, "○ Off → ● On");
    assert_eq!(rows[1].1, "disabled → required");
    for width in [80, 120] {
        let screen = text(&render(&state, width, 32));
        assert!(screen.contains("Draft ◆ 2"), "{screen}");
        assert!(screen.contains("◆ 2 unsaved"), "{screen}");
    }
    handle_key(&mut state, KeyCode::Down);
    handle_key(&mut state, KeyCode::Enter);
    assert_eq!(state.current_config_field(), Some((1, 35)));
    handle_key(&mut state, KeyCode::Char('H'));
    handle_key(&mut state, KeyCode::Char('s'));
    assert!(workbench::changes::draft(&state).is_empty());
    assert_eq!(workbench::changes::saved(&state), rows);
    handle_key(&mut state, KeyCode::Down);
    handle_key(&mut state, KeyCode::Enter);
    assert_eq!(state.current_config_field(), Some((1, 35)));
}

#[test]
fn changes_opens_the_edited_o2_policy_field_and_keeps_the_reviewed_save() {
    let (_root, mut s) = fixture();
    navigation::Destination::MemorySettings.open(&mut s);
    handle_key(&mut s, KeyCode::Char(' '));
    handle_key(&mut s, KeyCode::Char('H'));
    assert_eq!(
        workbench::changes::draft(&s),
        vec![("memory.enabled".into(), "○ Off → ● On".into())]
    );
    handle_key(&mut s, KeyCode::Char('a'));
    assert_eq!(s.current_screen(), Screen::Changes);
    handle_key(&mut s, KeyCode::Enter);
    assert_eq!(
        navigation::current(&s),
        navigation::Destination::MemorySettings
    );
    assert!(text(&render(&s, 120, 36)).contains("Current draft: ● On"));
}

#[test]
fn overview_uses_semantic_glyphs_including_on_the_selected_row() {
    use macc_core::onboarding::{ReadinessLadder, ReadinessStep};
    let (_project, mut state) = fixture();
    state.observations.latest = Some(observations::Snapshot {
        readiness: ReadinessLadder {
            steps: vec![
                ReadinessStep::done(1, "Project initialized", None),
                ReadinessStep::done(2, "Tool adapter selected", Some("codex")),
                ReadinessStep::done(3, "Config applied", None),
                ReadinessStep::done(4, "PRD/task available", Some("6 ready")),
                ReadinessStep::done(5, "Git identity configured", None),
                ReadinessStep::pending(6, "Coordinator running"),
                ReadinessStep::pending(7, "Performer connected"),
                ReadinessStep::pending(8, "First task dispatched"),
            ],
            blocking_count: 3,
        },
        trust: None,
        supervisor_status: None,
        intervention: None,
        prd: String::new(),
    });
    navigation::Destination::Overview.open(&mut state);
    assert_eq!(state.inspector_index, 5);
    let palette = theme();
    for (w, h) in [(80, 24), (120, 36)] {
        let buffer = render(&state, w, h);
        for (glyph, fg, bg) in [
            ("✓", palette.good, palette.background),
            ("▲", palette.warn, palette.operate_selection),
            ("·", palette.muted, palette.background),
        ] {
            assert!(
                buffer
                    .content
                    .iter()
                    .any(|c| c.symbol() == glyph && c.fg == fg && c.bg == bg),
                "{glyph} at {w} columns"
            );
        }
        assert!(text(&buffer).contains("Readiness · 5 of 8"));
        assert!(text(&buffer).contains("▲ Action needed"));
        assert!(buffer
            .content
            .iter()
            .all(|c| c.fg != Color::Reset && c.bg != Color::Reset));
        export_buffer("overview", &state, w, h);
    }
    handle_key(&mut state, KeyCode::Up);
    assert_eq!(state.inspector_index, 4);
}
