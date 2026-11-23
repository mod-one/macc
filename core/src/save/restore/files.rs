//! Plan every selected file and directory before publishing any section.
use super::normalize_session_json;
use crate::{skills_catalog::SkillsLockFile, MaccError, ProjectPaths, Result};
use std::{
    fs, io,
    path::{Path, PathBuf},
};

fn io_error(action: &str, path: &Path, source: io::Error) -> MaccError {
    MaccError::Io {
        action: action.into(),
        path: path.display().to_string(),
        source,
    }
}

pub(in crate::save) fn destination(
    section: &str,
    root: &Path,
    path: &Path,
    directory: bool,
) -> Result<()> {
    let relative = path.strip_prefix(root).map_err(|_| {
        MaccError::Validation(format!(
            "Restore {section} destination outside project: {}",
            path.display()
        ))
    })?;
    let mut part = root.to_owned();
    for component in relative.components() {
        part.push(component);
        let want_dir = part != path || directory;
        match fs::symlink_metadata(&part) {
            Ok(m) if (want_dir && m.is_dir()) || (!want_dir && m.is_file()) => {}
            Ok(_) => {
                return Err(MaccError::Validation(format!(
                    "Restore preflight ({section}): expected {} at {}",
                    if want_dir {
                        "directory"
                    } else {
                        "regular file"
                    },
                    part.display()
                )))
            }
            Err(e) if e.kind() == io::ErrorKind::NotFound => break,
            Err(e) => {
                return Err(io_error(
                    &format!("inspect {section} destination"),
                    &part,
                    e,
                ))
            }
        }
    }
    Ok(())
}

pub(super) fn plan(
    paths: &ProjectPaths,
    bundle: &Path,
    name: &crate::save::name::SaveName<'_>,
    manifest: &crate::save::SaveBundleManifest,
    config_only: bool,
    restore_sessions: bool,
    include_logs: bool,
) -> Result<Vec<FileSection>> {
    let mut sections = Vec::new();
    if manifest.includes.config {
        let rel =
            manifest.paths.config.as_deref().ok_or_else(|| {
                MaccError::Validation("Missing configuration path in bundle".into())
            })?;
        sections.push(FileSection::file(
            "configuration",
            paths,
            paths.config_path.clone(),
            Payload::Config(crate::save::sessions::payload(bundle, rel)?),
        )?);
        if let Some(lock) = crate::save::skill_locks::load(bundle)? {
            sections.push(FileSection::file(
                "skill lock",
                paths,
                paths.skills_lock_path(),
                Payload::SkillLock(lock),
            )?);
        }
    }
    if restore_sessions
        && manifest.includes.coordinator_sessions
        && manifest.session_store.is_none()
    {
        let rel = manifest
            .paths
            .coordinator_sessions
            .as_deref()
            .ok_or_else(|| {
                MaccError::Validation("Missing legacy sessions path in bundle".into())
            })?;
        sections.push(FileSection::file(
            "legacy sessions",
            paths,
            crate::session_store::legacy_path(&paths.root),
            Payload::Sessions(crate::save::sessions::payload(bundle, rel)?),
        )?);
    }
    if !config_only {
        for (selected, section, source, target) in [
            (
                manifest.includes.catalogs,
                "catalogs",
                "catalogs",
                paths.project_catalog_dir(),
            ),
            (
                include_logs && manifest.includes.logs,
                "archived logs",
                "logs",
                name.path(&paths.macc_dir.join("restored-logs")),
            ),
            (
                manifest.includes.prd,
                "task state",
                "automation/task",
                paths.macc_dir.join("automation/task"),
            ),
        ] {
            if selected {
                // Also inspect intermediate bundle components (notably automation/task).
                destination(section, bundle, &bundle.join(source), true)?;
                sections.push(FileSection::tree(
                    section,
                    paths,
                    bundle.join(source),
                    target,
                )?);
            }
        }
    }
    Ok(sections)
}

