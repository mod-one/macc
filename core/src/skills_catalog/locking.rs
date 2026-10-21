//! Lock resolution, path validation and compare-and-swap for reviewed skill changes.
use crate::{
    catalog::{SkillEntry, Source, SourceKind},
    fs_lock::AdvisoryLock,
    plan::ActionPlan,
    skills_catalog::SkillsLockFile,
    MaccError, ProjectPaths, Result,
};
use sha2::{Digest, Sha256};
use std::{
    path::{Component, Path, PathBuf},
    time::Duration,
};
pub fn is_sha(value: &str) -> bool {
    value.len() == 40 && value.bytes().all(|b| b.is_ascii_hexdigit())
}
pub fn relative(value: &str) -> bool {
    Path::new(value)
        .components()
        .all(|c| matches!(c, Component::Normal(_) | Component::CurDir))
}
pub fn revision(paths: &ProjectPaths) -> Result<String> {
    match std::fs::read(paths.skills_lock_path()) {
        Ok(bytes) => Ok(format!("{:x}", Sha256::digest(bytes))),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok("absent".into()),
        Err(source) => Err(MaccError::Io {
            path: paths.skills_lock_path().display().to_string(),
            action: "read lock revision".into(),
            source,
        }),
    }
}
pub fn guard(paths: &ProjectPaths, plan: &ActionPlan) -> Result<Option<AdvisoryLock>> {
    let Some(expected) = &plan.skills_lock_revision else {
        return Ok(None);
    };
    let lock = AdvisoryLock::acquire(
        &paths.macc_dir.join("state/skills-write.lock"),
        Duration::from_secs(30),
        "skills lockfile",
    )?;
    if &revision(paths)? != expected {
        return Err(MaccError::Validation(
            "Skills lockfile changed after preview; review Apply again".into(),
        ));
    }
    validate_local_files(paths, plan)?;
    Ok(Some(lock))
}
pub fn record<'a>(
    lock: &'a SkillsLockFile,
    id: &str,
    tool: &str,
) -> Option<&'a super::SkillLockEntry> {
    lock.find(id, tool).or_else(|| {
        lock.skills.iter().find(|entry| {
            entry.id == id
                && (entry.source.resolved_ref.is_some() || entry.source.checksum.is_some())
        })
    })
}
pub fn source(entry: &SkillEntry, lock: &SkillsLockFile, tool: &str) -> Result<Source> {
    let mut source = entry.source.clone();
    if let Some(locked) = record(lock, &entry.id, tool) {
        if locked.source.kind == "http" && locked.source.checksum.is_some() {
            source.kind = SourceKind::Http;
            source.url = locked
                .source
                .url
                .clone()
                .ok_or_else(|| MaccError::Validation("Locked archive URL is missing".into()))?;
            source.checksum = locked.source.checksum.clone();
        }
        if locked.source.kind == "git" {
            if let Some(sha) = &locked.source.resolved_ref {
                if !is_sha(sha) {
                    return Err(MaccError::Validation(format!(
                        "Invalid locked SHA for '{}' ({tool}); explicitly update this skill",
                        entry.id
                    )));
                }
                source.kind = SourceKind::Git;
                source.reference = sha.clone();
                source.url = locked.source.url.clone().ok_or_else(|| {
                    MaccError::Validation("Locked Git source URL is missing".into())
                })?;
                source.checksum = locked.source.checksum.clone();
            } else if !locked.source.pinned {
                if let Some(reference) = &locked.source.requested_ref {
                    source.reference = reference.clone();
                }
                if let Some(url) = &locked.source.url {
                    source.url = url.clone();
                }
            } else if locked.source.pinned {
                return Err(MaccError::Validation(format!(
                    "Pinned skill '{}' has no resolved SHA; explicitly update it",
                    entry.id
                )));
            }
        }
    }
    Ok(source)
}
pub fn cache_location(project_cache: &Path, key: &str) -> PathBuf {
    if !relative(key) {
        return project_cache.join("invalid-cache-key");
    }
    let local = project_cache.join(key);
    if local.exists() {
        return local;
    }
    let paths = ProjectPaths::from_root(Path::new("."));
    paths
        .user_cache_dir()
        .map(|base| base.join(key))
        .filter(|path| path.exists())
        .unwrap_or(local)
}

/// Initialize a worker's pins without replacing its in-progress context.
pub fn inherit(repo: &ProjectPaths, worker: &ProjectPaths) -> Result<()> {
    if !worker.skills_lock_path().exists() && repo.skills_lock_path().is_file() {
        SkillsLockFile::load(&repo.skills_lock_path())?.save(&worker.skills_lock_path())?;
    }
    Ok(())
}

fn validate_local_files(paths: &ProjectPaths, plan: &ActionPlan) -> Result<()> {
    let lock = SkillsLockFile::load(&paths.skills_lock_path())?;
    for entry in &lock.skills {
        for target in &entry.installed.targets {
            let proposed = plan.actions.iter().find_map(|action| match action {
                crate::plan::Action::WriteFile { path, content, .. } if path == &target.dest => {
                    Some(content)
                }
                _ => None,
            });
            if let (Some(proposed), Some(expected)) = (proposed, &target.digest) {
                if !relative(&target.dest) {
                    return Err(MaccError::Validation("Unsafe locked destination".into()));
                }
                match super::file_digest(&paths.root.join(&target.dest)) {
                    Ok(actual)
                        if &actual != expected && actual != super::sha256_digest(proposed) =>
                    {
                        return Err(MaccError::Validation(format!(
                            "Locally modified skill file {}; refresh the review before overwriting",
                            target.dest
                        )))
                    }
                    Err(MaccError::Io { source, .. })
                        if source.kind() == std::io::ErrorKind::NotFound => {}
                    Err(error) => return Err(error),
                    _ => {}
                }
            }
        }
    }
    Ok(())
}
