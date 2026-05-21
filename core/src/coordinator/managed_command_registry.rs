use crate::{MaccError, ProjectPaths, Result};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::fs;
use std::io::ErrorKind;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::thread;
use std::time::Duration;

const STORE_RELATIVE_PATH: &str = ".macc/state/managed_commands.json";
const LOCK_RETRY_DELAY: Duration = Duration::from_millis(10);
const LOCK_MAX_ATTEMPTS: usize = 200;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ManagedCommandRecord {
    pub project_root: PathBuf,
    pub command: String,
    pub pid: i32,
    pub started_at: String,
    #[serde(default)]
    pub last_heartbeat: String,
}

impl ManagedCommandRecord {
    pub fn new(paths: &ProjectPaths, command: impl Into<String>, pid: i32) -> Self {
        let now = Utc::now().to_rfc3339();
        Self {
            project_root: paths.root.clone(),
            command: command.into(),
            pid,
            started_at: now.clone(),
            last_heartbeat: now,
        }
    }

    pub fn elapsed_secs(&self) -> u64 {
        DateTime::parse_from_rfc3339(&self.started_at)
            .ok()
            .map(|ts| {
                Utc::now()
                    .signed_duration_since(ts.with_timezone(&Utc))
                    .num_seconds()
                    .max(0) as u64
            })
            .unwrap_or(0)
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct ManagedCommandStore {
    records: Vec<ManagedCommandRecord>,
}

impl ManagedCommandStore {
    pub fn load(repo_root: &Path) -> Result<Self> {
        let path = store_path(repo_root);
        let raw = match fs::read_to_string(&path) {
            Ok(raw) => raw,
            Err(e) if e.kind() == ErrorKind::NotFound => return Ok(Self::default()),
            Err(e) => {
                return Err(MaccError::Io {
                    path: path.to_string_lossy().into(),
                    action: "read managed command store".into(),
                    source: e,
                });
            }
        };

        let mut store = serde_json::from_str::<Self>(&raw).map_err(|e| MaccError::Storage {
            backend: "json",
            message: format!(
                "Failed to parse managed command store '{}': {}",
                path.display(),
                e
            ),
        })?;
        store.evict_dead_pids();
        Ok(store)
    }

    fn save(&self, repo_root: &Path) -> Result<()> {
        let path = store_path(repo_root);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).map_err(|e| MaccError::Io {
                path: parent.to_string_lossy().into(),
                action: "create managed command store parent directory".into(),
                source: e,
            })?;
        }

        let mut body = serde_json::to_string_pretty(self).map_err(|e| MaccError::Storage {
            backend: "json",
            message: format!(
                "Failed to serialize managed command store '{}': {}",
                path.display(),
                e
            ),
        })?;
        body.push('\n');

        let tmp = temp_path(&path);
        fs::write(&tmp, body).map_err(|e| MaccError::Io {
            path: tmp.to_string_lossy().into(),
            action: "write managed command store temp file".into(),
            source: e,
        })?;
        fs::rename(&tmp, &path).map_err(|e| {
            let _ = fs::remove_file(&tmp);
            MaccError::Io {
                path: path.to_string_lossy().into(),
                action: format!("replace managed command store from {}", tmp.display()),
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

    pub fn get(&self, paths: &ProjectPaths) -> Option<&ManagedCommandRecord> {
        self.records
            .iter()
            .find(|record| record.project_root == paths.root)
    }

    pub fn upsert(&mut self, record: ManagedCommandRecord) {
        if let Some(existing) = self
            .records
            .iter_mut()
            .find(|existing| existing.project_root == record.project_root)
        {
            *existing = record;
            return;
        }
        self.records.push(record);
    }

    pub fn remove(&mut self, paths: &ProjectPaths) -> Option<ManagedCommandRecord> {
        let index = self
            .records
            .iter()
            .position(|record| record.project_root == paths.root)?;
        Some(self.records.remove(index))
    }

    fn evict_dead_pids(&mut self) {
        self.records.retain(|record| pid_is_alive(record.pid));
    }
}

pub fn get_managed_command(paths: &ProjectPaths) -> Result<Option<ManagedCommandRecord>> {
    Ok(ManagedCommandStore::load(&paths.root)?.get(paths).cloned())
}

pub fn upsert_managed_command(paths: &ProjectPaths, command: &str, pid: i32) -> Result<()> {
    ManagedCommandStore::load_and_modify(&paths.root, |store| {
        store.upsert(ManagedCommandRecord::new(paths, command, pid));
        Ok(())
    })
}

pub fn remove_managed_command(paths: &ProjectPaths) -> Result<Option<ManagedCommandRecord>> {
    ManagedCommandStore::load_and_modify(&paths.root, |store| Ok(store.remove(paths)))
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
    repo_root.join(".macc/state/managed_commands.json.lock")
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
                action: "create managed command lock parent directory".into(),
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
                        action: "acquire managed command store lock".into(),
                        source: e,
                    });
                }
            }
        }

        Err(MaccError::Io {
            path: path.to_string_lossy().into(),
            action: "acquire managed command store lock timeout".into(),
            source: std::io::Error::new(
                ErrorKind::TimedOut,
                "timed out waiting for managed command store lock",
            ),
        })
    }
}

