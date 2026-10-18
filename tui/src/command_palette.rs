//! Searchable commands; execution returns through the normal key dispatcher.
use super::*;
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum CommandPaletteAction {
    Key(KeyCode),
    Navigate(Screen),
    Doctor,
    OpenLogs,
    OpenO2,
    OpenSessions,
    OpenSkills,
    OpenMcp,
    ApplyConfig,
}

#[derive(Debug, Clone)]
pub(crate) struct CommandPaletteItem {
    pub(crate) title: String,
    pub(crate) hint: String,
    pub(crate) action: CommandPaletteAction,
    pub(crate) enabled: bool,
}

pub(crate) fn command_palette_items() -> Vec<CommandPaletteItem> {
    vec![
        CommandPaletteItem {
            title: "O2 — Memory & context".into(),
            hint:
                "Configure shared memory, context, compaction, code intelligence and policy trials"
                    .into(),
            action: CommandPaletteAction::OpenO2,
            enabled: true,
        },
        CommandPaletteItem {
            title: "Sessions — Conversations & settings".into(),
            hint: "Inspect session history, ownership, checkpoints, usage and storage migration"
                .into(),
            action: CommandPaletteAction::OpenSessions,
            enabled: true,
        },
        CommandPaletteItem {
            title: "Doctor".into(),
            hint: "Check setup and readiness".into(),
            action: CommandPaletteAction::Doctor,
            enabled: true,
        },
        CommandPaletteItem {
            title: "Open logs".into(),
            hint: "Inspect MACC and coordinator logs".into(),
            action: CommandPaletteAction::OpenLogs,
            enabled: true,
        },
        CommandPaletteItem {
            title: "Open skills".into(),
            hint: "Enable, disable, or inspect skills".into(),
            action: CommandPaletteAction::OpenSkills,
            enabled: true,
        },
        CommandPaletteItem {
            title: "Open MCP".into(),
            hint: "Configure MCP server selections".into(),
            action: CommandPaletteAction::OpenMcp,
            enabled: true,
        },
        CommandPaletteItem {
            title: "Apply config".into(),
            hint: "Preview consent and write managed config".into(),
            action: CommandPaletteAction::ApplyConfig,
            enabled: true,
        },
    ]
}

pub(crate) fn contextual_items(state: &AppState) -> Vec<CommandPaletteItem> {
    let query = state.command_palette_query.trim().to_ascii_lowercase();
    let mut items: Vec<_> = command_catalog::active(state)
        .into_iter()
        .flat_map(|c| {
            let keys = match c.shortcut.as_str() {
                "↑↓" => vec![("↑".into(), KeyCode::Up), ("↓".into(), KeyCode::Down)],
                "←→" => vec![("←".into(), KeyCode::Left), ("→".into(), KeyCode::Right)],
                "PgUp/PgDn" => vec![
                    ("PgUp".into(), KeyCode::PageUp),
                    ("PgDn".into(), KeyCode::PageDown),
                ],
                _ => c
                    .key
                    .map(|key| vec![(c.shortcut.clone(), key)])
                    .unwrap_or_default(),
            };
            keys.into_iter()
                .map(move |(shortcut, key)| CommandPaletteItem {
                    title: format!(
                        "[{}] {}{}",
                        shortcut,
                        c.label.split(" — ").next().unwrap_or(&c.label),
                        if c.enabled { "" } else { " (unavailable)" }
                    ),
                    hint: c
                        .label
                        .split_once(" — ")
                        .map(|(_, hint)| hint.to_string())
                        .unwrap_or_else(|| {
                            "Existing confirmations and ownership checks still apply.".into()
                        }),
                    action: CommandPaletteAction::Key(key),
                    enabled: c.enabled,
                })
        })
        .collect();
    if !command_catalog::exclusive(state) {
        items.extend(command_palette_items());
        items.extend(
            [
                (Screen::Home, "Home"),
                (Screen::Tools, "Tools"),
                (Screen::Automation, "Settings"),
                (Screen::CoordinatorLive, "Coordinator live"),
            ]
            .into_iter()
            .map(|(screen, label)| CommandPaletteItem {
                title: format!("Go to {label}"),
                hint: "Navigation".into(),
                action: CommandPaletteAction::Navigate(screen),
                enabled: true,
            }),
        );
    }
    items.retain(|c| {
        query.is_empty()
            || c.title.to_ascii_lowercase().contains(&query)
            || c.hint.to_ascii_lowercase().contains(&query)
    });
    items
}

