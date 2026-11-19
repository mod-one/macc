//! Presentation primitives for interactive administration; never execute domain actions.
use ratatui::{
    layout::{Constraint, Direction, Layout, Rect},
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Cell, Paragraph, Row, Table, TableState, Wrap},
    Frame,
};
use serde_json::Value;
pub(crate) fn tab_line(
    f: &mut Frame,
    area: Rect,
    labels: &[&str],
    selected: usize,
    accent: ratatui::style::Color,
) {
    let theme = crate::ui::theme();
    let mut spans: Vec<_> = labels
        .iter()
        .enumerate()
        .map(|(i, label)| {
            Span::styled(
                format!("{} {}  ", i + 1, label),
                if i == selected {
                    Style::default()
                        .fg(accent)
                        .add_modifier(Modifier::UNDERLINED)
                        .add_modifier(Modifier::BOLD)
                } else {
                    Style::default().fg(theme.muted)
                },
            )
        })
        .collect();
    // The tab strip has a single content row. Keep the active tab in view
    // even when the terminal cannot fit all preceding labels.
    let visible = usize::from(area.width);
    let mut first = selected.min(spans.len().saturating_sub(1));
    let mut used = spans.get(first).map_or(0, Span::width);
    while first > 0 && used + spans[first - 1].width() <= visible {
        first -= 1;
        used += spans[first].width();
    }
    spans.drain(..first);
    f.render_widget(Paragraph::new(Line::from(spans)), area);
}
pub(crate) fn columns(area: Rect) -> (Rect, Rect) {
    let parts = if area.width >= 72 {
        Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Percentage(55), Constraint::Percentage(45)])
            .split(area)
    } else {
        Layout::default()
            .constraints([Constraint::Percentage(55), Constraint::Percentage(45)])
            .split(area)
    };
    (parts[0], parts[1])
}
pub(crate) fn table(
    f: &mut Frame,
    area: Rect,
    title: &str,
    headers: &[&str],
    rows: Vec<Vec<String>>,
    selected: Option<usize>,
    mode: crate::navigation::Mode,
) {
    let theme = crate::ui::theme();
    let widths = measured_widths(
        headers,
        rows.iter().map(Vec::as_slice),
        area.width.saturating_sub(4),
    );
    let mut state = TableState::default().with_selected(selected);
    f.render_stateful_widget(
        Table::new(
            rows.into_iter().map(|r| {
                Row::new(r.into_iter().map(|text| {
                    let style = theme
                        .glyph_color(&text)
                        .map(|c| Style::default().fg(c))
                        .unwrap_or_default();
                    Cell::from(text).style(style)
                }))
            }),
            widths.into_iter().map(Constraint::Length),
        )
        .header(
            Row::new(headers.iter().copied()).style(
                Style::default()
                    .fg(theme.mode_accent(mode))
                    .add_modifier(Modifier::BOLD),
            ),
        )
        .block(crate::ui::focus_panel(title, mode))
        .highlight_symbol("› ")
        .highlight_style(Style::default().bg(theme.selection(mode))),
        area,
        &mut state,
    );
}
/// Allocate cells by displayed width. Short status columns stop growing as
/// soon as their contents fit, leaving the remaining cells for long subjects.
pub(crate) fn measured_widths<'a>(
    headers: &[&str],
    rows: impl Iterator<Item = &'a [String]> + Clone,
    width: u16,
) -> Vec<u16> {
    let available = usize::from(width).saturating_sub(headers.len().saturating_sub(1));
    let measure = |s: &str| Span::raw(s).width();
    let target: Vec<_> = headers
        .iter()
        .enumerate()
        .map(|(i, h)| {
            rows.clone()
                .filter_map(|r| r.get(i))
                .map(|c| measure(c))
                .max()
                .unwrap_or(0)
                .max(measure(h))
                .max(1)
        })
        .collect();
    let mut cells = vec![0usize; headers.len()];
    // Round-robin allocation avoids starving the final columns on small screens.
    let mut remaining = available;
    for limits in [
        headers
            .iter()
            .map(|s| measure(s).max(1))
            .collect::<Vec<_>>(),
        target,
    ] {
        while remaining > 0 {
            let before = remaining;
            for (used, max) in cells.iter_mut().zip(&limits) {
                if remaining > 0 && *used < *max {
                    *used += 1;
                    remaining -= 1;
                }
            }
            if remaining == before {
                break;
            }
        }
    }
    cells.into_iter().map(|w| w as u16).collect()
}
pub(crate) fn switch(enabled: bool) -> &'static str {
    if enabled {
        "● On"
    } else {
        "○ Off"
    }
}
pub(crate) fn display_setting(raw: String) -> String {
    match raw.as_str() {
        "" => "—".into(),
        "true" => switch(true).into(),
        "false" => switch(false).into(),
        _ => raw,
    }
}
pub(crate) fn value(v: &Value) -> String {
    match v {
        Value::Bool(v) => switch(*v).into(),
        Value::Null => "—".into(),
        Value::String(v) => v.clone(),
        _ => v.to_string(),
    }
}
pub(crate) fn evidence(raw: &str, show_raw: bool) -> String {
    if show_raw {
        return raw.into();
    }
    let (prefix, payload) = raw
        .find("\n{")
        .or_else(|| raw.find("\n["))
        .map(|n| (&raw[..n], &raw[n + 1..]))
        .unwrap_or(("", raw));
    let Ok(v) = serde_json::from_str::<Value>(payload) else {
        return raw.into();
    };
    fn visit(v: &Value, depth: usize, lines: &mut Vec<String>) {
        if lines.len() >= 100 {
            return;
        }
        match v {
            Value::Object(map) => {
                for (k, v) in map {
                    if lines.len() >= 100 {
                        break;
                    }
                    let label = k.replace('_', " ");
                    if (v.is_object() || v.is_array()) && depth < 3 {
                        lines.push(format!("{}{label}:", "  ".repeat(depth)));
                        visit(v, depth + 1, lines)
                    } else {
                        lines.push(format!(
                            "{}{label}: {}",
                            "  ".repeat(depth),
                            crate::ui::truncate_middle(&value(v), 512)
                        ))
                    }
                }
            }
            Value::Array(rows) => {
                for row in rows.iter().take(20) {
                    visit(row, depth, lines)
                }
            }
            _ => lines.push(format!(
                "{}{}",
                "  ".repeat(depth),
                crate::ui::truncate_middle(&value(v), 512)
            )),
        }
    }
    let mut lines = if prefix.is_empty() {
        Vec::new()
    } else {
        vec![prefix.to_string(), String::new()]
    };
    visit(&v, 0, &mut lines);
    lines.push("\n[Ctrl+R] inspect complete raw evidence".into());
    lines.join("\n")
}
pub(crate) fn details(f: &mut Frame, area: Rect, title: &str, text: String, scroll: u16) {
    f.render_widget(
        Paragraph::new(text)
            .block(crate::ui::panel(title))
            .wrap(Wrap { trim: false })
            .scroll((scroll, 0)),
        area,
    );
}
