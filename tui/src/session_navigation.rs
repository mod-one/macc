//! Thin navigation and ownership wiring for the two administration workspaces.
use crate::{screen::Screen, state::AppState};
use crossterm::event::KeyCode;
pub(crate) fn open(state: &mut AppState, o2: bool) {
    let Some(paths) = state.project_paths.clone() else {
        return;
    };
    let result = if o2 {
        state.sessions.open_o2(&paths.root)
    } else {
        state.sessions.show_sessions();
        state.sessions.refresh(&paths.root)
    };
    if let Err(e) = result {
        state.sessions.message = e.to_string();
    }
    let target = if o2 { Screen::O2 } else { Screen::Sessions };
    if state.current_screen() != target {
        state.push_screen(target);
    }
}
pub(crate) fn handle(state: &mut AppState, key: KeyCode) -> bool {
    let screen = state.current_screen();
    let workspace = matches!(screen, Screen::Sessions | Screen::O2);
    if (!workspace || !state.sessions.input_active()) && matches!(key, KeyCode::Char('O' | 'J')) {
        open(state, key == KeyCode::Char('O'));
        return true;
    }
    if !workspace {
        return false;
    }
    if key == KeyCode::F(1) || key == KeyCode::Char('?') && !state.sessions.input_active() {
        state.toggle_help();
        return true;
    }
    let Some(key) = state.sessions.command_key(key) else {
        return true;
    };
    if !state.sessions.input_active() && matches!(key, KeyCode::Char('O' | 'J')) {
        open(state, key == KeyCode::Char('O'));
        return true;
    }
    let Some(paths) = state.project_paths.clone() else {
        return true;
    };
    if screen == Screen::O2 {
        state.sessions.message.clear();
    }
    let gate = if state.sessions.is_mutation(key) {
        state.gate_project_mutation()
    } else {
        Ok(())
    };
    match gate.and_then(|_| state.sessions.key(&paths.root, key)) {
        Ok(true) => state.pop_screen(),
        Ok(false) => {
            if state.sessions.requested_sessions {
                state.sessions.requested_sessions = false;
                state.goto_screen(Screen::Sessions);
            } else if screen == Screen::O2 && !state.sessions.o2_open() {
                state.pop_screen();
            } else if screen == Screen::Sessions && state.sessions.o2_open() {
                state.push_screen(Screen::O2);
            }
            sync_saved(state, &paths.config_path, key);
        }
        Err(e) => state.sessions.message = e.to_string(),
    }
    true
}
fn sync_saved(state: &mut AppState, config_path: &std::path::Path, key: KeyCode) {
    if matches!(key, KeyCode::Char('s' | 'a')) {
        if let Ok(saved) = macc_core::load_canonical_config(config_path) {
            if let Some(config) = &mut state.config {
                config.memory = saved.memory.clone();
                config.context_engine = saved.context_engine.clone();
                config.adaptive_compaction = saved.adaptive_compaction.clone();
                config.code_intelligence = saved.code_intelligence.clone();
            }
            if let Some(draft) = &mut state.working_copy {
                draft.memory = saved.memory;
                draft.context_engine = saved.context_engine;
                draft.adaptive_compaction = saved.adaptive_compaction;
                draft.code_intelligence = saved.code_intelligence;
            }
        }
    }
    if key == KeyCode::Char('s') {
        if let Some(view) = &state.sessions.view {
            let policy = if view.policy_source == "defaults" {
                None
            } else {
                Some(view.policy.clone())
            };
            if let Some(config) = &mut state.config {
                config.session_management = policy.clone();
            }
            if let Some(draft) = &mut state.working_copy {
                draft.session_management = policy;
            }
        }
    }
}
pub(crate) fn help(state: &AppState) -> Vec<(String, String)> {
    let mut rows = vec![
        ("F2".into(), "Browse named actions with ↑↓ and Enter".into()),
        ("Tab / Shift+Tab".into(), "Next / previous section".into()),
        (
            "1–7".into(),
            "Open a numbered section (Sessions uses 1–2)".into(),
        ),
        (
            "Enter".into(),
            "Edit the selected setting or inspect evidence".into(),
        ),
        (
            "← / →".into(),
            "Switch O2 setting groups or trial fields".into(),
        ),
        ("PgUp / PgDn".into(), "Scroll the details pane".into()),
        ("F4".into(), "Toggle complete raw evidence".into()),
        (
            "O / J".into(),
            "Open O2 / Sessions, retaining drafts".into(),
        ),
        (
            "Esc".into(),
            "Back; unsaved drafts require save or discard".into(),
        ),
    ];
    rows.extend(state.sessions.commands().into_iter().map(|c| {
        let key = match c.key {
            KeyCode::Char(k) => k.to_string(),
            _ => c.shortcut.into(),
        };
        (
            key,
            format!(
                "{} — {}{}",
                c.label,
                c.hint,
                if c.enabled {
                    ""
                } else {
                    " [currently unavailable]"
                }
            ),
        )
    }));
    rows
}
