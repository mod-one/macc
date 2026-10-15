//! Additive checkpoint schema requires a reviewed backup on existing SQL projects.
use super::super::*;
use rusqlite::{params, OptionalExtension};
use std::path::Path;
pub(super) const SCHEMA: &str = "
CREATE TABLE IF NOT EXISTS task_checkpoints(
 id TEXT PRIMARY KEY, project_id TEXT NOT NULL,task_id TEXT NOT NULL,role TEXT NOT NULL,tool_id TEXT NOT NULL,
 phase TEXT NOT NULL,worktree_id TEXT NOT NULL,content_json TEXT NOT NULL,content_sha256 TEXT NOT NULL,
 validation_state TEXT NOT NULL,reason TEXT NOT NULL,created_at INTEGER NOT NULL);
CREATE INDEX IF NOT EXISTS checkpoint_task ON task_checkpoints(project_id,task_id,role,created_at DESC);
INSERT OR IGNORE INTO session_meta VALUES('checkpoint_schema_version','1');";
pub(crate) fn ready(db: &rusqlite::Connection) -> Result<bool> {
    let version: Option<String> = sql(db
        .query_row(
            "SELECT value FROM session_meta WHERE key='checkpoint_schema_version'",
            [],
            |r| r.get(0),
        )
        .optional())?;
    match version.as_deref() {
        None => Ok(false),
        Some("1") => Ok(true),
        _ => Err(error("Unsupported checkpoint schema version")),
    }
}
pub fn upgrade(root: &Path, apply: bool, expected: Option<&str>) -> Result<MigrationReport> {
    let _guard = lock(root)?;
    super::super::migration::stopped(root)?;
    let (mut db, a) = db::active(root)?;
    if ready(&db)? {
        return Err(error("Checkpoint schema is already installed"));
    }
    let active: i64 = sql(db.query_row("SELECT count(*) FROM session_leases", [], |r| r.get(0)))?;
    if active != 0 {
        return Err(error(
            "Stop runners and recover outstanding leases before upgrading",
        ));
    }
    let state: String = sql(db.query_row("SELECT coalesce(group_concat(id || ':' || generation || ':' || status),'') FROM (SELECT * FROM logical_sessions ORDER BY id)",[],|r|r.get(0)))?;
    let sha = digest(format!("{}:{state}:checkpoint-v1", a.project_id).as_bytes());
    let mut report = MigrationReport {
        direction: "sqlite_checkpoints".into(),
        source_sha256: sha,
        imported: 0,
        quarantined: 0,
        backup: None,
        applied: false,
    };
    if !apply {
        return Ok(report);
    }
    if expected != Some(report.source_sha256.as_str()) {
        return Err(error("Upgrade source changed; preview again"));
    }
    let backup = root.join(format!(
        ".macc/state/pre-checkpoints-{}.sqlite",
        uuid::Uuid::new_v4()
    ));
    db::snapshot(&db, &backup)?;
    let tx = sql(db.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate))?;
    sql(tx.execute_batch(SCHEMA))?;
    db::event(
        &tx,
        "",
        "checkpoint_schema_upgraded",
        serde_json::json!({"version":1}),
    )?;
    sql(tx.commit())?;
    report.backup = Some(backup.display().to_string());
    report.applied = true;
    write(
        root,
        &root.join(format!(
            ".macc/reports/checkpoint-upgrade-{}.json",
            uuid::Uuid::new_v4()
        )),
        &serde_json::to_vec_pretty(&report).map_err(|e| error(e.to_string()))?,
    )?;
    Ok(report)
}
pub(super) fn insert(db: &rusqlite::Connection, c: &super::Checkpoint, limit: u64) -> Result<()> {
    let raw = serde_json::to_string(c).map_err(|e| error(e.to_string()))?;
    if raw.len() as u64 > limit {
        return Err(error(
            "Checkpoint exceeds configured size; mandatory criteria were not truncated",
        ));
    }
    sql(db.execute(
        "INSERT INTO task_checkpoints VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,'verified',?10,?11)",
        params![
            c.id,
            c.project_id,
            c.task_id,
            c.role,
            c.tool_id,
            c.phase,
            c.worktree,
            raw,
            digest(raw.as_bytes()),
            c.reason,
            now()
        ],
    ))?;
    db::event(
        db,
        &c.id,
        "checkpoint_created",
        serde_json::json!({"task":c.task_id,"phase":c.phase,"reason":c.reason}),
    )
}
