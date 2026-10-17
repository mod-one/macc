//! Deterministic phase observations are proposals, never learned facts or delivery.
use super::*;
use rusqlite::{Connection, OptionalExtension};
use serde_json::{json, Value};
use store::checkpoints::Checkpoint;
/// Called within checkpoint persistence: no new worker, invocation or scheduling loop.
pub(crate) fn phase_in(
    db: &Connection,
    root: &Path,
    c: &Checkpoint,
    outcome: &str,
) -> Result<Option<String>> {
    let p = policy::load(root)?;
    if !p.memory.enabled
        || !p.memory.phase_extraction_enabled
        || !super::ready(db)?
        || !["failure", "changes_requested"].contains(&outcome)
    {
        return Ok(None);
    }
    let id = format!(
        "phase-{}",
        digest(
            &serde_json::to_vec(&json!([
                c.project_id,
                c.task_id,
                c.role,
                c.phase,
                c.source_revision,
                c.worktree_content_digest,
                outcome,
                c.review_objections
            ]))
            .map_err(|e| error(e.to_string()))?
        )
    );
    let exists: bool = sql(db.query_row(
        "SELECT EXISTS(SELECT 1 FROM memories WHERE id=?1)",
        [&id],
        |r| r.get(0),
    ))?;
    if exists {
        return Ok(Some(id));
    } // Do not overwrite operator review or duplicate unchanged failures.
    let mut evidence=vec![memory::Evidence{binding:None,kind:"review".into(),reference:format!("checkpoint:{}",c.id),sha256:Some(digest(&serde_json::to_vec(c).map_err(|e|error(e.to_string()))?)),revision:Some(c.source_revision.clone()),summary:"Normalized phase outcome from a checksummed checkpoint; no cause or resolution inferred".into()}];
    for path in c.changed_files.iter().take(3) {
        if let Ok((_, raw)) = paths::source(Path::new(&c.worktree), path, 1048576) {
            if std::str::from_utf8(&raw).is_ok_and(|s| paths::safe_text(s).is_ok()) {
                evidence.push(memory::Evidence {
                    binding: None,
                    kind: "file".into(),
                    reference: path.clone(),
                    sha256: Some(digest(&raw)),
                    revision: Some(c.source_revision.clone()),
                    summary: "Observed file snapshot; not proof of a root cause or successful fix"
                        .into(),
                });
            }
        }
    }
    let proposal=memory::Proposal{kind:"failed_attempt".into(),title:format!("{}: {} requires investigation",c.task_id,c.phase).chars().take(160).collect(),body:format!("Phase {} returned {} for task {}. Investigate the referenced checkpoint and source evidence before recording a durable lesson. No root cause, successful test, correction or task completion is inferred.",c.phase,outcome,c.task_id),scope:"task".into(),scope_ref:Some(c.task_id.clone()),confidence:0.6,evidence,tags:vec!["phase_observation".into(),format!("outcome:{outcome}")],source_task:Some(c.task_id.clone()),source_revision:Some(c.source_revision.clone())};
    if let Err(e) = agent::clean(&proposal.body)
        .and_then(|_| memory::validate(&proposal, p.memory.max_body_chars))
    {
        schema::audit(
            db,
            "memory_extraction_skipped",
            &c.id,
            json!({"reason":"candidate validation failed; phase evidence retained"}),
        )?;
        tracing::warn!(checkpoint=%c.id,error=%e,"Phase memory proposal skipped");
        return Ok(None);
    }
    memory::propose_in(db, &c.project_id, &proposal, &id, p.memory.max_body_chars)?;
    schema::audit(
        db,
        "phase_memory_proposed",
        &id,
        json!({"checkpoint":c.id,"task":c.task_id,"phase":c.phase,"outcome":outcome,"status":"proposed"}),
    )?;
    Ok(Some(id))
}
/// Explicit operator backfill reads the recorded outcome, never an asserted verdict.
pub fn from_checkpoint(root: &Path, id: &str) -> Result<Value> {
    let _guard = store::lock(root)?;
    let (mut db, a) = database(root)?;
    let tx = sql(db.transaction())?;
    let cp = store::checkpoints::load(&tx, id)?;
    if cp.project_id != a.project_id {
        return Err(error("Foreign extraction checkpoint"));
    }
    let state: String = sql(tx.query_row(
        "SELECT validation_state FROM task_checkpoints WHERE id=?1",
        [id],
        |r| r.get(0),
    ))?;
    if !["verified", "reconciled"].contains(&state.as_str()) {
        return Err(error("Reconcile checkpoint before extracting candidates"));
    }
    let outcome:Option<String>=sql(tx.query_row("SELECT json_extract(payload_json,'$.outcome') FROM session_events WHERE event_type='checkpoint_saved' AND json_extract(payload_json,'$.checkpoint')=?1 ORDER BY rowid DESC LIMIT 1",[id],|r|r.get(0)).optional())?;
    let outcome =
        outcome.ok_or_else(|| error("No normalized invocation outcome recorded for checkpoint"))?;
    let candidate = phase_in(&tx, root, &cp, &outcome)?;
    sql(tx.commit())?;
    Ok(
        json!({"checkpoint":id,"candidate_id":candidate,"status":if candidate.is_some(){"proposed_or_previously_reviewed"}else{"not_extracted"},"notice":"No task state, tests, memory activation or coordinator restart is authorized by extraction"}),
    )
}
