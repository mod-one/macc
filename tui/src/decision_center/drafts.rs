//! Project-local JSON draft maps. A failed load never acquires a writable path.
use serde::{de::DeserializeOwned, Serialize};
use std::{
    collections::BTreeMap,
    fs::File,
    io::{BufReader, ErrorKind},
    ops::{Deref, DerefMut},
    path::{Path, PathBuf},
};

#[derive(Default)]
pub(super) struct Drafts<T> {
    entries: BTreeMap<String, T>,
    writable_path: Option<PathBuf>,
    dirty: bool,
    pub(super) warning: Option<String>,
}

impl<T> Deref for Drafts<T> {
    type Target = BTreeMap<String, T>;
    fn deref(&self) -> &Self::Target {
        &self.entries
    }
}
impl<T> DerefMut for Drafts<T> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        self.dirty = true;
        &mut self.entries
    }
}

impl<T: Default + DeserializeOwned + Serialize> Drafts<T> {
    pub(super) fn load(path: PathBuf) -> Self {
        let result = match std::fs::symlink_metadata(&path) {
            Ok(meta) if meta.is_file() => File::open(&path).and_then(|file| {
                serde_json::from_reader(BufReader::new(file)).map_err(std::io::Error::from)
            }),
            Ok(_) => Err(std::io::Error::new(
                ErrorKind::InvalidData,
                "expected a regular file",
            )),
            Err(e) if e.kind() == ErrorKind::NotFound => Ok(BTreeMap::new()),
            Err(e) => Err(e),
        };
        match result {
            Ok(entries) => Self { entries, writable_path: Some(path), ..Self::default() },
            Err(error) => Self {
                warning: Some(format!("Draft saving paused. Could not load {}: {error}. Repair the file and reopen the project.", path.display())),
                ..Self::default()
            },
        }
    }

    pub(super) fn save(&mut self, root: &Path) {
        let Some(path) = self.writable_path.as_ref().filter(|_| self.dirty) else {
            return;
        };
        let result = serde_json::to_vec(&self.entries)
            .map_err(|e| e.to_string())
            .and_then(|bytes| {
                macc_core::atomic_write(&macc_core::ProjectPaths::from_root(root), path, &bytes)
                    .map_err(|e| e.to_string())
            });
        match result {
            Ok(()) => {
                self.dirty = false;
                self.warning = None;
            }
            Err(error) => {
                self.warning = Some(format!("Drafts not saved to {}: {error}", path.display()))
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::decision_center::{Center, Draft};
    use std::fs;

    #[test]
    fn restart_loads_exactly_one_mib_and_larger_collections() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path();
        let path = root.join(".macc/cache/decision-drafts.json");
        let mut drafts = Drafts::<Draft>::load(path.clone());
        drafts.insert("first@1".into(), Draft::default());
        let overhead = serde_json::to_vec(&*drafts).unwrap().len();
        let first = "a".repeat(1024 * 1024 - overhead);
        drafts.get_mut("first@1").unwrap().text = first.clone();
        drafts.save(root);
        assert_eq!(fs::metadata(&path).unwrap().len(), 1024 * 1024);
        let mut reopened = Drafts::<Draft>::load(path.clone());
        assert_eq!(reopened["first@1"].text, first);
        reopened.entry("second@2".into()).or_default().text = "second answer".into();
        reopened.save(root);
        assert!(fs::metadata(&path).unwrap().len() > 1024 * 1024);
        let reopened = Drafts::<Draft>::load(path);
        assert_eq!(reopened["first@1"].text, first);
        assert_eq!(reopened["second@2"].text, "second answer");
    }

    #[test]
    fn invalid_files_survive_edits_project_switch_and_drop() {
        let temp = tempfile::tempdir().unwrap();
        let cache = temp.path().join(".macc/cache");
        fs::create_dir_all(&cache).unwrap();
        let files = ["decision-drafts.json", "decision-action-drafts.json"];
        for name in files {
            fs::write(cache.join(name), b"{invalid").unwrap();
        }
        let mut center = Center::default();
        center.tick(Some(temp.path().into()), true);
        let warning = center.new_draft_warning().unwrap();
        for name in files {
            assert!(warning.contains(name));
        }
        center.drafts.entry("q@1".into()).or_default().text = "new".into();
        center
            .actions
            .drafts
            .entry("q@1:revise".into())
            .or_default();
        center.save_drafts();
        center.tick(None, true);
        drop(center);
        for name in files {
            assert_eq!(fs::read(cache.join(name)).unwrap(), b"{invalid");
        }
    }

    #[test]
    fn failed_write_is_reported_and_retried_with_the_in_memory_draft() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("drafts.json");
        let mut drafts = Drafts::<String>::load(path.clone());
        drafts.insert("q@1".into(), "answer".into());
        fs::create_dir(&path).unwrap();
        drafts.save(temp.path());
        assert!(drafts.warning.as_deref().unwrap().contains("drafts.json"));
        fs::remove_dir(&path).unwrap();
        drafts.save(temp.path());
        assert!(drafts.warning.is_none());
        assert_eq!(Drafts::<String>::load(path)["q@1"], "answer");
    }
}
