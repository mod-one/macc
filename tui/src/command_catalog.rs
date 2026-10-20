//! Active-context command metadata shared by footer, help and palette.
use super::*;

#[derive(Clone, Debug)]
pub(crate) struct Command {
    pub shortcut: String,
    pub label: String,
    pub key: Option<KeyCode>,
    pub enabled: bool,
}
impl Command {
    fn new(shortcut: &str, label: &str) -> Self {
        Self {
            shortcut: shortcut.into(),
            label: label.into(),
            key: key_for(shortcut),
            enabled: true,
        }
    }
}
fn key_for(shortcut: &str) -> Option<KeyCode> {
    Some(match shortcut {
        "Enter" => KeyCode::Enter,
        "Esc" | "Esc/q" => KeyCode::Esc,
        "Space" | "Space/Enter" => KeyCode::Char(' '),
        "Tab" | "Tab/1-7" => KeyCode::Tab,
        "Backspace" => KeyCode::Backspace,
        "Shift+Tab" => KeyCode::BackTab,
        "Delete" => KeyCode::Delete,
        "F4" => KeyCode::F(4),
        s if s.chars().count() == 1 && !"↑↓←→".contains(s) => {
            KeyCode::Char(s.chars().next()?)
        }
        _ => return None,
    })
}
fn rows(bindings: Vec<(&str, &str)>) -> Vec<Command> {
    bindings
        .into_iter()
        .map(|(k, l)| Command::new(k, l))
        .collect()
}

/// Input is isolated from page navigation. Function keys remain available.
pub(crate) fn input_active(state: &AppState) -> bool {
    (state.current_screen() == Screen::Apply && state.apply_confirmation_open())
        || state.search_editing
        || (state.current_screen() == Screen::ToolSettings && state.is_tool_field_editing())
        || (matches!(
            state.current_screen(),
            Screen::Automation | Screen::Settings
        ) && (state.automation_field_editing || state.settings_field_editing))
        || (matches!(state.current_screen(), Screen::Sessions | Screen::O2)
            && state.sessions.input_active())
}

/// Global destinations must never bypass an active modal or text editor.
pub(crate) fn exclusive(state: &AppState) -> bool {
    input_active(state)
        || state.launch_dialog.is_some()
        || state.approval_window.is_some()
        || state.prd_queue_editor.is_some()
        || state.coordinator_task_diff_popup.is_some()
        || state.coordinator_task_explain_popup.is_some()
        || state.has_coordinator_finished_prompt()
        || state.has_coordinator_pause_prompt()
        || state.coordinator_stop_dialog_open
        || state.coordinator_stop_task_confirm_id.is_some()
        || state.coordinator_recover_dialog_open
        || state
            .coordinator_ownership
            .pending_incoming_request
            .is_some()
        || state.is_tool_install_confirmation_open()
        || state.tool_priority_editor_active
        || state.tool_parallel_editor_active
        || state.supervisor_commands_active()
}

