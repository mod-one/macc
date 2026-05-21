use crate::{MaccError, Result};
use std::fs;
use std::io::ErrorKind;
use std::path::{Path, PathBuf};
use std::thread;
use std::time::Duration;

use super::{evict_stale_clients, HeartbeatConfig, OwnershipRecord, ProcessHandle};

const STORE_RELATIVE_PATH: &str = ".macc/state/process_ownership.json";
const LOCK_RETRY_DELAY: Duration = Duration::from_millis(10);
const LOCK_MAX_ATTEMPTS: usize = 200;

#[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize, PartialEq)]
pub struct OwnershipStore {
    pub(crate) records: Vec<OwnershipRecord>,
}

impl OwnershipStore {
    pub fn load(repo_root: &Path) -> Result<Self> {
        let path = store_path(repo_root);
        let raw = match fs::read_to_string(&path) {
            Ok(raw) => raw,
            Err(e) if e.kind() == ErrorKind::NotFound => return Ok(Self::default()),
            Err(e) => {
                return Err(MaccError::Io {
                    path: path.to_string_lossy().into(),
                    action: "read process ownership store".into(),
                    source: e,
                });
            }
        };

        let mut store = serde_json::from_str::<Self>(&raw).map_err(|e| MaccError::Storage {
            backend: "json",
            message: format!(
                "Failed to parse process ownership store '{}': {}",
                path.display(),
                e
            ),
        })?;
        store.evict_stale_records();
        Ok(store)
    }

    pub fn save(&self, repo_root: &Path) -> Result<()> {
        let path = store_path(repo_root);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).map_err(|e| MaccError::Io {
                path: parent.to_string_lossy().into(),
                action: "create process ownership store parent directory".into(),
                source: e,
            })?;
        }

        let mut body = serde_json::to_string_pretty(self).map_err(|e| MaccError::Storage {
            backend: "json",
            message: format!(
                "Failed to serialize process ownership store '{}': {}",
                path.display(),
                e
            ),
        })?;
        body.push('\n');

        let tmp = temp_path(&path);
        fs::write(&tmp, body).map_err(|e| MaccError::Io {
            path: tmp.to_string_lossy().into(),
            action: "write process ownership store temp file".into(),
            source: e,
        })?;
        fs::rename(&tmp, &path).map_err(|e| {
            let _ = fs::remove_file(&tmp);
            MaccError::Io {
                path: path.to_string_lossy().into(),
                action: format!("replace process ownership store from {}", tmp.display()),
                source: e,
            }
        })?;

        Ok(())
    }

    pub fn load_and_modify<T, F>(repo_root: &Path, modify: F) -> Result<T>
    where
        F: FnOnce(&mut Self) -> Result<T>,
    {
        let _guard = StoreLockGuard::acquire(repo_root)?;
        let mut store = Self::load(repo_root)?;
        let output = modify(&mut store)?;
        store.save(repo_root)?;
        Ok(output)
    }

    pub fn get_record(&self, handle: &ProcessHandle) -> Option<&OwnershipRecord> {
        self.records.iter().find(|record| &record.process == handle)
    }

    pub fn upsert_record(&mut self, record: OwnershipRecord) {
        if let Some(existing) = self
            .records
            .iter_mut()
            .find(|existing| existing.process == record.process)
        {
            *existing = record;
            return;
        }

        self.records.push(record);
    }

    pub fn remove_record(&mut self, handle: &ProcessHandle) -> Option<OwnershipRecord> {
        let index = self
            .records
            .iter()
            .position(|record| &record.process == handle)?;
        Some(self.records.remove(index))
    }

    fn evict_stale_records(&mut self) {
        // L6-OWN-007: takeover timeout / default response come from project config.
        let mut cfg = HeartbeatConfig::default();
        if let Some(record) = self.records.first() {
            let repo_root = record.process.project_root.clone();
            if let Some((timeout, response)) = load_takeover_policy(&repo_root) {
                cfg.takeover_timeout_seconds = timeout;
                cfg.takeover_default_response = response;
            }
        }
        evict_stale_clients(self, &cfg);
    }
}

fn load_takeover_policy(
    repo_root: &Path,
) -> Option<(u64, crate::process_ownership::TakeoverDefaultResponse)> {
    let config_path = repo_root.join(".macc/macc.yaml");
    let config = crate::load_canonical_config(&config_path).ok()?;
    let coordinator = config.automation.coordinator.as_ref()?;
    let timeout = coordinator.takeover_timeout_seconds.unwrap_or(60);
    let response = coordinator
        .takeover_default_response
        .as_deref()
        .map(crate::process_ownership::TakeoverDefaultResponse::from_str_lossy)
        .unwrap_or(crate::process_ownership::TakeoverDefaultResponse::Deny);
    Some((timeout, response))
}

fn store_path(repo_root: &Path) -> PathBuf {
    repo_root.join(STORE_RELATIVE_PATH)
}

fn temp_path(path: &Path) -> PathBuf {
    let mut os = path.as_os_str().to_os_string();
    os.push(".tmp");
    PathBuf::from(os)
}