impl Drop for StoreLockGuard {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.path);
    }
}

fn pid_is_alive(pid: i32) -> bool {
    if pid <= 0 {
        return false;
    }
    Command::new("kill")
        .arg("-0")
        .arg(pid.to_string())
        .stderr(std::process::Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use super::{
        get_managed_command, remove_managed_command, upsert_managed_command, ManagedCommandStore,
    };
    use crate::ProjectPaths;
    use std::fs;
    use std::path::PathBuf;
    use std::process::Command;
    use std::time::{SystemTime, UNIX_EPOCH};

    #[test]
    fn upsert_deduplicates_by_project_root() {
        let paths = temp_paths("dedupe");
        upsert_managed_command(&paths, "run", std::process::id() as i32).expect("insert");
        upsert_managed_command(&paths, "sync", std::process::id() as i32).expect("replace");

        let store = ManagedCommandStore::load(&paths.root).expect("load");
        assert_eq!(store.records.len(), 1);
        assert_eq!(store.records[0].command, "sync");
        cleanup(&paths.root);
    }

    #[test]
    fn remove_returns_existing_record() {
        let paths = temp_paths("remove");
        upsert_managed_command(&paths, "run", std::process::id() as i32).expect("insert");

        let removed = remove_managed_command(&paths).expect("remove");
        assert_eq!(removed.expect("record").command, "run");
        assert!(get_managed_command(&paths).expect("get").is_none());
        cleanup(&paths.root);
    }

    #[test]
    fn dead_pid_is_evicted_on_load() {
        let paths = temp_paths("evict");
        upsert_managed_command(&paths, "run", 999_999).expect("insert");

        assert!(get_managed_command(&paths).expect("get").is_none());
        cleanup(&paths.root);
    }

    fn temp_paths(label: &str) -> ProjectPaths {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("time")
            .as_nanos();
        let root = std::env::temp_dir().join(format!("macc-managed-command-{label}-{unique}"));
        fs::create_dir_all(&root).expect("mkdir root");
        ProjectPaths {
            root: root.clone(),
            macc_dir: root.join(".macc"),
            config_path: root.join(".macc/macc.yaml"),
            backups_dir: root.join(".macc/backups"),
            tmp_dir: root.join(".macc/tmp"),
            catalog_dir: root.join(".macc/catalog"),
            cache_dir: root.join(".macc/cache"),
        }
    }

    fn cleanup(root: &PathBuf) {
        let _ = Command::new("rm").arg("-rf").arg(root).status();
    }
}
