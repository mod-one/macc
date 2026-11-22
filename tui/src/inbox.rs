//! Inspect the actual request before opening its governed response form.
use super::*;
use macc_core::coordinator::decisions::Decision;
#[derive(Copy, Clone, Default, PartialEq, Eq, Debug)]
pub(crate) enum Section {
    #[default]
    Decisions,
    Approvals,
    History,
}
impl Section {
    pub fn next(self, reverse: bool) -> Self {
        let all = [Self::Decisions, Self::Approvals, Self::History];
        all[(self as usize + if reverse { 2 } else { 1 }) % all.len()]
    }
}
fn decisions(state: &AppState) -> Vec<Decision> {
    state
        .decision_center
        .inbox_rows(state.inbox_section == Section::History)
}
pub(super) fn supervisor_decision_index(s: &AppState) -> Option<usize> {
    let tasks = &s
        .observations
        .latest
        .as_ref()?
        .intervention
        .as_ref()?
        .task_ids;
    s.decision_center.pending_task_index(tasks)
}
pub(super) fn count(s: &AppState) -> usize {
    match s.inbox_section {
        Section::Approvals => s.awaiting_approvals().count(),
        _ => decisions(s).len(),
    }
}
pub(super) fn open(s: &mut AppState) {
    if s.inbox_section == Section::Approvals {
        s.open_approval_at(s.inspector_index);
    } else if let Some(decision) = decisions(s).get(s.inspector_index) {
        s.decision_center.show_exact(decision.clone());
    } else {
        s.decision_center.show();
    }
}
pub(super) fn draw(f: &mut Frame, s: &AppState, area: Rect) {
    let layout = Layout::default()
        .constraints([Constraint::Length(1), Constraint::Min(1)])
        .split(area);
    let approvals = format!("Approvals ({})", s.awaiting_approvals().count());
    workspace_ui::tab_line(
        f,
        layout[0],
        &["Decisions", &approvals, "History"],
        s.inbox_section as usize,
        theme().accent,
    );
    let (list, inspector) = workspace_ui::columns(layout[1]);
    let index = s.inspector_index;
    let (rows, description) = if s.inbox_section == Section::Approvals {
        let gates: Vec<_> = s.awaiting_approvals().collect();
        let detail = gates.get(index).map(|g| {
            if s.inbox_raw {
                serde_json::to_string_pretty(g).unwrap_or_default()
            } else {
                format!(
                    "{}\nRevision: {}\n\n{}\n\nMissing roles: {}\nEvidence: {}\nQuorum: {}\n{}",
                    g.subject_task,
                    g.subject_revision.as_deref().unwrap_or("Unknown"),
                    g.description
                        .as_deref()
                        .unwrap_or("Review the requested change."),
                    g.missing_roles.join(", "),
                    if g.evidence_required {
                        g.evidence_type.as_deref().unwrap_or("Required")
                    } else {
                        "Optional"
                    },
                    g.quorum,
                    g.note.as_deref().unwrap_or("")
                )
            }
        });
        (
            gates
                .iter()
                .map(|g| vec![g.task_id.clone(), g.status.clone()])
                .collect::<Vec<_>>(),
            detail,
        )
    } else {
        let decisions = decisions(s);
        let detail = decisions.get(index).map(|d| {
            if s.inbox_raw {
                serde_json::to_string_pretty(d).unwrap_or_default()
            } else {
                format!(
                    "{}\n\n{}\n\n{}\n\nDecision {} · revision {}",
                    d.request.question, d.request.context, d.status, d.id, d.version
                )
            }
        });
        (
            decisions
                .iter()
                .map(|d| vec![d.request.task_id.clone(), d.status.clone()])
                .collect(),
            detail,
        )
    };
    if rows.is_empty() {
        let empty = match s.inbox_section {
            Section::Decisions => "No decisions awaiting a response.",
            Section::Approvals => "No approval gates awaiting a decision.",
            Section::History => "No recorded decisions.",
        };
        f.render_widget(wrapped_paragraph(empty, "Inbox"), list);
    } else {
        workspace_ui::table(
            f,
            list,
            "Inbox",
            &["Task", "Status"],
            rows,
            Some(index),
            crate::navigation::Mode::Operate,
        );
    }
    let detail = description.unwrap_or_else(|| {
        s.coordinator_run_summary
            .as_ref()
            .map(|r| format!("{}\n\n{}\n\n{}", r.headline, r.cause, r.next_action))
            .unwrap_or_else(|| {
                "Requests appear here when a performer or the supervisor needs your input.".into()
            })
    });
    workspace_ui::details(
        f,
        inspector,
        if s.inbox_raw { "Evidence" } else { "Inspector" },
        detail,
        s.inspector_scroll,
    );
}
