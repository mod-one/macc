//! User-level JSON snapshots are scoped to the canonical checkout root.
//! Older releases used a shared basename directory. Its contents are readable
//! only when the stored absolute root resolves to this checkout.
use super::{SavedSessionMeta, Value};
use crate::{MaccError, Result};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

#[derive(Debug, Serialize, Deserialize)]
pub(super) struct SessionSnapshot {
    saved_at: String,
    #[serde(default)]
    project_id: Option<String>,
    pub(super) project_root: String,
    pub(super) name: String,
    pub(super) tools: Value,
}

impl SessionSnapshot {
    pub(super) fn metadata(&self) -> SavedSessionMeta {
        let tools = self.tools.as_object();
        let count = |key: &str| {
            tools
                .into_iter()
                .flat_map(|t| t.values())
                .filter_map(|tool| tool[key].as_object())
                .map(|sessions| sessions.len())
                .sum()
        };
        SavedSessionMeta {
            name: self.name.clone(),
            saved_at: self.saved_at.clone(),
            project_root: self.project_root.clone(),
            tool_count: tools.map_or(0, |t| t.len()),
            active_session_count: count("sessions"),
            archived_session_count: count("archived"),
        }
    }
}

fn io_error(path: &Path, action: &str, source: io::Error) -> MaccError {
    MaccError::Io {
        path: path.display().to_string(),
        action: action.into(),
        source,
    }
}

pub(super) struct SnapshotStore {
    root: PathBuf,
    id: String,
    directory: PathBuf,
    legacy: PathBuf,
}

impl SnapshotStore {
    pub(super) fn open(root: &Path) -> Result<Self> {
        let home = crate::find_user_home().ok_or(MaccError::HomeDirNotFound)?;
        Self::new(root, &home.join(".macc/sessions"))
    }

    fn new(root: &Path, base: &Path) -> Result<Self> {
        let canonical = root
            .canonicalize()
            .map_err(|e| io_error(root, "resolve snapshot project root", e))?;
        let id = crate::session_store::digest(canonical.as_os_str().as_encoded_bytes());
        let slug: String = root
            .file_name()
            .map(|n| n.to_string_lossy())
            .unwrap_or("unknown".into())
            .chars()
            .map(|c| {
                if c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.') {
                    c
                } else {
                    '_'
                }
            })
            .collect();
        Ok(Self {
            root: canonical,
            directory: base.join("by-root").join(&id),
            id,
            legacy: base.join(slug),
        })
    }

    pub(super) fn path(&self, name: &str) -> Result<PathBuf> {
        if name.is_empty() || matches!(name, "." | "..") || name.contains(['/', '\\', ':', '\0']) {
            return Err(MaccError::Validation(
                "Snapshot name must be a single filename".into(),
            ));
        }
        Ok(self.directory.join(format!("{name}.json")))
    }

    fn read_at(&self, path: &Path, name: &str) -> Result<SessionSnapshot> {
        let metadata =
            fs::symlink_metadata(path).map_err(|e| io_error(path, "inspect snapshot", e))?;
        if !metadata.is_file() {
            return Err(MaccError::Validation(format!(
                "Expected a regular snapshot file at {}",
                path.display()
            )));
        }
        let bytes = fs::read(path).map_err(|e| io_error(path, "read snapshot", e))?;
        let snapshot: SessionSnapshot = serde_json::from_slice(&bytes).map_err(|e| {
            MaccError::Validation(format!("Invalid session snapshot {}: {e}", path.display()))
        })?;
        let stored = Path::new(&snapshot.project_root);
        let matches_root = match &snapshot.project_id {
            Some(id) => id == &self.id && snapshot.project_root == self.root.to_string_lossy(),
            None => {
                path.parent() == Some(self.legacy.as_path())
                    && stored.is_absolute()
                    && stored.canonicalize().is_ok_and(|p| p == self.root)
            }
        };
        if !matches_root || snapshot.name != name {
            return Err(MaccError::Validation(format!(
                "Snapshot {} does not match this project or snapshot name",
                path.display()
            )));
        }
        Ok(snapshot)
    }

    fn locate(&self, name: &str) -> Result<PathBuf> {
        let path = self.path(name)?;
        match fs::symlink_metadata(&path) {
            Ok(_) => Ok(path),
            Err(e) if e.kind() == io::ErrorKind::NotFound => {
                Ok(self.legacy.join(format!("{name}.json")))
            }
            Err(e) => Err(io_error(&path, "inspect snapshot", e)),
        }
    }

    pub(super) fn read(&self, name: &str) -> Result<SessionSnapshot> {
        self.read_at(&self.locate(name)?, name)
    }

    pub(super) fn save(
        &self,
        name: String,
        saved_at: String,
        tools: Value,
    ) -> Result<SavedSessionMeta> {
        let path = self.path(&name)?;
        let snapshot = SessionSnapshot {
            name,
            saved_at,
            tools,
            project_id: Some(self.id.clone()),
            project_root: self.root.to_string_lossy().into_owned(),
        };
        match self.read_at(&path, &snapshot.name) {
            Ok(_) => (),
            Err(MaccError::Io { source, .. }) if source.kind() == io::ErrorKind::NotFound => (),
            Err(e) => return Err(e),
        }
        let mut bytes = serde_json::to_vec_pretty(&snapshot)
            .map_err(|e| MaccError::Validation(e.to_string()))?;
        bytes.push(b'\n');
        crate::session_store::write(&self.root, &path, &bytes)?;
        Ok(snapshot.metadata())
    }

    pub(super) fn delete(&self, name: &str) -> Result<()> {
        let path = self.locate(name)?;
        self.read_at(&path, name)?;
        // Old binaries can still replace this shared file after our identity check.
        if path.parent() == Some(self.legacy.as_path()) {
            return Err(MaccError::Validation(format!(
                "Legacy snapshot {} is read-only. Restore it and save a project-scoped copy before removing the old file manually.",
                path.display()
            )));
        }
        fs::remove_file(&path).map_err(|e| io_error(&path, "delete snapshot", e))
    }

    pub(super) fn list(&self) -> Result<Vec<SavedSessionMeta>> {
        let mut seen = BTreeSet::new();
        let mut result = Vec::new();
        for dir in [&self.directory, &self.legacy] {
            let entries = match fs::read_dir(dir) {
                Ok(entries) => entries,
                Err(e) if e.kind() == io::ErrorKind::NotFound => continue,
                Err(e) => return Err(io_error(dir, "list snapshots", e)),
            };
            for entry in entries {
                let path = entry
                    .map_err(|e| io_error(dir, "list snapshots", e))?
                    .path();
                let Some(name) = path
                    .file_name()
                    .and_then(|v| v.to_str())
                    .and_then(|v| v.strip_suffix(".json"))
                else {
                    continue;
                };
                if self.path(name).is_ok() && seen.insert(name.to_owned()) {
                    if let Ok(snapshot) = self.read_at(&path, name) {
                        result.push(snapshot.metadata());
                    }
                }
            }
        }
        result.sort_by(|a, b| {
            b.saved_at
                .cmp(&a.saved_at)
                .then_with(|| a.name.cmp(&b.name))
        });
        Ok(result)
    }
}
