//! Task-oriented terminal presentation; action execution and policy validation stay in core.
use super::*;
use crate::workspace_ui as ui;
use ratatui::{
    layout::{Constraint, Layout},
    style::Style,
    widgets::{Paragraph, Wrap},
};
impl Panel {
    pub fn draw(&self, f: &mut Frame, area: Rect, outer_message: &str) {
        let tabs = [
            "Memory",
            "Context",
            "Compaction",
            "Code",
            "Settings",
            "Overview",
            "Trials",
        ];
        let tab_height = if area.width < 80 { 4 } else { 3 };
        let status_len = if !outer_message.is_empty() {
            outer_message.len()
        } else {
            self.message.len()
        };
        let status_height = if status_len > area.width.saturating_sub(2) as usize {
            4
        } else {
            3
        };
        let parts = Layout::default()
            .constraints([
                Constraint::Length(tab_height),
                Constraint::Length(2),
                Constraint::Min(3),
                Constraint::Length(status_height),
            ])
            .split(area);
        ui::tabs(f, parts[0], &tabs, self.tab);
        let summary = match &self.view {
            Some(v) => format!(
                "O2 {}  •  Storage: {}  •  Schema: {}\n{}",
                self.section_name(),
                if v.sqlite { "SQLite" } else { "JSON" },
                if v.ready { "ready" } else { "setup required" },
                if v.ready {
                    "F2 Actions opens named operations. Optional features live under 5 Settings."
                } else {
                    "Setup required. Review storage migration first; 5 Settings stays available."
                }
            ),
            None => "O2 state unavailable. Press r to retry; inspect the status message.".into(),
        };
        f.render_widget(
            Paragraph::new(summary)
                .wrap(Wrap { trim: false })
                .style(
                    Style::default().fg(if self.view.as_ref().is_some_and(|v| v.ready) {
                        crate::ui::theme().good
                    } else {
                        crate::ui::theme().warn
                    }),
                ),
            parts[1],
        );
        if let Some(form) = &self.form {
            let dialog = crate::ui::centered_rect(90, 85, f.size());
            f.render_widget(ratatui::widgets::Clear, dialog);
            self.draw_form(f, dialog, form);
        } else {
            let (list, details) = ui::columns(parts[2]);
            if self.tab == 4 {
                self.draw_policy_table(f, list);
            } else if !self.view.as_ref().is_some_and(|v| v.ready) {
                ui::details(
                    f,
                    list,
                    "Get started with shared memory",
                    self.setup_text(),
                    0,
                );
            } else if self.rows.is_empty() {
                let empty=match self.tab {
                    0=>"No memories in this scope.\n\nF2 → Propose a sourced memory, import proposals, or change the search scope. Only reviewed active facts can enter automatic context.",
                    1=>"No context manifests yet.\n\nRun a task with the context engine enabled in Settings. This section records the mandatory and optional context supplied to each invocation.",
                    2=>"No compaction observations yet.\n\nF2 → Inspect runtime decisions. Configure optional native settings under Settings → Compaction; capability claims require real adapter observations.",
                    3=>"No provider evidence yet.\n\nF2 → Prepare provider lifecycle action. The native provider ships with MACC; external providers are optional and require reviewed setup.",
                    5=>"No task evidence yet.\n\nRun a task to record sessions, checkpoints and context. Use F2 to filter tasks or open scoped memory and sessions.",
                    _=>"No policy trial selected.\n\nF2 → Pre-register a policy trial or inspect an existing trial. Trials never start model calls or change policies automatically.",
                };
                ui::details(f, list, self.section_name(), empty.into(), 0);
            } else {
                self.draw_records(f, list);
            }
            let text = if self.pending.is_some() {
                format!("REVIEW REQUIRED\n[a] Confirm  [Esc] Discard this operation\nChanging its inputs requires another preview.\n\n{}",ui::evidence(&self.details,self.raw_details))
            } else if !self.details.is_empty() {
                ui::evidence(&self.details, self.raw_details)
            } else {
                self.selection_details()
            };
            ui::details(
                f,
                details,
                "Details — PgUp / PgDn scroll; F4 raw evidence",
                text,
                self.scroll,
            );
        }
        let status = if !outer_message.is_empty() {
            outer_message
        } else if !self.message.is_empty() {
            &self.message
        } else {
            "F2 Actions  •  ? Help  •  Tab Sections  •  Enter Inspect  •  Esc Back"
        };
        f.render_widget(
            Paragraph::new(status)
                .wrap(Wrap { trim: false })
                .style(Style::default().fg(crate::ui::theme().accent_dim))
                .block(crate::ui::panel("Status & next action")),
            parts[3],
        );
        self.menu.draw(f, &self.commands());
    }
    pub(crate) fn draw_overlay(&self, f: &mut Frame) {
        if let Some(form) = &self.form {
            let dialog = crate::ui::centered_rect(90, 85, f.size());
            f.render_widget(ratatui::widgets::Clear, dialog);
            self.draw_form(f, dialog, form);
        }
        self.menu.draw(f, &self.commands());
    }
    fn setup_text(&self) -> String {
        if self.view.as_ref().is_some_and(|v| v.sqlite) {
            "Step 1 — Upgrade shared storage\nF2 → Prepare O2 schema upgrade. Inspect the backup and source hash, then confirm with a.\n\nStep 2 — Choose features\n5 Settings opens Memory, Context, Compaction and Code groups.\n\nFeatures remain disabled until explicitly saved.".into()
        } else {
            "Step 1 — Prepare SQLite storage\nJ opens Sessions. F2 → Prepare SQLite migration. Stop active runners, inspect the plan, then confirm with a.\n\nStep 2 — Return here with O\nF2 → Prepare O2 schema upgrade if needed.\n\nStep 3 — Choose features\n5 Settings controls optional features. No facts or settings are activated automatically.".into()
        }
    }
    fn draw_form(&self, f: &mut Frame, area: Rect, form: &forms::Form) {
        let index = form.values.len();
        let prompt = form
            .labels
            .get(index)
            .map(String::as_str)
            .unwrap_or("Review completed input");
        let input = if form.input.is_empty() {
            "Type your value here…".into()
        } else {
            form.input.clone()
        };
        let parts = Layout::default()
            .constraints([
                Constraint::Length(4),
                Constraint::Min(2),
                Constraint::Length(4),
            ])
            .split(area);
        ui::details(
            f,
            parts[0],
            &format!(
                "{} — step {} of {}",
                form.name.replace('_', " "),
                index + 1,
                form.labels.len()
            ),
            prompt.into(),
            0,
        );
        ui::details(f, parts[1], "Current value", input, 0);
        ui::details(f,parts[2],"Form controls","Enter next/prepare  •  Shift+Tab previous  •  Esc cancel\nWrites require a separate reviewed confirmation.".into(),0);
    }
    fn draw_policy_table(&self, f: &mut Frame, area: Rect) {
        let Some(view) = &self.view else { return };
        let group = view
            .fields
            .get(self.selected)
            .and_then(|v| v.path.split('.').next())
            .unwrap_or("memory");
        let group_name = match group {
            "memory" => "Memory",
            "context_engine" => "Context",
            "adaptive_compaction" => "Compaction",
            _ => "Code intelligence",
        };
        let fields: Vec<_> = self
            .setting_indices()
            .into_iter()
            .map(|i| (i, &view.fields[i]))
            .collect();
        let saved = serde_json::to_value(&view.policy).unwrap_or_default();
        let selected = fields.iter().position(|(i, _)| *i == self.selected);
        let rows = fields
            .iter()
            .map(|(_, field)| {
                let pointer = format!("/{}", field.path.replace('.', "/"));
                let value = self.draft.pointer(&pointer).unwrap_or(&field.default);
                vec![
                    field.label.clone(),
                    ui::value(value),
                    if saved.pointer(&pointer) != Some(value) {
                        "Edited".into()
                    } else {
                        "Saved".into()
                    },
                ]
            })
            .collect();
        ui::table(
            f,
            area,
            &format!("{group_name} settings — ← / → group, / find"),
            &["Setting", "Value", "Draft"],
            rows,
            selected,
        );
    }
    fn selection_details(&self) -> String {
        if self.tab == 4 {
            if let Some(field) = self.view.as_ref().and_then(|v| v.fields.get(self.selected)) {
                let pointer = format!("/{}", field.path.replace('.', "/"));
                let choices = if field.kind == "bool" {
                    "On / Off".into()
                } else if !field.choices.is_empty() {
                    field.choices.join(", ")
                } else {
                    format!(
                        "{}–{}",
                        field
                            .min
                            .map(|v| v.to_string())
                            .unwrap_or_else(|| "—".into()),
                        field
                            .max
                            .map(|v| v.to_string())
                            .unwrap_or_else(|| "—".into())
                    )
                };
                return format!("{}\n\nCurrent draft: {}\nDefault: {}\nAllowed: {}\nActivation: {}\n\nEnter: toggle, choose or edit\np: preview exact draft\ns: save after preview\nx: discard edits\n\nGroup: ← / →    Find: /\nCanonical key: {}",field.label,ui::value(self.draft.pointer(&pointer).unwrap_or(&field.default)),ui::value(&field.default),choices,field.activation,field.path);
            }
        }
        if let Some(row) = self.rows.get(self.selected) {
            return ui::evidence(&row.to_string(), self.raw_details);
        }
        match self.tab {
            0=>"Memory preserves sourced facts across tasks.\n\nProposal → inspect evidence → review → explicit confirmation.\n\nUse F2 to browse actions by their full names.".into(),
            1=>"Context manifests show what was supplied to an invocation. Mandatory PRD/criteria remain separate from optional memory and estimates.".into(),
            2=>"Compaction controls conversation pressure and checkpoint rotation. Unsupported native capabilities remain explicitly unverified.".into(),
            3=>"Code intelligence uses registered provider adapters with native fallback. Installation and indexing require a reviewed plan.".into(),
            5=>"Overview joins observed task evidence. It never marks a task delivered.\n\nF2: inspect scoped memory, context or sessions.".into(),
            _=>format!("Selected safe trial field: {}\n\n← / → choose field. F2 lists trial registration, quality review, evaluation and adoption steps. No policy changes automatically.",macc_core::o2::tuning::FIELDS[self.tuning_field]),
        }
    }
    fn draw_records(&self, f: &mut Frame, area: Rect) {
        let (headers, rows): (&[&str], Vec<Vec<String>>) = match self.tab {
            0 if self.conflicts => (
                &["Conflict", "Status", "Reason"],
                self.rows
                    .iter()
                    .map(|r| {
                        vec![
                            ui::value(&r["id"]),
                            ui::value(&r["status"]),
                            ui::value(&r["reason"]),
                        ]
                    })
                    .collect(),
            ),
            0 => (
                &["Memory", "Status", "Scope"],
                self.rows
                    .iter()
                    .map(|r| {
                        vec![
                            ui::value(&r["memory"]["proposal"]["title"]),
                            ui::value(&r["memory"]["status"]),
                            ui::value(&r["memory"]["proposal"]["scope"]),
                        ]
                    })
                    .collect(),
            ),
            1 => (
                &["Task", "Phase", "Optional tokens"],
                self.rows
                    .iter()
                    .map(|r| {
                        vec![
                            ui::value(&r["task_id"]),
                            ui::value(&r["phase"]),
                            ui::value(&r["optional_estimated_tokens"]),
                        ]
                    })
                    .collect(),
            ),
            2 => (
                &["Tool", "State", "Native auto"],
                self.rows
                    .iter()
                    .map(|r| {
                        vec![
                            ui::value(&r["tool_id"]),
                            ui::value(&r["state"]),
                            ui::value(&r["native_auto"]),
                        ]
                    })
                    .collect(),
            ),
            3 => (
                &["Provider", "Status", "Version"],
                self.rows
                    .iter()
                    .map(|r| {
                        vec![
                            ui::value(&r["provider"]["id"]),
                            ui::value(&r["status"]),
                            ui::value(&r["provider"]["version"]),
                        ]
                    })
                    .collect(),
            ),
            5 => (
                &["Task", "Sessions", "Checkpoints", "Proposals"],
                self.rows
                    .iter()
                    .map(|r| {
                        vec![
                            ui::value(&r["task_id"]),
                            ui::value(&r["sessions"]),
                            ui::value(&r["checkpoints"]),
                            ui::value(&r["proposals"]),
                        ]
                    })
                    .collect(),
            ),
            _ => (
                &["Trial", "State"],
                self.rows
                    .iter()
                    .map(|r| vec![ui::value(&r["id"]), ui::value(&r["state"])])
                    .collect(),
            ),
        };
        ui::table(
            f,
            area,
            self.section_name(),
            headers,
            rows,
            Some(self.selected),
        );
    }
}
