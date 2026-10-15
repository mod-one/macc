use super::*;
use rusqlite::OptionalExtension;
pub(crate) const SCHEMA:&str="
CREATE TABLE IF NOT EXISTS memories(id TEXT PRIMARY KEY,project_id TEXT NOT NULL,scope TEXT NOT NULL,scope_ref TEXT,status TEXT NOT NULL,kind TEXT NOT NULL,record_json TEXT NOT NULL,checksum TEXT NOT NULL,updated_at INTEGER NOT NULL);
CREATE INDEX IF NOT EXISTS memory_scope_state ON memories(project_id,scope,scope_ref,status);
CREATE TABLE IF NOT EXISTS memory_search_projection(id INTEGER PRIMARY KEY,memory_id TEXT UNIQUE NOT NULL REFERENCES memories(id),title TEXT NOT NULL,body TEXT NOT NULL,refs TEXT NOT NULL);
CREATE VIRTUAL TABLE IF NOT EXISTS memory_fts USING fts5(title,body,refs,content='memory_search_projection',content_rowid='id');
CREATE TRIGGER IF NOT EXISTS memory_search_insert AFTER INSERT ON memory_search_projection BEGIN INSERT INTO memory_fts(rowid,title,body,refs) VALUES(new.id,new.title,new.body,new.refs); END;
CREATE TRIGGER IF NOT EXISTS memory_search_delete AFTER DELETE ON memory_search_projection BEGIN INSERT INTO memory_fts(memory_fts,rowid,title,body,refs) VALUES('delete',old.id,old.title,old.body,old.refs); END;
CREATE TRIGGER IF NOT EXISTS memory_search_update AFTER UPDATE ON memory_search_projection BEGIN INSERT INTO memory_fts(memory_fts,rowid,title,body,refs) VALUES('delete',old.id,old.title,old.body,old.refs); INSERT INTO memory_fts(rowid,title,body,refs) VALUES(new.id,new.title,new.body,new.refs); END;
CREATE TABLE IF NOT EXISTS memory_evidence(id TEXT PRIMARY KEY,memory_id TEXT NOT NULL REFERENCES memories(id),evidence_json TEXT NOT NULL);
CREATE TABLE IF NOT EXISTS memory_conflicts(id TEXT PRIMARY KEY,left_id TEXT NOT NULL REFERENCES memories(id),right_id TEXT NOT NULL REFERENCES memories(id),reason TEXT NOT NULL,status TEXT NOT NULL);
CREATE TABLE IF NOT EXISTS o2_audit(id TEXT PRIMARY KEY,kind TEXT NOT NULL,record_id TEXT,at INTEGER NOT NULL,payload_json TEXT NOT NULL);
CREATE TABLE IF NOT EXISTS context_content_cache(fingerprint TEXT PRIMARY KEY,content_json TEXT NOT NULL,checksum TEXT NOT NULL);
CREATE TABLE IF NOT EXISTS context_bundles(id TEXT PRIMARY KEY,task_id TEXT NOT NULL,phase TEXT NOT NULL,manifest_json TEXT NOT NULL,created_at INTEGER NOT NULL,checkpoint_id TEXT NOT NULL);
CREATE INDEX IF NOT EXISTS context_by_checkpoint ON context_bundles(checkpoint_id);
CREATE TABLE IF NOT EXISTS provider_receipts(provider_id TEXT PRIMARY KEY,receipt_json TEXT NOT NULL);
CREATE TABLE IF NOT EXISTS provider_plans(id TEXT PRIMARY KEY,plan_json TEXT NOT NULL,applied INTEGER NOT NULL DEFAULT 0,result_json TEXT);
CREATE TABLE IF NOT EXISTS context_usage_items(invocation_id TEXT NOT NULL REFERENCES invocation_usage(invocation_id),source_kind TEXT NOT NULL,source_ref TEXT NOT NULL,estimated_tokens INTEGER NOT NULL,PRIMARY KEY(invocation_id,source_kind,source_ref));
CREATE TABLE IF NOT EXISTS compaction_pressure(session_id TEXT PRIMARY KEY,generation INTEGER NOT NULL,context_tokens INTEGER,window INTEGER,compactions INTEGER);
CREATE TABLE IF NOT EXISTS compaction_probes(tool_id TEXT PRIMARY KEY,probe_json TEXT NOT NULL);
INSERT OR IGNORE INTO session_meta VALUES('o2_schema_version','1');";
pub fn ready(db: &rusqlite::Connection) -> Result<bool> {
    let v: Option<String> = sql(db
        .query_row(
            "SELECT value FROM session_meta WHERE key='o2_schema_version'",
            [],
            |r| r.get(0),
        )
        .optional())?;
    match v.as_deref() {
        None => Ok(false),
        Some("1") => Ok(true),
        _ => Err(error("Unsupported O2 schema version")),
    }
}
pub fn upgrade(root: &Path, apply: bool, expected: Option<&str>) -> Result<store::MigrationReport> {
    let _guard = store::lock(root)?;
    store::require_stopped(root)?;
    let (mut db, a) = store::active_database(root)?;
    if ready(&db)? {
        return Err(error("O2 schema already installed"));
    }
    let owners: i64 = sql(db.query_row("SELECT count(*) FROM session_leases", [], |r| r.get(0)))?;
    if owners != 0 {
        return Err(error("Recover outstanding leases before O2 upgrade"));
    }
    let sha = super::store::fingerprint::connection(&db)?;
    let mut report = store::MigrationReport {
        direction: "sqlite_o2".into(),
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
        return Err(error("O2 source changed; preview again"));
    }
    let backup = root.join(format!(
        ".macc/state/pre-o2-{}.sqlite",
        uuid::Uuid::new_v4()
    ));
    store::snapshot_database(&db, &backup)?;
    let tx = sql(db.transaction())?;
    sql(tx.execute_batch(SCHEMA))?;
    audit(
        &tx,
        "o2_schema_upgraded",
        &a.project_id,
        serde_json::json!({"version":1}),
    )?;
    sql(tx.commit())?;
    report.applied = true;
    report.backup = Some(backup.display().to_string());
    store::write(
        root,
        &root.join(format!(
            ".macc/reports/o2-upgrade-{}.json",
            uuid::Uuid::new_v4()
        )),
        &serde_json::to_vec_pretty(&report).map_err(|e| error(e.to_string()))?,
    )?;
    Ok(report)
}
pub(crate) fn audit(
    db: &rusqlite::Connection,
    kind: &str,
    id: &str,
    payload: serde_json::Value,
) -> Result<()> {
    sql(db.execute(
        "INSERT INTO o2_audit VALUES(?1,?2,?3,?4,?5)",
        rusqlite::params![
            uuid::Uuid::new_v4().to_string(),
            kind,
            id,
            now(),
            payload.to_string()
        ],
    ))?;
    Ok(())
}