fn lock_path(repo_root: &Path) -> PathBuf {
    repo_root.join(".macc/state/process_ownership.json.lock")
}

struct StoreLockGuard {
    path: PathBuf,
}

impl StoreLockGuard {
    fn acquire(repo_root: &Path) -> Result<Self> {
        let path = lock_path(repo_root);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).map_err(|e| MaccError::Io {
                path: parent.to_string_lossy().into(),
                action: "create process ownership lock parent directory".into(),
                source: e,
            })?;
        }

        for _ in 0..LOCK_MAX_ATTEMPTS {
            match fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&path)
            {
                Ok(_) => return Ok(Self { path }),
                Err(e) if e.kind() == ErrorKind::AlreadyExists => thread::sleep(LOCK_RETRY_DELAY),
                Err(e) => {
                    return Err(MaccError::Io {
                        path: path.to_string_lossy().into(),
                        action: "acquire process ownership store lock".into(),
                        source: e,
                    });
                }
            }
        }

        Err(MaccError::Io {
            path: path.to_string_lossy().into(),
            action: "acquire process ownership store lock timeout".into(),
            source: std::io::Error::new(
                ErrorKind::TimedOut,
                "timed out waiting for process ownership store lock",
            ),
        })
    }
}

impl Drop for StoreLockGuard {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.path);
    }
}

#[cfg(test)]
mod tests {
    use super::OwnershipStore;
    use crate::process_ownership::{
        ClientIdentity, ClientKind, OwnershipRecord, ProcessHandle, ProcessKind, TakeoverRequest,
    };
    use chrono::{Duration, Utc};
    use std::collections::HashSet;
    use std::path::{Path, PathBuf};
    use std::sync::Arc;
    use std::sync::Barrier;
    use std::thread;
    use tempfile::TempDir;

    #[test]
    fn load_returns_empty_when_file_missing() {
        let repo_root = temp_repo_root("missing");

        let store = OwnershipStore::load(repo_root.path()).expect("load missing store");

        assert!(store.records.is_empty());
    }

    #[test]
    fn save_and_load_round_trip_record() {
        let repo_root = temp_repo_root("roundtrip");
        let record = sample_record(7);
        let mut store = OwnershipStore::default();
        store.upsert_record(record.clone());

        store.save(repo_root.path()).expect("save store");
        let loaded = OwnershipStore::load(repo_root.path()).expect("reload store");

        assert_eq!(loaded.get_record(&record.process), Some(&record));
    }

    #[test]
    fn upsert_and_remove_record() {
        let mut store = OwnershipStore::default();
        let first = sample_record(1);
        let mut updated = sample_record(1);
        updated.viewers.push(ClientIdentity {
            client_id: "viewer-extra".into(),
            kind: ClientKind::Cli,
            connected_at: Utc::now().to_rfc3339(),
            last_heartbeat: Utc::now().to_rfc3339(),
        });

        store.upsert_record(first.clone());
        store.upsert_record(updated.clone());

        assert_eq!(store.get_record(&first.process), Some(&updated));
        assert_eq!(store.remove_record(&first.process), Some(updated));
        assert!(store.get_record(&first.process).is_none());
    }

    #[test]
    fn save_cleans_up_temp_file() {
        let repo_root = temp_repo_root("temp-cleanup");
        let mut store = OwnershipStore::default();
        store.upsert_record(sample_record(3));

        store.save(repo_root.path()).expect("save store");

        assert!(!repo_root
            .path()
            .join(".macc/state/process_ownership.json.tmp")
            .exists());
    }

    #[test]
    fn load_and_modify_serializes_concurrent_claims_without_duplicates() {
        let repo_root = Arc::new(temp_repo_root("concurrent"));
        let thread_count = 10;
        let barrier = Arc::new(Barrier::new(thread_count));
        let mut workers = Vec::new();

        for worker in 0..thread_count {
            let repo_root = Arc::clone(&repo_root);
            let barrier = Arc::clone(&barrier);
            workers.push(thread::spawn(move || {
                let claim = client_with_age(&format!("client-{worker}"), ClientKind::Tui, 0, 0);
                barrier.wait();

                OwnershipStore::load_and_modify(repo_root.path(), |store| {
                    let handle = shared_process_handle();
                    let mut record =
                        store
                            .get_record(&handle)
                            .cloned()
                            .unwrap_or_else(|| OwnershipRecord {
                                process: handle.clone(),
                                owner: None,
                                viewers: Vec::new(),
                                takeover_request: None,
                                started_at: Utc::now().to_rfc3339(),
                            });

                    if record.owner.is_none() {
                        record.owner = Some(claim.clone());
                    } else if record
                        .owner
                        .as_ref()
                        .is_some_and(|owner| owner.client_id != claim.client_id)
                        && !record
                            .viewers
                            .iter()
                            .any(|viewer| viewer.client_id == claim.client_id)
                    {
                        record.viewers.push(claim.clone());
                    }

                    store.upsert_record(record);
                    Ok(())
                })?;

                Ok::<(), crate::MaccError>(())
            }));
        }

        for worker in workers {
            worker.join().expect("worker join").expect("worker update");
        }

        let loaded = OwnershipStore::load(repo_root.path()).expect("reload store");
        assert_eq!(loaded.records.len(), 1);

        let record = loaded
            .get_record(&shared_process_handle())
            .expect("shared record to exist");
        let owner = record.owner.as_ref().expect("owner to be claimed");
        let unique_clients: HashSet<_> = std::iter::once(owner.client_id.as_str())
            .chain(
                record
                    .viewers
                    .iter()
                    .map(|viewer| viewer.client_id.as_str()),
            )
            .collect();

        assert_eq!(unique_clients.len(), thread_count);
        assert_eq!(record.viewers.len(), thread_count - 1);
    }

