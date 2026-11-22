//! Shared mode rail and inspectors for operator work and configuration drafts.
use super::*;
use navigation::{Destination, Mode};
pub(super) mod changes;
pub(super) use changes::edit_change;

pub(super) fn pending(s: &AppState) -> usize {
    s.decision_center.pending() + s.awaiting_approvals().count()
}
pub(super) fn rail(f: &mut Frame, s: &AppState, area: Rect, drafts: usize) -> Rect {
    if area.width < 105 || area.height < 14 {
        return area;
    }
    let split = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Length(23), Constraint::Min(1)])
        .split(area);
    let mode = navigation::mode(s);
    let current = navigation::current(s);
    let accent = if mode == Mode::Operate {
        theme().accent
    } else {
        theme().configure
    };
    let mut lines = Vec::new();
    let grouped = area.height >= if mode == Mode::Configure { 21 } else { 17 };
    for &(key, dest) in mode.routes() {
        let group = match dest {
            Destination::Overview => Some("RUN"),
            Destination::Sessions => Some("INSPECT"),
            Destination::Changes => Some("PENDING"),
            Destination::General => Some("PROJECT"),
            Destination::Hooks => Some("RUNTIME"),
            _ => None,
        };
        if let Some(label) = group.filter(|_| grouped) {
            if !lines.is_empty() {
                lines.push(Line::raw(""));
            }
            lines.push(Line::styled(
                format!(" {label}"),
                Style::default()
                    .fg(theme().muted)
                    .add_modifier(Modifier::BOLD),
            ));
        }
        let selected = current == dest
            || (dest == Destination::Live && current == Destination::Workers)
            || (dest == Destination::Skills && current == Destination::Agents);
        let label = match dest {
            Destination::Coordinator => "Coordinator",
            Destination::SessionSettings => "Sessions",
            Destination::MemorySettings => "O2 Memory",
            Destination::Skills => "Skills & agents",
            _ => dest.label(),
        };
        let mut entry = Line::from(vec![
            Span::styled(
                format!("{} {key}  ", if selected { "›" } else { " " }),
                Style::default().fg(accent),
            ),
            Span::styled(label, Style::default().fg(theme().foreground)),
        ]);
        if selected {
            entry = entry.style(
                Style::default()
                    .bg(theme().selection(mode))
                    .add_modifier(Modifier::BOLD),
            );
        }
        let count = match dest {
            Destination::Changes if drafts > 0 => Some((format!(" ◆ {drafts}"), theme().draft)),
            Destination::Inbox if pending(s) > 0 => {
                Some((format!(" ▲ {}", pending(s)), theme().warn))
            }
            _ => None,
        };
        if let Some((text, color)) = count {
            entry
                .spans
                .push(Span::styled(text, Style::default().fg(color)));
        }
        if selected {
            let remaining =
                usize::from(split[0].width.saturating_sub(2)).saturating_sub(entry.width());
            entry.spans.push(Span::raw(" ".repeat(remaining)));
        }
        lines.push(entry);
    }
    let pending = pending(s);
    if grouped {
        let badge = match mode {
            Mode::Operate if drafts > 0 => Some((format!(" ◆ {drafts} unsaved"), theme().draft)),
            Mode::Configure if pending > 0 => {
                Some((format!(" ▲ {pending} in inbox"), theme().warn))
            }
            _ => None,
        };
        let footer_len = 2 + usize::from(badge.is_some());
        let pad =
            (usize::from(area.height).saturating_sub(2 + footer_len)).saturating_sub(lines.len());
        lines.extend((0..pad).map(|_| Line::raw("")));
        lines.push(Line::styled(
            if mode == Mode::Operate {
                " CONFIGURE"
            } else {
                " OPERATE"
            },
            Style::default().fg(theme().muted),
        ));
        if let Some((text, color)) = badge {
            lines.push(Line::styled(text, Style::default().fg(color)));
        }
        lines.push(Line::styled(
            " Ctrl+T  Switch mode",
            Style::default().fg(accent),
        ));
    }
    f.render_widget(Paragraph::new(lines).block(panel(mode.label())), split[0]);
    split[1]
}
pub(super) fn has_draft(s: &AppState) -> bool {
    s.working_copy != s.config || s.sessions.policy_changes().next().is_some()
}
pub fn row_count(s: &AppState) -> usize {
    if s.current_screen() == Screen::Home {
        s.observations
            .latest
            .as_ref()
            .map_or(0, |o| o.readiness.steps.len())
    } else if s.current_screen() == Screen::Inbox {
        crate::inbox::count(s)
    } else if s.current_screen() == Screen::Doctor {
        s.doctor_findings.len() + s.coordinator_throttled_tools.len()
    } else {
        changes::draft(s).len() + changes::saved(s).len()
    }
}
pub fn live_panes(area: Rect, logs: bool) -> [Rect; 3] {
    let result_height = if logs { (area.height / 3).min(12) } else { 0 };
    let detail_height = ((area.height - result_height) / 2).min(14);
    let panes = Layout::default()
        .constraints([
            Constraint::Min(0),
            Constraint::Length(detail_height),
            Constraint::Length(result_height),
        ])
        .split(area);
    [panes[0], panes[1], panes[2]]
}