pub(crate) fn open_command_palette(state: &mut AppState) {
    state.command_palette_open = true;
    state.command_palette_query.clear();
    state.command_palette_selection = 0;
}

pub(crate) fn close_command_palette(state: &mut AppState) {
    state.command_palette_open = false;
    state.command_palette_query.clear();
    state.command_palette_selection = 0;
}

pub(crate) fn handle_command_palette_key(state: &mut AppState, key: KeyCode) {
    match key {
        KeyCode::Esc | KeyCode::F(2) => close_command_palette(state),
        KeyCode::Backspace => {
            state.command_palette_query.pop();
            state.command_palette_selection = 0;
        }
        KeyCode::Up => {
            state.command_palette_selection = state.command_palette_selection.saturating_sub(1);
        }
        KeyCode::Down => {
            let max = contextual_items(state).len().saturating_sub(1);
            state.command_palette_selection = (state.command_palette_selection + 1).min(max);
        }
        KeyCode::Enter => {
            let items = contextual_items(state);
            if let Some(item) = items
                .get(state.command_palette_selection)
                .filter(|c| c.enabled)
                .cloned()
            {
                close_command_palette(state);
                execute_command_palette_action(state, item.action);
            }
        }
        KeyCode::Char(c) => {
            state.command_palette_query.push(c);
            state.command_palette_selection = 0;
        }
        _ => {}
    }
}

pub(crate) fn execute_command_palette_action(state: &mut AppState, action: CommandPaletteAction) {
    match action {
        CommandPaletteAction::Key(key) => super::handle_key(state, key),
        CommandPaletteAction::Navigate(screen) => state.push_screen(screen),
        CommandPaletteAction::Doctor => {
            state.goto_screen(Screen::Home);
            state.run_home_doctor_check();
        }
        CommandPaletteAction::OpenO2 => session_navigation::open(state, true),
        CommandPaletteAction::OpenSessions => session_navigation::open(state, false),
        CommandPaletteAction::OpenLogs => state.push_screen(Screen::Logs),
        CommandPaletteAction::OpenSkills => state.push_screen(Screen::Skills),
        CommandPaletteAction::OpenMcp => state.push_screen(Screen::Mcp),
        CommandPaletteAction::ApplyConfig => state.open_apply_screen(),
    }
}

pub(crate) fn render_command_palette_overlay(f: &mut Frame, state: &AppState) {
    let area = ui::centered_rect(88, 90, ui::content_area(f.size()));
    let theme = theme();
    let block = panel("Command palette · current page / window + navigation");
    let inner = block.inner(area);
    f.render_widget(Clear, area);
    f.render_widget(block, area);
    let parts = Layout::default()
        .constraints([
            Constraint::Length(1),
            Constraint::Min(1),
            Constraint::Length(3),
        ])
        .split(inner);
    f.render_widget(
        Paragraph::new(format!("Search: {}_", state.command_palette_query))
            .style(Style::default().fg(theme.accent)),
        parts[0],
    );
    let commands = contextual_items(state);
    if commands.is_empty() {
        f.render_widget(Paragraph::new("No matching command."), parts[1]);
        return;
    }
    let items = commands
        .iter()
        .map(|c| {
            ListItem::new(c.title.clone()).style(Style::default().fg(if c.enabled {
                theme.accent
            } else {
                theme.muted
            }))
        })
        .collect::<Vec<_>>();
    let selected = state.command_palette_selection.min(commands.len() - 1);
    let mut selection = ListState::default();
    selection.select(Some(selected));
    f.render_stateful_widget(
        List::new(items).highlight_symbol("› ").highlight_style(
            Style::default()
                .bg(theme.highlight_bg)
                .add_modifier(Modifier::BOLD),
        ),
        parts[1],
        &mut selection,
    );
    let item = &commands[selected];
    let hint = if item.enabled {
        item.hint.clone()
    } else {
        format!("Unavailable in the current state. {}", item.hint)
    };
    f.render_widget(Paragraph::new(hint).wrap(Wrap { trim: true }), parts[2]);
}
