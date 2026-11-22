//! Resolve and present commands from the same context that executes them.
use super::*;
pub(crate) mod keys;
mod overlays;
mod pages;
use keys as k;
pub(crate) use pages::Action;
#[derive(Clone, Debug)]
pub(crate) struct Command {
    pub key: Option<KeyCode>,
    pub control: bool,
    pub label: String,
    pub hint: String,
    pub action: Action,
    pub enabled: bool,
}
impl Command {
    pub fn new(key: char, label: &str, hint: &str) -> Self {
        let mut c = Self::widget(KeyCode::Char(key), label);
        c.hint = hint.into();
        c
    }
    fn widget(key: KeyCode, label: &str) -> Self {
        Self::bound(key, label, Action::Widget(key))
    }
    pub(crate) fn ctrl(key: char, label: &str, action: Action) -> Self {
        Self {
            control: true,
            ..Self::bound(KeyCode::Char(key), label, action)
        }
    }
    fn bound(key: KeyCode, label: &str, action: Action) -> Self {
        Self {
            key: Some(key),
            control: false,
            label: label.into(),
            hint: String::new(),
            action,
            enabled: true,
        }
    }
    pub fn when(mut self, enabled: bool) -> Self {
        self.enabled = enabled;
        self
    }
    pub fn shortcut(&self) -> String {
        match self.key {
            Some(k) if self.control => format!("Ctrl+{}", keys::label(k).to_uppercase()),
            Some(k) => keys::label(k),
            None => "Palette".into(),
        }
    }
}
fn rows(bindings: Vec<(k::Keys, &str)>) -> Vec<Command> {
    bindings
        .into_iter()
        .flat_map(|(keys, label)| {
            [Some(keys.0), keys.1]
                .into_iter()
                .flatten()
                .map(move |key| Command::widget(key, label))
        })
        .collect()
}
/// Global destinations must never bypass an active modal or text editor.
pub(crate) fn exclusive(s: &AppState) -> bool {
    use crate::focus::Layer;
    match Layer::current(s) {
        Layer::Page => false,
        Layer::Hooks => s.hooks.modal(),
        Layer::Workspace => s.sessions.input_active(),
        _ => true,
    }
}
fn entering_text(s: &AppState) -> bool {
    use crate::focus::Layer;
    match Layer::current(s) {
        Layer::ToolField | Layer::ConfigField | Layer::Search => true,
        Layer::Supervisor => s.supervisor_command_editing(),
        Layer::Workspace => s.sessions.input_active(),
        Layer::Hooks => s.hooks.input_active(),
        Layer::Decision => s.decision_center.accepts_text(),
        Layer::Queue => s.prd_queue_editor.as_ref().is_some_and(|e| e.is_editing()),
        Layer::Approval => s.approval_window.as_ref().is_some_and(|w| w.is_editing()),
        Layer::Apply => s.apply_assistant_editing(),
        _ => false,
    }
}
fn contextual(state: &AppState) -> Vec<Command> {
    use crate::focus::Layer;
    match Layer::current(state) {
        Layer::Launch => rows(vec![
            (k::TAB, "Next option"),
            (k::BACKTAB, "Previous option"),
            (k::VERTICAL, "Select option"),
            (k::HORIZONTAL, "Change value / button"),
            (k::ENTER, "Activate selection"),
            (k::PAGES, "Scroll warnings"),
            (k::ESC, "Cancel launch"),
        ]),
        Layer::Apply if state.apply_assistant_open() => rows(state.apply_assistant_bindings()),
        Layer::Apply => rows(vec![
            (k::HORIZONTAL, "Choose Cancel / Apply"),
            (k::TAB, "Next button"),
            (k::ENTER, "Activate selected button"),
            (k::BACKSPACE, "Delete consent character"),
            (k::PAGES, "Scroll reviewed preparation"),
            (k::ESC, "Cancel confirmation"),
        ]),
        Layer::TaskDetail => rows(vec![
            (k::PAGES, "Scroll details"),
            (k::ESC, "Close details"),
        ]),
        Layer::Decision => rows(vec![
            (k::TAB, "Next field or button"),
            (k::BACKTAB, "Previous field or button"),
            (k::VERTICAL, "Choose a value / action"),
            (k::HORIZONTAL, "Choose an option"),
            (k::ENTER, "Edit text or activate focused button"),
            (k::HOME, "Evidence / approvals / lifecycle actions"),
            (k::PAGES, "Scroll request details"),
            (k::ESC, "Later — keep draft"),
        ]),
        Layer::Approval => rows(state.approval_window.as_ref().unwrap().command_bindings()),
        Layer::Finished => rows(vec![(k::ENTER, "Dismiss"), (k::ESC, "Close")]),
        Layer::Paused => pause_commands(state),
        Layer::Queue => rows(state.prd_queue_editor.as_ref().unwrap().command_bindings()),
        Layer::Recovery | Layer::Stop => rows(vec![
            (k::VERTICAL, "Select mode"),
            (k::ENTER, "Confirm selected mode"),
            (k::ESC, "Cancel"),
        ]),
        Layer::StopTask => rows(vec![(k::ENTER, "Confirm task stop"), (k::ESC, "Cancel")]),
        Layer::Takeover => rows(vec![
            (k::ch('a'), "Accept takeover"),
            (k::ch('r'), "Reject takeover"),
            (k::ESC, "Dismiss"),
        ]),
        Layer::Install => rows(vec![(k::ENTER, "Confirm installation"), (k::ESC, "Cancel")]),
        Layer::Priority => rows(vec![
            (k::VERTICAL, "Select / move tool"),
            (k::SPACE, "Grab / release"),
            (k::ENTER, "Release / save"),
            (k::ch('s'), "Save order"),
            (k::ESC, "Release / cancel"),
        ]),
        Layer::Parallel => rows(vec![
            (k::VERTICAL, "Select tool"),
            (k::HORIZONTAL, "Adjust limit"),
            (k::ENTER, "Finish"),
            (k::ESC, "Close"),
        ]),
        Layer::Supervisor => rows(state.supervisor_command_bindings()),
        Layer::Hooks => rows(state.hooks.bindings()),
        Layer::ConfigField | Layer::ToolField | Layer::Search => editor_commands(),
        Layer::Workspace if state.sessions.input_active() => {
            let mut cs = editor_commands();
            if state.current_screen() == Screen::O2 {
                cs.push(Command::widget(KeyCode::BackTab, "Previous form field"));
            }
            cs
        }
        Layer::Workspace => {
            let mut cs = state.sessions.commands();
            if state.sessions.supports_raw() {
                cs.push(Command::ctrl('r', "Raw evidence", Action::WorkspaceRaw));
            }
            cs.extend(rows(state.sessions.navigation_bindings()));
            cs
        }
        Layer::Page => pages::commands(state),
    }
}
fn editor_commands() -> Vec<Command> {
    rows(vec![
        (k::ENTER, "Confirm input"),
        (k::BACKSPACE, "Delete character"),
        (k::ESC, "Cancel input"),
    ])
}
fn pause_commands(state: &AppState) -> Vec<Command> {
    let summary = state.coordinator_pause_summary.as_ref();
    if summary.is_some_and(|s| s.scoped_prd.is_some()) {
        return rows(vec![(k::ch('o'), "Open logs"), (k::ESC, "Keep stopped")]);
    }
    if crate::precondition_stop(
        state.coordinator_pause_error.as_deref().unwrap_or_default(),
        summary.and_then(|s| s.error_code.as_deref()),
        summary
            .map(|s| s.unmet_preconditions.as_slice())
            .unwrap_or_default(),
    ) {
        return rows(vec![
            (k::ch('d'), "Inspect decision / failed check"),
            (k::ch('o'), "Open logs"),
            (k::ESC, "Keep stopped"),
        ]);
    }
    rows(vec![
        (k::ch('d'), "Inspect decision / failed check"),
        (k::ch('r'), "Retry failed phase"),
        (k::ENTER, "Retry failed phase"),
        (k::ch('s'), "Skip failed phase"),
        (k::ch('o'), "Open logs"),
        (k::ch('u'), "Resume signal"),
        (k::ch('c'), "Continue"),
        (k::ESC, "Keep stopped"),
    ])
}
mod execute;
pub(crate) use execute::execute;
pub(crate) fn active(s: &AppState) -> Vec<Command> {
    let mut commands = contextual(s);
    if !exclusive(s) {
        let mode = crate::navigation::mode(s);
        for &(key, dest) in mode.routes() {
            // Page letters are reserved for navigation. A specialist workspace
            // operation sharing a letter remains executable by name in the palette.
            let key = KeyCode::Char(key);
            if let Some(local) = commands.iter_mut().find(|c| c.key == Some(key)) {
                local.key = None;
            }
            commands.push(Command::bound(key, dest.label(), Action::Go(dest)));
        }
        commands.push(Command::ctrl('t', mode.other_label(), Action::SwitchMode));
        if mode == crate::navigation::Mode::Configure
            && !matches!(s.current_screen(), Screen::O2 | Screen::Sessions)
        {
            commands
                .push(Command::ctrl('z', "Redo edit", Action::Redo).when(!s.redo_stack.is_empty()));
            for (key, label, action) in [
                ('s', "Save configuration", Action::Save),
                ('u', "Undo edit", Action::Undo),
            ] {
                if !commands.iter().any(|c| c.key == Some(KeyCode::Char(key))) {
                    commands.push(Command::bound(KeyCode::Char(key), label, action));
                }
            }
        }
        if !commands.iter().any(|c| c.key == Some(KeyCode::Esc)) {
            commands.push(Command::bound(
                KeyCode::Esc,
                if s.screen_stack.len() > 1 {
                    "Back"
                } else {
                    "Quit"
                },
                Action::Back,
            ));
        }
        for &dest in crate::navigation::ALL {
            if !commands.iter().any(|c| c.action == Action::Go(dest)) {
                commands.push(Command {
                    key: None,
                    control: false,
                    label: dest.label().into(),
                    hint: dest.mode().label().into(),
                    action: Action::Go(dest),
                    enabled: true,
                });
            }
        }
    }
    if !exclusive(s) {
        for c in &mut commands {
            if c.action == Action::Undo {
                c.enabled = !s.undo_stack.is_empty();
            }
            if c.action == Action::Save {
                c.enabled = s.working_copy.is_some()
                    && (s.current_screen() != Screen::Changes || s.working_copy != s.config);
                if !c.enabled {
                    c.hint="Open an outstanding policy draft to use its reviewed save, or edit a configuration setting.".into();
                }
            }
        }
    }
    let mut help = Command::bound(KeyCode::Char('?'), "Help", Action::Help);
    if entering_text(s) {
        help.key = None;
    }
    commands.extend([help, palette_command(s), quit_command()]);
    commands
}

fn quit_command() -> Command {
    Command::ctrl('c', "Quit TUI", Action::Quit)
}

pub(crate) fn palette_command(s: &AppState) -> Command {
    if entering_text(s) {
        Command::ctrl('p', "Commands", Action::Palette)
    } else {
        let mut command = Command::bound(KeyCode::Char(':'), "Commands", Action::Palette);
        command.hint = "Also available with Ctrl+P".into();
        command
    }
}

pub(crate) fn resolve(state: &AppState, key: KeyCode, control: bool) -> Option<Command> {
    let commands = focused(state);
    if control && key == KeyCode::Char('p') {
        return commands.into_iter().find(|c| {
            c.action == Action::Palette
                || (state.command_palette_open && c.key == Some(KeyCode::Char(':')))
        });
    }
    commands
        .into_iter()
        .find(|c| c.key == Some(key) && c.control == control)
}

pub(crate) fn focused(s: &AppState) -> Vec<Command> {
    overlays::bindings(s).unwrap_or_else(|| active(s))
}
