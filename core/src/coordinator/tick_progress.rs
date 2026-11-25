//! Operation in progress, readable even when the coordinator is waiting on SQLite.
use crate::{fs_lock::AdvisoryLock, MaccError, ProjectPaths, Result};
use chrono::Utc;
use serde::{Deserialize, Serialize};
use std::{path::Path, time::Duration};

const FILE: &str = ".macc/state/coordinator-tick.json";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TickProgress {
    pub run_id: String,
    pub epoch: i64,
    pub pid: u32,
    pub stage: String,
    pub since_ms: i64,
}

pub fn lock(root: &Path) -> Result<AdvisoryLock> {
    AdvisoryLock::acquire(
        &root.join(FILE).with_extension("lock"),
        Duration::from_secs(3),
        "coordinator tick",
    )
}

pub(crate) fn enter(root: &Path, run_id: &str, epoch: i64, stage: &str) -> Result<()> {
    let _guard = lock(root)?;
    let progress = TickProgress {
        run_id: run_id.into(),
        epoch,
        pid: std::process::id(),
        stage: stage.into(),
        since_ms: Utc::now().timestamp_millis(),
    };
    let data = serde_json::to_vec(&progress).map_err(|e| MaccError::Validation(e.to_string()))?;
    crate::atomic_write(&ProjectPaths::from_root(root), &root.join(FILE), &data)
}

pub fn read(root: &Path) -> Result<Option<TickProgress>> {
    let path = root.join(FILE);
    match std::fs::read(&path) {
        Ok(raw) => serde_json::from_slice(&raw)
            .map(Some)
            .map_err(|e| MaccError::Validation(format!("Invalid coordinator tick record: {e}"))),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(source) => Err(MaccError::Io {
            path: path.display().to_string(),
            action: "read coordinator tick".into(),
            source,
        }),
    }
}
