//! Presentation primitives for interactive administration; never execute domain actions.
use crossterm::event::KeyCode;
use ratatui::{
    layout::{Constraint, Direction, Layout, Rect},
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{
        Block, Borders, Cell, Clear, List, ListItem, ListState, Paragraph, Row, Table, TableState,
        Wrap,
    },
    Frame,
};
use serde_json::Value;
#[derive(Clone)]
pub(crate) struct Command {
    pub key: KeyCode,
    pub shortcut: &'static str,
    pub label: &'static str,
    pub hint: &'static str,
    pub enabled: bool,
}
impl Command {
    pub fn new(key: char, label: &'static str, hint: &'static str) -> Self {
        Self {
            key: KeyCode::Char(key),
            shortcut: "",
            label,
            hint,
            enabled: true,
        }
    }
    pub fn when(mut self, enabled: bool) -> Self {
        self.enabled = enabled;
        self
    }
}
#[derive(Default)]
pub(crate) struct ActionMenu {
    pub open: bool,
    selected: usize,
}
impl ActionMenu {
    /// Resolve before the caller's mutation gate, so menu selection cannot bypass ownership.
    pub fn resolve(&mut self, key: KeyCode, commands: &[Command]) -> Option<KeyCode> {
        if !self.open {
            if key == KeyCode::F(2) {
                self.open = true;
                self.selected = 0;
                return None;
            }
            return Some(key);
        }
        self.selected = self.selected.min(commands.len().saturating_sub(1));
        match key {
            KeyCode::Esc | KeyCode::F(2) => self.open = false,
            KeyCode::Up => self.selected = self.selected.saturating_sub(1),
            KeyCode::Down => {
                self.selected = (self.selected + 1).min(commands.len().saturating_sub(1))
            }
            KeyCode::Enter => {
                if let Some(c) = commands.get(self.selected).filter(|c| c.enabled) {
                    self.open = false;
                    return Some(c.key);
                }
            }
            _ => {
                if let Some(c) = commands.iter().find(|c| c.key == key && c.enabled) {
                    self.open = false;
                    return Some(c.key);
                }
            }
        }
        None
    }
    pub fn draw(&self, f: &mut Frame, commands: &[Command]) {
        if !self.open {
            return;
        }
        let theme = crate::ui::theme();
        let area = crate::ui::centered_rect(85, 85, f.size());
        f.render_widget(Clear, area);
        let parts = Layout::default()
            .constraints([Constraint::Min(3), Constraint::Length(4)])
            .split(area);
        let rows: Vec<_> = commands
            .iter()
            .map(|c| {
                let shortcut = if c.shortcut.is_empty() {
                    match c.key {
                        KeyCode::Char(v) => v.to_string(),
                        _ => String::new(),
                    }
                } else {
                    c.shortcut.into()
                };
                ListItem::new(format!(
                    "[{shortcut}] {}{}",
                    c.label,
                    if c.enabled { "" } else { " (unavailable)" }
                ))
                .style(Style::default().fg(if c.enabled {
                    theme.accent_dim
                } else {
                    theme.muted
                }))
            })
            .collect();
        let mut state = ListState::default().with_selected(Some(self.selected));
        f.render_stateful_widget(
            List::new(rows)
                .block(
                    Block::default()
                        .title("Actions — choose with ↑↓, then Enter")
                        .borders(Borders::ALL),
                )
                .highlight_style(
                    Style::default()
                        .bg(theme.highlight_bg)
                        .add_modifier(Modifier::BOLD),
                )
                .highlight_symbol("› "),
            parts[0],
            &mut state,
        );
        let hint = commands
            .get(self.selected)
            .map(|c| {
                format!(
                    "{}{}\nEsc: close without changes",
                    if c.enabled {
                        ""
                    } else {
                        "Unavailable in the current state. "
                    },
                    c.hint
                )
            })
            .unwrap_or_default();
        f.render_widget(
            Paragraph::new(hint)
                .wrap(Wrap { trim: false })
                .block(crate::ui::panel("What this action does")),
            parts[1],
        );
    }
}
pub(crate) fn tabs(f: &mut Frame, area: Rect, labels: &[&str], selected: usize) {
    let theme = crate::ui::theme();
    let spans: Vec<_> = labels
        .iter()
        .enumerate()
        .flat_map(|(i, label)| {
            [
                Span::styled(
                    format!("{}:{} ", i + 1, label),
                    if i == selected {
                        Style::default()
                            .fg(theme.accent)
                            .bg(theme.highlight_bg)
                            .add_modifier(Modifier::BOLD)
                    } else {
                        Style::default().fg(theme.muted)
                    },
                ),
                Span::raw(""),
            ]
        })
        .collect();
    f.render_widget(
        Paragraph::new(Line::from(spans))
            .wrap(Wrap { trim: false })
            .block(crate::ui::panel("Sections — number or Tab / Shift+Tab")),
        area,
    );
}
pub(crate) fn columns(area: Rect) -> (Rect, Rect) {
    let parts = if area.width >= 76 || area.height < 14 {
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
) {
    let theme = crate::ui::theme();
    let weights: Vec<u16> = match headers.len() {
        2 => vec![65, 35],
        3 => vec![55, 25, 20],
        4 => vec![40, 20, 20, 20],
        5 => vec![30, 18, 22, 20, 10],
        n => vec![(100 / n.max(1)) as u16; n],
    };
    let widths: Vec<_> = weights.iter().map(|w| Constraint::Percentage(*w)).collect();
    let mut state = TableState::default().with_selected(selected);
    f.render_stateful_widget(
        Table::new(
            rows.into_iter()
                .map(|r| Row::new(r.into_iter().map(Cell::from))),
            widths,
        )
        .header(
            Row::new(headers.iter().copied()).style(
                Style::default()
                    .fg(theme.accent)
                    .add_modifier(Modifier::BOLD),
            ),
        )
        .block(crate::ui::panel(title))
        .highlight_symbol("› ")
        .highlight_style(Style::default().bg(theme.highlight_bg)),
        area,
        &mut state,
    );
}
pub(crate) fn value(v: &Value) -> String {
    match v {
        Value::Bool(v) => if *v { "On" } else { "Off" }.into(),
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
    lines.push("\nF4: inspect complete raw evidence".into());
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
