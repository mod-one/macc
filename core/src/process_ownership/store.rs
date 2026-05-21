use crate::{MaccError, Result};
use std::fs;
use std::io::ErrorKind;
use std::path::{Path, PathBuf};
use std::thread;
use std::time::Duration;

use super::{OwnershipRecord, ProcessHandle};

const STORE_RELATIVE_PATH: &str = ".macc/state/process_ownership.json";
const LOCK_RETRY_DELAY: Duration = Duration::from_millis(10);
const LOCK_MAX_ATTEMPTS: usize = 200;

#[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize, PartialEq)]
pub struct OwnershipStore {
    records: Vec<OwnershipRecord>,
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
        // TODO(L6-OWN-004): replace the no-op stub with heartbeat-based stale eviction.
    }
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
    use std::fs;
    use std::path::{Path, PathBuf};
    use std::sync::Arc;
    use std::thread;
    use std::time::{SystemTime, UNIX_EPOCH};

    #[test]
    fn load_returns_empty_when_file_missing() {
        let repo_root = temp_repo_root("missing");

        let store = OwnershipStore::load(&repo_root).expect("load missing store");

        assert!(store.records.is_empty());
    }

    #[test]
    fn save_and_load_round_trip_record() {
        let repo_root = temp_repo_root("roundtrip");
        let record = sample_record(7);
        let mut store = OwnershipStore::default();
        store.upsert_record(record.clone());

        store.save(&repo_root).expect("save store");
        let loaded = OwnershipStore::load(&repo_root).expect("reload store");

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
            connected_at: "2026-05-21T12:10:00Z".into(),
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

        store.save(&repo_root).expect("save store");

        assert!(!repo_root
            .join(".macc/state/process_ownership.json.tmp")
            .exists());
    }

    #[test]
    fn load_and_modify_serializes_concurrent_updates() {
        let repo_root = Arc::new(temp_repo_root("concurrent"));
        let thread_count = 8;
        let iterations = 20;
        let mut workers = Vec::new();

        for worker in 0..thread_count {
            let repo_root = Arc::clone(&repo_root);
            workers.push(thread::spawn(move || {
                for iteration in 0..iterations {
                    OwnershipStore::load_and_modify(&repo_root, |store| {
                        let record = sample_record((worker * iterations + iteration) as i32);
                        store.upsert_record(record);
                        Ok(())
                    })?;
                }

                Ok::<(), crate::MaccError>(())
            }));
        }

        for worker in workers {
            worker.join().expect("worker join").expect("worker update");
        }

        let loaded = OwnershipStore::load(&repo_root).expect("reload store");
        assert_eq!(loaded.records.len(), thread_count * iterations);
    }

    fn temp_repo_root(label: &str) -> PathBuf {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("time drift")
            .as_nanos();
        let path = std::env::temp_dir().join(format!("macc-ownership-store-{label}-{unique}"));
        fs::create_dir_all(&path).expect("create temp repo root");
        path
    }

    fn sample_record(id: i32) -> OwnershipRecord {
        OwnershipRecord {
            process: ProcessHandle {
                kind: ProcessKind::Coordinator,
                project_root: Path::new("/tmp/project").join(format!("repo-{id}")),
                pid: Some(1000 + id),
            },
            owner: Some(ClientIdentity {
                client_id: format!("owner-{id}"),
                kind: ClientKind::Tui,
                connected_at: "2026-05-21T12:00:00Z".into(),
            }),
            viewers: vec![ClientIdentity {
                client_id: format!("viewer-{id}"),
                kind: ClientKind::Web,
                connected_at: "2026-05-21T12:05:00Z".into(),
            }],
            takeover_request: Some(TakeoverRequest {
                request_id: format!("request-{id}"),
                requester: ClientIdentity {
                    client_id: format!("requester-{id}"),
                    kind: ClientKind::Cli,
                    connected_at: "2026-05-21T12:06:00Z".into(),
                },
                requested_at: "2026-05-21T12:07:00Z".into(),
            }),
            started_at: "2026-05-21T11:59:00Z".into(),
        }
    }
}
