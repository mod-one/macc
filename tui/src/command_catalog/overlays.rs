//! The help viewer and command picker own their bindings while they have focus.
use super::*;
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum OverlayAction {
    Close,
    Up,
    Down,
    PageUp,
    PageDown,
    Run,
    Delete,
}
pub(super) fn bindings(s: &AppState) -> Option<Vec<Command>> {
    use OverlayAction as A;
    let keys = if s.command_palette_open {
        vec![
            (KeyCode::Up, "Previous", A::Up),
            (KeyCode::Down, "Next", A::Down),
            (KeyCode::Enter, "Run", A::Run),
            (KeyCode::Esc, "Close", A::Close),
            (KeyCode::Char(':'), "Close", A::Close),
            (KeyCode::Backspace, "Delete letter", A::Delete),
        ]
    } else if s.help_open {
        vec![
            (KeyCode::Up, "Scroll up", A::Up),
            (KeyCode::Down, "Scroll down", A::Down),
            (KeyCode::PageUp, "Previous page", A::PageUp),
            (KeyCode::PageDown, "Next page", A::PageDown),
            (KeyCode::Esc, "Close", A::Close),
            (KeyCode::Char('?'), "Close", A::Close),
        ]
    } else {
        return None;
    };
    Some(
        keys.into_iter()
            .map(|(k, l, a)| Command::bound(k, l, Action::Overlay(a)))
            .chain(std::iter::once(quit_command()))
            .collect(),
    )
}
pub(super) fn execute(s: &mut AppState, action: OverlayAction) {
    use OverlayAction::*;
    if s.command_palette_open {
        match action {
            Close => close_command_palette(s),
            Up => s.command_palette_selection = s.command_palette_selection.saturating_sub(1),
            Down => {
                s.command_palette_selection = (s.command_palette_selection + 1)
                    .min(contextual_items(s).len().saturating_sub(1))
            }
            Delete => {
                s.command_palette_query.pop();
                s.command_palette_selection = 0;
            }
            Run => {
                if let Some(item) = contextual_items(s)
                    .get(s.command_palette_selection)
                    .filter(|c| c.enabled)
                    .cloned()
                {
                    close_command_palette(s);
                    super::execute(s, item.action);
                }
            }
            PageUp | PageDown => {}
        }
    } else if s.help_open {
        match action {
            Close => s.toggle_help(),
            Down | PageDown => {
                s.help_scroll = s
                    .help_scroll
                    .saturating_add(if action == Down { 1 } else { 8 })
                    .min(active(s).len().saturating_sub(1) as u16)
            }
            Up | PageUp => {
                s.help_scroll = s
                    .help_scroll
                    .saturating_sub(if action == Up { 1 } else { 8 })
            }
            _ => {}
        }
    }
}
