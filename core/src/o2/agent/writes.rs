//! Fenced, idempotent proposal-only writes. No lifecycle transition is exported.
use super::*;
use rusqlite::{Connection, OptionalExtension, TransactionBehavior};
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AgentProposal {
    pub request_id: String,
    pub kind: String,
    pub title: String,
    pub body: String,
    pub confidence: f64,
    pub evidence_paths: Vec<String>,
    #[serde(default)]
    pub tags: Vec<String>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Challenge {
    pub request_id: String,
    pub id: String,
    pub reason: String,
}
fn write(
    root: &Path,
    c: &Credentials,
    request_id: &str,
    make: impl FnOnce(&Connection, &Scope) -> Result<memory::Proposal>,
) -> Result<Value> {
    if request_id.is_empty() || request_id.len() > 64 {
        return Err(error("A bounded idempotency request_id is required"));
    }
    clean(request_id)?;
    let _guard = store::lock(root)?;
    let (mut db, _) = database(root)?;
    let tx = sql(db.transaction_with_behavior(TransactionBehavior::Immediate))?;
    let scope = authorize(root, &tx, c)?;
    let p = enabled(root)?;
    if !p.memory.agent_proposals_allowed {
        return Err(error("Agent proposals are disabled"));
    }
    let proposal = make(&tx, &scope)?;
    let payload = digest(&serde_json::to_vec(&proposal).map_err(|e| error(e.to_string()))?);
    let id = format!(
        "agent-{}",
        digest(format!("{}:{}:{request_id}", c.session_id, c.generation).as_bytes())
    );
    let previous: Option<String> = sql(tx
        .query_row(
            "SELECT payload_json FROM o2_audit WHERE kind='agent_memory_proposed' AND record_id=?1",
            [&id],
            |r| r.get(0),
        )
        .optional())?;
    if let Some(raw) = previous {
        let old: Value = store::json(raw.as_bytes())?;
        if old["payload_sha256"] != payload {
            return Err(error(
                "Proposal request_id was reused with different content",
            ));
        }
        let m = memory::load(&tx, &id, &scope.project)?;
        return Ok(json!({"id":m.id,"status":m.status,"replayed":true}));
    }
    let count:i64=sql(tx.query_row("SELECT count(*) FROM o2_audit WHERE kind='agent_memory_proposed' AND json_extract(payload_json,'$.session_id')=?1",[&c.session_id],|r|r.get(0)))?;
    if count >= p.memory.mcp_max_proposals_per_session as i64 {
        return Err(error(
            "Agent proposal limit reached for this logical session",
        ));
    }
    let m = memory::propose_in(&tx, &scope.project, &proposal, &id, p.memory.max_body_chars)?;
    super::super::schema::audit(
        &tx,
        "agent_memory_proposed",
        &id,
        json!({"session_id":c.session_id,"generation":c.generation,"task":scope.task,"role":scope.role,"payload_sha256":payload}),
    )?;
    sql(tx.commit())?;
    Ok(json!({"id":m.id,"status":"proposed","replayed":false}))
}
pub fn propose(root: &Path, c: &Credentials, request: AgentProposal) -> Result<Value> {
    clean(&request.title)?;
    clean(&request.body)?;
    for tag in &request.tags {
        clean(tag)?;
    }
    if request.evidence_paths.is_empty() || request.evidence_paths.len() > 10 {
        return Err(error("One to ten source paths are required"));
    }
    write(root, c, &request.request_id, |_, scope| {
        let mut evidence = Vec::new();
        for path in &request.evidence_paths {
            clean(path)?;
            let (_, raw) = paths::source(&scope.worktree, path, 1048576)?;
            clean(std::str::from_utf8(&raw).map_err(|e| error(e.to_string()))?)?;
            evidence.push(memory::Evidence{binding:None,kind:"file".into(),reference:path.clone(),sha256:Some(digest(&raw)),revision:None,summary:"Agent-supplied assertion; exact source bytes captured; operator review required".into()});
        }
        Ok(memory::Proposal {
            kind: request.kind,
            title: request.title,
            body: request.body,
            scope: "task".into(),
            scope_ref: Some(scope.task.clone()),
            confidence: request.confidence,
            evidence,
            tags: request.tags,
            source_task: Some(scope.task.clone()),
            source_revision: Some(crate::git::head_commit(&scope.worktree)?),
        })
    })
}
pub fn challenge(root: &Path, c: &Credentials, request: Challenge) -> Result<Value> {
    clean(&request.reason)?;
    if request.reason.trim().is_empty() || request.reason.chars().count() > 500 {
        return Err(error("A bounded challenge reason is required"));
    }
    write(root, c, &request.request_id, |db, scope| {
        let m = super::reads::approved(db, root, scope, &request.id)?;
        Ok(memory::Proposal {
            kind: "gotcha".into(),
            title: format!("Challenge to memory {}", m.memory.id),
            body: request.reason,
            scope: "task".into(),
            scope_ref: Some(scope.task.clone()),
            confidence: 0.6,
            evidence: vec![memory::Evidence {
                binding: None,
                kind: "review".into(),
                reference: format!("memory:{}", m.memory.id),
                sha256: Some(m.checksum),
                revision: None,
                summary:
                    "Unverified agent challenge; target remains unchanged until operator review"
                        .into(),
            }],
            tags: vec!["agent_challenge".into(), format!("target:{}", m.memory.id)],
            source_task: Some(scope.task.clone()),
            source_revision: None,
        })
    })
}
