//! Invocation provenance belongs to the existing canonical usage transaction.
use super::*;
fn environment(db: &rusqlite::Connection) -> Result<String> {
    let mut q = sql(db.prepare(
        "SELECT id,checksum FROM memories WHERE status='active' ORDER BY project_id,id LIMIT 10001",
    ))?;
    let memories = sql(q.query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?))))?
        .map(sql)
        .collect::<Result<Vec<_>>>()?;
    let mut q = sql(db.prepare(
        "SELECT provider_id,receipt_json FROM provider_receipts ORDER BY provider_id LIMIT 101",
    ))?;
    let providers = sql(q.query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?))))?
        .map(sql)
        .collect::<Result<Vec<_>>>()?;
    if memories.len() > 10000 || providers.len() > 100 {
        return Err(error("Trial provenance corpus exceeds its bound"));
    }
    hash(&(memories, providers))
}
/// Called inside the existing usage transaction. Session/tool adaptation stays upstream.
pub(crate) fn capture(
    root: &Path,
    db: &rusqlite::Connection,
    invocation: &str,
    identity: &Value,
    generation: i64,
) -> Result<()> {
    if !schema::ready(db)? {
        return Ok(());
    }
    let Some(trial) = trials::open(db)? else {
        return Ok(());
    };
    let task = identity["task_id"].as_str().unwrap_or_default();
    let Some((_, arm)) = trial.plan.cases.iter().find_map(|c| {
        ["baseline", "candidate"]
            .into_iter()
            .find(|arm| c.execution_task(arm) == task)
            .map(|arm| (c, arm))
    }) else {
        return Ok(());
    };
    let policy = policy::load(root)?;
    if !policy.context_engine.tuning_enabled {
        return Ok(());
    }
    let expected = if arm == "baseline" {
        &trial.baseline
    } else {
        &trial.candidate
    };
    if &policy != expected {
        return Err(error("Trial execution identity requires its pre-registered arm policy; restore the exact arm or cancel"));
    }
    let target = providers::native_worktree(root, identity["execution_root"].as_str())?;
    let stamp = Stamp {
        invocation: invocation.into(),
        task: task.into(),
        arm: arm.into(),
        policy: hash(&policy)?,
        revision: crate::git::head_commit(&target)?,
        clean: crate::git::status_porcelain(&target)?.trim().is_empty(),
        execution: hash(&identity["execution_config"])?,
        environment: environment(db)?,
        generation,
    };
    schema::audit(
        db,
        "tuning_invocation",
        &trial.id,
        serde_json::to_value(stamp).map_err(|e| error(e.to_string()))?,
    )
}
pub(crate) fn finish(root: &Path, db: &rusqlite::Connection, id: &str) -> Result<()> {
    if !schema::ready(db)? {
        return Ok(());
    }
    let exists:bool=sql(db.query_row("SELECT EXISTS(SELECT 1 FROM o2_audit WHERE kind='tuning_invocation' AND json_extract(payload_json,'$.invocation')=?1)",[id],|r|r.get(0)))?;
    if exists {
        schema::audit(
            db,
            "tuning_finish",
            id,
            json!({"policy":hash(&policy::load(root)?)?}),
        )?;
    }
    Ok(())
}
