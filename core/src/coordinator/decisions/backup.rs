//! Consistent local backups. These are explicit same-project/root snapshots,
//! never a way to import unsigned foreign votes as trusted approvals.
use super::{store::*, *};
use serde::{Deserialize, Serialize};
use std::{path::Path, time::Duration};
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ControlSnapshot {
    pub version: u32,
    pub database_path: String,
    pub database_sha256: String,
    pub project_id: String,
    pub project_root: String,
    pub state_digest: String,
}
fn bytes(path: &Path) -> crate::Result<Vec<u8>> {
    std::fs::read(path).map_err(|e| error(e.to_string()))
}
fn readonly(root: &Path, path: &Path) -> crate::Result<Store> {
    let db = sql(rusqlite::Connection::open_with_flags(
        path,
        rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY,
    ))?;
    let v: i64 = sql(db.query_row("PRAGMA user_version", [], |r| r.get(0)))?;
    if v != 1 {
        return Err(error("Unsupported decision backup schema"));
    }
    let project_id = sql(db.query_row(
        "SELECT value FROM metadata WHERE key='project_id'",
        [],
        |r| r.get(0),
    ))?;
    let store = Store {
        root: root.into(),
        path: path.into(),
        project_id,
        db,
    };
    store.verify()?;
    Ok(store)
}
pub fn capture_backup(root: &Path, directory: &Path) -> crate::Result<Option<ControlSnapshot>> {
    let location = match Store::location(root) {
        Ok(path) => path,
        Err(_) => return Ok(None),
    };
    if !location.exists() {
        return Ok(None);
    }
    let store = Store::open(root)?;
    let _lock = store.mutation_lock()?;
    let destination = directory.join("state/control.sqlite");
    std::fs::create_dir_all(destination.parent().unwrap()).map_err(|e| error(e.to_string()))?;
    sql(store
        .db
        .execute("VACUUM INTO ?1", [destination.to_string_lossy().as_ref()]))?;
    let snapshot = readonly(root, &destination)?;
    let data = bytes(&destination)?;
    if crate::security::scan_bytes(
        "state/control.sqlite",
        String::from_utf8_lossy(&data).as_bytes(),
    )
    .iter()
    .any(|f| f.severity == crate::security::Severity::Error)
    {
        return Err(error("Decision backup contains potential credentials; remove secrets from decision evidence or save without sessions"));
    }
    Ok(Some(ControlSnapshot {
        version: 1,
        database_path: "state/control.sqlite".into(),
        database_sha256: digest(&data),
        state_digest: snapshot.state_digest()?,
        project_id: snapshot.project_id,
        project_root: root
            .canonicalize()
            .map_err(|e| error(e.to_string()))?
            .to_string_lossy()
            .into(),
    }))
}
pub fn validate_backup(root: &Path, source: &Path, meta: &ControlSnapshot) -> crate::Result<()> {
    crate::session_store::require_stopped(root)?;
    if meta.version != 1
        || meta.project_root
            != root
                .canonicalize()
                .map_err(|e| error(e.to_string()))?
                .to_string_lossy()
        || digest(&bytes(source)?) != meta.database_sha256
    {
        return Err(error("Decision backup identity/root/checksum mismatch; foreign evidence cannot restore authority"));
    }
    let snapshot = readonly(root, source)?;
    if snapshot.project_id != meta.project_id || snapshot.state_digest()? != meta.state_digest {
        return Err(error("Decision backup project identity mismatch"));
    }
    let location = Store::location(root)?;
    if location.exists() {
        let current = Store::open(root)?;
        if current.project_id != meta.project_id {
            return Err(error(
                "Existing decision authority has a different identity",
            ));
        }
        let count: i64 = sql(current
            .db
            .query_row("SELECT count(*) FROM events", [], |r| r.get(0)))?;
        let saved_count: i64 =
            sql(snapshot
                .db
                .query_row("SELECT count(*) FROM events", [], |r| r.get(0)))?;
        let prefix = count.min(saved_count);
        if prefix > 0 {
            let a: String = sql(current.db.query_row(
                "SELECT hash FROM events ORDER BY seq LIMIT 1 OFFSET ?1",
                [prefix - 1],
                |r| r.get(0),
            ))?;
            let b: String = sql(snapshot.db.query_row(
                "SELECT hash FROM events ORDER BY seq LIMIT 1 OFFSET ?1",
                [prefix - 1],
                |r| r.get(0),
            ))?;
            if a != b {
                return Err(error("Decision backup diverges from live history; reconcile explicitly instead of overwriting it"));
            }
        }
    }
    Ok(())
}
pub fn restore_backup(root: &Path, source: &Path, meta: &ControlSnapshot) -> crate::Result<()> {
    let location = Store::location(root)?;
    let _controller = crate::fs_lock::AdvisoryLock::acquire(
        &location.with_file_name("controller.lock"),
        Duration::ZERO,
        "decision backup recovery",
    )?;
    validate_backup(root, source, meta)?;
    let snapshot = readonly(root, source)?;
    let exists = location.exists();
    let mut current = Store::open(root)?;
    let _lock = current.mutation_lock()?;
    let count: i64 = sql(current
        .db
        .query_row("SELECT count(*) FROM events", [], |r| r.get(0)))?;
    let saved_count: i64 = sql(snapshot
        .db
        .query_row("SELECT count(*) FROM events", [], |r| r.get(0)))?;
    // Never roll back a newer valid live authority to an old save.
    if exists && count >= saved_count {
        return Ok(());
    }
    // Preserve pre-restore evidence using SQLite's own consistent snapshot.
    if exists {
        let backup =
            location.with_file_name(format!("pre-restore-{}.sqlite", uuid::Uuid::new_v4()));
        sql(current
            .db
            .execute("VACUUM INTO ?1", [backup.to_string_lossy().as_ref()]))?;
    }
    sql(current.db.execute(
        "ATTACH DATABASE ?1 AS decision_backup",
        [source.to_string_lossy().as_ref()],
    ))?;
    let tx = sql(current
        .db
        .transaction_with_behavior(rusqlite::TransactionBehavior::Immediate))?;
    for table in [
        "metadata",
        "plans",
        "events",
        "requests",
        "contexts",
        "outbox",
        "operations",
        "legacy_approvals",
        "authorizations",
    ] {
        sql(tx.execute(&format!("DELETE FROM main.{table}"), []))?;
        sql(tx.execute(
            &format!("INSERT INTO main.{table} SELECT * FROM decision_backup.{table}"),
            [],
        ))?;
    }
    sql(tx.commit())?;
    current.project_id = meta.project_id.clone();
    current.verify()?;
    Ok(())
}

impl Store {
    pub fn state_digest(&self) -> crate::Result<String> {
        use rusqlite::OptionalExtension;
        let head: Option<String> = sql(self
            .db
            .query_row(
                "SELECT hash FROM events ORDER BY seq DESC LIMIT 1",
                [],
                |r| r.get(0),
            )
            .optional())?;
        Ok(digest(&serde_json::json!([self.project_id, head])))
    }
}
pub fn current_state_digest(root: &Path) -> crate::Result<Option<String>> {
    let location = match Store::location(root) {
        Ok(path) => path,
        Err(_) => return Ok(None),
    };
    if !location.exists() {
        return Ok(None);
    }
    Store::open(root)?.state_digest().map(Some)
}
