//! Canonical, provider-neutral session ownership. All mutations share an authority
//! lock; SQLite lease transitions additionally use BEGIN IMMEDIATE and fencing.
pub(crate) mod bundle;
pub mod checkpoints;
mod db;
pub(crate) mod fingerprint;
mod leases;
mod migration;
mod policy;
#[cfg(test)]
mod tests;
mod types;
pub mod usage;
pub use crate::config::transaction::revision as policy_revision;
use crate::{MaccError, Result};
pub use bundle::Snapshot as SessionStoreSnapshot;
pub use leases::{acquire, bind, close, heartbeat, read_hint, recover, release};
pub use migration::{migrate, rollback};
pub use policy::{inspect, remove_policy_override, update_policy};
use std::path::{Path, PathBuf};
pub use types::*;

pub(crate) fn error(message: impl Into<String>) -> MaccError {
    MaccError::Validation(format!("Session store: {}", message.into()))
}
pub(crate) fn sql<T>(value: rusqlite::Result<T>) -> Result<T> {
    value.map_err(|e| error(e.to_string()))
}
pub(crate) fn json<T: serde::de::DeserializeOwned>(bytes: &[u8]) -> Result<T> {
    serde_json::from_slice(bytes).map_err(|e| error(e.to_string()))
}
pub(crate) fn legacy_path(root: &Path) -> PathBuf {
    root.join(".macc/state/tool-sessions.json")
}
pub(crate) fn marker_path(root: &Path) -> PathBuf {
    legacy_path(root)
}
pub(crate) fn bytes(path: &Path) -> Result<Vec<u8>> {
    std::fs::read(path).map_err(|source| MaccError::Io {
        path: path.display().to_string(),
        action: "read session store".into(),
        source,
    })
}
pub(crate) fn lock(root: &Path) -> Result<crate::fs_lock::AdvisoryLock> {
    crate::fs_lock::AdvisoryLock::acquire_with_retry(
        &legacy_path(root).with_extension("json.lock"),
        std::time::Duration::from_secs(8),
        std::time::Duration::from_millis(50),
        "session store authority",
    )
}
pub fn authority(root: &Path) -> Result<Option<Authority>> {
    let path = marker_path(root);
    if !path.exists() {
        if root.join(".macc/memory.db").exists() {
            return Err(error(
                "Authority marker missing; explicit migration/recovery required",
            ));
        }
        return Ok(None);
    }
    let document: serde_json::Value = json(&bytes(&path)?)?;
    let Some(metadata) = document.get("session_store") else {
        return Ok(None);
    };
    let value: Authority =
        serde_json::from_value(metadata.clone()).map_err(|e| error(e.to_string()))?;
    if value.version != 1 || !matches!(value.mode.as_str(), "json" | "sqlite") {
        return Err(error("Unknown authority format; refusing fallback"));
    }
    if value.project_root
        != root
            .canonicalize()
            .map_err(|e| error(e.to_string()))?
            .to_string_lossy()
    {
        return Err(error(
            "Project identity/root mismatch; explicit migration required",
        ));
    }
    Ok(Some(value))
}
pub fn is_sqlite(root: &Path) -> Result<bool> {
    Ok(authority(root)?
        .map(|a| a.mode == "sqlite")
        .unwrap_or(false))
}
pub(crate) fn digest(data: &[u8]) -> String {
    use sha2::{Digest, Sha256};
    format!("{:x}", Sha256::digest(data))
}
pub(crate) fn write(_root: &Path, path: &Path, data: &[u8]) -> Result<()> {
    use std::io::Write;
    let parent = path
        .parent()
        .ok_or_else(|| error("Missing destination parent"))?;
    std::fs::create_dir_all(parent).map_err(|e| error(e.to_string()))?;
    let temp = parent.join(format!(".session-write-{}.tmp", uuid::Uuid::new_v4()));
    let result = (|| {
        let mut options = std::fs::OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        let mut file = options.open(&temp).map_err(|e| error(e.to_string()))?;
        file.write_all(data).map_err(|e| error(e.to_string()))?;
        file.sync_all().map_err(|e| error(e.to_string()))?;
        std::fs::rename(&temp, path).map_err(|e| error(e.to_string()))?;
        #[cfg(unix)]
        {
            std::fs::File::open(parent)
                .and_then(|f| f.sync_all())
                .map_err(|e| error(e.to_string()))?;
        }
        Ok(())
    })();
    if result.is_err() {
        let _ = std::fs::remove_file(&temp);
    }
    result
}
pub(crate) fn publish(root: &Path, a: &Authority) -> Result<()> {
    // One atomic authority switch. The deliberately incompatible tools array
    // makes P0-A JSON writers fail closed rather than start a second pool.
    let frozen = serde_json::json!({"session_store":a,"tools":[],"notice":"Session metadata moved to SQLite. Use session-store rollback to restore JSON authority."});
    write(
        root,
        &marker_path(root),
        &serde_json::to_vec_pretty(&frozen).map_err(|e| error(e.to_string()))?,
    )
}
pub(crate) fn now() -> i64 {
    chrono::Utc::now().timestamp()
}

/// SQLite-consistent snapshot. Never copy a live database without its WAL.
pub fn backup(root: &Path) -> Result<String> {
    let db = {
        let _guard = lock(root)?;
        let (db, _) = db::active(root)?;
        db
    };
    let path = root.join(format!(
        ".macc/state/session-store-backup-{}.sqlite",
        uuid::Uuid::new_v4()
    ));
    db::snapshot(&db, &path)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600))
            .map_err(|e| error(e.to_string()))?;
    }
    Ok(path.display().to_string())
}
pub fn events(root: &Path, id: &str) -> Result<Vec<serde_json::Value>> {
    let _guard = lock(root)?;
    let (db, _) = db::active(root)?;
    let mut q=sql(db.prepare("SELECT event_type,payload_json,occurred_at FROM session_events WHERE logical_session_id=?1 ORDER BY occurred_at DESC,rowid DESC LIMIT 200"))?;
    let rows = sql(q.query_map([id], |r| {
        Ok((
            r.get::<_, String>(0)?,
            r.get::<_, String>(1)?,
            r.get::<_, i64>(2)?,
        ))
    }))?;
    rows.map(|row|{let (kind,payload,at)=sql(row)?;Ok(serde_json::json!({"type":kind,"payload":json::<serde_json::Value>(payload.as_bytes())?,"at":at}))}).collect()
}
