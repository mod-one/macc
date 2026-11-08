//! Response-dependent requirements and stable keyboard focus.
use super::{interaction::Control, *};
impl Center {
    pub(super) fn response_mode(&self) -> dm::InterventionResponse {
        let Some(d) = self.current.as_ref() else {
            return dm::InterventionResponse::Completed;
        };
        let draft = self.drafts.get(&Self::key(d)).cloned().unwrap_or_default();
        dm::intervention_response(
            &d.request,
            draft
                .option
                .checked_sub(1)
                .and_then(|i| d.request.options.get(i))
                .map(|o| o.id.as_str()),
            &draft.fields,
        )
        .unwrap_or(dm::InterventionResponse::Completed)
    }
    pub(super) fn completed_intervention(&self) -> bool {
        self.current
            .as_ref()
            .is_some_and(|d| d.request.kind == "intervention")
            && self.response_mode() == dm::InterventionResponse::Completed
    }
    pub(super) fn set_response_mode(&mut self, mode: dm::InterventionResponse) {
        if let Some(d) = &self.current {
            let draft = self.drafts.entry(Self::key(d)).or_default();
            // This is an explicit operator response, never a guess from an option label.
            draft
                .fields
                .insert(dm::RESPONSE_MODE.into(), mode.value().into());
            if draft
                .option
                .checked_sub(1)
                .and_then(|i| d.request.options.get(i))
                .is_some_and(|o| o.intervention_response.is_none_or(|v| v != mode))
            {
                draft.option = 0;
            }
            draft.fields.remove("performed");
            draft.fields.remove("result");
            draft.apply_patch = false;
            draft.outcome = if mode == dm::InterventionResponse::Unable {
                dm::AnswerOutcome::Reject
            } else {
                dm::AnswerOutcome::Answer
            };
        }
    }
    pub(super) fn next_control(&mut self, previous: Control) {
        let controls = self.controls();
        let next = controls
            .iter()
            .position(|c| *c == previous)
            .map(|i| (i + 1) % controls.len())
            .unwrap_or(0);
        self.focus_control(controls[next]);
    }
    pub(super) fn field_required(&self, field: &dm::ResponseField) -> bool {
        if field.id == "__attestation" {
            return true;
        }
        let Some(d) = self.current.as_ref() else {
            return field.required;
        };
        let draft = self.drafts.get(&Self::key(d)).cloned().unwrap_or_default();
        field.required
            && draft.outcome == dm::AnswerOutcome::Answer
            && !(d.request.kind == "intervention"
                && (self.response_mode() == dm::InterventionResponse::DeferObservation
                    || self.response_mode() == dm::InterventionResponse::ConfirmFact
                        && matches!(field.id.as_str(), "performed" | "result")))
    }
    pub(super) fn field_visible(&self, field: &dm::ResponseField) -> bool {
        if self
            .current
            .as_ref()
            .is_some_and(|d| d.request.kind == "intervention")
        {
            if self.response_mode() == dm::InterventionResponse::DeferObservation
                && field.id != "__attestation"
            {
                return self.advanced_fields;
            }
            if !self.completed_intervention() && matches!(field.id.as_str(), "performed" | "result")
            {
                return false;
            }
        }
        true
    }
    pub(super) fn keyboard_hint(&self, focus: Control) -> &'static str {
        match focus {
            Control::Send => "[Enter] Send response  [Shift+Tab] Edit  [Esc] Later",
            Control::Option | Control::Mode => {
                "[↑↓/number] Choose  [Enter/Tab] Continue  [Esc] Later"
            }
            Control::Field(i)
                if self
                    .form_fields()
                    .get(i)
                    .is_some_and(|f| !f.choices.is_empty()) =>
            {
                "[↑↓/number] Choose  [Enter/Tab] Continue  [Esc] Later"
            }
            Control::Note => "[Enter] New line  [Tab] Continue  [Esc] Later (draft kept)",
            Control::Field(i) if self.form_fields().get(i).is_some_and(|f| f.multiline) => {
                "[Enter] New line  [Tab] Continue  [Esc] Later (draft kept)"
            }
            Control::Field(_) => "[Enter/Tab] Continue  [Esc] Later (draft kept)",
            _ => "[Tab/Shift+Tab] Navigate  [Enter] Activate  [Esc] Later",
        }
    }
    pub(super) fn response_overview(&self) -> String {
        let Some(d) = self
            .current
            .as_ref()
            .filter(|d| d.status == "pending_human")
        else {
            return String::new();
        };
        let draft = self.drafts.get(&Self::key(d)).cloned().unwrap_or_default();
        let mut lines = vec![];
        if let Some(o) = draft
            .option
            .checked_sub(1)
            .and_then(|i| d.request.options.get(i))
        {
            lines.push(format!("Selected: {}", o.label));
        }
        if d.request.kind == "intervention" {
            lines.push(format!("Response type: {}", self.response_mode().label()));
        }
        for f in self
            .form_fields()
            .iter()
            .filter(|f| self.field_visible(f) && self.field_required(f))
        {
            let v = draft
                .fields
                .get(&f.id)
                .map(|v| v.trim())
                .unwrap_or_default();
            lines.push(format!(
                "{}: {}",
                f.label,
                if v.is_empty() {
                    "required".into()
                } else {
                    v.chars().take(65).collect::<String>().replace('\n', " ")
                }
            ));
        }
        format!("\n{}\n", lines.join("\n"))
    }
    pub(super) fn delivery_overview(&self) -> String {
        let Some(d) = self.current.as_ref().filter(|d| d.resolution.is_some()) else {
            return String::new();
        };
        let mut lines = vec!["Response recorded (operator statement).".to_string()];
        let rows = self
            .snapshot
            .as_ref()
            .and_then(|s| s["delivery"].as_array());
        let mut found = false;
        for row in rows.into_iter().flatten().filter(|r| {
            r["decisions"].as_array().is_some_and(|ds| {
                ds.iter()
                    .any(|b| b["id"] == d.id && b["version"] == d.version)
            })
        }) {
            found = true;
            lines.push(format!(
                "{} / {}: delivered={} · acknowledged={} · independent check={}{}",
                row["task"].as_str().unwrap_or("?"),
                row["phase"].as_str().unwrap_or("?"),
                row["delivered"] == true,
                row["acknowledged"] == true,
                row["compliance"][&d.id]["status"]
                    .as_str()
                    .unwrap_or("pending"),
                if row["current"] == true {
                    ""
                } else {
                    " (context must be refreshed)"
                }
            ));
            if let Some(e) = row["compliance"][&d.id]["evidence"].as_str() {
                lines.push(format!("Independent evidence: {e}"));
            }
        }
        if !found {
            lines.push(
                "Delivery pending · acknowledgement pending · independent check pending".into(),
            );
        }
        format!("\n{}\n", lines.join("\n"))
    }
    /// Prefer the exact failed native binding for this task over unrelated history.
    pub fn show_for_task(&mut self, task: Option<&str>) {
        self.show();
        if let Some(task) = task {
            let rows = self
                .snapshot
                .as_ref()
                .and_then(|s| s["delivery"].as_array());
            let failed = self.list().into_iter().find(|d| {
                rows.into_iter().flatten().any(|r| {
                    r["task"] == task
                        && r["compliance"][&d.id]["status"]
                            .as_str()
                            .is_some_and(|s| s != "pass")
                        && r["decisions"].as_array().is_some_and(|ds| {
                            ds.iter()
                                .any(|b| b["id"] == d.id && b["version"] == d.version)
                        })
                })
            });
            self.current = failed
                .or_else(|| self.list().into_iter().find(|d| d.request.task_id == task))
                .or_else(|| self.current.clone());
        }
    }
}
