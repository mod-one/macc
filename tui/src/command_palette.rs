//! Searchable projection of the active command registry.
use super::*;
use command_catalog::Action;
#[derive(Clone)]
pub(crate) struct CommandPaletteItem {
    pub title: String,
    pub hint: String,
    pub action: Action,
    pub enabled: bool,
}
pub(crate) fn contextual_items(s: &AppState) -> Vec<CommandPaletteItem> {
    let query = s.command_palette_query.trim().to_lowercase();
    command_catalog::active(s)
        .into_iter()
        .map(|c| {
            let shortcut = if c.key.is_some() {
                format!("[{}] ", c.shortcut())
            } else {
                String::new()
            };
            CommandPaletteItem {
                title: format!(
                    "{}{}{}",
                    shortcut,
                    c.label,
                    if c.enabled { "" } else { " (unavailable)" }
                ),
                hint: c.hint,
                action: c.action,
                enabled: c.enabled,
            }
        })
        .filter(|c| {
            query.is_empty()
                || c.title.to_lowercase().contains(&query)
                || c.hint.to_lowercase().contains(&query)
        })
        .collect()
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
                .bg(theme.selection(navigation::mode(state)))
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
