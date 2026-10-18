//! Test actual application routing and terminal buffers, not duplicated UI decisions.
use super::*;
use macc_core::{tool::ToolRegistry, MaccEngine, ProjectPaths};
use ratatui::{backend::TestBackend, buffer::Buffer};
use std::sync::Arc;
fn fixture() -> (tempfile::TempDir, AppState) {
    let t = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(t.path().join(".macc")).unwrap();
    std::fs::write(t.path().join(".macc/macc.yaml"), "tools:\n  enabled: []\n").unwrap();
    let mut state = AppState::with_engine(Arc::new(MaccEngine::new(ToolRegistry::new())));
    state.project_paths = Some(ProjectPaths::from_root(t.path()));
    state.config = Some(Default::default());
    state.working_copy = Some(Default::default());
    (t, state)
}
fn render(state: &AppState, width: u16, height: u16) -> Buffer {
    let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
    terminal.draw(|f| ui(f, state, true)).unwrap();
    terminal.backend().buffer().clone()
}
fn text(b: &Buffer) -> String {
    (0..b.area.height)
        .map(|y| {
            (0..b.area.width)
                .map(|x| b.get(x, y).symbol())
                .collect::<String>()
        })
        .collect::<Vec<_>>()
        .join("\n")
}
#[test]
fn o2_is_a_direct_destination_in_home_palette_help_and_breadcrumbs() {
    let (_t, mut state) = fixture();
    let home = text(&render(&state, 120, 32));
    assert!(home.contains("[O] O2"));
    assert!(home.contains("[J] Sessions"));
    assert!(filtered_command_palette_items("memory")
        .iter()
        .any(|v| v.action == CommandPaletteAction::OpenO2));
    assert!(Screen::Home
        .help_keybindings()
        .iter()
        .any(|(key, label)| *key == "O" && label.contains("O2")));
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
    assert!(help.contains("Browse named actions"));
    handle_key(&mut state, KeyCode::PageDown);
    assert!(state.help_scroll > 0);
    handle_key(&mut state, KeyCode::Esc);
    assert!(!state.help_open);
    assert_eq!(state.current_screen(), Screen::O2);
}
#[test]
fn standard_terminal_shows_selected_setting_actions_and_empty_conversations() {
    let (_t, mut state) = fixture();
    handle_key(&mut state, KeyCode::Char('J'));
    let small = text(&render(&state, 80, 24));
    assert!(small.contains("Actions: F2 Actions"));
    assert!(small.contains("Lease TTL"));
    assert!(small.contains("Details"));
    assert!(small.contains("Settings"));
    handle_key(&mut state, KeyCode::Char('2'));
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
    handle_key(&mut state, KeyCode::Char('J'));
    handle_key(&mut state, KeyCode::F(2));
    let menu = text(&render(&state, 120, 32));
    assert!(menu.contains("Save session settings"));
    // Enter selects Save; the translated mutation must pass the existing owner gate.
    for _ in 0..4 {
        handle_key(&mut state, KeyCode::Down);
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
    handle_key(&mut state, KeyCode::F(2));
    handle_key(&mut state, KeyCode::Char('a'));
    assert_eq!(
        macc_core::session_store::inspect(t.path()).unwrap().backend,
        "json"
    );
}
#[test]
fn settings_groups_forms_and_workspace_switches_preserve_drafts() {
    let (_t, mut state) = fixture();
    handle_key(&mut state, KeyCode::Char('O'));
    handle_key(&mut state, KeyCode::Char('5'));
    let before = text(&render(&state, 120, 32));
    assert!(before.contains("Memory settings"));
    handle_key(&mut state, KeyCode::Right);
    assert!(text(&render(&state, 120, 32)).contains("Context settings"));
    handle_key(&mut state, KeyCode::Char('/'));
    for c in "memory.enabled".chars() {
        handle_key(&mut state, KeyCode::Char(c));
    }
    handle_key(&mut state, KeyCode::Enter);
    handle_key(&mut state, KeyCode::Enter);
    handle_key(&mut state, KeyCode::Char('J'));
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
    for (name, key) in [
        ("sessions", 'J'),
        ("o2-overview", 'O'),
        ("o2-settings", '5'),
    ] {
        handle_key(&mut state, KeyCode::Char(key));
        for (width, height) in [(120, 36), (80, 24)] {
            let b = render(&state, width, height);
            std::fs::write(directory.join(format!("{name}-{width}.txt")), text(&b)).unwrap();
            // Cell colors and positions are preserved for terminal screenshot generation.
            let cells:Vec<_>=b.content.iter().map(|c|serde_json::json!({"text":c.symbol(),"fg":format!("{:?}",c.fg),"bg":format!("{:?}",c.bg)})).collect();
            std::fs::write(
                directory.join(format!("{name}-{width}.json")),
                serde_json::json!({"width":width,"height":height,"cells":cells}).to_string(),
            )
            .unwrap();
        }
    }
    handle_key(&mut state, KeyCode::F(2));
    std::fs::write(
        directory.join("actions-menu.txt"),
        text(&render(&state, 120, 36)),
    )
    .unwrap();
}

#[test]
fn storage_review_cancels_without_migration_and_form_text_keeps_navigation_letters() {
    let (t, mut state) = fixture();
    handle_key(&mut state, KeyCode::Char('J'));
    handle_key(&mut state, KeyCode::Char('m'));
    assert!(text(&render(&state, 120, 32)).contains("REVIEW REQUIRED"));
    handle_key(&mut state, KeyCode::Esc);
    assert_eq!(state.current_screen(), Screen::Sessions);
    assert_eq!(
        macc_core::session_store::inspect(t.path()).unwrap().backend,
        "json"
    );
    handle_key(&mut state, KeyCode::Char('O'));
    handle_key(&mut state, KeyCode::Char('5'));
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
    handle_key(&mut state, KeyCode::Char('J'));
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
    handle_key(&mut state, KeyCode::Char('2'));
    let selected = text(&render(&state, 120, 36));
    assert!(selected.contains("TASK-42"));
    assert!(selected.contains("future-ai"));
    assert!(selected.contains("reviewer"));
    handle_key(&mut state, KeyCode::Enter);
    assert!(text(&render(&state, 120, 36)).contains("session-example"));
}

#[test]
fn command_palette_switch_keeps_a_session_edit_without_hijacking_the_o2_workspace() {
    let (_t, mut state) = fixture();
    handle_key(&mut state, KeyCode::Char('J'));
    handle_key(&mut state, KeyCode::Enter);
    assert!(state.sessions.input_active());
    handle_key(&mut state, KeyCode::Backspace);
    handle_key(&mut state, KeyCode::Char(':'));
    for c in "O2".chars() {
        handle_key(&mut state, KeyCode::Char(c));
    }
    handle_key(&mut state, KeyCode::Enter);
    assert_eq!(state.current_screen(), Screen::O2);
    assert!(!state.sessions.input_active());
    handle_key(&mut state, KeyCode::Char('J'));
    assert_eq!(state.current_screen(), Screen::Sessions);
    assert!(state.sessions.input_active());
    assert!(text(&render(&state, 120, 36)).contains("180 seconds"));
}
