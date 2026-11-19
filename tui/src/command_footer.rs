//! Fixed command area, rendered after overlays so modal commands stay visible.
use super::*;

pub(crate) fn draw(f: &mut Frame, state: &AppState) {
    let area = f.size();
    let height = ui::command_height(area);
    let area = Rect::new(
        area.x,
        area.bottom().saturating_sub(height),
        area.width,
        height,
    );
    let theme = theme();
    let accent = theme.mode_accent(navigation::mode(state));
    if height == 1 {
        let shortcut = if state.help_open || state.command_palette_open {
            "[Esc] Close".into()
        } else {
            palette_hint(state, "Commands")
        };
        f.render_widget(
            Paragraph::new(shortcut).style(Style::default().fg(accent)),
            area,
        );
        return;
    }
    let width = area.width.saturating_sub(2) as usize;
    let (controls, actions) = if state.command_palette_open || state.help_open {
        (
            shortcut_line(command_catalog::focused(state).iter(), width, accent, None),
            Line::raw(if state.help_open {
                "Help for the current page or window"
            } else {
                "Type to filter commands"
            }),
        )
    } else {
        let mut commands = command_catalog::active(state);
        // Put editing operations before numbered section jumps on tight terminals.
        if navigation::mode(state) == navigation::Mode::Configure
            && !command_catalog::exclusive(state)
        {
            commands.sort_by_key(|c| match c.key {
                Some(KeyCode::Char('p')) if state.current_screen() == Screen::O2 => 0,
                Some(KeyCode::Char('s')) => 1,
                Some(KeyCode::Char('x')) if state.current_screen() == Screen::O2 => 2,
                _ => 3,
            });
        }
        use command_catalog::Action;
        let chrome = |c: &command_catalog::Command| match c.action {
            Action::Palette => Some(0),
            Action::Help => Some(1),
            _ if c.key == Some(KeyCode::Esc) => Some(2),
            Action::SwitchMode => Some(3),
            Action::Quit => Some(4),
            _ => None,
        };
        let controls = shortcut_line(
            (0..5).filter_map(|i| commands.iter().find(|c| chrome(c) == Some(i))),
            width,
            theme.muted,
            None,
        );
        let actions = shortcut_line(
            commands.iter().filter(|c| chrome(c).is_none()),
            width,
            accent,
            Some(&palette_hint(state, "All")),
        );
        (controls, actions)
    };
    f.render_widget(Clear, area);
    let lines = vec![actions, controls];
    f.render_widget(
        Paragraph::new(lines).block(
            panel(&format!("Commands · {}", state.display_breadcrumbs())).borders(Borders::TOP),
        ),
        area,
    );
}

fn palette_hint(state: &AppState, label: &str) -> String {
    let key = command_catalog::palette_command(state).shortcut();
    format!("[{key}] {label}")
}

fn shortcut_line<'a>(
    commands: impl IntoIterator<Item = &'a command_catalog::Command>,
    width: usize,
    key_color: Color,
    more_hint: Option<&str>,
) -> Line<'static> {
    let theme = theme();
    let mut line = Line::default();
    let mut commands = commands.into_iter().peekable();
    let mut more = false;
    while let Some(c) = commands.next() {
        if c.key.is_none() {
            more = true;
            continue;
        }
        let key = Span::styled(
            format!("[{}]", c.shortcut()),
            Style::default()
                .fg(if c.enabled { key_color } else { theme.muted })
                .add_modifier(Modifier::BOLD),
        );
        let label = Span::styled(
            format!(
                " {}{}",
                c.label,
                if c.enabled { "" } else { " (unavailable)" }
            ),
            Style::default().fg(if c.enabled {
                theme.foreground
            } else {
                theme.muted
            }),
        );
        let gap = if line.spans.is_empty() { "" } else { "  " };
        let reserve = if commands.peek().is_some() || more {
            more_hint.map_or(0, |hint| hint.len() + 3)
        } else {
            0
        };
        if line.width() + gap.len() + key.width() + label.width() + reserve > width {
            more = true;
            break;
        }
        line.spans.extend([Span::raw(gap), key, label]);
    }
    if let Some(hint) = more_hint.filter(|_| more) {
        line.spans.push(Span::styled(
            format!(" … {hint}"),
            Style::default().fg(theme.muted),
        ));
    }
    line
}