enum Payload {
    Directory,
    Copy(PathBuf),
    Config(PathBuf),
    Sessions(PathBuf),
    SkillLock(SkillsLockFile),
}

pub(super) struct FileSection {
    pub name: &'static str,
    pub target: PathBuf,
    entries: Vec<(PathBuf, Payload)>,
}

impl FileSection {
    fn file(
        name: &'static str,
        paths: &ProjectPaths,
        target: PathBuf,
        payload: Payload,
    ) -> Result<Self> {
        destination(name, &paths.root, &target, false)?;
        Ok(Self {
            name,
            target: target.clone(),
            entries: vec![(target, payload)],
        })
    }

    fn tree(
        name: &'static str,
        paths: &ProjectPaths,
        source: PathBuf,
        target: PathBuf,
    ) -> Result<Self> {
        let mut section = Self {
            name,
            target: target.clone(),
            entries: Vec::new(),
        };
        section.collect(&paths.root, &source, target)?;
        Ok(section)
    }

    fn collect(&mut self, root: &Path, source: &Path, target: PathBuf) -> Result<()> {
        let action = format!("inspect {} bundle", self.name);
        let inspect = |e| io_error(&action, source, e);
        let kind = fs::symlink_metadata(source).map_err(inspect)?.file_type();
        if !kind.is_file() && !kind.is_dir() || (self.entries.is_empty() && !kind.is_dir()) {
            return Err(MaccError::Validation(format!(
                "Invalid {} bundle entry at {}",
                self.name,
                source.display()
            )));
        }
        destination(self.name, root, &target, kind.is_dir())?;
        if kind.is_dir() {
            self.entries.push((target.clone(), Payload::Directory));
            for child in fs::read_dir(source).map_err(inspect)? {
                let child = child.map_err(inspect)?;
                self.collect(root, &child.path(), target.join(child.file_name()))?;
            }
        } else {
            self.entries
                .push((target, Payload::Copy(source.to_owned())));
        }
        Ok(())
    }

    pub fn restore(&self, paths: &ProjectPaths) -> Result<()> {
        for (target, payload) in &self.entries {
            destination(
                self.name,
                &paths.root,
                target,
                matches!(payload, Payload::Directory),
            )?;
            match payload {
                Payload::Directory => fs::create_dir_all(target)
                    .map_err(|e| io_error("create directory", target, e))?,
                Payload::Copy(source) => copy(source, target)?,
                Payload::Config(source) => {
                    if target
                        .try_exists()
                        .map_err(|e| io_error("inspect configuration", target, e))?
                    {
                        let backup = target
                            .with_file_name(format!("macc.yaml.backup.{}", uuid::Uuid::new_v4()));
                        copy(target, &backup)?;
                    }
                    copy(source, target)?;
                }
                Payload::SkillLock(lock) => lock.save(target)?,
                Payload::Sessions(source) => {
                    let mut bytes = fs::read(source)
                        .map_err(|e| io_error("read legacy sessions", source, e))?;
                    if let Ok(mut value) = serde_json::from_slice(&bytes) {
                        normalize_session_json(&mut value, &paths.root);
                        bytes = serde_json::to_vec_pretty(&value)
                            .map_err(|e| MaccError::Validation(e.to_string()))?;
                    }
                    crate::atomic_write(paths, target, &bytes)?;
                }
            }
        }
        Ok(())
    }
}

// Stage on the destination filesystem so a failed copy preserves the existing file.
fn copy(source: &Path, target: &Path) -> Result<()> {
    let parent = target.parent().expect("restore destination has a parent");
    fs::create_dir_all(parent).map_err(|e| io_error("create restore directory", parent, e))?;
    let stage = tempfile::NamedTempFile::new_in(parent)
        .map_err(|e| io_error("stage restore", target, e))?;
    fs::copy(source, stage.path())
        .map_err(|e| io_error(&format!("copy from {}", source.display()), target, e))?;
    stage
        .persist(target)
        .map_err(|e| io_error("publish restored file", target, e.error))?;
    Ok(())
}
