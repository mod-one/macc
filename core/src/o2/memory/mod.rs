//! Durable claims, evidence and review, never task completion authority.
mod model;
mod review;
pub(crate) mod search;
use super::*;
pub use model::*;
pub use review::{conflict, review};
use rusqlite::params;
pub use search::{fts_rebuild, search};
pub(super) fn validate(p: &Proposal, max: u64) -> Result<()> {
    if ![
        "constraint",
        "discovery",
        "decision",
        "gotcha",
        "api_contract",
        "failed_attempt",
        "performance_note",
        "security_note",
        "migration_note",
        "provider_note",
        "merge_note",
    ]
    .contains(&p.kind.as_str())
        || !["project", "feature", "task", "worktree", "tool", "user"].contains(&p.scope.as_str())
    {
        return Err(error("Invalid memory kind/scope"));
    }
    if p.title.trim().is_empty()
        || p.title.chars().count() > 160
        || p.body.trim().is_empty()
        || p.body.chars().count() as u64 > max
        || !p.confidence.is_finite()
        || !(0.0..=1.0).contains(&p.confidence)
        || p.tags.len() > 20
        || p.tags.iter().any(|t| t.len() > 80)
        || p.evidence.len() > 20
        || p.scope_ref.as_ref().is_some_and(|s| s.len() > 512)
    {
        return Err(error(
            "Invalid/bounded memory content, confidence or references",
        ));
    }
    if (p.scope == "project") != p.scope_ref.is_none() {
        return Err(error(
            "Non-project scopes require a scope reference; project scope has none",
        ));
    }
    if p.body.contains("MACC_TASK_RESULT:")
        || p.body
            .lines()
            .filter(|l| l.starts_with("20") && l.contains(" ERROR "))
            .count()
            > 2
    {
        return Err(error(
            "Raw logs are not durable memory; propose a concise sourced lesson",
        ));
    }
    for e in &p.evidence {
        if !["file", "prd", "test", "commit", "review", "human_approval"].contains(&e.kind.as_str())
            || e.reference.len() > 512
            || e.reference.trim().is_empty()
            || e.summary.chars().count() > 500
        {
            return Err(error("Invalid bounded evidence"));
        }
    }
    for value in [&p.title, &p.body]
        .into_iter()
        .chain(p.tags.iter())
        .chain(p.scope_ref.iter())
    {
        super::paths::safe_text(value)?;
    }
    for e in &p.evidence {
        super::paths::safe_text(&e.reference)?;
        super::paths::safe_text(&e.summary)?;
    }
    super::paths::safe_text(&serde_json::to_string(p).map_err(|e| error(e.to_string()))?)
}
pub fn propose(root: &Path, p: &Proposal) -> Result<Memory> {
    let policy = super::policy::load(root)?;
    if !policy.memory.enabled {
        return Err(error("Enable Memory Ledger before proposing facts"));
    }
    validate(p, policy.memory.max_body_chars)?;
    let _guard = store::lock(root)?;
    let (mut db, a) = database(root)?;
    let tx = sql(db.transaction())?;
    let m = propose_in(
        &tx,
        &a.project_id,
        p,
        &uuid::Uuid::new_v4().to_string(),
        policy.memory.max_body_chars,
    )?;
    sql(tx.commit())?;
    Ok(m)
}
pub(super) fn propose_in(
    db: &rusqlite::Connection,
    project: &str,
    p: &Proposal,
    id: &str,
    max: u64,
) -> Result<Memory> {
    validate(p, max)?;
    let m = Memory {
        id: id.into(),
        project_id: project.into(),
        status: "proposed".into(),
        proposal: p.clone(),
        created_at: now(),
        updated_at: now(),
        verified_revision: None,
        reviewed_by: None,
        staleness_reason: None,
    };
    save(db, &m)?;
    super::schema::audit(
        db,
        "memory_proposed",
        &m.id,
        serde_json::json!({"scope":p.scope,"task":p.source_task}),
    )?;
    Ok(m)
}
pub(super) fn load(db: &rusqlite::Connection, id: &str, project: &str) -> Result<Memory> {
    let (raw, sha): (String, String) = sql(db.query_row(
        "SELECT record_json,checksum FROM memories WHERE id=?1 AND project_id=?2",
        params![id, project],
        |r| Ok((r.get(0)?, r.get(1)?)),
    ))?;
    if digest(raw.as_bytes()) != sha {
        return Err(error("Memory checksum mismatch"));
    }
    let m: Memory = store::json(raw.as_bytes())?;
    if m.project_id != project || m.id != id {
        return Err(error("Foreign memory identity"));
    }
    Ok(m)
}
pub fn get(root: &Path, id: &str, access: &Access) -> Result<Hit> {
    let _guard = store::lock(root)?;
    let (db, a) = database(root)?;
    let m = load(&db, id, &a.project_id)?;
    if !in_scope(&m.proposal, access) {
        return Err(error("Memory outside requested scope"));
    }
    hit(m, 0, "explicit scoped inspection")
}
pub(super) fn in_scope(p: &Proposal, a: &Access) -> bool {
    match p.scope.as_str() {
        "project" => true,
        "task" => p.scope_ref == a.task_id,
        "feature" => p.scope_ref == a.feature,
        "worktree" => p.scope_ref == a.worktree,
        "tool" => p.scope_ref == a.tool_id,
        "user" => p.scope_ref == a.user_id,
        _ => false,
    }
}
pub(super) fn hit(m: Memory, score: i64, reason: &str) -> Result<Hit> {
    let checksum = digest(
        serde_json::to_string(&m)
            .map_err(|e| error(e.to_string()))?
            .as_bytes(),
    );
    Ok(Hit {
        memory: m,
        checksum,
        rank_reason: reason.into(),
        score,
    })
}
pub(super) fn save(db: &rusqlite::Connection, m: &Memory) -> Result<()> {
    let raw = serde_json::to_string(m).map_err(|e| error(e.to_string()))?;
    sql(db.execute("INSERT INTO memories VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9) ON CONFLICT(id) DO UPDATE SET status=excluded.status,record_json=excluded.record_json,checksum=excluded.checksum,updated_at=excluded.updated_at",params![m.id,m.project_id,m.proposal.scope,m.proposal.scope_ref,m.status,m.proposal.kind,raw,digest(raw.as_bytes()),m.updated_at]))?;
    let refs = m
        .proposal
        .tags
        .iter()
        .cloned()
        .chain(m.proposal.evidence.iter().map(|e| e.reference.clone()))
        .collect::<Vec<_>>()
        .join(" ");
    sql(db.execute("INSERT INTO memory_search_projection(memory_id,title,body,refs) VALUES(?1,?2,?3,?4) ON CONFLICT(memory_id) DO UPDATE SET title=excluded.title,body=excluded.body,refs=excluded.refs",params![m.id,m.proposal.title,m.proposal.body,refs]))?;
    sql(db.execute("DELETE FROM memory_evidence WHERE memory_id=?1", [&m.id]))?;
    for (i, e) in m.proposal.evidence.iter().enumerate() {
        sql(db.execute(
            "INSERT INTO memory_evidence VALUES(?1,?2,?3)",
            params![
                format!("{}:{i}", m.id),
                m.id,
                serde_json::to_string(e).map_err(|e| error(e.to_string()))?
            ],
        ))?;
    }
    Ok(())
}
pub(super) fn freshness(worktree: &Path, m: &Memory, ttl: u64) -> Result<()> {
    if m.proposal.kind == "provider_note" && now() - m.updated_at > (ttl * 86400) as i64 {
        return Err(error(
            "Provider note expired; verify against installed version",
        ));
    }
    for e in &m.proposal.evidence {
        if ["file", "prd", "test"].contains(&e.kind.as_str()) {
            let (_, data) = super::paths::source(worktree, &e.reference, 1048576)?;
            if e.sha256.as_deref() != Some(&digest(&data)) {
                return Err(error(format!("Evidence changed: {}", e.reference)));
            }
        }
    }
    Ok(())
}

mod pull;
pub use pull::{pull, read, PullPage, ReadPage};
