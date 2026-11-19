//! Shared field order for worker rows and the pinned supervisor intervention.
use super::*;
use chrono::{DateTime, Utc};
use macc_core::{
    coordinator::{
        view_model::{LiveTaskRow, TaskHealth},
        RuntimeStatus,
    },
    supervisor::live::SupervisorLive,
};

const HEADERS: [&str; 9] = [
    "Health", "Worker", "Task ID", "Status", "Tool", "Model", "Effort", "Age", "HB",
];

struct TaskRow {
    values: [String; 9],
    health: Color,
    status: Color,
}

fn worker(task: &LiveTaskRow) -> TaskRow {
    let t = theme();
    let (glyph, health) = match task.health {
        TaskHealth::Warning if task.runtime_status == RuntimeStatus::Failed => ("✕", t.bad),
        TaskHealth::Stale => ("◐", t.warn),
        TaskHealth::Waiting => ("·", t.muted),
        other => {
            let glyph = other.symbol();
            (glyph, t.glyph_color(glyph).unwrap_or(t.muted))
        }
    };
    let status = match task.runtime_status {
        RuntimeStatus::Failed => t.bad,
        RuntimeStatus::Paused | RuntimeStatus::WaitingForUser | RuntimeStatus::Stale => t.warn,
        RuntimeStatus::Dispatched | RuntimeStatus::Running | RuntimeStatus::PhaseDone => t.good,
        RuntimeStatus::Idle => t.muted,
    };
    let value = |s: &str| {
        if s.is_empty() {
            "—".to_owned()
        } else {
            s.to_owned()
        }
    };
    let phase = task.phase.compact_label();
    let mut label = task.status_label();
    if !phase.is_empty() {
        label.push(' ');
        label.push_str(phase);
    }
    TaskRow {
        values: [
            glyph.into(),
            value(&task.worker_id),
            task.task_id.clone(),
            label,
            value(&task.tool),
            value(&task.model),
            value(&task.effort),
            task.age_label(),
            value(&task.heartbeat_age_label()),
        ],
        health,
        status,
    }
}

impl TaskRow {
    fn style(&self, index: usize) -> Style {
        let theme = theme();
        Style::default().fg(match index {
            0 => self.health,
            3 => self.status,
            7 | 8 => theme.muted,
            _ => theme.foreground,
        })
    }
}

// Keep every field available when a long task ID or model exceeds its cell.
fn wrapped(value: &str, width: u16) -> Vec<Line<'static>> {
    let mut lines = Vec::new();
    let mut current = String::new();
    let mut used = 0;
    for ch in value.chars() {
        let mut buf = [0; 4];
        let n = Span::raw(&*ch.encode_utf8(&mut buf)).width();
        if used + n > usize::from(width) && !current.is_empty() {
            lines.push(Line::raw(std::mem::take(&mut current)));
            used = 0;
        }
        current.push(ch);
        used += n;
    }
    lines.push(Line::raw(current));
    lines
}

pub(super) fn draw(
    f: &mut Frame,
    area: Rect,
    workers: &[LiveTaskRow],
    live: Option<&SupervisorLive>,
    now: DateTime<Utc>,
    selected: Option<usize>,
    empty: impl FnOnce() -> Vec<Line<'static>>,
) {
    let theme = theme();
    let title = match live {
        Some(s) if s.active => "LIVE TASKS · supervisor intervention running",
        Some(_) => "LIVE TASKS · supervisor needs attention",
        None => "LIVE TASKS",
    };
    let block = ui::focus_panel(title, navigation::Mode::Operate);
    let inner = block.inner(area);
    f.render_widget(block, area);
    let rows: Vec<_> = live
        .into_iter()
        .map(|s| {
            let color = if s.active { theme.good } else { theme.warn };
            TaskRow {
                values: supervisor::row_cells(s, now),
                health: color,
                status: color,
            }
        })
        .chain(workers.iter().map(worker))
        .collect();
    let content_width = inner.width.saturating_sub(2);
    let minimum: usize = HEADERS.iter().map(|h| h.len()).sum::<usize>() + HEADERS.len() - 1;
    if usize::from(content_width) < minimum && !rows.is_empty() {
        // On a portrait terminal use labelled fields within the same selectable row.
        let items = rows.iter().map(|row| {
            let mut lines = Vec::new();
            let mut line = Line::default();
            for (i, label) in HEADERS.iter().enumerate() {
                let field = format!("{label} {}", row.values[i]);
                for part in wrapped(&field, content_width.max(1)) {
                    if !line.spans.is_empty() {
                        if line.width() + 2 + part.width() > usize::from(content_width) {
                            lines.push(std::mem::take(&mut line));
                        } else {
                            line.spans.push(Span::raw("  "));
                        }
                    }
                    line.spans
                        .extend(part.spans.into_iter().map(|s| s.style(row.style(i))));
                }
            }
            lines.push(line);
            ListItem::new(lines)
        });
        f.render_stateful_widget(
            List::new(items)
                .highlight_symbol("› ")
                .highlight_style(Style::default().bg(theme.operate_selection)),
            inner,
            &mut ListState::default().with_selected(selected),
        );
        return;
    }
    let mut widths = workspace_ui::measured_widths(
        &HEADERS,
        rows.iter().map(|r| r.values.as_slice()),
        content_width,
    );
    let used = widths.iter().sum::<u16>() + HEADERS.len() as u16 - 1;
    widths[2] += content_width.saturating_sub(used);
    let table_rows = rows.iter().map(|row| {
        let mut height = 1;
        let cells: Vec<_> = row
            .values
            .iter()
            .enumerate()
            .map(|(i, value)| {
                let lines = wrapped(value, widths[i]);
                height = height.max(lines.len());
                Cell::from(ratatui::text::Text::from(lines)).style(row.style(i))
            })
            .collect();
        Row::new(cells)
            .height(height.min(usize::from(inner.height.saturating_sub(1).max(1))) as u16)
    });
    let table = Table::new(table_rows, widths.iter().copied().map(Constraint::Length))
        .header(
            Row::new(HEADERS).style(
                Style::default()
                    .fg(theme.muted)
                    .add_modifier(Modifier::BOLD),
            ),
        )
        .highlight_symbol("› ")
        .highlight_style(
            Style::default()
                .bg(theme.operate_selection)
                .add_modifier(Modifier::BOLD),
        );
    f.render_stateful_widget(
        table,
        inner,
        &mut TableState::default().with_selected(selected),
    );
    if rows.is_empty() {
        let message = Rect::new(
            inner.x,
            inner.y.saturating_add(1),
            inner.width,
            inner.height.saturating_sub(1),
        );
        f.render_widget(Paragraph::new(empty()).wrap(Wrap { trim: true }), message);
    }
}