pub(crate) fn active(state: &AppState) -> Vec<Command> {
    if state.launch_dialog.is_some() {
        return rows(vec![
            ("Tab", "Next option"),
            ("↑↓", "Select option"),
            ("←→", "Change value / button"),
            ("Enter", "Activate selection"),
            ("PgUp/PgDn", "Scroll launch warnings"),
            ("Esc", "Cancel launch"),
        ]);
    }
    if state.coordinator_task_diff_popup.is_some() || state.coordinator_task_explain_popup.is_some()
    {
        return rows(vec![("Esc", "Close details")]);
    }
    if let Some(window) = &state.approval_window {
        return rows(window.command_bindings());
    }
    if state.has_coordinator_finished_prompt() {
        return rows(vec![("Enter", "Dismiss"), ("Esc", "Close")]);
    }
    if state.has_coordinator_pause_prompt() {
        return rows(vec![
            ("r", "Retry failed phase"),
            ("s", "Skip failed phase"),
            ("o", "Open logs"),
            ("u", "Resume signal"),
            ("c", "Continue"),
            ("Esc", "Keep stopped"),
        ]);
    }
    if let Some(editor) = &state.prd_queue_editor {
        return rows(editor.command_bindings());
    }
    let screen = state.current_screen();
    if screen == Screen::CoordinatorLive {
        if state.coordinator_stop_dialog_open || state.coordinator_recover_dialog_open {
            return rows(vec![
                ("↑↓", "Select mode"),
                ("Enter", "Confirm selected mode"),
                ("Esc", "Cancel"),
            ]);
        }
        if state.coordinator_stop_task_confirm_id.is_some() {
            return rows(vec![("Enter", "Confirm task stop"), ("Esc", "Cancel")]);
        }
        if state
            .coordinator_ownership
            .pending_incoming_request
            .is_some()
        {
            return rows(vec![
                ("a", "Accept takeover"),
                ("r", "Reject takeover"),
                ("Esc", "Dismiss"),
            ]);
        }
    }
    if screen == Screen::Tools && state.is_tool_install_confirmation_open() {
        return rows(vec![("Enter", "Confirm installation"), ("Esc", "Cancel")]);
    }
    if screen == Screen::Automation && state.tool_priority_editor_active {
        return rows(vec![
            ("↑↓", "Select / move tool"),
            ("Space", "Grab / release"),
            ("Enter", "Release / save"),
            ("s", "Save order"),
            ("Esc", "Release / cancel"),
        ]);
    }
    if screen == Screen::Automation && state.tool_parallel_editor_active {
        return rows(vec![
            ("↑↓", "Select tool"),
            ("←→", "Adjust limit"),
            ("Enter", "Finish"),
            ("Esc", "Close"),
        ]);
    }
    if matches!(screen, Screen::Automation | Screen::Settings) && state.supervisor_commands_active()
    {
        return rows(state.supervisor_command_bindings());
    }
    if screen == Screen::Apply {
        if state.apply_assistant_open() {
            return rows(state.apply_assistant_bindings());
        }
        if state.apply_confirmation_open() {
            return rows(vec![
                ("←→", "Choose Cancel / Apply"),
                ("Tab", "Next button"),
                ("Enter", "Activate selected button"),
                ("Backspace", "Delete consent character"),
                ("PgUp/PgDn", "Scroll reviewed preparation"),
                ("Esc", "Cancel confirmation"),
            ]);
        }
        let mut commands = rows(screen_action_bindings(screen));
        commands.extend(rows(vec![("v", "Toggle technical output")]));
        if let Some(command) = commands.iter_mut().find(|c| c.key == Some(KeyCode::Enter)) {
            command.enabled = state.apply_context.is_some()
                && !state.apply_preparing()
                && state.apply_feedback.is_none();
        }
        return commands;
    }
    if input_active(state) {
        let mut commands = rows(vec![
            ("Enter", "Confirm input"),
            ("Backspace", "Delete character"),
            ("Esc", "Cancel input"),
        ]);
        if screen == Screen::O2 {
            commands.push(Command::new("Shift+Tab", "Previous form field"));
        }
        return commands;
    }
    if matches!(screen, Screen::Sessions | Screen::O2) {
        let mut commands = rows(state.sessions.navigation_bindings());
        commands.extend(state.sessions.commands().into_iter().map(|c| Command {
            shortcut: match c.key {
                KeyCode::Char(k) => k.to_string(),
                _ => c.shortcut.into(),
            },
            label: format!("{} — {}", c.label, c.hint),
            key: Some(c.key),
            enabled: c.enabled,
        }));
        let mut unique: Vec<Command> = Vec::new();
        for c in commands {
            if let Some(index) = unique
                .iter()
                .position(|existing| existing.shortcut == c.shortcut)
            {
                unique[index] = c;
            } else {
                unique.push(c);
            }
        }
        return unique;
    }
    let mut commands = rows(screen_action_bindings(screen));
    if matches!(screen, Screen::Automation | Screen::Settings) {
        commands.extend((1..=7).map(|n| Command::new(&n.to_string(), "Open configuration tab")));
        commands.push(Command::new("Shift+Tab", "Previous tab"));
        commands.push(Command::new(
            "←→",
            "Adjust focused tool / supervisor setting",
        ));
    }
    if screen == Screen::CoordinatorLive {
        let selected = state.selected_live_task().is_some();
        let has_owner = state
            .coordinator_ownership
            .record
            .as_ref()
            .is_some_and(|r| r.owner.is_some());
        let can_control = !has_owner || state.coordinator_ownership.is_owner;
        for c in &mut commands {
            if matches!(
                c.key,
                Some(KeyCode::Char('r' | 's' | 'd')) | Some(KeyCode::Enter)
            ) {
                c.enabled = selected;
            }
            if matches!(
                c.key,
                Some(KeyCode::Char('R' | 'y' | 'c' | 'u' | 'v' | 'k' | 'r' | 's'))
            ) {
                c.enabled &= can_control;
            }
            if c.key == Some(KeyCode::Char('T')) {
                c.enabled = !state.coordinator_ownership.is_owner;
            }
        }
    }
    if !commands.iter().any(|c| c.key == Some(KeyCode::Esc)) {
        commands.push(Command::new(
            "Esc",
            if screen == Screen::Home {
                "Quit"
            } else {
                "Back"
            },
        ));
    }
    let mut common = vec![
        ("h", "Home"),
        ("t", "Tools"),
        ("o", "Coordinator settings"),
        ("m", "MCP servers"),
        ("g", "Logs"),
        ("p", "Preview configuration"),
        ("x", "Apply configuration…"),
        ("O", "O2 workspace"),
        ("J", "Sessions"),
        ("A", "Waiting approval"),
        ("U", "Redo configuration"),
    ];
    if screen != Screen::CoordinatorLive {
        common.extend([
            ("e", "General settings"),
            ("v", "Coordinator live"),
            ("s", "Save configuration"),
            ("u", "Undo configuration"),
        ]);
    }
    for c in rows(common) {
        if !commands.iter().any(|local| local.key == c.key) {
            commands.push(c);
        }
    }
    commands
}

