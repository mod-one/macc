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
    #[serde(alias = "command")]
    pub kind: String,
    pub pid: i32,
    pub started_at: String,
    #[serde(default)]
    pub last_heartbeat: String,
}

impl ManagedCommandRecord {
    pub fn new(paths: &ProjectPaths, kind: impl Into<String>, pid: i32) -> Self {
        let now = Utc::now().to_rfc3339();
        Self {
            project_root: paths.root.clone(),
            kind: kind.into(),
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

    fn matches(&self, key: &ManagedCommandKey<'_>) -> bool {
        self.project_root == key.project_root && self.kind == key.kind
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ManagedCommandKey<'a> {
    project_root: &'a Path,
    kind: &'a str,
}

impl<'a> ManagedCommandKey<'a> {
    pub fn new(paths: &'a ProjectPaths, kind: &'a str) -> Self {
        Self {
            project_root: &paths.root,
            kind,
        }
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct ManagedCommandStore {
    records: Vec<ManagedCommandRecord>,
}

impl ManagedCommandStore {
    fn load_raw(repo_root: &Path) -> Result<Self> {
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

        serde_json::from_str::<Self>(&raw).map_err(|e| MaccError::Storage {
            backend: "json",
            message: format!(
                "Failed to parse managed command store '{}': {}",
                path.display(),
                e
            ),
        })
    }

    pub fn load(repo_root: &Path) -> Result<Self> {
        Self::load_and_reconcile(repo_root, |store| Ok(store.clone()))
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
        Self::load_and_reconcile(repo_root, |store| {
            let output = modify(store)?;
            Ok(output)
        })
    }

    fn load_and_reconcile<T, F>(repo_root: &Path, action: F) -> Result<T>
    where
        F: FnOnce(&mut Self) -> Result<T>,
    {
        let _guard = StoreLockGuard::acquire(repo_root)?;
        let mut store = Self::load_raw(repo_root)?;
        let evicted = store.evict_dead_pids();
        let output = action(&mut store)?;
        if evicted || store_path(repo_root).exists() || !store.records.is_empty() {
            store.save(repo_root)?;
        }
        Ok(output)
    }

    pub fn get(&self, key: ManagedCommandKey<'_>) -> Option<&ManagedCommandRecord> {
        self.records.iter().find(|record| record.matches(&key))
    }

    pub fn list_for_project(&self, paths: &ProjectPaths) -> Vec<&ManagedCommandRecord> {
        self.records
            .iter()
            .filter(|record| record.project_root == paths.root)
            .collect()
    }

    pub fn upsert(&mut self, record: ManagedCommandRecord) {
        let key = ManagedCommandKey {
            project_root: &record.project_root,
            kind: &record.kind,
        };
        if let Some(existing) = self
            .records
            .iter_mut()
            .find(|existing| existing.matches(&key))
        {
            *existing = record;
            return;
        }
        self.records.push(record);
    }

    pub fn remove(&mut self, key: ManagedCommandKey<'_>) -> Option<ManagedCommandRecord> {
        let index = self
            .records
            .iter()
            .position(|record| record.matches(&key))?;
        Some(self.records.remove(index))
    }

    fn evict_dead_pids(&mut self) -> bool {
        let original_len = self.records.len();
        self.records.retain(|record| pid_is_alive(record.pid));
        self.records.len() != original_len
    }
}

pub struct ManagedCommandRegistry<'a> {
    repo_root: &'a Path,
}

impl<'a> ManagedCommandRegistry<'a> {
    pub fn new(repo_root: &'a Path) -> Self {
        Self { repo_root }
    }

    pub fn list(&self, paths: &ProjectPaths) -> Result<Vec<ManagedCommandRecord>> {
        ManagedCommandStore::load(self.repo_root).map(|store| {
            store
                .list_for_project(paths)
                .into_iter()
                .cloned()
                .collect::<Vec<_>>()
        })
    }

    pub fn get(&self, paths: &ProjectPaths, kind: &str) -> Result<Option<ManagedCommandRecord>> {
        Ok(ManagedCommandStore::load(self.repo_root)?
            .get(ManagedCommandKey::new(paths, kind))
            .cloned())
    }

    pub fn upsert(&self, record: ManagedCommandRecord) -> Result<()> {
        ManagedCommandStore::load_and_modify(self.repo_root, |store| {
            store.upsert(record);
            Ok(())
        })
    }

    pub fn remove(&self, paths: &ProjectPaths, kind: &str) -> Result<Option<ManagedCommandRecord>> {
        ManagedCommandStore::load_and_modify(self.repo_root, |store| {
            Ok(store.remove(ManagedCommandKey::new(paths, kind)))
        })
    }
}

pub fn list_managed_commands(paths: &ProjectPaths) -> Result<Vec<ManagedCommandRecord>> {
    ManagedCommandRegistry::new(&paths.root).list(paths)
}

pub fn get_managed_command(
    paths: &ProjectPaths,
    kind: &str,
) -> Result<Option<ManagedCommandRecord>> {
    ManagedCommandRegistry::new(&paths.root).get(paths, kind)
}

pub fn upsert_managed_command(paths: &ProjectPaths, kind: &str, pid: i32) -> Result<()> {
    ManagedCommandRegistry::new(&paths.root).upsert(ManagedCommandRecord::new(paths, kind, pid))
}

pub fn remove_managed_command(
    paths: &ProjectPaths,
    kind: &str,
) -> Result<Option<ManagedCommandRecord>> {
    ManagedCommandRegistry::new(&paths.root).remove(paths, kind)
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
        get_managed_command, list_managed_commands, remove_managed_command, upsert_managed_command,
        ManagedCommandStore,
    };
    use crate::ProjectPaths;
    use std::collections::HashSet;
    use std::path::Path;
    use std::sync::{Arc, Barrier};
    use std::thread;
    use tempfile::TempDir;

    #[test]
    fn upsert_deduplicates_by_project_root_and_kind() {
        let repo_root = Arc::new(temp_repo_root("dedupe"));
        let paths = paths_for(repo_root.path());
        let thread_count = 8;
        let barrier = Arc::new(Barrier::new(thread_count));
        let mut workers = Vec::new();

        for _ in 0..thread_count {
            let repo_root = Arc::clone(&repo_root);
            let barrier = Arc::clone(&barrier);
            workers.push(thread::spawn(move || {
                let paths = paths_for(repo_root.path());
                barrier.wait();
                upsert_managed_command(&paths, "run", std::process::id() as i32)
            }));
        }

        for worker in workers {
            worker.join().expect("worker join").expect("worker update");
        }

        let store = ManagedCommandStore::load(repo_root.path()).expect("load");
        assert_eq!(store.records.len(), 1);
        assert_eq!(store.records[0].kind, "run");
        assert_eq!(
            get_managed_command(&paths, "run")
                .expect("get")
                .expect("record")
                .kind,
            "run"
        );
    }

    #[test]
    fn list_and_remove_are_keyed_by_kind() {
        let repo_root = temp_repo_root("remove");
        let paths = paths_for(repo_root.path());
        upsert_managed_command(&paths, "run", std::process::id() as i32).expect("insert run");
        upsert_managed_command(&paths, "sync_registry", std::process::id() as i32)
            .expect("insert sync");

        let listed = list_managed_commands(&paths).expect("list");
        let kinds: HashSet<_> = listed.iter().map(|record| record.kind.as_str()).collect();
        assert_eq!(kinds, HashSet::from(["run", "sync_registry"]));

        let removed = remove_managed_command(&paths, "run").expect("remove");
        assert_eq!(removed.expect("record").kind, "run");
        assert!(get_managed_command(&paths, "run")
            .expect("get run")
            .is_none());
        assert_eq!(
            get_managed_command(&paths, "sync_registry")
                .expect("get sync")
                .expect("sync record")
                .kind,
            "sync_registry"
        );
    }

    #[test]
    fn dead_pid_is_evicted_on_load_and_saved() {
        let repo_root = temp_repo_root("evict");
        let paths = paths_for(repo_root.path());
        upsert_managed_command(&paths, "run", 999_999).expect("insert");

        assert!(get_managed_command(&paths, "run").expect("get").is_none());

        let store = ManagedCommandStore::load(repo_root.path()).expect("reload");
        assert!(store.records.is_empty());
    }

    fn temp_repo_root(label: &str) -> TempDir {
        tempfile::Builder::new()
            .prefix(&format!("macc-managed-command-{label}-"))
            .tempdir()
            .expect("temp repo root")
    }

    fn paths_for(root: &Path) -> ProjectPaths {
        ProjectPaths {
            root: root.to_path_buf(),
            macc_dir: root.join(".macc"),
            config_path: root.join(".macc/macc.yaml"),
            backups_dir: root.join(".macc/backups"),
            tmp_dir: root.join(".macc/tmp"),
            catalog_dir: root.join(".macc/catalog"),
            cache_dir: root.join(".macc/cache"),
        }
    }
}
