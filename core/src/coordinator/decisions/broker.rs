//! Restartable, explicit human approval through an independently administered broker.
//! No signing key, provider credential or automatic human identity is manufactured.
use super::{store::*, *};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::{
    io::Write,
    path::Path,
    process::{Command, Stdio},
};
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BrokerConfig {
    pub command: Vec<String>,
    #[serde(default = "timeout")]
    pub timeout_seconds: u64,
}
fn timeout() -> u64 {
    30
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ApprovalTicket {
    #[serde(default)]
    pub version: u64,
    pub id: String,
    pub status: String,
    pub intent: OperatorIntent,
    pub challenge: Value,
    pub policy_digest: String,
    pub external_id: String,
    pub approval_url: Option<String>,
    pub created_at: i64,
    pub message: String,
    #[serde(default)]
    pub attestation: Option<Attestation>,
    #[serde(default)]
    pub result: Option<Value>,
}
#[derive(Debug, Deserialize)]
#[serde(tag = "status", rename_all = "snake_case", deny_unknown_fields)]
enum BrokerReply {
    Pending {
        request_id: String,
        approval_url: Option<String>,
    },
    Approved {
        attestation: Attestation,
    },
    Rejected {
        reason: String,
    },
}
fn exchange(
    root: &Path,
    p: &Policy,
    operation: &str,
    ticket: Option<&str>,
    challenge: &Value,
) -> crate::Result<BrokerReply> {
    let config = super::secure::broker_configuration(p)?;
    if config.command.is_empty() || !(1..=120).contains(&config.timeout_seconds) {
        return Err(error(
            "Approval broker requires a protected executable and a 1..120 second deadline",
        ));
    }
    super::secure::protected(std::path::Path::new(&config.command[0]))?;
    let dir = Store::location(root)?.parent().unwrap().join("broker");
    std::fs::create_dir_all(&dir).map_err(|e| error(e.to_string()))?;
    let path = dir.join(format!("{}.json", uuid::Uuid::new_v4()));
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let file = options.open(&path).map_err(|e| error(e.to_string()))?;
    let mut command = Command::new(&config.command[0]);
    command
        .args(&config.command[1..])
        .current_dir(root)
        .env_clear()
        .env("PATH", "/usr/local/bin:/usr/bin:/bin")
        .env("LANG", "C.UTF-8")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        command.process_group(0);
    }
    let data = super::secure::canonical_bytes(
        &json!({"protocol":1,"operation":operation,"request_id":ticket,"challenge":challenge,"action_digest":digest(challenge)}),
    )?;
    if data.len() > p.max_request_bytes {
        return Err(error("Approval challenge exceeds broker request budget"));
    }
    let mut child = command.spawn().map_err(|e| error(e.to_string()))?;
    let mut stdin = child.stdin.take().unwrap();
    let writer = std::thread::spawn(move || stdin.write_all(&data));
    #[cfg(unix)]
    let captured = super::operations::capture_unix(
        &mut child,
        file,
        &path,
        &OperationAdapter {
            timeout_seconds: config.timeout_seconds,
            max_output_bytes: p.max_request_bytes,
            ..Default::default()
        },
    );
    #[cfg(not(unix))]
    let captured: crate::Result<(Option<i32>, String, String)> = {
        let _ = child.kill();
        let _ = child.wait();
        Err(error(
            "Isolated broker capture requires a supported platform",
        ))
    };
    let _ = writer.join();
    let result = (|| {
        let (code, _, _) = captured?;
        if code != Some(0) {
            return Err(error("Independent approver unavailable or exceeded its deadline; no decision was recorded"));
        }
        let bytes = std::fs::read(&path).map_err(|e| error(e.to_string()))?;
        serde_json::from_slice(&bytes)
            .map_err(|e| error(format!("Invalid independent approver response: {e}")))
    })();
    let _ = std::fs::remove_file(path);
    result
}
fn valid_url(url: &str) -> bool {
    url.len() <= 2048
        && !url.chars().any(|c| c.is_control())
        && (url.starts_with("https://")
            || url.starts_with("http://127.0.0.1:")
            || url.starts_with("http://localhost:"))
}
impl Store {
    pub fn approval_tickets(&self) -> crate::Result<Vec<ApprovalTicket>> {
        let mut statement = sql(self.db.prepare(
            "SELECT value FROM metadata WHERE key LIKE 'approval-ticket:%' ORDER BY key",
        ))?;
        let rows = sql(statement.query_map([], |r| r.get::<_, String>(0)))?;
        rows.map(|r| decode(&sql(r)?)).collect()
    }
    pub fn approval_ticket(&self, id: &str) -> crate::Result<ApprovalTicket> {
        decode(&sql(self.db.query_row(
            "SELECT value FROM metadata WHERE key=?1",
            [format!("approval-ticket:{id}")],
            |r| r.get::<_, String>(0),
        ))?)
    }
    fn save_ticket(&mut self, ticket: &mut ApprovalTicket, actor: &str) -> crate::Result<()> {
        use rusqlite::OptionalExtension;
        let _lock = self.mutation_lock()?;
        let tx = sql(self
            .db
            .transaction_with_behavior(rusqlite::TransactionBehavior::Immediate))?;
        let key = format!("approval-ticket:{}", ticket.id);
        let existing: Option<String> = sql(tx
            .query_row("SELECT value FROM metadata WHERE key=?1", [&key], |r| {
                r.get(0)
            })
            .optional())?;
        if let Some(existing) = existing {
            let previous: ApprovalTicket = decode(&existing)?;
            if previous.version != ticket.version {
                return Err(error("Approval workflow changed in another client; refresh its status before retrying"));
            }
            if serde_json::to_value(&previous).ok() == serde_json::to_value(&ticket).ok() {
                return Ok(());
            }
        } else if ticket.version != 0 {
            return Err(error(
                "Approval workflow disappeared; prepare a new request",
            ));
        }
        ticket.version += 1;
        sql(tx.execute("INSERT INTO metadata(key,value) VALUES(?1,?2) ON CONFLICT(key) DO UPDATE SET value=excluded.value",rusqlite::params![key,encode(ticket)?]))?;
        event(
            &tx,
            "project",
            "approval.workflow",
            &json!({"actor":actor,"ticket":ticket}),
        )?;
        sql(tx.commit())?;
        Ok(())
    }
    pub fn begin_approval(
        &mut self,
        intent: OperatorIntent,
        actor: &str,
    ) -> crate::Result<ApprovalTicket> {
        let p = policy(&self.root)?;
        if p.trust_mode != "secure_approval" {
            return Err(error(
                "Independent broker approvals require secure_approval mode",
            ));
        }
        let challenge = self.operator_challenge(&intent)?;
        let _begin = crate::fs_lock::AdvisoryLock::acquire(
            &self.path.with_file_name(format!(
                "approval-begin-{}.lock",
                digest(&challenge).replace(':', "-")
            )),
            std::time::Duration::from_secs(5),
            "prepare independent approval",
        )?;
        if let Some(existing) = self.approval_tickets()?.into_iter().find(|t| {
            t.challenge == challenge
                && matches!(t.status.as_str(), "pending" | "approved" | "applying")
        }) {
            return Ok(existing);
        }
        let reply = exchange(&self.root, &p, "begin", None, &challenge)?;
        if self.operator_challenge(&intent)? != challenge {
            return Err(error("Subject changed while independent approval was being prepared; no action was applied"));
        }
        let mut ticket = ApprovalTicket {
            version: 0,
            id: uuid::Uuid::new_v4().to_string(),
            status: "pending".into(),
            intent,
            challenge,
            policy_digest: digest(&p),
            external_id: String::new(),
            approval_url: None,
            created_at: chrono::Utc::now().timestamp(),
            message: "Authenticate independently and review the exact action before approving"
                .into(),
            attestation: None,
            result: None,
        };
        Self::accept_broker_reply(&self.root, &p, &mut ticket, reply)?;
        self.save_ticket(&mut ticket, actor)?;
        Ok(ticket)
    }
    fn accept_broker_reply(
        root: &Path,
        p: &Policy,
        ticket: &mut ApprovalTicket,
        reply: BrokerReply,
    ) -> crate::Result<()> {
        match reply {
            BrokerReply::Pending {
                request_id,
                approval_url,
            } => {
                if request_id.trim().is_empty()
                    || request_id.len() > 256
                    || approval_url.as_ref().is_some_and(|u| !valid_url(u))
                {
                    return Err(error(
                        "Approver returned an invalid request identity or approval URL",
                    ));
                }
                ticket.external_id = request_id;
                ticket.approval_url = approval_url;
                ticket.status = "pending".into();
            }
            BrokerReply::Approved { attestation } => {
                let action = ticket.challenge["action"]
                    .as_str()
                    .ok_or_else(|| error("Missing approval action"))?;
                super::secure::verify_attestation(
                    root,
                    p,
                    &attestation,
                    action,
                    &digest(&ticket.challenge),
                    chrono::Utc::now().timestamp(),
                )?;
                ticket.attestation = Some(attestation);
                ticket.status = "approved".into();
                ticket.message =
                    "Independent approval verified; apply this exact reviewed action explicitly"
                        .into();
            }
            BrokerReply::Rejected { reason } => {
                ticket.status = "rejected".into();
                ticket.message = reason;
            }
        }
        Ok(())
    }
    pub fn poll_approval(&mut self, id: &str, actor: &str) -> crate::Result<ApprovalTicket> {
        let mut ticket = self.approval_ticket(id)?;
        if ticket.status != "pending" {
            return Ok(ticket);
        }
        let p = policy(&self.root)?;
        if digest(&p) != ticket.policy_digest
            || self.operator_challenge(&ticket.intent).ok().as_ref() != Some(&ticket.challenge)
        {
            ticket.status = "stale".into();
            ticket.message="The request, subject or policy changed. Your draft is preserved; prepare a new approval.".into();
            self.save_ticket(&mut ticket, actor)?;
            return Ok(ticket);
        }
        if chrono::Utc::now()
            .timestamp()
            .saturating_sub(ticket.created_at)
            > 3600
        {
            ticket.status = "expired".into();
            ticket.message = "The external approval request expired; no action was applied".into();
            self.save_ticket(&mut ticket, actor)?;
            return Ok(ticket);
        }
        let reply = exchange(
            &self.root,
            &p,
            "poll",
            Some(&ticket.external_id),
            &ticket.challenge,
        )?;
        Self::accept_broker_reply(&self.root, &p, &mut ticket, reply)?;
        self.save_ticket(&mut ticket, actor)?;
        Ok(ticket)
    }
    pub fn apply_approval(&mut self, id: &str, actor: &str) -> crate::Result<ApprovalTicket> {
        let mut ticket = self.approval_ticket(id)?;
        if ticket.status == "applied" {
            return Ok(ticket);
        }
        if !matches!(ticket.status.as_str(), "approved" | "applying") {
            return Err(error("Independent approval is not ready to apply"));
        }
        let proof = ticket
            .attestation
            .as_ref()
            .ok_or_else(|| error("Missing verified approver attestation"))?;
        if let Some(result) = self.operator_result(&ticket.intent, proof)? {
            ticket.result = Some(result);
            ticket.status = "applied".into();
            ticket.message = "The exact approved action was already durably committed".into();
            self.save_ticket(&mut ticket, actor)?;
            return Ok(ticket);
        }
        if self.operator_challenge(&ticket.intent)? != ticket.challenge {
            return Err(error(
                "Subject changed; the isolated approval cannot authorize another action",
            ));
        }
        ticket.status = "applying".into();
        self.save_ticket(&mut ticket, actor)?;
        let result =
            self.apply_operator(&ticket.intent.clone(), ticket.attestation.as_ref(), actor)?;
        ticket.status = "applied".into();
        ticket.result = Some(result);
        ticket.message =
            "Approved action durably recorded; delivery and compliance remain separate".into();
        self.save_ticket(&mut ticket, actor)?;
        Ok(ticket)
    }
}
