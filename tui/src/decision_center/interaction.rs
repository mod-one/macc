//! A keyboard form: Tab moves focus; Enter only sends on the Send button.
use super::*;

#[derive(Clone, Copy, PartialEq)]
pub(super) enum Control {
    Note,
    Option,
    Mode,
    Field(usize),
    Send,
    Later,
    Details,
    More,
}
pub(super) const MORE: &[&str] = &[
    "Return to response",
    "Answer / confirm",
    "I cannot do this / reject",
    "Ask the performer to investigate",
    "Request changes",
    "Evidence, approvals and lifecycle actions",
    "Next request / history",
    "Add a project direction",
    "Inspect task contexts and receipts",
    "Edit approval conditions / advanced fields",
    "Accept / clear the displayed task-plan patch",
    "Toggle advisory guidance",
    "Prepare independent approver challenge",
    "Confirm a fact (operator statement)",
    "Explicitly defer observation; keep it unverified",
];
impl Center {
    pub(super) fn selected_patch(&self) -> Option<&dm::Patch> {
        let d = self.current.as_ref()?;
        let selected = self
            .drafts
            .get(&Self::key(d))
            .and_then(|draft| draft.option.checked_sub(1))
            .and_then(|i| d.request.options.get(i))
            .map(|o| o.id.as_str());
        d.request.patch_for(selected)
    }
    pub(super) fn form_fields(&self) -> Vec<dm::ResponseField> {
        let Some(d) = &self.current else {
            return vec![];
        };
        let mut fields = d.request.response_fields.clone();
        for field in interaction_fields(&d.request.kind) {
            if !fields.iter().any(|f| f.id == field.id) {
                fields.push(field);
            }
        }
        if matches!(d.request.kind.as_str(), "approval" | "authorization")
            && !fields.iter().any(|f| f.id == "evidence")
        {
            fields.push(dm::ResponseField {
                id: "evidence".into(),
                label: "Approval evidence".into(),
                required: true,
                multiline: true,
                choices: vec![],
            });
        }
        if self.policy().trust_mode == "secure_approval"
            && self
                .snapshot
                .as_ref()
                .is_none_or(|s| s["approval_broker_available"] != true)
        {
            fields.push(dm::ResponseField {
                id: "__attestation".into(),
                label: "Independent approver attestation JSON".into(),
                required: true,
                multiline: true,
                choices: vec![],
            });
        }
        fields
    }
    pub(super) fn controls(&self) -> Vec<Control> {
        let mut controls = Vec::new();
        if self.feedback {
            controls.push(Control::Note);
        } else if let Some(d) = &self.current {
            if d.status == "pending_human"
                && self.policy().human_question_min_risk != dm::Consultation::NeverAsk
            {
                let fields = self.form_fields();
                let draft = self.drafts.get(&Self::key(d)).cloned().unwrap_or_default();
                let completed_intervention =
                    d.request.kind == "intervention" && draft.outcome == dm::AnswerOutcome::Answer;
                let choice_fields = d.request.response_fields.iter().any(|f| {
                    !f.choices.is_empty()
                        && (self.field_required(f)
                            || self.advanced_fields
                            || draft.fields.contains_key(&f.id))
                        && self.field_visible(f)
                }) && !matches!(
                    d.request.kind.as_str(),
                    "intervention" | "approval" | "authorization"
                );
                if !d.request.options.is_empty() {
                    controls.push(Control::Option);
                }
                if d.request.kind == "intervention"
                    && !d.request.options.is_empty()
                    && (self.advanced_fields
                        || draft
                            .option
                            .checked_sub(1)
                            .and_then(|i| d.request.options.get(i))
                            .is_none_or(|o| o.intervention_response.is_none()))
                {
                    controls.push(Control::Mode);
                }
                if completed_intervention && self.completed_intervention() {
                    for id in ["performed", "result"] {
                        if let Some(i) = fields.iter().position(|f| f.id == id) {
                            controls.push(Control::Field(i));
                        }
                    }
                }
                if !completed_intervention
                    && ((!choice_fields && d.request.options.is_empty())
                        || !d.request.options.is_empty() && draft.option == 0
                        || self.advanced_fields
                        || !draft.text.is_empty())
                {
                    controls.push(Control::Note);
                }
                controls.extend(
                    fields
                        .iter()
                        .enumerate()
                        .filter(|(_, f)| {
                            (self.field_required(f)
                                || self.advanced_fields
                                || draft.fields.contains_key(&f.id))
                                && self.field_visible(f)
                                && (self.advanced_fields || !f.id.starts_with("__condition_"))
                                && !(d.request.kind == "intervention"
                                    && matches!(f.id.as_str(), "performed" | "result"))
                        })
                        .map(|(i, _)| Control::Field(i)),
                );
                if completed_intervention && (self.advanced_fields || !draft.text.is_empty()) {
                    controls.push(Control::Note);
                }
            }
        }
        if self.feedback
            || controls
                .iter()
                .any(|c| matches!(c, Control::Note | Control::Field(_) | Control::Option))
        {
            controls.push(Control::Send);
        }
        controls.extend([Control::Later, Control::Details, Control::More]);
        controls
    }
    pub(super) fn focus_control(&mut self, target: Control) {
        self.scroll = 0;
        self.focus = self
            .controls()
            .iter()
            .position(|c| *c == target)
            .unwrap_or(0);
    }
    fn attempt_send(&mut self, challenge: bool) -> Option<Action> {
        self.notice_success = false;
        if self.submitting {
            self.message =
                "Please wait for the current operation to finish. Your draft is kept.".into();
            return None;
        }
        if self.feedback {
            if challenge {
                self.message =
                    "Prepare challenges for an existing request; Send records a project direction."
                        .into();
                return None;
            }
            return self.legacy_key(KeyCode::F(5));
        }
        let current = self.current.as_ref()?;
        if current.status != "pending_human"
            || self.policy().human_question_min_risk == dm::Consultation::NeverAsk
        {
            return None;
        }
        if self
            .list()
            .iter()
            .find(|d| d.id == current.id)
            .is_some_and(|fresh| fresh.version != current.version || fresh.status != current.status)
        {
            self.message = "This request changed. More > Next request / history reviews the current version. Your draft is kept.".into();
            return None;
        }
        let key = Self::key(current);
        let draft = self.drafts.get(&key).cloned().unwrap_or_default();
        let fields = self.form_fields();
        let order = self
            .controls()
            .iter()
            .filter_map(|control| {
                if let Control::Field(i) = control {
                    Some(*i)
                } else {
                    None
                }
            })
            .collect::<Vec<_>>();
        for i in order {
            let field = &fields[i];
            if challenge && field.id == "__attestation" {
                continue;
            }
            let value = draft
                .fields
                .get(&field.id)
                .map(String::as_str)
                .unwrap_or("");
            let required = self.field_required(field);
            if required && value.trim().is_empty()
                || !value.is_empty()
                    && !field.choices.is_empty()
                    && !field.choices.iter().any(|v| v == value)
            {
                self.message = format!(
                    "Complete \"{}\" before sending. Your response is kept.",
                    field.label
                );
                self.focus_control(Control::Field(i));
                return None;
            }
        }
        if draft.outcome == dm::AnswerOutcome::Answer {
            if current.request.kind == "intervention"
                && self.completed_intervention()
                && (draft.fields.get("performed").map(String::as_str) != Some("yes")
                    || draft.advisory)
            {
                self.message = "Confirm that you performed the action, or choose 'I cannot do this' in More. No action is inferred from your text.".into();
                if let Some(i) = self.form_fields().iter().position(|f| f.id == "performed") {
                    self.focus_control(Control::Field(i));
                }
                return None;
            }
        }
        let has_text = !response_text(&current.request, &draft).trim().is_empty();
        if !has_text {
            self.message =
                "Write an answer or choose an option before sending. Your draft is kept.".into();
            self.focus_control(Control::Note);
            return None;
        }
        self.legacy_key(if challenge {
            KeyCode::F(12)
        } else {
            KeyCode::F(5)
        })
    }
    pub fn key_event(&mut self, key: KeyCode) -> Option<Action> {
        self.notice_success = false;
        // Old function-key codes remain internal command IDs and compatible aliases;
        // every command has a visible, ordinary-key route through the form/menu.
        if matches!(key, KeyCode::F(_)) {
            if key == KeyCode::F(5) && !self.actions.open {
                return self.attempt_send(false);
            }
            let result = self.legacy_key(key);
            if key == KeyCode::F(9) {
                let index = self
                    .current
                    .as_ref()
                    .and_then(|d| self.drafts.get(&Self::key(d)))
                    .map(|d| d.active_field)
                    .unwrap_or(0);
                self.advanced_fields = true;
                self.focus_control(if index == 0 {
                    Control::Note
                } else {
                    Control::Field(index - 1)
                });
            }
            return result;
        }
        if self.reviewing_patch {
            match key {
                KeyCode::Esc => self.reviewing_patch = false,
                KeyCode::Tab | KeyCode::BackTab | KeyCode::Left | KeyCode::Right => {
                    self.patch_accept = !self.patch_accept
                }
                KeyCode::Enter => {
                    self.reviewing_patch = false;
                    if self.patch_accept {
                        return self.legacy_key(KeyCode::F(10));
                    }
                }
                KeyCode::PageUp => self.scroll = self.scroll.saturating_sub(8),
                KeyCode::PageDown => self.scroll = self.scroll.saturating_add(8),
                _ => {}
            }
            return None;
        }
        if self.quick_menu {
            match key {
                KeyCode::Esc => self.quick_menu = false,
                KeyCode::Up | KeyCode::BackTab => {
                    self.menu_index = (self.menu_index + MORE.len() - 1) % MORE.len()
                }
                KeyCode::Down | KeyCode::Tab => {
                    self.menu_index = (self.menu_index + 1) % MORE.len()
                }
                KeyCode::Enter => {
                    self.quick_menu = false;
                    match self.menu_index {
                        1..=4 => {
                            if let Some(d) = &self.current {
                                let draft = self.drafts.entry(Self::key(d)).or_default();
                                draft.outcome = match self.menu_index {
                                    2 => dm::AnswerOutcome::Reject,
                                    3 => dm::AnswerOutcome::Investigate,
                                    4 => dm::AnswerOutcome::RequestChanges,
                                    _ => dm::AnswerOutcome::Answer,
                                };
                                draft.apply_patch = false;
                                draft.fields.retain(|k, _| !k.starts_with("__condition_"));
                                self.focus_control(Control::Note);
                            }
                        }
                        5 => {
                            self.actions.show();
                            self.manual = true;
                            self.scroll = 0;
                        }
                        6 => {
                            self.focus = 0;
                            return self.legacy_key(KeyCode::F(2));
                        }
                        7 => {
                            self.focus = 0;
                            return self.legacy_key(KeyCode::F(8));
                        }
                        8 => {
                            self.expanded = true;
                            return self.legacy_key(KeyCode::F(4));
                        }
                        9 => {
                            self.advanced_fields = !self.advanced_fields;
                            self.focus = 0;
                        }
                        10 => {
                            if self.selected_patch().is_some() {
                                self.reviewing_patch = true;
                                self.patch_accept = false;
                                self.scroll = 0;
                            } else {
                                self.message =
                                    "This choice has no task-plan patch to accept.".into();
                            }
                        }
                        11 => {
                            return self.legacy_key(KeyCode::F(3));
                        }
                        12 => return self.attempt_send(true),
                        13 | 14 => {
                            self.set_response_mode(if self.menu_index == 13 {
                                dm::InterventionResponse::ConfirmFact
                            } else {
                                dm::InterventionResponse::DeferObservation
                            });
                            self.focus_control(Control::Send);
                        }
                        _ => {}
                    }
                    self.save_drafts();
                }
                _ => {}
            }
            return None;
        }
        if self.actions.open
            || matches!(
                key,
                KeyCode::Esc | KeyCode::Home | KeyCode::PageUp | KeyCode::PageDown
            )
        {
            return self.legacy_key(key);
        }
        let controls = self.controls();
        self.focus %= controls.len();
        if matches!(key, KeyCode::Tab | KeyCode::BackTab) {
            self.scroll = 0;
            self.focus = (self.focus
                + if key == KeyCode::Tab {
                    1
                } else {
                    controls.len() - 1
                })
                % controls.len();
            self.save_drafts();
            return None;
        }
        if matches!(
            controls[self.focus],
            Control::Note | Control::Field(_) | Control::Option
        ) && matches!(
            key,
            KeyCode::Char(_) | KeyCode::Enter | KeyCode::Backspace | KeyCode::Delete
        ) {
            self.message.clear();
        }
        match controls[self.focus] {
            Control::Send if matches!(key, KeyCode::Enter | KeyCode::Char(' ')) => {
                return self.attempt_send(false)
            }
            Control::Later if key == KeyCode::Enter => return self.legacy_key(KeyCode::Esc),
            Control::Details if key == KeyCode::Enter => {
                self.expanded = !self.expanded;
                self.scroll = 0;
            }
            Control::More if key == KeyCode::Enter => {
                self.quick_menu = true;
                self.menu_index = 0;
            }
            Control::Mode => {
                let modes = [
                    dm::InterventionResponse::Completed,
                    dm::InterventionResponse::ConfirmFact,
                    dm::InterventionResponse::DeferObservation,
                    dm::InterventionResponse::Unable,
                ];
                if let KeyCode::Char(c @ '1'..='4') = key {
                    self.set_response_mode(modes[(c as u8 - b'1') as usize]);
                    self.next_control(Control::Mode);
                } else if key == KeyCode::Enter {
                    self.next_control(Control::Mode);
                } else if matches!(
                    key,
                    KeyCode::Up | KeyCode::Down | KeyCode::Left | KeyCode::Right
                ) {
                    let index = modes
                        .iter()
                        .position(|m| *m == self.response_mode())
                        .unwrap_or(0);
                    self.set_response_mode(
                        modes[(index
                            + if matches!(key, KeyCode::Up | KeyCode::Left) {
                                3
                            } else {
                                1
                            })
                            % 4],
                    );
                }
            }
            Control::Option => {
                if let KeyCode::Char(c @ '0'..='9') = key {
                    let choice = (c as u8 - b'0') as usize;
                    if self
                        .current
                        .as_ref()
                        .is_some_and(|d| choice <= d.request.options.len())
                    {
                        let d = self.current.as_ref().unwrap();
                        let draft = self.drafts.entry(Self::key(d)).or_default();
                        draft.fields.remove(dm::RESPONSE_MODE);
                        draft.fields.remove("performed");
                        draft.fields.remove("result");
                        draft.outcome = if choice
                            .checked_sub(1)
                            .and_then(|i| d.request.options.get(i))
                            .is_some_and(|o| {
                                o.intervention_response == Some(dm::InterventionResponse::Unable)
                            }) {
                            dm::AnswerOutcome::Reject
                        } else {
                            dm::AnswerOutcome::Answer
                        };
                        draft.option = choice;
                        draft.apply_patch = false;
                        self.next_control(Control::Option);
                        self.save_drafts();
                        return None;
                    }
                }
                if key == KeyCode::Enter {
                    self.focus = (self.focus + 1) % controls.len();
                    return None;
                }
                if matches!(key, KeyCode::Char(c) if c!=' ') {
                    if let Some(d) = &self.current {
                        let draft = self.drafts.entry(Self::key(d)).or_default();
                        draft.option = 0;
                        draft.apply_patch = false;
                    }
                    self.focus_control(Control::Note);
                    return self.key_event(key);
                }
                if matches!(
                    key,
                    KeyCode::Left
                        | KeyCode::Up
                        | KeyCode::Right
                        | KeyCode::Down
                        | KeyCode::Char(' ')
                        | KeyCode::Enter
                ) {
                    if let Some(d) = &self.current {
                        let count = d.request.options.len() + 1;
                        let draft = self.drafts.entry(Self::key(d)).or_default();
                        draft.fields.remove(dm::RESPONSE_MODE);
                        draft.option = (draft.option
                            + if matches!(key, KeyCode::Left | KeyCode::Up) {
                                count - 1
                            } else {
                                1
                            })
                            % count;
                        draft.apply_patch = false;
                        draft.fields.remove("performed");
                        draft.fields.remove("result");
                        draft.outcome = if draft
                            .option
                            .checked_sub(1)
                            .and_then(|i| d.request.options.get(i))
                            .is_some_and(|o| {
                                o.intervention_response == Some(dm::InterventionResponse::Unable)
                            }) {
                            dm::AnswerOutcome::Reject
                        } else {
                            dm::AnswerOutcome::Answer
                        };
                    }
                }
            }
            Control::Field(i) => {
                let fields = self.form_fields();
                if let Some(field) = fields.get(i) {
                    let d = self.current.as_ref()?;
                    let draft = self.drafts.entry(Self::key(d)).or_default();
                    draft.active_field = i + 1;
                    let value = draft.fields.entry(field.id.clone()).or_default();
                    if !field.choices.is_empty() {
                        if let KeyCode::Char(c @ '1'..='9') = key {
                            if let Some(choice) = field.choices.get((c as u8 - b'1') as usize) {
                                *value = choice.clone();
                                self.focus = (self.focus + 1) % self.controls().len();
                                self.save_drafts();
                                return None;
                            }
                        }
                        if key == KeyCode::Enter && !value.is_empty() {
                            self.focus = (self.focus + 1) % controls.len();
                            return None;
                        }
                        if matches!(
                            key,
                            KeyCode::Left
                                | KeyCode::Up
                                | KeyCode::Right
                                | KeyCode::Down
                                | KeyCode::Char(' ')
                                | KeyCode::Enter
                        ) {
                            let pos = field.choices.iter().position(|c| c == value);
                            let index = pos
                                .map(|i| {
                                    (i + if matches!(key, KeyCode::Left | KeyCode::Up) {
                                        field.choices.len() - 1
                                    } else {
                                        1
                                    }) % field.choices.len()
                                })
                                .unwrap_or(0);
                            *value = field.choices[index].clone();
                        }
                    } else {
                        if key == KeyCode::Enter && !field.multiline {
                            self.focus = (self.focus + 1) % controls.len();
                            return None;
                        }
                        let cursor = draft.cursors.entry(field.id.clone()).or_insert(value.len());
                        edit_text(value, cursor, key, field.multiline);
                    }
                }
            }
            Control::Note => {
                let name = if self.feedback {
                    "feedback".into()
                } else {
                    self.current.as_ref().map(Self::key).unwrap_or_default()
                };
                let draft = self.drafts.entry(name).or_default();
                draft.active_field = 0;
                let cursor = draft
                    .cursors
                    .entry("__text".into())
                    .or_insert(draft.text.len());
                edit_text(&mut draft.text, cursor, key, true);
            }
            _ => {}
        }
        self.save_drafts();
        None
    }
}

