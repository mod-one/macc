use super::super::*;
use rusqlite::OptionalExtension;
use std::path::Path;
pub(super) const SCHEMA:&str="
CREATE TABLE IF NOT EXISTS invocation_usage(
 invocation_id TEXT PRIMARY KEY,session_id TEXT NOT NULL,generation INTEGER NOT NULL,lease_hash TEXT NOT NULL,
 plan_json TEXT NOT NULL,record_json TEXT NOT NULL,receipt TEXT,status TEXT NOT NULL,started_at INTEGER NOT NULL,
 task_id TEXT NOT NULL,tool_id TEXT NOT NULL,phase TEXT NOT NULL,currency TEXT,cost_quality TEXT NOT NULL,
 input_uncached INTEGER,cached_read INTEGER,cached_write INTEGER,output INTEGER,reasoning INTEGER,cost INTEGER);
CREATE UNIQUE INDEX IF NOT EXISTS invocation_usage_generation ON invocation_usage(session_id,generation);
CREATE INDEX IF NOT EXISTS usage_task ON invocation_usage(task_id,started_at DESC);
CREATE TABLE IF NOT EXISTS usage_watermarks(scope TEXT PRIMARY KEY,reported_json TEXT NOT NULL);
INSERT OR IGNORE INTO session_meta VALUES('usage_schema_version','1');";
pub(crate) fn ready(db: &rusqlite::Connection) -> Result<bool> {
    let v: Option<String> = sql(db
        .query_row(
            "SELECT value FROM session_meta WHERE key='usage_schema_version'",
            [],
            |r| r.get(0),
        )
        .optional())?;
    match v.as_deref() {
        None => Ok(false),
        Some("1") => Ok(true),
        _ => Err(error("Unsupported usage schema version")),
    }
}
pub fn upgrade(root: &Path, apply: bool, expected: Option<&str>) -> Result<MigrationReport> {
    let _guard = lock(root)?;
    migration::stopped(root)?;
    let (mut db, a) = db::active(root)?;
    if ready(&db)? {
        return Err(error("Usage schema already installed"));
    }
    let active: i64 = sql(db.query_row("SELECT count(*) FROM session_leases", [], |r| r.get(0)))?;
    if active != 0 {
        return Err(error(
            "Stop runners and recover outstanding leases before usage upgrade",
        ));
    }
    let state:String=sql(db.query_row("SELECT coalesce(group_concat(id || ':' || generation || ':' || status),'') FROM (SELECT * FROM logical_sessions ORDER BY id)",[],|r|r.get(0)))?;
    let sha = digest(format!("{}:{state}:usage-v1", a.project_id).as_bytes());
    let mut report = MigrationReport {
        direction: "sqlite_usage".into(),
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
        return Err(error("Usage upgrade source changed; preview again"));
    }
    let backup = root.join(format!(
        ".macc/state/pre-usage-{}.sqlite",
        uuid::Uuid::new_v4()
    ));
    db::snapshot(&db, &backup)?;
    let tx = sql(db.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate))?;
    sql(tx.execute_batch(SCHEMA))?;
    db::event(
        &tx,
        "",
        "usage_schema_upgraded",
        serde_json::json!({"version":1}),
    )?;
    sql(tx.commit())?;
    report.backup = Some(backup.display().to_string());
    report.applied = true;
    write(
        root,
        &root.join(format!(
            ".macc/reports/usage-upgrade-{}.json",
            uuid::Uuid::new_v4()
        )),
        &serde_json::to_vec_pretty(&report).map_err(|e| error(e.to_string()))?,
    )?;
    Ok(report)
}
