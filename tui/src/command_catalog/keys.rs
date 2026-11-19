//! Physical bindings shared by focused controls, independently of their captions.
use crossterm::event::KeyCode::{self, *};
#[derive(Clone, Copy)]
pub(crate) struct Keys(pub KeyCode, pub Option<KeyCode>);
pub const fn ch(c: char) -> Keys {
    Keys(Char(c), None)
}
pub const ENTER: Keys = Keys(Enter, None);
pub const ESC: Keys = Keys(Esc, None);
pub const SPACE: Keys = ch(' ');
pub const TAB: Keys = Keys(Tab, None);
pub const BACKTAB: Keys = Keys(BackTab, None);
pub const HOME: Keys = Keys(Home, None);
pub const DELETE: Keys = Keys(Delete, None);
pub const BACKSPACE: Keys = Keys(Backspace, None);
pub const VERTICAL: Keys = Keys(Up, Some(Down));
pub const HORIZONTAL: Keys = Keys(Left, Some(Right));
pub const PAGES: Keys = Keys(PageUp, Some(PageDown));
pub fn label(code: KeyCode) -> String {
    match code {
        Char(' ') => "Space".into(),
        Char(c) => c.to_string(),
        F(n) => format!("F{n}"),
        Up => "↑".into(),
        Down => "↓".into(),
        Left => "←".into(),
        Right => "→".into(),
        PageUp => "PgUp".into(),
        PageDown => "PgDn".into(),
        BackTab => "Shift+Tab".into(),
        key => format!("{key:?}"),
    }
}
