//! Nonblocking decision inbox; drafts are keyed by immutable request version.
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
struct Draft {
    text: String,
    option: usize,
    advisory: bool,
}
#[derive(Default)]
pub struct Center {
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
    submitting: bool,
}
pub enum Action {
    Answer {
        id: String,
        version: u64,
        text: String,
        option: Option<String>,
        advisory: bool,
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
                        self.snapshot = Some(value);
                        if self.submitting {
                            if let Some(current) = &self.current {
                                if let Some(fresh) =
                                    self.list().into_iter().find(|d| d.id == current.id)
                                {
                                    self.current = Some(fresh);
                                }
                            }
                            self.message="Decision recorded. Context delivery and compliance are tracked independently.".into();
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
            if self.open && !self.manual {
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
        match key {
            KeyCode::Tab => {
                draft.option = (draft.option + 1) % (current.request.options.len() + 1);
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
                    dm::configure(&root, p)?;
                    return dm::snapshot(&root);
                }
                let p = dm::policy(&root)?;
                let (mut store, plan) = dm::active_plan(&root)?;
                match action {
                    Action::Answer {
                        id,
                        version,
                        text,
                        option,
                        advisory,
                    } => {
                        store.answer(&id, version, &text, option, &actor, advisory, &p)?;
                    }
                    Action::Feedback(text) => {
                        let request:dm::Request=serde_json::from_value(json!({"idempotency_key":format!("feedback-{}",chrono::Utc::now().timestamp_nanos_opt().unwrap_or_default()),"task_id":"*","scope":"project","kind":"feedback","question":"Operator guidance","context":text,"recommendation":text,"investigated":["Direct operator input"],"impact":{"evidence":["Local operator"]}})).map_err(|e|dm::error(e.to_string()))?;
                        let mut human = p.clone();
                        human.human_question_min_risk = dm::Consultation::Low;
                        let d = store.submit(&plan, request, &actor, "human", &human)?;
                        store.answer(&d.id, d.version, &text, None, &actor, false, &p)?;
                    }
                    Action::Policy(_) | Action::InspectContext(_) => unreachable!(),
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
        let compact = rect.height < 18 || rect.width < 65;
        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Min(1),
                Constraint::Length(if compact { 4 } else { 7 }),
                Constraint::Length(if compact { 3 } else { 6 }),
            ])
            .split(rect);
        let p = self.policy();
        let mut text=format!("Durable decisions: {} | mode: {:?} | pending: {}\nHigher risk threshold grants more autonomy. NeverAsk records agent choices without questions.\n{}\n\n",p.enabled,p.human_question_min_risk,self.pending(),self.message);
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
            if let Some(patch) = &d.request.proposed_patch {
                text.push_str(&format!(
                    "\nProposed graph change (recommended option applies it):\n{}\n",
                    serde_json::to_string_pretty(patch).unwrap_or_default()
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
        let answer = format!(
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
        frame.render_widget(Paragraph::new(if compact{"[F5] Send [Esc] Later [F2] Next\n[Tab] Option [PgUp/Dn] Details"}else{"[F2] Next / history  [Tab] Option  [F3] Advisory [F4] Contexts\n[F5] Send / save  [Enter] Newline  [PgUp/PgDn] Scroll\n[F6] Autonomy mode  [F7] Enable/disable  [F8] New direction\n[Esc] Later / close — never submits an answer"}).wrap(Wrap{trim:false}).block(Block::default().borders(Borders::ALL)),chunks[2]);
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
    fn center(mode: &str) -> Center {
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