pub(crate) fn screen_action_bindings(screen: Screen) -> Vec<(&'static str, &'static str)> {
    match screen {
        Screen::Home => vec![
            ("r", "Start coordinator"),
            ("O", "O2 workspace"),
            ("J", "Sessions"),
            ("d", "Doctor"),
            ("p", "Preview"),
        ],
        Screen::Tools => vec![
            ("↑↓", "Move"),
            ("Space", "Toggle"),
            ("Enter", "Configure"),
            ("i", "Install"),
            ("d", "Refresh"),
            ("f", "Context"),
        ],
        Screen::Automation | Screen::Settings => vec![
            ("Tab/1-7", "Tabs"),
            ("↑↓", "Move"),
            ("Space/Enter", "Edit"),
            ("s", "Save"),
        ],
        Screen::ToolSettings => vec![("↑↓", "Move"), ("Space/Enter", "Edit"), ("s", "Save")],
        Screen::Skills => vec![
            ("↑↓", "Move"),
            ("Space/Enter", "Toggle"),
            ("a", "All"),
            ("n", "None"),
            ("/", "Search"),
        ],
        Screen::Mcp => vec![
            ("↑↓", "Move"),
            ("Space/Enter", "Toggle"),
            ("a", "All"),
            ("n", "None"),
            ("/", "Search"),
        ],
        Screen::Agents => vec![
            ("↑↓", "Move"),
            ("Space/Enter", "Toggle"),
            ("a", "All"),
            ("n", "None"),
            ("/", "Search"),
        ],
        Screen::Logs => vec![
            ("↑↓", "Files"),
            ("PgUp/PgDn", "Scroll"),
            ("r", "Refresh"),
            ("/", "Search"),
        ],
        Screen::Preview => vec![
            ("↑↓", "Ops"),
            ("PgUp/PgDn", "Diff"),
            ("r", "Refresh"),
            ("x", "Apply"),
        ],
        Screen::Apply => vec![
            ("Enter", "Review and confirm apply…"),
            ("↑↓", "Select operation"),
            ("PgUp/PgDn", "Scroll result"),
            ("r", "Refresh plan"),
            ("Esc", "Back"),
        ],
        Screen::Watch => vec![("↑↓", "Workers"), ("r", "Refresh"), ("/", "Search")],
        Screen::Sessions | Screen::O2 => vec![
            ("F2", "Actions"),
            ("Tab", "Sections"),
            ("↑↓", "Select"),
            ("Enter", "Edit / events"),
            ("p/s", "Preview / apply policy"),
            ("O/J", "O2 / Sessions"),
        ],
        Screen::About => vec![("Esc/q", "Back")],
        Screen::CoordinatorLive => vec![
            ("R", "Run coordinator"),
            ("u", "Resume paused run"),
            ("y", "Sync registry"),
            ("c", "Reconcile"),
            ("v", "Recovery options"),
            ("l", "Toggle logs"),
            ("↑↓", "Select task"),
            ("Enter", "Explain task"),
            ("d", "Task diff"),
            ("r", "Requeue selected task"),
            ("/", "Search"),
            ("T", "Request ownership"),
            ("k", "Stop coordinator…"),
            ("s", "Stop selected task…"),
        ],
    }
}