    #[test]
    fn load_evicts_stale_owner() {
        let repo_root = temp_repo_root("stale-owner");
        let mut store = OwnershipStore::default();
        let mut record = sample_record(11);
        record.owner = Some(client_with_age("owner-stale", ClientKind::Tui, 120, 120));
        record.viewers.clear();
        record.takeover_request = None;
        store.upsert_record(record.clone());

        store.save(repo_root.path()).expect("save store");

        let loaded = OwnershipStore::load(repo_root.path()).expect("load store");
        let record = loaded
            .get_record(&record.process)
            .expect("record after load");

        assert!(record.owner.is_none());
    }

    #[test]
    fn load_evicts_only_stale_viewers() {
        let repo_root = temp_repo_root("stale-viewers");
        let mut store = OwnershipStore::default();
        let mut record = sample_record(12);
        let fresh_viewer = client_with_age("viewer-fresh", ClientKind::Web, 10, 10);
        record.viewers = vec![
            client_with_age("viewer-stale-1", ClientKind::Web, 120, 120),
            fresh_viewer.clone(),
            client_with_age("viewer-stale-2", ClientKind::Cli, 180, 180),
        ];
        store.upsert_record(record.clone());

        store.save(repo_root.path()).expect("save store");

        let loaded = OwnershipStore::load(repo_root.path()).expect("load store");
        let record = loaded
            .get_record(&record.process)
            .expect("record after load");

        assert_eq!(record.viewers, vec![fresh_viewer]);
    }

    #[test]
    fn load_promotes_fresh_viewer_when_owner_is_stale() {
        let repo_root = temp_repo_root("promote-viewer");
        let mut store = OwnershipStore::default();
        let mut record = sample_record(13);
        let promoted_viewer = client_with_age("viewer-fresh", ClientKind::Web, 30, 10);
        record.owner = Some(client_with_age("owner-stale", ClientKind::Tui, 120, 120));
        record.viewers = vec![promoted_viewer.clone()];
        store.upsert_record(record.clone());

        store.save(repo_root.path()).expect("save store");

        let loaded = OwnershipStore::load(repo_root.path()).expect("load store");
        let record = loaded
            .get_record(&record.process)
            .expect("record after load");

        assert_eq!(record.owner.as_ref(), Some(&promoted_viewer));
        assert!(record.viewers.is_empty());
    }

    #[test]
    fn ownership_record_round_trips_through_json_with_all_fields() {
        let record = sample_record(21);

        let serialized = serde_json::to_string(&record).expect("serialize record");
        let deserialized: OwnershipRecord =
            serde_json::from_str(&serialized).expect("deserialize record");

        assert_eq!(deserialized, record);
    }

    fn temp_repo_root(label: &str) -> TempDir {
        tempfile::Builder::new()
            .prefix(&format!("macc-ownership-store-{label}-"))
            .tempdir()
            .expect("create temp repo root")
    }

    fn sample_record(id: i32) -> OwnershipRecord {
        OwnershipRecord {
            process: ProcessHandle {
                kind: ProcessKind::Coordinator,
                project_root: Path::new("/tmp/project").join(format!("repo-{id}")),
                pid: Some(1000 + id),
            },
            owner: Some(client_with_age(
                &format!("owner-{id}"),
                ClientKind::Tui,
                0,
                0,
            )),
            viewers: vec![client_with_age(
                &format!("viewer-{id}"),
                ClientKind::Web,
                0,
                0,
            )],
            takeover_request: Some(TakeoverRequest {
                request_id: format!("request-{id}"),
                requester: client_with_age(&format!("requester-{id}"), ClientKind::Cli, 0, 0),
                requested_at: Utc::now().to_rfc3339(),
            }),
            started_at: Utc::now().to_rfc3339(),
        }
    }

    fn shared_process_handle() -> ProcessHandle {
        ProcessHandle {
            kind: ProcessKind::Coordinator,
            project_root: PathBuf::from("/tmp/project/shared-repo"),
            pid: Some(4242),
        }
    }

    fn client_with_age(
        client_id: &str,
        kind: ClientKind,
        connected_age_seconds: i64,
        heartbeat_age_seconds: i64,
    ) -> ClientIdentity {
        ClientIdentity {
            client_id: client_id.to_string(),
            kind,
            connected_at: (Utc::now() - Duration::seconds(connected_age_seconds)).to_rfc3339(),
            last_heartbeat: (Utc::now() - Duration::seconds(heartbeat_age_seconds)).to_rfc3339(),
        }
    }
}