// Byte positions are always normalized to Unicode character boundaries. Cursors
// travel with each draft field, so focus changes and resizes do not move edits.
fn edit_text(text: &mut String, cursor: &mut usize, key: KeyCode, multiline: bool) {
    *cursor = (*cursor).min(text.len());
    while !text.is_char_boundary(*cursor) {
        *cursor -= 1;
    }
    let before = text[..*cursor]
        .char_indices()
        .next_back()
        .map(|(i, _)| i)
        .unwrap_or(0);
    let after = text[*cursor..]
        .chars()
        .next()
        .map(|c| *cursor + c.len_utf8())
        .unwrap_or(*cursor);
    match key {
        KeyCode::Char(c) => {
            text.insert(*cursor, c);
            *cursor += c.len_utf8();
        }
        KeyCode::Enter if multiline => {
            text.insert(*cursor, '\n');
            *cursor += 1;
        }
        KeyCode::Backspace if *cursor > 0 => {
            text.drain(before..*cursor);
            *cursor = before;
        }
        KeyCode::Delete if *cursor < text.len() => {
            text.drain(*cursor..after);
        }
        KeyCode::Left => *cursor = before,
        KeyCode::Right => *cursor = after,
        KeyCode::End => *cursor = text.len(),
        _ => {}
    }
}
