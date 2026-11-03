//! Keyboard-driven lifecycle, operation and independent approval actions.
use super::*;
#[derive(Default, Clone, serde::Serialize, serde::Deserialize)]
#[serde(default)]
struct ActionDraft {
    actor: String,
    role: String,
    created: String,
    text: String,
    context: String,
    recommendation: String,
    expires: String,
    proof: String,
    key: String,
}
#[derive(Default)]
pub(super) struct Panel {
    pub open: bool,
    mode: String,
    index: usize,
    field: usize,
    drafts: BTreeMap<String, ActionDraft>,
    notice: String,
    operation: Option<Value>,
    gate: Option<Value>,
    outcome: usize,
}
const MODES: &[(&str, &str)] = &[
    ("revise", "Revise / propose replacement"),
    ("withdraw", "Withdraw unresolved request"),
    ("revoke", "Revoke binding decision"),
    ("effects", "Apply reviewed task-plan file"),
    ("approvals", "Independent approval requests"),
    ("gates", "Approval gates and roles"),
    ("operations", "Reconcile external operation"),
    ("analytics", "Decision diagnostics and related requests"),
    ("no_graph", "Answer: current task plan remains valid"),
    ("replan", "Answer: prepare a new task plan"),
];
impl Panel {
    pub fn show(&mut self) {
        self.open = true;
        self.mode = "menu".into();
        self.index = 0;
        self.field = 0;
    }
    pub fn save(&self, root: &std::path::Path) {
        if let Ok(bytes) = serde_json::to_vec(&self.drafts) {
            let _ = macc_core::atomic_write(
                &macc_core::ProjectPaths::from_root(root),
                &root.join(".macc/cache/decision-action-drafts.json"),
                &bytes,
            );
        }
    }
    pub fn load(&mut self, root: &std::path::Path) {
        let file = root.join(".macc/cache/decision-action-drafts.json");
        if std::fs::metadata(&file).is_ok_and(|m| m.len() < 1048576) {
            if let Ok(bytes) = std::fs::read(file) {
                self.drafts = serde_json::from_slice(&bytes).unwrap_or_default();
            }
        }
    }
    fn key(&self, d: Option<&dm::Decision>) -> String {
        if self.mode == "gates" {
            if let Some(g) = &self.gate {
                return format!("gate:{}@{}", g["task_id"], g["subject_revision"]);
            }
        }
        if self.mode == "operations" {
            if let Some(op) = &self.operation {
                return format!("operation:{}@{}", op["operation_id"], op["version"]);
            }
        }
        format!("{}:{}", d.map(Center::key).unwrap_or_default(), self.mode)
    }
    fn tickets(snapshot: &Value) -> Vec<Value> {
        snapshot["approval_tickets"]
            .as_array()
            .cloned()
            .unwrap_or_default()
            .into_iter()
            .filter(|t| {
                !matches!(
                    t["status"].as_str(),
                    Some("applied" | "rejected" | "stale" | "expired")
                )
            })
            .collect()
    }
    fn init(&mut self, d: Option<&dm::Decision>) {
        let key = self.key(d);
        self.drafts.entry(key).or_insert_with(|| {
            let mut draft = ActionDraft {
                key: uuid_key(),
                created: chrono::Utc::now().to_rfc3339(),
                ..Default::default()
            };
            if let Some(d) = d {
                if self.mode == "revise" {
                    draft.text = d.request.question.clone();
                    draft.context = d.request.context.clone();
                    draft.recommendation = d.request.recommendation.clone();
                }
            }
            draft
        });
    }
    fn field_count(&self, manual_proof: bool) -> usize {
        let n = if self.mode == "revise" || self.mode == "gates" {
            4
        } else {
            1
        };
        n + usize::from(manual_proof)
    }
    fn edit<'a>(&self, draft: &'a mut ActionDraft, manual_proof: bool) -> &'a mut String {
        if manual_proof && self.field == self.field_count(manual_proof) - 1 {
            return &mut draft.proof;
        }
        match (self.mode.as_str(), self.field) {
            ("gates", 0) => &mut draft.actor,
            ("gates", 1) => &mut draft.role,
            ("gates", 2) => &mut draft.text,
            ("gates", 3) => &mut draft.context,
            ("revise", 1) => &mut draft.context,
            ("revise", 2) => &mut draft.recommendation,
            ("revise", 3) => &mut draft.expires,
            _ => &mut draft.text,
        }
    }
    fn intent(
        &self,
        d: Option<&dm::Decision>,
        snapshot: &Value,
    ) -> Result<(dm::OperatorIntent, Option<dm::Attestation>), String> {
        let key = self.key(d);
        let draft = self.drafts.get(&key).cloned().unwrap_or_default();
        let proof = if draft.proof.trim().is_empty() {
            None
        } else {
            Some(
                serde_json::from_str(&draft.proof)
                    .map_err(|e| format!("Invalid approver attestation: {e}"))?,
            )
        };
        if self.mode == "gates" {
            let g = self.gate.as_ref().ok_or("No gate selected")?;
            let record = macc_core::coordinator::model::ApprovalRecord {
                actor: draft.actor,
                role: draft.role,
                decision: match self.outcome {
                    1 => macc_core::coordinator::model::ApprovalDecision::Rejected,
                    2 => macc_core::coordinator::model::ApprovalDecision::ChangesRequested,
                    _ => macc_core::coordinator::model::ApprovalDecision::Approved,
                },
                revision: g["subject_revision"].as_str().map(str::to_owned),
                evidence: (!draft.text.trim().is_empty()).then_some(draft.text),
                reason: (!draft.context.trim().is_empty()).then_some(draft.context),
                recorded_at: draft.created,
            };
            return Ok((
                dm::OperatorIntent::GateVote {
                    plan_id: snapshot["plan"]["id"]
                        .as_str()
                        .ok_or("Missing plan")?
                        .into(),
                    task: g["task_id"].as_str().ok_or("Missing gate identity")?.into(),
                    record,
                },
                proof,
            ));
        }
        if self.mode == "operations" {
            let op = self.operation.as_ref().ok_or("No operation selected")?;
            let state = match self.outcome {
                1 => dm::OperationState::Succeeded,
                2 => dm::OperationState::Reserved,
                _ => dm::OperationState::Failed,
            };
            return Ok((
                dm::OperatorIntent::Reconcile {
                    plan_id: snapshot["plan"]["id"]
                        .as_str()
                        .ok_or("Missing plan")?
                        .into(),
                    operation: op["operation_id"]
                        .as_str()
                        .ok_or("Missing operation")?
                        .into(),
                    version: op["version"].as_u64().ok_or("Missing version")?,
                    state,
                    evidence: draft.text,
                },
                proof,
            ));
        }
        let d = d.ok_or("No request selected")?;
        let intent = match self.mode.as_str() {
            "revise" => {
                let mut request = d.request.clone();
                request.idempotency_key = format!("revision-{}", draft.key);
                request.question = draft.text;
                request.context = draft.context;
                request.recommendation = draft.recommendation;
                request.revision_of = None;
                request.supersedes.clear();
                request.subject_revision = None;
                request.expires_at = if draft.expires.trim().is_empty() {
                    None
                } else {
                    Some(draft.expires)
                };
                request.proposed_patch = None;
                for option in &mut request.options {
                    option.requires_replan |= option.proposed_patch.is_some();
                    option.proposed_patch = None;
                }
                dm::OperatorIntent::Revise {
                    id: d.id.clone(),
                    version: d.version,
                    request,
                }
            }
            "withdraw" => dm::OperatorIntent::Withdraw {
                id: d.id.clone(),
                version: d.version,
                reason: draft.text,
            },
            "revoke" => dm::OperatorIntent::Revoke {
                id: d.id.clone(),
                version: d.version,
                reason: draft.text,
            },
            "effects" => {
                let path = std::path::Path::new(draft.text.trim());
                if std::fs::metadata(path).map_err(|e| e.to_string())?.len() > 262144 {
                    return Err("Prepared task-plan file exceeds budget".into());
                }
                let patch =
                    serde_json::from_slice(&std::fs::read(path).map_err(|e| e.to_string())?)
                        .map_err(|e| e.to_string())?;
                dm::OperatorIntent::Effects {
                    id: d.id.clone(),
                    version: d.version,
                    patch,
                }
            }
            _ => return Err("Select an action first".into()),
        };
        Ok((intent, proof))
    }
    fn gate_defaults(&mut self, d: Option<&dm::Decision>, snapshot: &Value) {
        if self.mode != "gates" {
            return;
        }
        let name = self.key(d);
        let draft = self.drafts.entry(name).or_default();
        if draft.actor.is_empty() {
            draft.actor = snapshot["operator_identity"].as_str().unwrap_or("").into();
        }
        if draft.role.is_empty() {
            draft.role = self
                .gate
                .as_ref()
                .and_then(|g| {
                    g["missing_roles"]
                        .as_array()
                        .and_then(|v| v.first())
                        .or_else(|| g["required_roles"].as_array().and_then(|v| v.first()))
                })
                .and_then(Value::as_str)
                .unwrap_or("")
                .into();
        }
    }
    pub fn prepare_gate(&mut self, decision: &crate::approval::Decision, snapshot: &Value) {
        self.show();
        self.mode = "gates".into();
        self.gate = snapshot["pending_approvals"]
            .as_array()
            .and_then(|v| v.iter().find(|g| g["task_id"] == decision.task_id))
            .cloned();
        self.init(None);
        self.gate_defaults(None, snapshot);
        let key = self.key(None);
        if let Some(draft) = self.drafts.get_mut(&key) {
            draft.role = decision.role.clone();
            draft.text = decision.evidence.clone().unwrap_or_default();
            draft.context = decision.reason.clone().unwrap_or_default();
        }
        self.outcome = match decision.decision {
            macc_core::coordinator::model::ApprovalDecision::Approved => 0,
            macc_core::coordinator::model::ApprovalDecision::Rejected => 1,
            _ => 2,
        };
    }
    pub fn key_event(
        &mut self,
        key: KeyCode,
        d: Option<&dm::Decision>,
        snapshot: &Value,
    ) -> Option<Action> {
        if key == KeyCode::Esc {
            if self.mode == "menu" {
                self.open = false;
            } else {
                self.mode = "menu".into();
                self.index = 0;
            }
            return None;
        }
        if self.mode == "menu" {
            match key {
                KeyCode::Up => self.index = (self.index + MODES.len() - 1) % MODES.len(),
                KeyCode::Down => self.index = (self.index + 1) % MODES.len(),
                KeyCode::Enter => {
                    let mode = MODES[self.index].0;
                    if mode == "no_graph" {
                        self.open = false;
                        return Some(Action::SetEffects(dm::AnswerEffects::NoGraphChange));
                    }
                    if mode == "replan" {
                        self.open = false;
                        return Some(Action::SetEffects(dm::AnswerEffects::Replan));
                    }
                    self.mode = mode.into();
                    self.index = 0;
                    self.field = 0;
                    self.gate = snapshot["pending_approvals"]
                        .as_array()
                        .and_then(|v| v.iter().find(|g| g["awaiting_decision"] == true))
                        .cloned();
                    self.operation = snapshot["operations"]
                        .as_array()
                        .and_then(|v| v.first())
                        .cloned();
                    self.init(d);
                    self.gate_defaults(d, snapshot);
                }
                _ => {}
            }
            return None;
        }
        if self.mode == "gates" {
            if matches!(key, KeyCode::Up | KeyCode::Down) {
                let gates = snapshot["pending_approvals"]
                    .as_array()
                    .cloned()
                    .unwrap_or_default()
                    .into_iter()
                    .filter(|g| g["awaiting_decision"] == true)
                    .collect::<Vec<_>>();
                if !gates.is_empty() {
                    self.index = if key == KeyCode::Up {
                        (self.index + gates.len() - 1) % gates.len()
                    } else {
                        (self.index + 1) % gates.len()
                    };
                    self.gate = Some(gates[self.index].clone());
                    self.init(d);
                    self.gate_defaults(d, snapshot);
                }
                return None;
            }
            if key == KeyCode::F(11) {
                self.outcome = (self.outcome + 1) % 3;
                return None;
            }
        }
        if self.mode == "analytics" {
            return None;
        }
        if self.mode == "approvals" {
            let tickets = Self::tickets(snapshot);
            if !tickets.is_empty() {
                self.index %= tickets.len();
                match key {
                    KeyCode::Up => self.index = (self.index + tickets.len() - 1) % tickets.len(),
                    KeyCode::Down => self.index = (self.index + 1) % tickets.len(),
                    KeyCode::F(4) => {
                        return Some(Action::Approval {
                            ticket: tickets[self.index]["id"]
                                .as_str()
                                .unwrap_or_default()
                                .into(),
                            apply: false,
                        })
                    }
                    KeyCode::F(5) => {
                        if matches!(
                            tickets[self.index]["status"].as_str(),
                            Some("approved" | "applying")
                        ) {
                            return Some(Action::Approval {
                                ticket: tickets[self.index]["id"]
                                    .as_str()
                                    .unwrap_or_default()
                                    .into(),
                                apply: true,
                            });
                        }
                        self.notice="Open the independent approval page, then F4 checks its result; pending is not consent".into();
                    }
                    _ => {}
                }
            }
            return None;
        }
        if self.mode == "operations" {
            if matches!(key, KeyCode::Up | KeyCode::Down) {
                if let Some(ops) = snapshot["operations"].as_array() {
                    if !ops.is_empty() {
                        self.index = if key == KeyCode::Up {
                            (self.index + ops.len() - 1) % ops.len()
                        } else {
                            (self.index + 1) % ops.len()
                        };
                        self.operation = Some(ops[self.index].clone());
                        self.init(d);
                    }
                }
                return None;
            }
            if key == KeyCode::F(4) {
                if let Some(op) = &self.operation {
                    return Some(Action::Probe {
                        operation: op["operation_id"].as_str().unwrap_or_default().into(),
                        version: op["version"].as_u64().unwrap_or(0),
                    });
                }
            }
            if key == KeyCode::F(11) {
                self.outcome = (self.outcome + 1) % 3;
                return None;
            }
        }
        let manual = snapshot["policy"]["trust_mode"] == "secure_approval"
            && snapshot["approval_broker_available"] != true;
        if key == KeyCode::F(9) {
            self.field = (self.field + 1) % self.field_count(manual);
            return None;
        }
        if key == KeyCode::F(5) || key == KeyCode::F(12) {
            match self.intent(d, snapshot) {
                Ok((intent, proof)) => {
                    return Some(Action::Operator {
                        intent,
                        proof,
                        challenge: key == KeyCode::F(12),
                    })
                }
                Err(e) => self.notice = e,
            }
            return None;
        }
        let name = self.key(d);
        let mut draft = self.drafts.remove(&name).unwrap_or_default();
        let value = self.edit(&mut draft, manual);
        match key {
            KeyCode::Char(c) => value.push(c),
            KeyCode::Enter => value.push('\n'),
            KeyCode::Backspace => {
                value.pop();
            }
            _ => {}
        }
        self.drafts.insert(name, draft);
        None
    }
    pub fn view(&self, d: Option<&dm::Decision>, snapshot: &Value) -> (String, String) {
        if self.mode == "menu" {
            let mut text =
                "Decision actions — ↑/↓ then Enter; Esc returns without applying anything\n"
                    .to_string();
            for (i, (_, label)) in MODES.iter().enumerate() {
                text.push_str(&format!(
                    "{} {label}\n",
                    if i == self.index { ">" } else { " " }
                ));
            }
            return (text, self.notice.clone());
        }
        if self.mode == "approvals" {
            let tickets = Self::tickets(snapshot);
            let mut text="Independent approval requests — ↑/↓ select, F4 checks status, F5 applies approved action\n".to_owned();
            for (i, ticket) in tickets.iter().enumerate() {
                text.push_str(&format!(
                    "{} {}: {}\n{}\n{}\n",
                    if i == self.index { ">" } else { " " },
                    ticket["intent"]["action"].as_str().unwrap_or("action"),
                    ticket["status"].as_str().unwrap_or("unknown"),
                    ticket["message"].as_str().unwrap_or(""),
                    ticket["approval_url"].as_str().unwrap_or("")
                ));
            }
            if let Some(ticket) = tickets.get(self.index) {
                text.push_str(&format!("\nExact submitted action (later draft edits do not alter it):\n{}\nPgUp/PgDn scroll; Esc returns.\n",serde_json::to_string_pretty(&ticket["intent"]).unwrap_or_default()));
            }
            if tickets.is_empty() {
                text.push_str("No pending independent approval. Signed/manual approval remains available through F12.\n");
            }
            return (text, self.notice.clone());
        }
        if self.mode == "analytics" {
            return (format!("Decision diagnostics (measured observations only)\n{}\nRelated requests are advisory, never automatic consent.\n{}",serde_json::to_string_pretty(&snapshot["analytics"]).unwrap_or_default(),serde_json::to_string_pretty(&snapshot["similar_requests"]).unwrap_or_default()),"Esc returns to actions".into());
        }
        let draft = self.drafts.get(&self.key(d)).cloned().unwrap_or_default();
        let manual = snapshot["policy"]["trust_mode"] == "secure_approval"
            && snapshot["approval_broker_available"] != true;
        let (mut copy, _) = (draft.clone(), 0);
        let value = self.edit(&mut copy, manual).clone();
        let label = if manual && self.field == self.field_count(manual) - 1 {
            "External approver attestation (advanced fallback)"
        } else {
            match (self.mode.as_str(), self.field) {
                ("gates", 0) => "Independent approver identity",
                ("gates", 1) => "Approval role",
                ("gates", 2) => "Evidence",
                ("gates", 3) => "Reason",
                ("revise", 0) => "Revised question",
                ("revise", 1) => "Revised context",
                ("revise", 2) => "Revised recommendation",
                ("revise", 3) => "Expiry RFC3339 (optional)",
                ("effects", _) => "Reviewed task-plan file path",
                ("operations", _) => "Concrete external outcome evidence",
                _ => "Reason",
            }
        };
        let title = MODES
            .iter()
            .find(|(mode, _)| *mode == self.mode)
            .map(|(_, title)| *title)
            .unwrap_or("Action");
        let mut text=format!("{title}\nF9 changes input field; F5 submits explicitly; F12 prepares a challenge.\n{}\n",self.notice);
        if let Some(d) = d {
            text.push_str(&format!(
                "Request {} v{}: {}\nExisting answer: {}\n",
                d.id,
                d.version,
                d.request.question,
                d.resolution
                    .as_ref()
                    .map(|r| r.text.as_str())
                    .unwrap_or("none")
            ));
        }
        if self.mode == "gates" {
            if let Some(g) = &self.gate {
                text.push_str(&format!("Gate {} — {}\nSubject {} at {}\nRequired roles: {}\nResponse: {} (F11 changes); ↑/↓ selects gate\n{}\n",g["task_id"],g["question"],g["subject_task"],g["subject_revision"],g["required_roles"],["approve","reject","request changes"][self.outcome],g["description"]));
            }
        }
        if self.mode == "operations" {
            if let Some(op) = &self.operation {
                text.push_str(&format!("Operation {} v{} — {}\nOutcome to record: {} (F11 changes; F4 runs trusted probe)\n",op["operation_id"],op["version"],op["state"],["final failure","verified success","verified keyed retry"][self.outcome]));
            }
        }
        (text, format!("{label}\n{value}"))
    }
}
fn uuid_key() -> String {
    format!(
        "{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos()
    )
}
