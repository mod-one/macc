//! The approval window: opens by itself when a human approval gate waits for
//! a decision, explains the request in full, and collects the decision.
//!
//! The window only gathers input. Recording goes through
//! `CoordinatorCommand::GateDecision`, so core validates the role, revision,
//! evidence and reason exactly as for `macc coordinator approve`.

use crossterm::event::KeyCode;
use macc_core::coordinator::approval::{approval_brief, short_revision, PendingApproval};
use macc_core::coordinator::model::ApprovalDecision;
use ratatui::{
    layout::{Constraint, Direction, Layout},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Clear, Paragraph, Wrap},
    Frame,
};

/// A decision ready to be recorded.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Decision {
    pub task_id: String,
    pub decision: ApprovalDecision,
    pub role: String,
    pub revision: Option<String>,
    pub evidence: Option<String>,
    pub reason: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Field {
    Revision,
    Evidence,
    Reason,
}

impl Field {
    fn label(self, gate: &PendingApproval) -> String {
        match self {
            Field::Revision => format!(
                "Revision of {} you reviewed (commit SHA)",
                gate.subject_task
            ),
            Field::Evidence => format!(
                "Evidence ({}: link or path){}",
                gate.evidence_type.as_deref().unwrap_or("optional"),
                if gate.evidence_required {
                    ""
                } else {
                    " — Enter to skip"
                }
            ),
            Field::Reason => "Reason (what must change, and why)".to_string(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum Mode {
    Reading,
    Role {
        decision: ApprovalDecision,
        selected: usize,
    },
    Input {
        draft: Decision,
        field: Field,
        buffer: String,
    },
    Confirm(Decision),
}

pub enum Outcome {
    Continue,
    /// Close the window; it re-opens when the gate or its revision changes,
    /// or on demand with `A`.
    Later,
    /// Show the next gate waiting for a decision.
    Next,
    Submit(Decision),
}

pub struct Window {
    pub gate: PendingApproval,
    /// Gates awaiting a decision in total, for the title.
    pub waiting: usize,
    mode: Mode,
    scroll: u16,
    message: Option<(bool, String)>,
}

impl Window {
    pub fn new(gate: PendingApproval, waiting: usize) -> Self {
        Self {
            gate,
            waiting,
            mode: Mode::Reading,
            scroll: 0,
            message: None,
        }
    }

    pub fn key_of(gate: &PendingApproval) -> (String, Option<String>) {
        (gate.task_id.clone(), gate.subject_revision.clone())
    }

    /// Replace the displayed gate with fresh data, keeping the reading
    /// position. Any half-filled form is kept only for the same revision.
    pub fn refresh(&mut self, gate: PendingApproval, waiting: usize) {
        if gate.subject_revision != self.gate.subject_revision {
            self.mode = Mode::Reading;
            self.message = Some((
                false,
                format!(
                    "{} moved to a new revision: read the request again before deciding.",
                    gate.subject_task
                ),
            ));
        }
        self.gate = gate;
        self.waiting = waiting;
    }

    pub fn report(&mut self, ok: bool, message: impl Into<String>) {
        self.mode = Mode::Reading;
        self.message = Some((ok, message.into()));
    }

    /// True while the operator is typing, so the window must not be replaced.
    pub(crate) fn command_bindings(&self) -> Vec<(&'static str, &'static str)> {
        match self.mode {
            Mode::Reading => vec![
                ("a", "Approve…"),
                ("r", "Reject…"),
                ("c", "Request changes…"),
                ("↑↓", "Scroll"),
                ("PgUp/PgDn", "Scroll page"),
                ("Tab", "Next approval"),
                ("Esc", "Decide later"),
            ],
            Mode::Role { .. } => vec![
                ("↑↓", "Select role"),
                ("Enter", "Choose role"),
                ("Esc", "Cancel decision"),
            ],
            Mode::Input { .. } => vec![
                ("Enter", "Next field"),
                ("Backspace", "Delete character"),
                ("Esc", "Cancel decision"),
            ],
            Mode::Confirm(_) => vec![("Enter", "Submit decision"), ("Esc", "Cancel decision")],
        }
    }

    pub fn is_editing(&self) -> bool {
        !matches!(self.mode, Mode::Reading)
    }

    fn roles(&self) -> Vec<String> {
        let mut roles = self.gate.missing_roles.clone();
        for role in &self.gate.required_roles {
            if !roles.contains(role) {
                roles.push(role.clone());
            }
        }
        roles
    }

    /// Next field to fill for `draft`, or `None` when it is complete.
    fn next_field(&self, draft: &Decision, skipped_evidence: bool) -> Option<Field> {
        match draft.decision {
            ApprovalDecision::Approved => {
                if draft.revision.is_none() {
                    Some(Field::Revision)
                } else if draft.evidence.is_none() && !skipped_evidence {
                    Some(Field::Evidence)
                } else {
                    None
                }
            }
            _ => draft.reason.is_none().then_some(Field::Reason),
        }
    }

    fn advance(&mut self, draft: Decision, skipped_evidence: bool) {
        self.mode = match self.next_field(&draft, skipped_evidence) {
            Some(field) => Mode::Input {
                draft,
                field,
                buffer: String::new(),
            },
            None => Mode::Confirm(draft),
        };
    }

    fn start(&mut self, decision: ApprovalDecision) {
        self.message = None;
        let roles = self.roles();
        if roles.len() == 1 {
            self.choose_role(decision, roles[0].clone());
        } else {
            self.mode = Mode::Role {
                decision,
                selected: 0,
            };
        }
    }

    fn choose_role(&mut self, decision: ApprovalDecision, role: String) {
        let draft = Decision {
            task_id: self.gate.task_id.clone(),
            decision,
            role,
            revision: self.gate.subject_revision.clone(),
            evidence: None,
            reason: None,
        };
        self.advance(draft, false);
    }

    pub fn key(&mut self, key: KeyCode) -> Outcome {
        match self.mode.clone() {
            Mode::Reading => match key {
                KeyCode::Char('a') => self.start(ApprovalDecision::Approved),
                KeyCode::Char('r') => self.start(ApprovalDecision::Rejected),
                KeyCode::Char('c') => self.start(ApprovalDecision::ChangesRequested),
                KeyCode::Down | KeyCode::Char('j') => self.scroll = self.scroll.saturating_add(1),
                KeyCode::Up | KeyCode::Char('k') => self.scroll = self.scroll.saturating_sub(1),
                KeyCode::PageDown | KeyCode::Char(' ') => {
                    self.scroll = self.scroll.saturating_add(10)
                }
                KeyCode::PageUp => self.scroll = self.scroll.saturating_sub(10),
                KeyCode::Home => self.scroll = 0,
                KeyCode::Tab | KeyCode::Char('n') if self.waiting > 1 => return Outcome::Next,
                KeyCode::Esc | KeyCode::Char('l') | KeyCode::Char('q') => return Outcome::Later,
                _ => {}
            },
            Mode::Role { decision, selected } => {
                let roles = self.roles();
                match key {
                    KeyCode::Up | KeyCode::Char('k') => {
                        self.mode = Mode::Role {
                            decision,
                            selected: selected.saturating_sub(1),
                        }
                    }
                    KeyCode::Down | KeyCode::Char('j') => {
                        self.mode = Mode::Role {
                            decision,
                            selected: (selected + 1).min(roles.len().saturating_sub(1)),
                        }
                    }
                    KeyCode::Char(c) if c.is_ascii_digit() => {
                        let n = c.to_digit(10).unwrap_or(0) as usize;
                        if (1..=roles.len()).contains(&n) {
                            self.choose_role(decision, roles[n - 1].clone());
                        }
                    }
                    KeyCode::Enter => {
                        if let Some(role) = roles.get(selected) {
                            self.choose_role(decision, role.clone());
                        }
                    }
                    KeyCode::Esc => self.mode = Mode::Reading,
                    _ => {}
                }
            }
            Mode::Input {
                mut draft,
                field,
                mut buffer,
            } => match key {
                KeyCode::Esc => self.mode = Mode::Reading,
                KeyCode::Backspace => {
                    buffer.pop();
                    self.mode = Mode::Input {
                        draft,
                        field,
                        buffer,
                    };
                }
                KeyCode::Char(c) => {
                    buffer.push(c);
                    self.mode = Mode::Input {
                        draft,
                        field,
                        buffer,
                    };
                }
                KeyCode::Enter => {
                    let value = buffer.trim().to_string();
                    let optional = field == Field::Evidence && !self.gate.evidence_required;
                    if value.is_empty() && !optional {
                        self.message = Some((false, "A value is required.".into()));
                        self.mode = Mode::Input {
                            draft,
                            field,
                            buffer,
                        };
                        return Outcome::Continue;
                    }
                    self.message = None;
                    let value = (!value.is_empty()).then_some(value);
                    match field {
                        Field::Revision => draft.revision = value,
                        Field::Evidence => draft.evidence = value,
                        Field::Reason => draft.reason = value,
                    }
                    self.advance(draft, field == Field::Evidence);
                }
                _ => {
                    self.mode = Mode::Input {
                        draft,
                        field,
                        buffer,
                    }
                }
            },
            Mode::Confirm(draft) => match key {
                KeyCode::Char('y') | KeyCode::Enter => {
                    self.mode = Mode::Reading;
                    return Outcome::Submit(draft);
                }
                KeyCode::Char('n') | KeyCode::Esc => {
                    self.mode = Mode::Reading;
                    self.message = Some((false, "Not recorded.".into()));
                }
                _ => {}
            },
        }
        Outcome::Continue
    }

    fn body_lines(&self) -> Vec<Line<'static>> {
        let heading = Style::default()
            .fg(Color::Cyan)
            .add_modifier(Modifier::BOLD);
        let mut lines = Vec::new();
        for section in approval_brief(&self.gate) {
            let style = match section.title {
                "Question" => Style::default()
                    .fg(Color::Yellow)
                    .add_modifier(Modifier::BOLD),
                "Risks to weigh" => Style::default().fg(Color::Red),
                _ => Style::default(),
            };
            lines.push(Line::from(Span::styled(section.title, heading)));
            for line in section.lines {
                lines.push(Line::from(Span::styled(format!("  {line}"), style)));
            }
            lines.push(Line::from(""));
        }
        lines
    }

    fn footer_lines(&self) -> Vec<Line<'static>> {
        let key = Style::default()
            .fg(Color::Cyan)
            .add_modifier(Modifier::BOLD);
        let mut lines = Vec::new();
        if let Some((ok, message)) = &self.message {
            let color = if *ok { Color::Green } else { Color::Red };
            lines.push(Line::from(Span::styled(
                message.clone(),
                Style::default().fg(color),
            )));
        }
        match &self.mode {
            Mode::Reading => {
                let mut spans = vec![
                    Span::styled("a", key),
                    Span::raw(" Approve  "),
                    Span::styled("r", key),
                    Span::raw(" Reject  "),
                    Span::styled("c", key),
                    Span::raw(" Request changes  "),
                    Span::styled("↑↓/PgUp/PgDn", key),
                    Span::raw(" Scroll  "),
                ];
                if self.waiting > 1 {
                    spans.push(Span::styled("Tab", key));
                    spans.push(Span::raw(" Next gate  "));
                }
                spans.push(Span::styled("Esc", key));
                spans.push(Span::raw(" Later (A re-opens)"));
                lines.push(Line::from(spans));
            }
            Mode::Role { decision, selected } => {
                lines.push(Line::from(format!(
                    "{} {} as which role? (↑↓ + Enter, or its number; Esc back)",
                    verb(*decision),
                    self.gate.task_id
                )));
                for (index, role) in self.roles().iter().enumerate() {
                    let state = if self.gate.missing_roles.contains(role) {
                        "still needed"
                    } else {
                        "already approved"
                    };
                    let marker = if index == *selected { ">" } else { " " };
                    let style = if index == *selected {
                        Style::default().add_modifier(Modifier::REVERSED)
                    } else {
                        Style::default()
                    };
                    lines.push(Line::from(Span::styled(
                        format!("{marker} {}) {role}  ({state})", index + 1),
                        style,
                    )));
                }
            }
            Mode::Input {
                draft,
                field,
                buffer,
            } => {
                lines.push(Line::from(format!(
                    "{} {} as {} — {}:",
                    verb(draft.decision),
                    draft.task_id,
                    draft.role,
                    field.label(&self.gate)
                )));
                lines.push(Line::from(vec![
                    Span::styled("> ", key),
                    Span::raw(buffer.clone()),
                    Span::styled("█", Style::default().fg(Color::Gray)),
                ]));
                lines.push(Line::from("Enter Next  Esc Back"));
            }
            Mode::Confirm(draft) => {
                lines.push(Line::from(Span::styled(
                    format!(
                        "Record {} on {} as {}?",
                        draft.decision.as_str(),
                        draft.task_id,
                        draft.role
                    ),
                    Style::default()
                        .fg(Color::Yellow)
                        .add_modifier(Modifier::BOLD),
                )));
                let mut detail = Vec::new();
                if let Some(rev) = draft.revision.as_deref() {
                    detail.push(format!("revision {}", short_revision(rev)));
                }
                if let Some(e) = draft.evidence.as_deref() {
                    detail.push(format!("evidence {e}"));
                }
                if let Some(r) = draft.reason.as_deref() {
                    detail.push(format!("reason: {r}"));
                }
                lines.push(Line::from(detail.join(" | ")));
                lines.push(Line::from(vec![
                    Span::styled("y/Enter", key),
                    Span::raw(" Record  "),
                    Span::styled("n/Esc", key),
                    Span::raw(" Cancel"),
                ]));
            }
        }
        lines
    }

    pub fn draw(&self, frame: &mut Frame) {
        let area = crate::ui::centered_rect(90, 90, crate::ui::content_area(frame.size()));
        frame.render_widget(Clear, area);
        let title = format!(
            " APPROVAL REQUIRED — {}{}{} ",
            self.gate.task_id,
            self.gate
                .title
                .as_deref()
                .map(|t| format!(" · {t}"))
                .unwrap_or_default(),
            if self.waiting > 1 {
                format!(" ({} gates waiting)", self.waiting)
            } else {
                String::new()
            }
        );
        let block = Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(Color::Yellow))
            .title(Span::styled(
                title,
                Style::default()
                    .fg(Color::Yellow)
                    .add_modifier(Modifier::BOLD),
            ));
        let inner = block.inner(area);
        frame.render_widget(block, area);
        let footer = self.footer_lines();
        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Min(3),
                Constraint::Length(footer.len() as u16 + 1),
            ])
            .split(inner);
        frame.render_widget(
            Paragraph::new(self.body_lines())
                .wrap(Wrap { trim: false })
                .scroll((self.scroll, 0)),
            chunks[0],
        );
        frame.render_widget(
            Paragraph::new(footer)
                .block(Block::default().borders(Borders::TOP))
                .wrap(Wrap { trim: false }),
            chunks[1],
        );
    }
}

