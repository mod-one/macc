use super::*;
use rusqlite::{Connection, OptionalExtension};
pub(super) fn memory_counts(db: &Connection, project: &str) -> Result<BTreeMap<String, u64>> {
    let mut s = sql(db.prepare(
        "SELECT status,count(*) FROM memories WHERE project_id=?1 GROUP BY status ORDER BY status",
    ))?;
    let rows = sql(s.query_map([project], |r| Ok((r.get(0)?, r.get(1)?))))?;
    rows.map(sql).collect()
}
pub(super) fn providers(db: &Connection) -> Result<Vec<Provider>> {
    let mut s = sql(db.prepare("SELECT provider_id,json_extract(receipt_json,'$.state'),json_extract(receipt_json,'$.version') FROM provider_receipts ORDER BY provider_id LIMIT 50"))?;
    let rows = sql(s.query_map([], |r| {
        Ok(Provider {
            id: r.get(0)?,
            state: r.get(1)?,
            version: r.get(2)?,
        })
    }))?;
    rows.map(sql).collect()
}
fn count(db: &Connection, statement: &str, task: &str, project: &str) -> Result<u64> {
    sql(db.query_row(statement, [task, project], |r| r.get(0)))
}
pub(super) fn tasks(db: &Connection, project: &str, q: &Query) -> Result<Vec<Task>> {
    let mut s = sql(db.prepare(
        "WITH task_ids AS (
        SELECT task_id FROM logical_sessions WHERE project_id=?4
        UNION SELECT task_id FROM task_checkpoints WHERE project_id=?4
        UNION SELECT task_id FROM context_bundles WHERE json_extract(manifest_json,'$.project_id')=?4
        UNION SELECT json_extract(record_json,'$.proposal.source_task') FROM memories WHERE project_id=?4
        UNION SELECT scope_ref FROM memories WHERE scope='task' AND project_id=?4)
        SELECT task_id FROM task_ids WHERE task_id IS NOT NULL AND task_id!=''
        AND (?1 IS NULL OR task_id=?1) AND (?2 IS NULL OR task_id>?2)
        ORDER BY task_id LIMIT ?3",
    ))?;
    let ids = sql(s.query_map(
        rusqlite::params![q.task_id, q.after, q.limit + 1, project],
        |r| r.get::<_, String>(0),
    ))?
    .map(sql)
    .collect::<Result<Vec<_>>>()?;
    ids.into_iter().map(|id| task(db, project, id)).collect()
}
fn task(db: &Connection, project: &str, id: String) -> Result<Task> {
    let context:Option<context::Bundle> = sql(db.query_row(
        "SELECT manifest_json FROM context_bundles WHERE task_id=?1 AND json_extract(manifest_json,'$.project_id')=?2 ORDER BY created_at DESC,rowid DESC LIMIT 1",
        [id.as_str(),project], |r|r.get::<_,String>(0)).optional())?
        .map(|raw|store::json(raw.as_bytes())).transpose()?;
    let checkpoint = sql(db.query_row(
        "SELECT id,phase,validation_state FROM task_checkpoints WHERE task_id=?1 AND project_id=?2 ORDER BY created_at DESC,rowid DESC LIMIT 1",
        [id.as_str(),project], |r|Ok(CheckpointRef{id:r.get(0)?,phase:r.get(1)?,validation_state:r.get(2)?})).optional())?;
    Ok(Task {
        sessions: count(db,"SELECT count(*) FROM logical_sessions WHERE task_id=?1 AND project_id=?2",&id,project)?,
        lease_records: count(db,"SELECT count(*) FROM session_leases l JOIN logical_sessions s ON s.id=l.logical_session_id WHERE s.task_id=?1 AND s.project_id=?2",&id,project)?,
        checkpoints: count(db,"SELECT count(*) FROM task_checkpoints WHERE task_id=?1 AND project_id=?2",&id,project)?,
        needs_reconciliation: count(db,"SELECT count(*) FROM (SELECT validation_state,row_number() OVER (PARTITION BY role ORDER BY created_at DESC,rowid DESC) AS position FROM task_checkpoints WHERE task_id=?1 AND project_id=?2) WHERE position=1 AND validation_state='needs_reconciliation'",&id,project)?,
        contexts: count(db,"SELECT count(*) FROM context_bundles WHERE task_id=?1 AND json_extract(manifest_json,'$.project_id')=?2",&id,project)?,
        proposals: count(db,"SELECT count(*) FROM memories WHERE status='proposed' AND project_id=?2 AND (json_extract(record_json,'$.proposal.source_task')=?1 OR (scope='task' AND scope_ref=?1))",&id,project)?,
        task_id: id,
        latest_context: context.map(|c| ContextRef {
            id:c.id,phase:c.phase,optional_estimated_tokens:c.optional_estimated_tokens,
            optional_limit_tokens:c.optional_limit_tokens,measurement_quality:c.measurement_quality,
        }),
        latest_checkpoint: checkpoint,
    })
}
