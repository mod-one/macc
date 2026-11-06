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
    condition_description: String,
    condition_task: String,
    condition_path: String,
    condition_digest: String,
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
    evidence_confirmation: Option<String>,
    button: Option<usize>,
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
    ("evidence", "Attach retained evidence"),
    ("upload", "Register an evidence file"),
    ("reuse", "Use a prior direction as fresh scoped guidance"),
];
impl Panel {
    pub fn show(&mut self) {
        self.open = true;
        self.button = None;
        self.evidence_confirmation = None;
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
        let n = if self.mode == "gates" {
            8
        } else if self.mode == "upload" || self.mode == "reuse" {
            3
        } else if self.mode == "revise" {
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
            ("reuse", 0) => &mut draft.text,
            ("reuse", 1) => &mut draft.context,
            ("reuse", 2) => &mut draft.recommendation,
            ("upload", 0) => &mut draft.text,
            ("upload", 1) => &mut draft.context,
            ("upload", 2) => &mut draft.recommendation,
            ("gates", 4) => &mut draft.condition_description,
            ("gates", 5) => &mut draft.condition_task,
            ("gates", 6) => &mut draft.condition_path,
            ("gates", 7) => &mut draft.condition_digest,
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
        if self.mode == "reuse" {
            let source = snapshot["reference_directions"]
                .as_array()
                .and_then(|rows| {
                    rows.iter()
                        .find(|r| r["id"].as_str() == Some(draft.text.trim()))
                })
                .ok_or("Select an available source direction ID")?;
            if draft.context.trim().is_empty() || draft.recommendation.trim().is_empty() {
                return Err("Supply the target task and why this direction applies".into());
            }
            return Ok((dm::OperatorIntent::Feedback {
                plan_id:snapshot["plan"]["id"].as_str().ok_or("Missing plan")?.into(),
                task:draft.context.trim().into(),
                text:format!("Explicit new scoped direction from {} v{} ({}).\nApplicability justification: {}\nExact prior direction:\n{}",source["id"].as_str().unwrap_or_default(),source["version"],source["source"].as_str().unwrap_or_default(),draft.recommendation,source["text"].as_str().unwrap_or_default()),
                advisory:source["advisory"].as_bool().unwrap_or(false),
            },proof));
        }
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
            if !draft.condition_description.trim().is_empty() {
                let optional = |v: String| (!v.trim().is_empty()).then_some(v);
                let condition = dm::ApprovalCondition {
                    id: "gate-condition".into(),
                    description: draft.condition_description,
                    task_id: optional(draft.condition_task),
                    source_path: optional(draft.condition_path),
                    source_digest: optional(draft.condition_digest),
                };
                return Ok((
                    dm::OperatorIntent::ConditionalGateVote {
                        plan_id: snapshot["plan"]["id"]
                            .as_str()
                            .ok_or("Missing plan")?
                            .into(),
                        task: g["task_id"].as_str().ok_or("Missing gate")?.into(),
                        record,
                        conditions: vec![condition],
                    },
                    proof,
                ));
            }
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
    fn buttons(&self) -> Vec<(&'static str, KeyCode)> {
        match self.mode.as_str() {
            "menu" | "analytics" => vec![],
            "evidence" => vec![
                ("Attach reference", KeyCode::F(5)),
                ("Review / confirm AI sharing", KeyCode::F(6)),
            ],
            "approvals" => vec![
                ("Check approval", KeyCode::F(4)),
                ("Apply approved action", KeyCode::F(5)),
            ],
            "operations" => vec![
                ("Send", KeyCode::F(5)),
                ("Run trusted probe", KeyCode::F(4)),
                ("Change outcome", KeyCode::F(11)),
                ("Prepare challenge", KeyCode::F(12)),
            ],
            "gates" => vec![
                ("Send", KeyCode::F(5)),
                ("Change response", KeyCode::F(11)),
                ("Prepare challenge", KeyCode::F(12)),
            ],
            "upload" => vec![("Register evidence", KeyCode::F(5))],
            _ => vec![
                ("Send", KeyCode::F(5)),
                ("Prepare challenge", KeyCode::F(12)),
            ],
        }
    }
    pub fn navigation(&self, snapshot: &Value) -> String {
        let buttons = self.buttons();
        let manual = snapshot["policy"]["trust_mode"] == "secure_approval"
            && snapshot["approval_broker_available"] != true;
        let count = self.field_count(manual);
        let values = buttons
            .iter()
            .enumerate()
            .map(|(i, (label, _))| {
                format!(
                    "{}[{label}]",
                    if self.button == Some(i) { "›" } else { " " }
                )
            })
            .collect::<Vec<_>>()
            .join("  ");
        format!("{values}\nTab/Shift+Tab: fields and buttons · Enter: activate button · Esc: back\nInput {} / {}", self.field + 1, count)
    }
    pub fn keyboard(
        &mut self,
        key: KeyCode,
        d: Option<&dm::Decision>,
        snapshot: &Value,
    ) -> Option<Action> {
        if self.mode == "menu" || self.mode == "analytics" {
            return self.key_event(key, d, snapshot);
        }
        let buttons = self.buttons();
        let manual = snapshot["policy"]["trust_mode"] == "secure_approval"
            && snapshot["approval_broker_available"] != true;
        let count = if matches!(self.mode.as_str(), "evidence" | "approvals") {
            0
        } else {
            self.field_count(manual)
        };
        if matches!(key, KeyCode::Tab | KeyCode::BackTab) {
            self.evidence_confirmation = None;
            let total = count + buttons.len();
            let pos = self
                .button
                .map(|b| count + b)
                .unwrap_or(self.field.min(count));
            let next =
                (pos + if key == KeyCode::Tab {
                    1
                } else {
                    total.saturating_sub(1)
                }) % total.max(1);
            self.button = (next >= count).then_some(next.saturating_sub(count));
            if next < count {
                self.field = next;
            }
            return None;
        }
        if key == KeyCode::Esc {
            self.button = None;
        }
        if let Some(button) = self.button {
            if matches!(key, KeyCode::Enter | KeyCode::Char(' ')) {
                return self.key_event(buttons.get(button)?.1, d, snapshot);
            }
            if matches!(key, KeyCode::Char(_) | KeyCode::Backspace) {
                return None;
            }
        }
        // List views have no text editor: Enter uses their first explicit action.
        if count == 0 && key == KeyCode::Enter {
            return self.key_event(buttons.first()?.1, d, snapshot);
        }
        self.key_event(key, d, snapshot)
    }
    pub fn key_event(
        &mut self,
        key: KeyCode,
        d: Option<&dm::Decision>,
        snapshot: &Value,
    ) -> Option<Action> {
        if key == KeyCode::Esc {
            self.evidence_confirmation = None;
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
        if self.mode == "evidence" {
            let rows = snapshot["artifacts"]
                .as_array()
                .cloned()
                .unwrap_or_default();
            if rows.is_empty() {
                self.notice = "No evidence yet. Home > Register an evidence file first.".into();
                return None;
            }
            self.index %= rows.len();
            if key == KeyCode::F(6) {
                let a = &rows[self.index];
                let id = a["id"].as_str().unwrap_or_default().to_owned();
                if self.evidence_confirmation.as_ref() == Some(&id) {
                    self.evidence_confirmation = None;
                    return Some(Action::Operator {
                        intent: dm::OperatorIntent::EvidencePermission {
                            id,
                            plan_id: snapshot["plan"]["id"].as_str().unwrap_or_default().into(),
                            version: a["permission_version"].as_u64().unwrap_or(0),
                            share: a["share_content"] != true,
                        },
                        proof: None,
                        challenge: false,
                    });
                }
                self.evidence_confirmation = Some(id);
                self.notice="Explicit disclosure: contents may be sent to configured external AI providers for this plan. activate Review / confirm AI sharing again to confirm sharing/revocation; Esc cancels. Do not disclose credentials.".into();
                return None;
            }
            self.evidence_confirmation = None;
            match key {
                KeyCode::Up => self.index = (self.index + rows.len() - 1) % rows.len(),
                KeyCode::Down => self.index = (self.index + 1) % rows.len(),
                KeyCode::F(5) => {
                    let a = &rows[self.index];
                    return Some(Action::SetArtifact(dm::ArtifactRef {
                        id: a["id"].as_str().unwrap_or_default().into(),
                        digest: a["digest"].as_str().unwrap_or_default().into(),
                    }));
                }
                _ => {}
            }
            return None;
        }
        if self.mode == "upload" && key == KeyCode::F(5) {
            let draft = self.drafts.get(&self.key(d)).cloned().unwrap_or_default();
            let path = PathBuf::from(draft.text.trim());
            let label = if draft.context.trim().is_empty() {
                path.file_name()
                    .map(|n| n.to_string_lossy().into_owned())
                    .unwrap_or_default()
            } else {
                draft.context
            };
            return Some(Action::ArtifactAdd {
                path,
                label,
                media_type: if draft.recommendation.trim().is_empty() {
                    "application/octet-stream".into()
                } else {
                    draft.recommendation
                },
            });
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
                        self.notice="Open the independent approval page, then Check approval checks its result; pending is not consent".into();
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
            let mut text="Independent approval requests — ↑/↓ select, Check approval checks status, Apply approved action applies approved action\n".to_owned();
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
                text.push_str("No pending independent approval. Signed/manual approval remains available through Prepare challenge.\n");
            }
            return (text, self.notice.clone());
        }
        if self.mode == "evidence" {
            let rows = snapshot["artifacts"]
                .as_array()
                .cloned()
                .unwrap_or_default();
            let mut text =
                "Retained evidence — ↑/↓ selects, Enter attaches reference; Tab to Review AI sharing for disclosure, Esc cancels\n"
                    .to_string();
            for (i, a) in rows.iter().enumerate() {
                text.push_str(&format!(
                    "{} {} ({} bytes)\n{}\n",
                    if i == self.index { ">" } else { " " },
                    a["label"],
                    a["size"],
                    a["digest"]
                ));
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
                ("reuse", 0) => "Prior direction ID (listed above)",
                ("reuse", 1) => "Target task in current plan",
                ("reuse", 2) => "Why this direction applies",
                ("upload", 0) => "Evidence file path",
                ("upload", 1) => "Readable evidence label",
                ("upload", 2) => "Media type (default application/octet-stream)",
                ("gates", 4) => "Condition description",
                ("gates", 5) => "Verification task ID",
                ("gates", 6) => "Condition evidence path",
                ("gates", 7) => "Expected SHA-256",
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
        let mut text=format!("{title}\nTab moves between input fields and action buttons; Enter activates a focused button.\n{}\n",self.notice);
        if self.mode == "reuse" {
            text.push_str("Creates new human guidance, without copying votes, conditional approvals, operation permissions or private attachments. Later revocation of the old record does not revoke this independently authored direction.\n");
            if let Some(rows) = snapshot["reference_directions"].as_array() {
                for row in rows {
                    text.push_str(&format!(
                        "{} v{} · {} · {}\n{}\n",
                        row["id"], row["version"], row["source"], row["question"], row["text"]
                    ));
                }
            }
        }
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
                text.push_str(&format!("Gate {} — {}\nSubject {} at {}\nRequired roles: {}\nResponse: {} (use Change response button); ↑/↓ selects gate\n{}\n",g["task_id"],g["question"],g["subject_task"],g["subject_revision"],g["required_roles"],["approve","reject","request changes"][self.outcome],g["description"]));
            }
        }
        if self.mode == "operations" {
            if let Some(op) = &self.operation {
                text.push_str(&format!("Operation {} v{} — {}\nOutcome to record: {} (use Change outcome / Run trusted probe buttons)\n",op["operation_id"],op["version"],op["state"],["final failure","verified success","verified keyed retry"][self.outcome]));
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

#[cfg(test)]
mod interaction_tests {
    use super::*;
    #[test]
    fn evidence_sharing_requires_fresh_explicit_confirmation_after_escape() {
        let snapshot = json!({"plan":{"id":"p"},"artifacts":[{"id":"file","digest":"sha256:123","share_content":false,"permission_version":0}]});
        let mut panel = Panel {
            open: true,
            mode: "evidence".into(),
            ..Default::default()
        };
        assert!(panel.key_event(KeyCode::F(6), None, &snapshot).is_none());
        assert!(panel
            .view(None, &snapshot)
            .1
            .contains("external AI providers"));
        assert!(panel.key_event(KeyCode::Esc, None, &snapshot).is_none());
        panel.mode = "evidence".into();
        assert!(panel.key_event(KeyCode::F(6), None, &snapshot).is_none());
        let Some(Action::Operator {
            intent:
                dm::OperatorIntent::EvidencePermission {
                    id,
                    plan_id,
                    share,
                    version,
                },
            ..
        }) = panel.key_event(KeyCode::F(6), None, &snapshot)
        else {
            panic!("Expected explicit evidence consent")
        };
        assert_eq!(id, "file");
        assert_eq!(plan_id, "p");
        assert!(share);
        assert_eq!(version, 0);
    }
    #[test]
    fn prior_direction_creates_fresh_task_guidance_with_exact_source_and_justification() {
        let snapshot = json!({"plan":{"id":"new-plan"},"reference_directions":[{"id":"old","version":3,"source":"old.prd.json","text":"Preserve SQLite.\nDo not discard user work.","advisory":false}]});
        let mut panel = Panel {
            open: true,
            mode: "reuse".into(),
            ..Default::default()
        };
        panel.init(None);
        let key = panel.key(None);
        let draft = panel.drafts.get_mut(&key).unwrap();
        draft.text = "old".into();
        draft.context = "NEW-A".into();
        assert!(panel.intent(None, &snapshot).is_err());
        panel.drafts.get_mut(&key).unwrap().recommendation = "Same storage contract".into();
        let (
            dm::OperatorIntent::Feedback {
                plan_id,
                task,
                text,
                advisory,
            },
            _,
        ) = panel.intent(None, &snapshot).unwrap()
        else {
            panic!("Expected new scoped feedback")
        };
        assert_eq!(plan_id, "new-plan");
        assert_eq!(task, "NEW-A");
        assert!(!advisory);
        assert!(text.contains("old v3"));
        assert!(text.contains("Same storage contract"));
        assert!(text.contains("Preserve SQLite.\nDo not discard user work."));
    }
}