fn verb(decision: ApprovalDecision) -> &'static str {
    match decision {
        ApprovalDecision::Approved => "Approve",
        ApprovalDecision::Rejected => "Reject",
        ApprovalDecision::ChangesRequested => "Request changes on",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn gate(missing: &[&str], evidence_required: bool) -> PendingApproval {
        PendingApproval {
            task_id: "FIN-APP-001".into(),
            subject_task: "FIN-CR-001".into(),
            subject_revision: Some("abcdef1234567890".into()),
            question: "Do you approve FIN-CR-001?".into(),
            awaiting_decision: true,
            required_roles: vec!["FINANCE".into(), "LEAD_TECHNIQUE".into()],
            missing_roles: missing.iter().map(|r| r.to_string()).collect(),
            evidence_type: Some("pull_request_review".into()),
            evidence_required,
            ..PendingApproval::default()
        }
    }

    fn type_text(w: &mut Window, text: &str) {
        for c in text.chars() {
            assert!(matches!(w.key(KeyCode::Char(c)), Outcome::Continue));
        }
    }

    #[test]
    fn approve_walks_role_then_evidence_then_confirmation() {
        let mut w = Window::new(gate(&["FINANCE", "LEAD_TECHNIQUE"], true), 1);
        w.key(KeyCode::Char('a'));
        w.key(KeyCode::Down);
        w.key(KeyCode::Enter);
        assert!(w.is_editing());
        // Evidence is required: an empty Enter is refused.
        w.key(KeyCode::Enter);
        assert_eq!(w.message.as_ref().unwrap().1, "A value is required.");
        type_text(&mut w, "https://git/pr/7");
        w.key(KeyCode::Enter);
        match w.key(KeyCode::Char('y')) {
            Outcome::Submit(d) => {
                assert_eq!(d.decision, ApprovalDecision::Approved);
                assert_eq!(d.role, "LEAD_TECHNIQUE");
                assert_eq!(d.revision.as_deref(), Some("abcdef1234567890"));
                assert_eq!(d.evidence.as_deref(), Some("https://git/pr/7"));
            }
            _ => panic!("expected a submission"),
        }
        assert!(!w.is_editing());
    }

    #[test]
    fn optional_evidence_can_be_skipped_and_reject_needs_a_reason() {
        let mut w = Window::new(gate(&["FINANCE"], false), 1);
        w.key(KeyCode::Char('a'));
        w.key(KeyCode::Char('1'));
        w.key(KeyCode::Enter); // skip optional evidence
        assert!(matches!(w.key(KeyCode::Enter), Outcome::Submit(d) if d.evidence.is_none()));

        let mut w = Window::new(gate(&["FINANCE"], true), 1);
        w.key(KeyCode::Char('r'));
        w.key(KeyCode::Char('1'));
        type_text(&mut w, "missing ETag");
        w.key(KeyCode::Enter);
        match w.key(KeyCode::Enter) {
            Outcome::Submit(d) => {
                assert_eq!(d.decision, ApprovalDecision::Rejected);
                assert_eq!(d.reason.as_deref(), Some("missing ETag"));
                assert!(d.evidence.is_none());
            }
            _ => panic!("expected a submission"),
        }
    }

    #[test]
    fn escape_backs_out_and_later_closes() {
        let mut w = Window::new(gate(&["FINANCE"], true), 2);
        w.key(KeyCode::Char('c'));
        w.key(KeyCode::Esc);
        assert!(!w.is_editing());
        assert!(matches!(w.key(KeyCode::Tab), Outcome::Next));
        assert!(matches!(w.key(KeyCode::Esc), Outcome::Later));
    }

    #[test]
    fn declining_the_confirmation_records_nothing() {
        let mut w = Window::new(gate(&["FINANCE"], false), 1);
        w.key(KeyCode::Char('a'));
        w.key(KeyCode::Char('1'));
        w.key(KeyCode::Enter);
        assert!(matches!(w.key(KeyCode::Char('n')), Outcome::Continue));
        assert_eq!(w.message.as_ref().unwrap().1, "Not recorded.");
    }

    #[test]
    fn a_new_subject_revision_resets_the_form() {
        let mut w = Window::new(gate(&["FINANCE"], true), 1);
        w.key(KeyCode::Char('a'));
        w.key(KeyCode::Char('1'));
        let mut moved = gate(&["FINANCE"], true);
        moved.subject_revision = Some("1111111222222".into());
        w.refresh(moved, 1);
        assert!(!w.is_editing());
        assert!(w.message.as_ref().unwrap().1.contains("new revision"));
    }
}
