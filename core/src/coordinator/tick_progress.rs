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
    #[serde(default)]
    pub activity: Option<Activity>,
    #[serde(default)]
    pub process_identity: Option<String>,
    pub stage: String,
    pub since_ms: i64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Activity {
    pub task: String,
    pub phase: String,
    pub worker: Option<String>,
}

impl TickProgress {
    pub fn live_activity(self) -> Option<Activity> {
        let activity = self.activity?;
        let identity = self.process_identity?;
        (crate::process_ownership::process_identity(self.pid.into()).as_ref() == Some(&identity))
            .then_some(activity)
    }
}

pub fn lock(root: &Path) -> Result<AdvisoryLock> {
    AdvisoryLock::acquire(
        &root.join(FILE).with_extension("lock"),
        Duration::from_secs(3),
        "coordinator tick",
    )
}

pub(crate) fn enter(
    root: &Path,
    run_id: &str,
    epoch: i64,
    stage: &str,
    activity: Option<Activity>,
) -> Result<()> {
    let _guard = lock(root)?;
    static IDENTITY: std::sync::OnceLock<Option<String>> = std::sync::OnceLock::new();
    let progress = TickProgress {
        run_id: run_id.into(),
        epoch,
        pid: std::process::id(),
        activity,
        process_identity: IDENTITY
            .get_or_init(|| crate::process_ownership::process_identity(std::process::id().into()))
            .clone(),
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
