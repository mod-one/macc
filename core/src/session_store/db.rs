use super::*;
use rusqlite::{Connection, OpenFlags};
use std::path::Path;
pub(super) const SCHEMA: &str = r#"
CREATE TABLE IF NOT EXISTS session_meta(key TEXT PRIMARY KEY,value TEXT NOT NULL);
CREATE TABLE IF NOT EXISTS logical_sessions(
 id TEXT PRIMARY KEY, project_id TEXT NOT NULL,task_id TEXT NOT NULL,role TEXT NOT NULL,tool_id TEXT NOT NULL,
 model_id TEXT,config_fingerprint TEXT NOT NULL,environment_fingerprint TEXT NOT NULL,
 identity_json TEXT NOT NULL, provider_session_id TEXT,lineage_id TEXT NOT NULL,status TEXT NOT NULL,
 generation INTEGER NOT NULL DEFAULT 0,use_count INTEGER NOT NULL DEFAULT 0,
 created_at INTEGER NOT NULL,updated_at INTEGER NOT NULL,last_verified_at INTEGER);
CREATE UNIQUE INDEX IF NOT EXISTS session_provider_unique ON logical_sessions(tool_id,provider_session_id)
 WHERE provider_session_id IS NOT NULL;
CREATE INDEX IF NOT EXISTS session_owner ON logical_sessions(project_id,task_id,role,tool_id,status);
CREATE TABLE IF NOT EXISTS session_leases(logical_session_id TEXT PRIMARY KEY REFERENCES logical_sessions(id),
 lease_token TEXT NOT NULL,fencing_counter INTEGER NOT NULL,owner_process_id INTEGER NOT NULL,
 process_identity TEXT NOT NULL,owner_run_id TEXT NOT NULL,owner_worktree_id TEXT NOT NULL,
 heartbeat_at INTEGER NOT NULL,expires_at INTEGER NOT NULL);
CREATE TABLE IF NOT EXISTS session_requests(request_id TEXT PRIMARY KEY,logical_session_id TEXT NOT NULL REFERENCES logical_sessions(id),generation INTEGER NOT NULL);
CREATE TABLE IF NOT EXISTS session_events(id TEXT PRIMARY KEY,logical_session_id TEXT NOT NULL,
 event_type TEXT NOT NULL,payload_json TEXT NOT NULL,occurred_at INTEGER NOT NULL);
CREATE INDEX IF NOT EXISTS session_event_order ON session_events(logical_session_id,occurred_at);
"#;
pub(super) fn open(root: &Path, initialize: bool) -> Result<Connection> {
    let path = root.join(".macc/memory.db");
    if !initialize && !path.exists() {
        return Err(error("SQLite database missing; refusing JSON fallback"));
    }
    if initialize && !path.exists() {
        let mut options = std::fs::OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        options.open(&path).map_err(|e| error(e.to_string()))?;
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        for file in [
            path.clone(),
            root.join(".macc/memory.db-wal"),
            root.join(".macc/memory.db-shm"),
        ] {
            if file.exists() {
                std::fs::set_permissions(&file, std::fs::Permissions::from_mode(0o600))
                    .map_err(|e| error(e.to_string()))?;
            }
        }
    }
    let flags = OpenFlags::SQLITE_OPEN_READ_WRITE
        | if initialize {
            OpenFlags::SQLITE_OPEN_CREATE
        } else {
            OpenFlags::empty()
        };
    let db = sql(Connection::open_with_flags(&path, flags))?;
    sql(db.busy_timeout(std::time::Duration::from_secs(5)))?;
    sql(db.execute_batch(
        "PRAGMA foreign_keys=ON; PRAGMA journal_mode=WAL; PRAGMA synchronous=FULL;",
    ))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600))
            .map_err(|e| error(e.to_string()))?;
    }
    if initialize {
        sql(db.execute_batch(SCHEMA))?;
        sql(db.execute_batch(super::checkpoints::initial_schema()))?;
        sql(db.execute_batch(super::usage::initial_schema()))?;
        sql(db.execute_batch(crate::o2::initial_schema()))?;
    }
    let version: Option<String> = sql(db
        .query_row(
            "SELECT value FROM session_meta WHERE key='schema_version'",
            [],
            |r| r.get(0),
        )
        .optional())?;
    if let Some(v) = version {
        if v != "1" {
            return Err(error("Unsupported session schema version"));
        }
    } else if initialize {
        sql(db.execute("INSERT INTO session_meta VALUES('schema_version','1')", []))?;
    } else {
        return Err(error("Session schema metadata missing"));
    }
    Ok(db)
}
use rusqlite::OptionalExtension;
pub(crate) fn active(root: &Path) -> Result<(Connection, Authority)> {
    let a = authority(root)?
        .filter(|a| a.mode == "sqlite")
        .ok_or_else(|| error("SQLite authority is not active"))?;
    let db = open(root, false)?;
    let project: String = sql(db.query_row(
        "SELECT value FROM session_meta WHERE key='project_id'",
        [],
        |r| r.get(0),
    ))?;
    if project != a.project_id {
        return Err(error("Database and authority project IDs differ"));
    }
    Ok((db, a))
}
pub(super) fn event(
    db: &Connection,
    id: &str,
    kind: &str,
    payload: serde_json::Value,
) -> Result<()> {
    sql(db.execute(
        "INSERT INTO session_events VALUES(?1,?2,?3,?4,?5)",
        rusqlite::params![
            uuid::Uuid::new_v4().to_string(),
            id,
            kind,
            payload.to_string(),
            now()
        ],
    ))?;
    Ok(())
}
pub(super) fn list(db: &Connection) -> Result<Vec<SessionView>> {
    let mut q=sql(db.prepare("SELECT s.id,s.project_id,s.task_id,s.role,s.tool_id,s.provider_session_id,s.lineage_id,
    s.status,s.generation,s.use_count,l.owner_process_id,l.expires_at,s.updated_at FROM logical_sessions s
    LEFT JOIN session_leases l ON l.logical_session_id=s.id ORDER BY s.updated_at DESC,s.id LIMIT 500"))?;
    let rows = sql(q.query_map([], |r| {
        Ok(SessionView {
            id: r.get(0)?,
            project_id: r.get(1)?,
            task_id: r.get(2)?,
            role: r.get(3)?,
            tool_id: r.get(4)?,
            provider_session_id: r.get(5)?,
            lineage_id: r.get(6)?,
            status: r.get(7)?,
            generation: r.get(8)?,
            use_count: r.get(9)?,
            owner_pid: r.get(10)?,
            expires_at: r.get(11)?,
            updated_at: r.get(12)?,
        })
    }))?;
    rows.map(sql).collect()
}
pub(crate) fn snapshot(db: &Connection, path: &Path) -> Result<()> {
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    options.open(path).map_err(|e| error(e.to_string()))?;
    sql(db.execute("VACUUM INTO ?1", [path.to_string_lossy().as_ref()]))?;
    Ok(())
}
