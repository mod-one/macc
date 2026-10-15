//! SQLite bundle boundary. Callers hold the authority lock across preflight/apply.
use super::*;
use rusqlite::{Connection, OpenFlags};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Snapshot {
    pub version: u32,
    pub database_path: String,
    pub database_sha256: String,
    pub state_sha256: String,
    pub project_id: String,
    pub project_root: String,
}

pub(crate) fn capture_locked(root: &Path, directory: &Path) -> Result<Snapshot> {
    let (db, authority) = db::active(root)?;
    let path = directory.join("state/memory.sqlite");
    std::fs::create_dir_all(path.parent().unwrap()).map_err(|e| error(e.to_string()))?;
    db::snapshot(&db, &path)?;
    let snapshot = read(&path)?;
    let state_sha256 = super::fingerprint::connection(&snapshot)?;
    write(
        root,
        &directory.join("state/tool-sessions.json"),
        &bytes(&marker_path(root))?,
    )?;
    Ok(Snapshot {
        version: 1,
        database_path: "state/memory.sqlite".into(),
        database_sha256: format!("sha256:{}", digest(&bytes(&path)?)),
        state_sha256,
        project_id: authority.project_id,
        project_root: authority.project_root,
    })
}
fn read(path: &Path) -> Result<Connection> {
    let db = sql(Connection::open_with_flags(
        path,
        OpenFlags::SQLITE_OPEN_READ_ONLY,
    ))?;
    let integrity: String = sql(db.query_row("PRAGMA integrity_check", [], |r| r.get(0)))?;
    if integrity != "ok" {
        return Err(error("Invalid SQLite snapshot integrity"));
    }
    let version: String = sql(db.query_row(
        "SELECT value FROM session_meta WHERE key='schema_version'",
        [],
        |r| r.get(0),
    ))?;
    if version != "1" {
        return Err(error("Unsupported snapshot session schema"));
    }
    super::checkpoints::ready(&db)?;
    super::usage::ready(&db)?;
    Ok(db)
}
pub(crate) fn validate_locked(root: &Path, source: &Path, meta: &Snapshot) -> Result<()> {
    migration::stopped(root)?;
    if meta.version != 1
        || meta.project_root
            != root
                .canonicalize()
                .map_err(|e| error(e.to_string()))?
                .to_string_lossy()
    {
        return Err(error(
            "SQLite sessions belong to another project/root; use config-only restore",
        ));
    }
    if format!("sha256:{}", digest(&bytes(source)?)) != meta.database_sha256 {
        return Err(error("Snapshot checksum mismatch"));
    }
    let snapshot = read(source)?;
    let project: String = sql(snapshot.query_row(
        "SELECT value FROM session_meta WHERE key='project_id'",
        [],
        |r| r.get(0),
    ))?;
    if project != meta.project_id || super::fingerprint::connection(&snapshot)? != meta.state_sha256
    {
        return Err(error("Snapshot identity/state mismatch"));
    }
    if let Some(a) = authority(root)? {
        if a.project_id != meta.project_id {
            return Err(error(
                "Snapshot project UUID mismatch; use config-only restore",
            ));
        }
    }
    let path = root.join(".macc/memory.db");
    if path.exists() {
        let db = db::open(root, false)?;
        let project: String = sql(db.query_row(
            "SELECT value FROM session_meta WHERE key='project_id'",
            [],
            |r| r.get(0),
        ))?;
        if project != meta.project_id {
            return Err(error("Existing database project UUID mismatch"));
        }
        let owners: i64 =
            sql(db.query_row("SELECT count(*) FROM session_leases", [], |r| r.get(0)))?;
        if owners != 0 {
            return Err(error(
                "Stop runners and recover outstanding leases before restoring sessions",
            ));
        }
    }
    if !is_sqlite(root)? && marker_path(root).exists() {
        reject_json_owners(&json::<serde_json::Value>(&bytes(&marker_path(root))?)?)?;
    }
    Ok(())
}
fn reject_json_owners(v: &serde_json::Value) -> Result<()> {
    match v {
        serde_json::Value::Object(o) => {
            if o.get("status").and_then(|v| v.as_str()) == Some("active")
                || o.get("owner_pid").is_some_and(|v| !v.is_null())
            {
                return Err(error(
                    "Stop/recover JSON session owners before restoring SQLite",
                ));
            }
            for value in o.values() {
                reject_json_owners(value)?;
            }
        }
        serde_json::Value::Array(a) => {
            for value in a {
                reject_json_owners(value)?;
            }
        }
        _ => {}
    }
    Ok(())
}

pub(crate) fn restore_locked(root: &Path, source: &Path, meta: &Snapshot) -> Result<()> {
    validate_locked(root, source, meta)?;
    let destination = root.join(".macc/memory.db");
    let stage = root.join(format!(".macc/.restore-{}.sqlite", uuid::Uuid::new_v4()));
    let result = (|| {
        let snapshot = read(source)?;
        db::snapshot(&snapshot, &stage)?;
        let mut staged = sql(Connection::open(&stage))?;
        let tx = sql(staged.transaction())?;
        let floor = if destination.exists() {
            let current = db::open(root, false)?;
            sql(current.query_row(
                "SELECT coalesce(max(generation),0) FROM logical_sessions",
                [],
                |r| r.get::<_, i64>(0),
            ))?
        } else {
            0
        };
        let maximum: i64 = sql(tx.query_row(
            "SELECT max(coalesce(max(generation),0),?1) FROM logical_sessions",
            [floor],
            |r| r.get(0),
        ))?;
        if maximum == i64::MAX {
            return Err(error("Session generation exhausted"));
        }
        // Restoring data cannot roll back native transcripts or prove an orphan
        // exited. Keep evidence, invalidate every resumable transcript and lease.
        sql(tx.execute("UPDATE logical_sessions SET status=CASE WHEN status IN ('closed','archived','legacy_untrusted') THEN status ELSE 'abandoned' END,generation=max(generation,?1)+1", [floor]))?;
        sql(tx.execute("DELETE FROM session_leases", []))?;
        if super::usage::ready(&tx)? {
            sql(tx.execute("DELETE FROM usage_watermarks", []))?;
        }
        db::event(
            &tx,
            "",
            "session_bundle_restored",
            serde_json::json!({"snapshot":meta.database_sha256,"transcripts":"quarantined"}),
        )?;
        sql(tx.commit())?;
        sql(staged.execute_batch("PRAGMA journal_mode=DELETE"))?;
        drop(staged);
        std::fs::File::open(&stage)
            .and_then(|f| f.sync_all())
            .map_err(|e| error(e.to_string()))?;
        let stamp = uuid::Uuid::new_v4();
        if destination.exists() {
            let current = db::open(root, false)?;
            db::snapshot(
                &current,
                &root.join(format!(".macc/state/pre-restore-{stamp}.sqlite")),
            )?;
            // No MACC writer/reader can enter while the authority lock is held.
            sql(current
                .execute_batch("PRAGMA wal_checkpoint(TRUNCATE); PRAGMA journal_mode=DELETE;"))?;
            drop(current);
        }
        if marker_path(root).exists() {
            write(
                root,
                &root.join(format!(".macc/state/pre-restore-{stamp}.json")),
                &bytes(&marker_path(root))?,
            )?;
        }
        std::fs::rename(&stage, &destination).map_err(|e| error(e.to_string()))?;
        publish(
            root,
            &Authority {
                version: 1,
                mode: "sqlite".into(),
                project_id: meta.project_id.clone(),
                project_root: meta.project_root.clone(),
            },
        )
    })();
    let _ = std::fs::remove_file(&stage);
    result
}
