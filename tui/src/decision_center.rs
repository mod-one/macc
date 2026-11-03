//! Nonblocking decision inbox; drafts are keyed by immutable request version.
mod actions;
use crossterm::event::KeyCode;
use macc_core::{
    coordinator::decisions as dm,
    process_ownership::{ProcessHandle, ProcessKind},
    service::process_ownership_gate::{gate_owner_action, ClientContext},
};
use ratatui::{
    layout::{Constraint, Direction, Layout, Rect},
    widgets::{Block, Borders, Clear, Paragraph, Wrap},
    Frame,
};
use serde_json::{json, Value};
use std::{
    collections::{BTreeMap, BTreeSet},
    path::PathBuf,
    sync::mpsc,
    time::{Duration, Instant},
};

#[derive(Default, Clone, serde::Serialize, serde::Deserialize)]
#[serde(default)]
struct Draft {
    effects: dm::AnswerEffects,
    fields: BTreeMap<String, String>,
    active_field: usize,
    apply_patch: bool,
    outcome: dm::AnswerOutcome,
    text: String,
    option: usize,
    advisory: bool,
}
#[derive(Default)]
pub struct Center {
    actions: actions::Panel,
    root: Option<PathBuf>,
    snapshot: Option<Value>,
    inspected_contexts: Option<Value>,
    rx: Option<mpsc::Receiver<Result<Value, String>>>,
    last: Option<Instant>,
    pub open: bool,
    manual: bool,
    current: Option<dm::Decision>,
    drafts: BTreeMap<String, Draft>,
    deferred: BTreeSet<String>,
    scroll: u16,
    message: String,
    feedback: bool,
    show_help: bool,
    submitting: bool,
}
pub enum Action {
    Operator {
        intent: dm::OperatorIntent,
        proof: Option<dm::Attestation>,
        challenge: bool,
    },
    Approval {
        ticket: String,
        apply: bool,
    },
    Probe {
        operation: String,
        version: u64,
    },
    SetEffects(dm::AnswerEffects),
    Answer {
        id: String,
        version: u64,
        text: String,
        option: Option<String>,
        advisory: bool,
        fields: BTreeMap<String, String>,
        apply_patch: bool,
        outcome: dm::AnswerOutcome,
        effects: dm::AnswerEffects,
    },
    Challenge {
        id: String,
        version: u64,
        input: dm::AnswerInput,
    },
    Policy(dm::Policy),
    Feedback(String),
    InspectContext(String),
}
impl Center {
    fn key(d: &dm::Decision) -> String {
        format!("{}@{}", d.id, d.version)
    }
    fn list(&self) -> Vec<dm::Decision> {
        self.snapshot
            .as_ref()
            .and_then(|s| s.get("decisions"))
            .and_then(|v| serde_json::from_value(v.clone()).ok())
            .unwrap_or_default()
    }
    pub fn policy(&self) -> dm::Policy {
        self.snapshot
            .as_ref()
            .and_then(|s| s.get("policy"))
            .and_then(|v| serde_json::from_value(v.clone()).ok())
            .unwrap_or_default()
    }
    fn save_drafts(&self) {
        if let Some(root) = &self.root {
            self.actions.save(root);
            if let Ok(bytes) = serde_json::to_vec(&self.drafts) {
                let _ = macc_core::atomic_write(
                    &macc_core::ProjectPaths::from_root(root),
                    &root.join(".macc/cache/decision-drafts.json"),
                    &bytes,
                );
            }
        }
    }
    pub fn tick(&mut self, root: Option<PathBuf>, busy: bool) {
        if self.root != root {
            self.save_drafts();
            *self = Self::default();
            self.root = root.clone();
            if let Some(root) = &root {
                self.actions.load(root);
                let file = root.join(".macc/cache/decision-drafts.json");
                if std::fs::metadata(&file).is_ok_and(|m| m.len() < 1048576) {
                    if let Ok(bytes) = std::fs::read(file) {
                        self.drafts = serde_json::from_slice(&bytes).unwrap_or_default();
                    }
                }
            }
        }
        if let Some(rx) = &self.rx {
            if let Ok(result) = rx.try_recv() {
                self.rx = None;
                match result {
                    Ok(value) => {
                        if let Some(contexts) = value.get("inspected_contexts") {
                            self.inspected_contexts = Some(contexts.clone());
                        }
                        let notice = value
                            .get("operator_notice")
                            .and_then(Value::as_str)
                            .map(str::to_owned);
                        let selected = value
                            .get("selected_decision_id")
                            .and_then(Value::as_str)
                            .map(str::to_owned);
                        self.snapshot = Some(value);
                        if let Some(id) = selected {
                            self.current = self.list().into_iter().find(|d| d.id == id);
                        }
                        if self.submitting {
                            if let Some(current) = &self.current {
                                if let Some(fresh) =
                                    self.list().into_iter().find(|d| d.id == current.id)
                                {
                                    self.current = Some(fresh);
                                }
                            }
                            self.message=notice.unwrap_or_else(||"Decision recorded. Context delivery and compliance are tracked independently.".into());
                        }
                    }
                    Err(error) => self.message = error,
                }
                self.submitting = false;
            }
        }
        if self.rx.is_none()
            && self
                .last
                .is_none_or(|t| t.elapsed() >= Duration::from_secs(3))
        {
            if let Some(root) = root {
                let (tx, rx) = mpsc::channel();
                std::thread::spawn(move || {
                    let _ = tx.send(dm::snapshot(&root).map_err(|e| e.to_string()));
                });
                self.rx = Some(rx);
                self.last = Some(Instant::now());
            }
        }
        if self.policy().human_question_min_risk == dm::Consultation::NeverAsk {
            let committed = self
                .current
                .as_ref()
                .and_then(|old| self.list().into_iter().find(|d| d.id == old.id))
                .is_some_and(|fresh| fresh.status != "pending_human");
            if self.open && !self.manual && committed {
                self.open = false;
            }
            return;
        }
        if !self.open && !busy {
            if let Some(next) = self
                .list()
                .into_iter()
                .find(|d| d.status == "pending_human" && !self.deferred.contains(&Self::key(d)))
            {
                self.current = Some(next);
                self.open = true;
                self.manual = false;
                self.scroll = 0;
            }
        }
    }
    pub fn prepare_gate(&mut self, decision: &crate::approval::Decision) {
        self.manual = true;
        self.open = true;
        if let Some(snapshot) = self.snapshot.as_ref() {
            self.actions.prepare_gate(decision, snapshot);
        }
        self.message="Review the approver identity and role. F5 requests independent approval; it does not invent a vote.".into();
    }
    pub fn show(&mut self) {
        self.current = self
            .list()
            .into_iter()
            .find(|d| d.status == "pending_human")
            .or_else(|| self.list().pop());
        self.open = true;
        self.manual = true;
        self.scroll = 0;
    }
    pub fn pending(&self) -> usize {
        self.list()
            .iter()
            .filter(|d| d.status == "pending_human")
            .count()
    }
    pub fn key_event(&mut self, key: KeyCode) -> Option<Action> {
        if !matches!(key, KeyCode::Char(_) | KeyCode::Enter | KeyCode::Backspace) {
            self.save_drafts();
        }
        if key == KeyCode::F(1) {
            self.show_help = !self.show_help;
            self.scroll = 0;
            return None;
        }
        if key == KeyCode::Home {
            self.actions.show();
            self.manual = true;
            return None;
        }
        if self.actions.open {
            if key == KeyCode::PageUp {
                self.scroll = self.scroll.saturating_sub(8);
                return None;
            }
            if key == KeyCode::PageDown {
                self.scroll = self.scroll.saturating_add(8);
                return None;
            }
            let snapshot = self.snapshot.clone().unwrap_or_default();
            let result = self
                .actions
                .key_event(key, self.current.as_ref(), &snapshot);
            if let Some(Action::SetEffects(effects)) = result {
                if let Some(d) = &self.current {
                    let draft = self.drafts.entry(Self::key(d)).or_default();
                    draft.effects = effects;
                    draft.apply_patch = false;
                }
                self.actions.open = false;
                self.save_drafts();
                return None;
            }
            self.save_drafts();
            return result;
        }
        if key == KeyCode::Esc {
            if let Some(d) = &self.current {
                self.deferred.insert(Self::key(d));
            }
            self.open = false;
            return None;
        }
        if key == KeyCode::F(4) {
            if let Some(d) = &self.current {
                return Some(Action::InspectContext(d.request.task_id.clone()));
            }
            return None;
        }
        if key == KeyCode::F(2) {
            self.inspected_contexts = None;
            let list = self.list();
            if !list.is_empty() {
                let i = self
                    .current
                    .as_ref()
                    .and_then(|c| list.iter().position(|d| d.id == c.id))
                    .map(|i| (i + 1) % list.len())
                    .unwrap_or(0);
                self.current = Some(list[i].clone());
                self.feedback = false;
            }
            return None;
        }
        if key == KeyCode::F(6) {
            let mut p = self.policy();
            p.human_question_min_risk = match p.human_question_min_risk {
                dm::Consultation::Low => dm::Consultation::Medium,
                dm::Consultation::Medium => dm::Consultation::High,
                dm::Consultation::High => dm::Consultation::Critical,
                dm::Consultation::Critical => dm::Consultation::NeverAsk,
                dm::Consultation::NeverAsk => dm::Consultation::Low,
            };
            return Some(Action::Policy(p));
        }
        if key == KeyCode::F(7) {
            let mut p = self.policy();
            p.enabled = !p.enabled;
            return Some(Action::Policy(p));
        }
        if key == KeyCode::F(8) {
            self.feedback = true;
            self.message = "New project direction: type your text; F5 records it".into();
            return None;
        }
        match key {
            KeyCode::PageUp => {
                self.scroll = self.scroll.saturating_sub(8);
                return None;
            }
            KeyCode::PageDown => {
                self.scroll = self.scroll.saturating_add(8);
                return None;
            }
            _ => {}
        }
        let name = if self.feedback {
            "feedback".into()
        } else {
            self.current.as_ref().map(Self::key).unwrap_or_default()
        };
        let secure = self.policy().trust_mode == "secure_approval"
            && self
                .snapshot
                .as_ref()
                .is_none_or(|s| s["approval_broker_available"] != true);
        let draft = self.drafts.entry(name).or_default();
        if self.feedback {
            match key {
                KeyCode::Char(c) => draft.text.push(c),
                KeyCode::Enter => draft.text.push('\n'),
                KeyCode::Backspace => {
                    draft.text.pop();
                }
                KeyCode::F(5) => return Some(Action::Feedback(draft.text.clone())),
                _ => {}
            }
            return None;
        }
        let Some(current) = &self.current else {
            return None;
        };
        if current.status != "pending_human"
            || self
                .snapshot
                .as_ref()
                .and_then(|s| s.pointer("/policy/human_question_min_risk"))
                .and_then(Value::as_str)
                == Some("never_ask")
        {
            return None;
        }
        let mut fields = current.request.response_fields.clone();
        if matches!(current.request.kind.as_str(), "approval" | "authorization")
            && !fields.iter().any(|f| f.id == "evidence")
        {
            fields.push(dm::ResponseField {
                id: "evidence".into(),
                label: "Approval evidence".into(),
                required: true,
                multiline: false,
                choices: vec![],
            });
        }
        if secure {
            fields.push(dm::ResponseField {
                id: "__attestation".into(),
                label: "Isolated approver attestation JSON".into(),
                required: true,
                multiline: true,
                choices: vec![],
            });
        }
        if key == KeyCode::F(12) {
            let mut fields = draft.fields.clone();
            fields.remove("__attestation");
            return Some(Action::Challenge {
                id: current.id.clone(),
                version: current.version,
                input: dm::AnswerInput {
                    effects: draft.effects,
                    patch: None,
                    text: if draft.text.trim().is_empty() {
                        current
                            .request
                            .options
                            .get(draft.option.wrapping_sub(1))
                            .map(|o| o.label.clone())
                            .unwrap_or_default()
                    } else {
                        draft.text.clone()
                    },
                    option: current
                        .request
                        .options
                        .get(draft.option.wrapping_sub(1))
                        .map(|o| o.id.clone()),
                    advisory: draft.advisory,
                    apply_patch: draft.apply_patch,
                    outcome: draft.outcome,
                    fields,
                    attestation: None,
                },
            });
        }
        if key == KeyCode::F(9) {
            draft.active_field = (draft.active_field + 1) % (fields.len() + 1);
            return None;
        }
        if key == KeyCode::F(10) {
            let selected = draft
                .option
                .checked_sub(1)
                .and_then(|i| current.request.options.get(i))
                .map(|o| o.id.as_str());
            if current.request.patch_for(selected).is_some() && !draft.advisory {
                draft.apply_patch = !draft.apply_patch;
                draft.effects = dm::AnswerEffects::Auto;
            }
            return None;
        }
        if key == KeyCode::F(11) {
            draft.outcome = match draft.outcome {
                dm::AnswerOutcome::Answer => dm::AnswerOutcome::Reject,
                dm::AnswerOutcome::Reject => dm::AnswerOutcome::RequestChanges,
                dm::AnswerOutcome::RequestChanges => dm::AnswerOutcome::Answer,
            };
            draft.apply_patch = false;
            return None;
        }
        if draft.active_field > 0 {
            if let Some(field) = fields.get(draft.active_field - 1) {
                let value = draft.fields.entry(field.id.clone()).or_default();
                match key {
                    KeyCode::Char(c) => {
                        value.push(c);
                        return None;
                    }
                    KeyCode::Enter => {
                        value.push('\n');
                        return None;
                    }
                    KeyCode::Backspace => {
                        value.pop();
                        return None;
                    }
                    KeyCode::Tab if !field.choices.is_empty() => {
                        let index = field
                            .choices
                            .iter()
                            .position(|s| s == value)
                            .map(|i| (i + 1) % field.choices.len())
                            .unwrap_or(0);
                        *value = field.choices[index].clone();
                        return None;
                    }
                    _ => {}
                }
            }
        }
        match key {
            KeyCode::Tab => {
                draft.option = (draft.option + 1) % (current.request.options.len() + 1);
                draft.apply_patch = false;
            }
            KeyCode::F(3) => draft.advisory = !draft.advisory,
            KeyCode::Char(c) => draft.text.push(c),
            KeyCode::Enter => draft.text.push('\n'),
            KeyCode::Backspace => {
                draft.text.pop();
            }
            KeyCode::F(5) => {
                let option = draft
                    .option
                    .checked_sub(1)
                    .and_then(|i| current.request.options.get(i));
                let text = if draft.text.trim().is_empty() {
                    option.map(|o| o.label.clone()).unwrap_or_default()
                } else {
                    draft.text.clone()
                };
                if text.trim().is_empty() {
                    self.message = "Choose an option with Tab or enter an answer".into();
                    return None;
                }
                return Some(Action::Answer {
                    id: current.id.clone(),
                    version: current.version,
                    text,
                    option: option.map(|o| o.id.clone()),
                    advisory: draft.advisory,
                    fields: draft.fields.clone(),
                    apply_patch: draft.apply_patch,
                    outcome: draft.outcome,
                    effects: draft.effects,
                });
            }
            _ => {}
        }
        None
    }
    pub fn submit(&mut self, root: PathBuf, client_id: String, action: Action) {
        self.save_drafts();
        if self.rx.is_some() {
            self.message = "Refresh in progress; submit again when complete".into();
            return;
        }
        let (tx, rx) = mpsc::channel();
        self.rx = Some(rx);
        self.submitting = !matches!(action, Action::InspectContext(_));
        self.message = "Recording… your draft is preserved".into();
        std::thread::spawn(move || {
            let result = (|| -> macc_core::Result<Value> {
                if let Action::InspectContext(task) = &action {
                    let (store, plan) = dm::inspect_plan(&root)?;
                    let mut snapshot = dm::snapshot(&root)?;
                    snapshot["inspected_contexts"] = json!(store.context_history(&plan.id, task)?);
                    return Ok(snapshot);
                }
                gate_owner_action(
                    &ClientContext {
                        client_id,
                        project_root: root.clone(),
                    },
                    &ProcessHandle {
                        kind: ProcessKind::Project,
                        project_root: root.clone(),
                        pid: None,
                    },
                )?;
                let actor = macc_core::service::coordinator_workflow::operator_identity(&root);
                if actor.is_empty() {
                    return Err(dm::error(
                        "Configure Git user.name/user.email for local attribution",
                    ));
                }
                if let Action::Policy(p) = action {
                    return run_operator(
                        &root,
                        dm::OperatorIntent::Configure { policy: p },
                        None,
                        false,
                        &actor,
                    );
                }
                let p = dm::policy(&root)?;
                let (mut store, plan) = dm::inspect_plan(&root)?;
                match action {
                    Action::Answer {
                        id,
                        version,
                        text,
                        option,
                        advisory,
                        mut fields,
                        apply_patch,
                        outcome,
                        effects,
                    } => {
                        let attestation = fields
                            .remove("__attestation")
                            .filter(|s| !s.trim().is_empty())
                            .map(|s| serde_json::from_str(&s).map_err(|e| dm::error(e.to_string())))
                            .transpose()?;
                        return run_operator(
                            &root,
                            dm::OperatorIntent::Answer {
                                id,
                                version,
                                input: dm::AnswerInput {
                                    effects,
                                    patch: None,
                                    attestation: attestation.clone(),
                                    text,
                                    option,
                                    advisory,
                                    fields,
                                    apply_patch,
                                    outcome,
                                },
                            },
                            attestation,
                            false,
                            &actor,
                        );
                    }
                    Action::Challenge { id, version, input } => {
                        let d = store.decision_in_scope(&plan, &id)?;
                        if d.version != version {
                            return Err(dm::error(
                                "Request changed; refresh before preparing a challenge",
                            ));
                        }
                        let file = root
                            .join(".macc/runtime/decisions/challenges")
                            .join(format!("{id}-{version}.json"));
                        macc_core::atomic_write(
                            &macc_core::ProjectPaths::from_root(&root),
                            &file,
                            &serde_json::to_vec_pretty(&dm::answer_challenge(&d, &input, &p))
                                .map_err(|e| dm::error(e.to_string()))?,
                        )?;
                        let mut snapshot = dm::snapshot(&root)?;
                        snapshot["inspected_contexts"] = json!({"approver_challenge_file":file,"challenge":dm::answer_challenge(&d,&input,&p)});
                        return Ok(snapshot);
                    }
                    Action::Feedback(text) => {
                        return run_operator(
                            &root,
                            dm::OperatorIntent::Feedback {
                                plan_id: plan.id.clone(),
                                task: "*".into(),
                                text,
                                advisory: false,
                            },
                            None,
                            false,
                            &actor,
                        )
                    }
                    Action::Operator {
                        intent,
                        proof,
                        challenge,
                    } => return run_operator(&root, intent, proof, challenge, &actor),
                    Action::Approval { ticket, apply } => {
                        let result = if apply {
                            store.apply_approval(&ticket, &actor)?
                        } else {
                            store.poll_approval(&ticket, &actor)?
                        };
                        let mut snapshot = dm::snapshot(&root)?;
                        snapshot["operator_notice"] = json!(result.message);
                        if let Some(value) = result.result {
                            if value.get("request").is_some() {
                                snapshot["selected_decision_id"] = value["id"].clone();
                            }
                        }
                        return Ok(snapshot);
                    }
                    Action::Probe { operation, version } => {
                        store.probe_operation(&plan.id, &operation, version)?;
                    }
                    Action::Policy(_) | Action::InspectContext(_) | Action::SetEffects(_) => {
                        unreachable!()
                    }
                }
                dm::snapshot(&root)
            })();
            let _ = tx.send(result.map_err(|e| e.to_string()));
        });
    }
    pub fn draw(&self, frame: &mut Frame) {
        if !self.open {
            return;
        }
        let area = frame.size();
        let rect = Rect {
            x: area.x + area.width / 20,
            y: area.y + area.height / 20,
            width: area.width.saturating_sub(area.width / 10),
            height: area.height.saturating_sub(area.height / 10),
        };
        frame.render_widget(Clear, rect);
        if rect.height < 10 || rect.width < 30 {
            let rect = area;
            frame.render_widget(Clear, rect);
            let key = if self.feedback {
                "feedback".into()
            } else {
                self.current.as_ref().map(Self::key).unwrap_or_default()
            };
            let draft = self.drafts.get(&key).cloned().unwrap_or_default();
            let (details, editor) = if self.actions.open {
                self.actions.view(
                    self.current.as_ref(),
                    self.snapshot.as_ref().unwrap_or(&Value::Null),
                )
            } else {
                let d = self.current.as_ref();
                let editor = if draft.active_field > 0 {
                    d.and_then(|d| d.request.response_fields.get(draft.active_field - 1))
                        .map(|f| {
                            format!(
                                "{}: {}",
                                f.label,
                                draft.fields.get(&f.id).cloned().unwrap_or_default()
                            )
                        })
                        .unwrap_or_else(|| {
                            format!(
                                "Evidence: {}",
                                draft.fields.get("evidence").cloned().unwrap_or_default()
                            )
                        })
                } else {
                    format!("Answer: {}", draft.text)
                };
                (format!("{}\n{}\n{}\nF2 next · Tab choice · F9 field · Home actions\nPgUp/PgDn scroll; Enter newline; F5 submits explicitly",d.map(|d|d.request.question.as_str()).unwrap_or("Decision inbox"),d.map(|d|d.request.context.as_str()).unwrap_or(""),self.message),editor)
            };
            let body = Rect {
                height: rect.height.saturating_sub(1),
                ..rect
            };
            frame.render_widget(
                Paragraph::new(if self.show_help {
                    details
                } else {
                    format!("{editor}\n{details}")
                })
                .wrap(Wrap { trim: false })
                .scroll((self.scroll, 0)),
                body,
            );
            if rect.height > 0 {
                frame.render_widget(
                    Paragraph::new("F1 Info F5 Send Esc"),
                    Rect {
                        x: rect.x,
                        y: rect.y + rect.height - 1,
                        width: rect.width,
                        height: 1,
                    },
                );
            }
            return;
        }
        let compact = rect.height < 18 || rect.width < 65;
        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Min(1),
                Constraint::Length(if compact { 4 } else { 7 }),
                Constraint::Length(if compact { 4 } else { 6 }),
            ])
            .split(rect);
        let p = self.policy();
        let mut text=format!("Durable decisions: {} | mode: {:?} | pending: {}\nHigher risk threshold grants more autonomy. NeverAsk records agent choices without questions.\n{}\n\n",p.enabled,p.human_question_min_risk,self.pending(),self.message);
        if self.show_help {
            text.push_str("KEYS: Home opens lifecycle/approval/operation actions; F2 next request/history; Tab option or field choice; F3 advisory; F4 delivery/ACK/compliance; F5 explicit submit; F6 autonomy; F7 enable; F8 new guidance; F9 edit next response field; F10 explicitly accept displayed patch; F11 answer/reject/request changes; F12 write isolated approver challenge; Enter newline; PgUp/PgDn scroll evidence; Esc defer without submitting.\n\n");
        }
        if let Some(d) = &self.current {
            text.push_str(&format!("{}\nTask: {} | kind: {} | risk: {:?} | status: {} | version {}\n\n{}\n\nRecommendation: {}\n",d.request.question,d.request.task_id,d.request.kind,d.effective_risk,d.status,d.version,d.request.context,d.request.recommendation));
            for (i, o) in d.request.options.iter().enumerate() {
                text.push_str(&format!(
                    "{}. {} — {}\n",
                    i + 1,
                    o.label,
                    o.consequences.join("; ")
                ));
            }
            let option = self
                .drafts
                .get(&Self::key(d))
                .and_then(|draft| draft.option.checked_sub(1))
                .and_then(|i| d.request.options.get(i))
                .map(|o| o.id.as_str());
            if let Some(patch) = d
                .request
                .patch_for(option)
                .or(d.request.proposed_patch.as_ref())
            {
                text.push_str(&format!(
                    "\nProposed graph change (F10 explicitly accepts the displayed patch):\n{}\n",
                    patch.describe().join("\n")
                ));
            }
            if let Some(r) = &d.resolution {
                text.push_str(&format!(
                    "\nRecorded outcome [{} by {}]:\n{}\n",
                    r.origin, r.actor, r.text
                ));
            }
        } else {
            text.push_str(
                "No decisions yet. F7 enables the feature; F8 records a new project direction.\n",
            );
        }
        if let Some(contexts) = self.inspected_contexts.as_ref() {
            text.push_str(&format!(
                "\nExact issued contexts and separate receipts:\n{}\n",
                serde_json::to_string_pretty(contexts).unwrap_or_default()
            ));
        }
        let name = if self.feedback {
            "feedback".into()
        } else {
            self.current.as_ref().map(Self::key).unwrap_or_default()
        };
        let draft = self.drafts.get(&name).cloned().unwrap_or_default();
        let mut answer = format!(
            "{} (option {}; 0 = custom; advisory {}):\n{}",
            if self.feedback {
                "New project direction"
            } else {
                "Answer draft"
            },
            draft.option,
            draft.advisory,
            draft.text
        );
        let secure = self.policy().trust_mode == "secure_approval"
            && self
                .snapshot
                .as_ref()
                .is_none_or(|s| s["approval_broker_available"] != true);
        if let Some(d) = &self.current {
            let mut fields = d.request.response_fields.clone();
            if matches!(d.request.kind.as_str(), "approval" | "authorization")
                && !fields.iter().any(|f| f.id == "evidence")
            {
                fields.push(dm::ResponseField {
                    id: "evidence".into(),
                    label: "Approval evidence".into(),
                    required: true,
                    multiline: false,
                    choices: vec![],
                });
            }
            if secure {
                fields.push(dm::ResponseField {
                    id: "__attestation".into(),
                    label: "Isolated approver attestation JSON".into(),
                    required: true,
                    multiline: true,
                    choices: vec![],
                });
            }
            if draft.active_field > 0 {
                if let Some(field) = fields.get(draft.active_field - 1) {
                    answer = format!(
                        "Editing {}{} (F9 next field)\n{}",
                        field.label,
                        if field.required { " *" } else { "" },
                        draft.fields.get(&field.id).cloned().unwrap_or_default()
                    );
                }
            }
        }
        answer = format!(
            "Outcome: {:?}; effects: {:?}; explicit patch acceptance: {}\n{}",
            draft.outcome, draft.effects, draft.apply_patch, answer
        );
        if self.actions.open {
            let (snapshot_text, editor) = self.actions.view(
                self.current.as_ref(),
                self.snapshot.as_ref().unwrap_or(&Value::Null),
            );
            text = format!("{}\n{}", self.message, snapshot_text);
            answer = editor;
        }
        frame.render_widget(
            Paragraph::new(text)
                .wrap(Wrap { trim: false })
                .scroll((self.scroll, 0))
                .block(
                    Block::default()
                        .borders(Borders::ALL)
                        .title("Decisions — source and outcome remain attributed"),
                ),
            chunks[0],
        );
        let width = usize::from(chunks[1].width.saturating_sub(2)).max(1);
        let lines = answer
            .lines()
            .map(|line| line.chars().count().div_ceil(width).max(1))
            .sum::<usize>();
        let offset = lines
            .saturating_sub(usize::from(chunks[1].height.saturating_sub(2)))
            .min(u16::MAX as usize) as u16;
        frame.render_widget(
            Paragraph::new(answer)
                .wrap(Wrap { trim: false })
                .scroll((offset, 0))
                .block(
                    Block::default()
                        .borders(Borders::ALL)
                        .title("Your response — F5 sends, Esc keeps draft"),
                ),
            chunks[1],
        );
        frame.render_widget(Paragraph::new(if compact{"[F1] Keys [Home] Actions\n[F5] Send [Esc] Later"}else{"[F5] Send  [Esc] Later  [F1] Keys  [Home] Actions\n[F2] Next / history  [Tab] Option  [F3] Advisory [F4] Contexts\n[F5] Send / save  [Enter] Newline  [PgUp/PgDn] Scroll\n[F6] Autonomy [F7] Enable [F8] Direction [F9] Field [F10] Patch [F11] Outcome [F12] Approver challenge\n[Home] Actions / approvals / operations  [Esc] Later / close — never submits an answer"}).wrap(Wrap{trim:false}).block(Block::default().borders(Borders::ALL)),chunks[2]);
    }
}
impl Drop for Center {
    fn drop(&mut self) {
        self.save_drafts();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    pub(super) fn center(mode: &str) -> Center {
        let d:dm::Decision=serde_json::from_value(json!({"id":"q","plan_id":"p","version":1,"status":"pending_human","effective_risk":"high","request":{"idempotency_key":"q","task_id":"A","question":"Which contract?","context":"Two consumers","recommendation":"Preserve the contract","investigated":["Read call sites"],"options":[{"id":"keep","label":"Keep","consequences":[]}],"recommended_option_id":"keep"},"resolution":null,"created_at":"2026-10-29T00:00:00Z","subject_digest":"digest"})).unwrap();
        let mut c = Center::default();
        c.snapshot =
            Some(json!({"policy":{"enabled":true,"human_question_min_risk":mode},"decisions":[d]}));
        c.show();
        c
    }
    #[test]
    fn enter_adds_newline_and_only_f5_sends() {
        let mut c = center("high");
        assert!(c.key_event(KeyCode::Char('é')).is_none());
        assert!(c.key_event(KeyCode::Enter).is_none());
        assert_eq!(c.drafts["q@1"].text, "é\n");
        let Some(Action::Answer { text, .. }) = c.key_event(KeyCode::F(5)) else {
            panic!("explicit F5 must submit")
        };
        assert_eq!(text, "é\n");
    }
    #[test]
    fn escape_defers_and_keeps_draft() {
        let mut c = center("high");
        c.key_event(KeyCode::Char('x'));
        assert!(c.key_event(KeyCode::Esc).is_none());
        assert!(!c.open);
        assert!(c.deferred.contains("q@1"));
        assert_eq!(c.drafts["q@1"].text, "x");
    }
    #[test]
    fn full_autonomy_history_never_accepts_an_answer() {
        let mut c = center("never_ask");
        assert!(c.key_event(KeyCode::Char('x')).is_none());
        assert!(c.key_event(KeyCode::F(5)).is_none());
        assert!(c.drafts["q@1"].text.is_empty());
    }
    #[test]
    fn dialog_renders_without_panicking_at_small_and_large_sizes() {
        for (w, h) in [(20, 6), (40, 12), (80, 24), (180, 50)] {
            let backend = ratatui::backend::TestBackend::new(w, h);
            let mut terminal = ratatui::Terminal::new(backend).unwrap();
            let c = center("high");
            terminal.draw(|frame| c.draw(frame)).unwrap();
        }
    }
}

#[cfg(test)]
mod structured_tests {
    use super::*;
    fn center() -> Center {
        let d:dm::Decision=serde_json::from_value(json!({"id":"fields","plan_id":"p","version":1,"status":"pending_human","effective_risk":"high","request":{"idempotency_key":"f","task_id":"A","question":"Required evidence?","recommendation":"Keep compatibility","investigated":["Read sources"],"response_fields":[{"id":"reason","label":"Rationale","required":true,"multiline":true,"choices":[]}]},"resolution":null,"created_at":"2026-10-30T00:00:00Z","subject_digest":"s"})).unwrap();
        let mut c = Center::default();
        c.snapshot = Some(
            json!({"enabled":true,"policy":dm::Policy{enabled:true,..Default::default()},"decisions":[d]}),
        );
        c.show();
        c
    }
    #[test]
    fn structured_field_editing_keeps_answer_text_and_never_submits_on_newline() {
        let mut c = center();
        for ch in "Answer".chars() {
            c.key_event(KeyCode::Char(ch));
        }
        c.key_event(KeyCode::F(9));
        for ch in "Evidence".chars() {
            c.key_event(KeyCode::Char(ch));
        }
        assert!(c.key_event(KeyCode::Enter).is_none());
        match c.key_event(KeyCode::F(5)).unwrap() {
            Action::Answer {
                text,
                fields,
                apply_patch,
                ..
            } => {
                assert_eq!(text, "Answer");
                assert_eq!(fields["reason"], "Evidence\n");
                assert!(!apply_patch);
            }
            _ => panic!("expected explicit answer"),
        }
    }
    #[test]
    fn resize_and_help_preserve_multiline_structured_draft() {
        let mut c = center();
        c.key_event(KeyCode::Char('é'));
        c.key_event(KeyCode::F(9));
        c.key_event(KeyCode::Char('à'));
        c.key_event(KeyCode::F(1));
        for (width, height) in [(24, 9), (40, 14), (100, 30)] {
            let backend = ratatui::backend::TestBackend::new(width, height);
            let mut terminal = ratatui::Terminal::new(backend).unwrap();
            terminal.draw(|f| c.draw(f)).unwrap();
        }
        match c.key_event(KeyCode::F(5)).unwrap() {
            Action::Answer { text, fields, .. } => {
                assert_eq!(text, "é");
                assert_eq!(fields["reason"], "à");
            }
            _ => panic!("draft changed"),
        }
    }
}

fn run_operator(
    root: &std::path::Path,
    intent: dm::OperatorIntent,
    proof: Option<dm::Attestation>,
    challenge: bool,
    actor: &str,
) -> macc_core::Result<Value> {
    let mut store = dm::Store::open(root)?;
    let p = dm::policy(root)?;
    if challenge {
        let value = store.operator_challenge(&intent)?;
        let file = root
            .join(".macc/runtime/decisions/challenges")
            .join(format!(
                "{}.json",
                dm::digest(&intent).trim_start_matches("sha256:")
            ));
        macc_core::atomic_write(
            &macc_core::ProjectPaths::from_root(root),
            &file,
            &serde_json::to_vec_pretty(&value).map_err(|e| dm::error(e.to_string()))?,
        )?;
        let mut snapshot = dm::snapshot(root)?;
        snapshot["inspected_contexts"] = json!({"approver_challenge_file":file,"challenge":value});
        snapshot["operator_notice"] =
            json!("Exact action challenge prepared; no action was applied");
        return Ok(snapshot);
    }
    if p.trust_mode == "secure_approval" && proof.is_none() && dm::broker_available(&p) {
        let ticket = store.begin_approval(intent, actor)?;
        let mut snapshot = dm::snapshot(root)?;
        snapshot["operator_notice"]=json!(format!("Independent approval {}: {}. Home > Independent approval requests opens its page/status. Your draft remains unsent.",ticket.status,ticket.message));
        return Ok(snapshot);
    }
    let result = store.apply_operator(&intent, proof.as_ref(), actor)?;
    let mut snapshot = dm::snapshot(root)?;
    snapshot["operator_notice"]=json!("Action durably recorded; task-plan effects, context delivery and compliance remain separate");
    if result.get("request").is_some() {
        snapshot["selected_decision_id"] = result["id"].clone();
    }
    Ok(snapshot)
}

#[cfg(test)]
mod lifecycle_tests {
    use super::*;
    #[test]
    fn action_menu_withdraws_only_on_explicit_f5_and_retains_multiline_reason() {
        let mut c = super::tests::center("high");
        c.key_event(KeyCode::Home);
        assert!(c.key_event(KeyCode::F(5)).is_none());
        c.key_event(KeyCode::Down);
        c.key_event(KeyCode::Enter);
        for ch in "Obsolete".chars() {
            assert!(c.key_event(KeyCode::Char(ch)).is_none());
        }
        assert!(c.key_event(KeyCode::Enter).is_none());
        let Some(Action::Operator {
            intent:
                dm::OperatorIntent::Withdraw {
                    id,
                    version,
                    reason,
                },
            ..
        }) = c.key_event(KeyCode::F(5))
        else {
            panic!("expected explicit versioned withdrawal")
        };
        assert_eq!(id, "q");
        assert_eq!(version, 1);
        assert_eq!(reason, "Obsolete\n");
    }
    #[test]
    fn terminal_fallback_keeps_explicit_send_and_escape_visible_after_resize() {
        let mut c = super::tests::center("high");
        c.key_event(KeyCode::Char('é'));
        c.key_event(KeyCode::Enter);
        for (width, height) in [(20, 6), (28, 8), (80, 24)] {
            let mut terminal =
                ratatui::Terminal::new(ratatui::backend::TestBackend::new(width, height)).unwrap();
            terminal.draw(|f| c.draw(f)).unwrap();
            let rendered = terminal
                .backend()
                .buffer()
                .content
                .iter()
                .map(|cell| cell.symbol())
                .collect::<String>();
            assert!(rendered.contains("F5"), "{width}x{height}: {rendered}");
            assert!(rendered.contains("Esc"));
        }
        assert_eq!(c.drafts["q@1"].text, "é\n");
    }
    #[test]
    fn alternative_patch_acceptance_and_replanning_are_discoverable_separate_actions() {
        let mut c = super::tests::center("high");
        let d = c.current.as_mut().unwrap();
        d.request.options.push(serde_json::from_value(json!({"id":"b","label":"Alternative B","proposed_patch":{"plan_id":"p","base_graph_version":1,"idempotency_key":"b","reason":"Alternative","operations":[{"op":"add_task","task":{"id":"B"}}]}})).unwrap());
        c.key_event(KeyCode::Tab);
        c.key_event(KeyCode::Tab);
        c.key_event(KeyCode::F(10));
        let Some(Action::Answer {
            option,
            apply_patch,
            ..
        }) = c.key_event(KeyCode::F(5))
        else {
            panic!("missing answer")
        };
        assert_eq!(option.as_deref(), Some("b"));
        assert!(apply_patch);
    }
}
