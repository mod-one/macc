//! Session settings and conversation evidence have separate, responsive panes.
use super::*;
use crate::workspace_ui as ui;
use ratatui::{
    layout::{Constraint, Layout},
    style::Style,
    widgets::{Paragraph, Wrap},
};
impl SessionsScreen {
    pub(super) fn draw_workspace(&self, f: &mut Frame, area: Rect) {
        let parts = Layout::default()
            .constraints([
                Constraint::Length(3),
                Constraint::Length(3),
                Constraint::Min(3),
                Constraint::Length(3),
            ])
            .split(area);
        ui::tabs(f, parts[0], &["Settings", "Conversations"], self.tab);
        let summary = match &self.view {
            Some(v) => format!(
                "Storage: {}  •  {} conversations  •  Settings: {}{}\n{}",
                v.backend.to_uppercase(),
                v.sessions.len(),
                v.policy_source,
                if self.inherit
                    || serde_json::to_value(&v.policy).ok().as_ref() != Some(&self.draft)
                {
                    " (unsaved draft)"
                } else {
                    ""
                },
                if v.backend == "sqlite" {
                    "Changes take effect on the next session claim."
                } else {
                    "Shared O2 data needs SQLite. F2 → Prepare SQLite migration; inspect the plan before confirming."
                }
            ),
            None => "Session state unavailable. Press r to retry; see the status below.".into(),
        };
        f.render_widget(Paragraph::new(summary).wrap(Wrap { trim: false }), parts[1]);
        let (list, detail) = ui::columns(parts[2]);
        if let Some(v) = &self.view {
            if self.tab == 0 {
                let rows = v
                    .fields
                    .iter()
                    .map(|field| {
                        vec![
                            field.label.clone(),
                            format!(
                                "{} {}",
                                ui::value(
                                    self.draft
                                        .pointer(&format!("/{}", field.path.replace('.', "/")))
                                        .unwrap_or(&Value::Null)
                                ),
                                field.unit
                            ),
                        ]
                    })
                    .collect();
                ui::table(
                    f,
                    list,
                    "Session settings",
                    &["Setting", "Value"],
                    rows,
                    Some(self.selected),
                );
            } else if v.sessions.is_empty() {
                ui::details(f,list,"Conversations","No recorded conversations yet.\n\nRun a coordinator task to record a session. Existing conversations appear here with their task, role and lifecycle status.\n\nF2 Actions opens usage, checkpoints and storage administration.".into(),0);
            } else {
                let rows = v
                    .sessions
                    .iter()
                    .map(|s| {
                        vec![
                            s.task_id.clone(),
                            s.role.clone(),
                            s.tool_id.clone(),
                            s.status.clone(),
                            s.use_count.to_string(),
                        ]
                    })
                    .collect();
                ui::table(
                    f,
                    list,
                    "Recorded conversations",
                    &["Task", "Role", "Tool", "Status", "Uses"],
                    rows,
                    self.selected.checked_sub(v.fields.len()),
                );
            }
        }
        let mut details = if self.details.is_empty() {
            self.selection_details()
        } else {
            ui::evidence(&self.details, self.raw_details)
        };
        if let Some(plan) = &self.plan {
            details=format!("REVIEW REQUIRED — storage change\nStop coordinator, supervisor and runners first.\n[a] Apply this exact plan  [x] Cancel\n\n{}",ui::evidence(&serde_json::to_string_pretty(plan).unwrap_or_default(),self.raw_details));
        } else if let Some(id) = &self.closing {
            details=format!("REVIEW REQUIRED — close conversation\nSession: {id}\n\n[a] Confirm closure  [x] Cancel\nA session with an active lease cannot close.");
        }
        ui::details(
            f,
            detail,
            if self.editing.is_some() {
                "Edit value — Enter saves draft; Esc cancels"
            } else {
                "Details — PgUp / PgDn scroll; F4 raw evidence"
            },
            details,
            self.scroll,
        );
        let status = if self.message.is_empty() {
            "Select a setting or conversation to inspect its details."
        } else {
            &self.message
        };
        f.render_widget(
            Paragraph::new(status)
                .wrap(Wrap { trim: false })
                .style(Style::default().fg(crate::ui::theme().accent_dim))
                .block(crate::ui::panel("Status & next action")),
            parts[3],
        );
    }
    fn selection_details(&self) -> String {
        let Some(v) = &self.view else {
            return "No session snapshot available.".into();
        };
        if let Some(field) = v.fields.get(self.selected).filter(|_| self.tab == 0) {
            let current = self.editing.clone().unwrap_or_else(|| {
                ui::value(
                    self.draft
                        .pointer(&format!("/{}", field.path.replace('.', "/")))
                        .unwrap_or(&Value::Null),
                )
            });
            format!("{}\n\n{}\n\nValue: {} {}\nAllowed range: {}–{} {}\n\nEnter: edit value\np: validate and preview\ns: save settings\n\nCanonical key: {}\nSaved revision: {}",field.label,field.help,current,field.unit,field.min,field.max,field.unit,field.path,if v.revision==self.draft_revision {"current"}else{"changed externally; discard/review before saving"})
        } else if let Some(s) = self
            .selected_id()
            .and_then(|id| v.sessions.iter().find(|s| s.id == id))
        {
            format!("Task: {}\nRole: {}\nTool: {}\nStatus: {}\nGeneration: {}\nUses: {}\nOwner process: {}\n\nSession ID: {}\n\nEnter: inspect events\nF2: usage, checkpoints or prepare closure",s.task_id,s.role,s.tool_id,s.status,s.generation,s.use_count,s.owner_pid.map(|p|p.to_string()).unwrap_or_else(||"none".into()),s.id)
        } else {
            "Select a setting or conversation to inspect its purpose and available actions.".into()
        }
    }
}
