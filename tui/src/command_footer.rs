//! Fixed command area, rendered after overlays so modal commands stay visible.
use super::*;

pub(crate) fn draw(f: &mut Frame, state: &AppState) {
    let area = f.size();
    let height = area.height.min(4);
    let area = Rect::new(
        area.x,
        area.bottom().saturating_sub(height),
        area.width,
        height,
    );
    let theme = theme();
    let width = area.width.saturating_sub(2) as usize;
    let (controls, actions) = if state.command_palette_open {
        (
            "[↑↓] Select  [Enter] Run  [Esc] Close".to_string(),
            "Type to filter commands".to_string(),
        )
    } else if state.help_open {
        (
            "[↑↓] Scroll  [PgUp/PgDn] Page  [F1/Esc] Close".to_string(),
            "Help for the current page or window".into(),
        )
    } else {
        let commands = command_catalog::active(state);
        let escape = commands
            .iter()
            .find(|c| c.key == Some(KeyCode::Esc))
            .map(|c| c.label.as_str())
            .unwrap_or("Back / cancel");
        let controls = format!("[F2] Commands  [F1] Help  [Esc] {escape}");
        let mut chunks = Vec::new();
        let count = commands.len();
        let mut used = 0;
        for c in commands {
            if c.key == Some(KeyCode::Esc) {
                continue;
            }
            let chunk = format!(
                "[{}] {}{}",
                c.shortcut,
                c.label.split(" — ").next().unwrap_or(&c.label),
                if c.enabled { "" } else { " (unavailable)" }
            );
            let next = used + chunk.chars().count() + if chunks.is_empty() { 0 } else { 5 };
            if next + 13 > width {
                break;
            }
            used = next;
            chunks.push(chunk);
        }
        let more = chunks.len() + 1 < count;
        let mut actions = chunks.join("  |  ");
        if more {
            actions.push_str(" … [F2] All");
        }
        (controls, actions)
    };
    f.render_widget(Clear, area);
    let lines = vec![
        Line::styled(controls, Style::default().fg(theme.accent)),
        Line::from(fit(&actions, width)),
    ];
    f.render_widget(
        Paragraph::new(lines).block(panel(&format!(
            "Commands · {}",
            state.display_breadcrumbs()
        ))),
        area,
    );
}

fn fit(text: &str, width: usize) -> String {
    if text.chars().count() <= width {
        return text.into();
    }
    let suffix = " … [F2] All";
    if width < suffix.chars().count() {
        return suffix.chars().take(width).collect();
    }
    text.chars()
        .take(width - suffix.chars().count())
        .chain(suffix.chars())
        .collect()
}
