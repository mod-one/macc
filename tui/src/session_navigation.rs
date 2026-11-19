//! Thin navigation and ownership wiring for the two administration workspaces.
use crate::{screen::Screen, state::AppState};
use crossterm::event::KeyCode;
pub(crate) fn open(state: &mut AppState, o2: bool, configure: bool) {
    let Some(paths) = state.project_paths.clone() else {
        return;
    };
    let result = if o2 {
        state.sessions.open_o2(&paths.root, configure)
    } else {
        state.sessions.show_sessions(&paths.root, configure)
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
    if !workspace {
        return false;
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
    let o2 = state.current_screen() == Screen::O2 && matches!(key, KeyCode::Char('s' | 'a'));
    let session = state.current_screen() == Screen::Sessions
        && key == KeyCode::Char('s')
        && state.sessions.configuring();
    if !o2 && !session {
        return;
    }
    let (Some(base), Some(draft)) = (&mut state.config, &mut state.working_copy) else {
        return;
    };
    let Ok(saved) = macc_core::load_canonical_config(config_path) else {
        return;
    };
    // Preserve unrelated edits when a workspace service persists its own section.
    fn accept<T: PartialEq + Clone>(base: &mut T, draft: &mut T, saved: T) {
        if draft == base {
            *draft = saved.clone();
        }
        *base = saved;
    }
    if o2 {
        accept(&mut base.memory, &mut draft.memory, saved.memory);
        accept(
            &mut base.context_engine,
            &mut draft.context_engine,
            saved.context_engine,
        );
        accept(
            &mut base.adaptive_compaction,
            &mut draft.adaptive_compaction,
            saved.adaptive_compaction,
        );
        accept(
            &mut base.code_intelligence,
            &mut draft.code_intelligence,
            saved.code_intelligence,
        );
    } else {
        accept(
            &mut base.session_management,
            &mut draft.session_management,
            saved.session_management,
        );
    }
}
