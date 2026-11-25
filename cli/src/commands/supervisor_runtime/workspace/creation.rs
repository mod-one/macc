//! Persist the checkout identity before Git can create a supervisor slot.
use super::{git, io, Target, OWNER};
use macc_core::{MaccError, Result};
use serde::{Deserialize, Serialize};
use std::{
    fs,
    io::ErrorKind,
    path::{Path, PathBuf},
};

const INTENT: &str = ".macc/state/supervisor-worktree-creation.json";

#[derive(Serialize, Deserialize, PartialEq)]
struct Owner {
    incident_id: String,
    base: String,
    branch: Option<String>,
    repair_branch: String,
}

#[derive(Serialize, Deserialize)]
struct Intent {
    project: PathBuf,
    slot: u16,
    owner: Owner,
}

fn publish<T: Serialize>(path: &Path, value: &T) -> Result<()> {
    let parent = path.parent().expect("project file has a parent");
    fs::create_dir_all(parent).map_err(|e| io(parent, e))?;
    let mut temp = tempfile::NamedTempFile::new_in(parent).map_err(|e| io(parent, e))?;
    serde_json::to_writer_pretty(&mut temp, value)
        .map_err(|e| MaccError::Validation(e.to_string()))?;
    temp.as_file().sync_all().map_err(|e| io(path, e))?;
    temp.persist_noclobber(path)
        .map_err(|e| io(path, e.error))?;
    Ok(())
}

pub(super) fn create(
    root: &Path,
    slot: u16,
    incident: &str,
    target: &Target,
    repair_branch: String,
) -> Result<PathBuf> {
    let intent = Intent {
        project: root.canonicalize().map_err(|e| io(root, e))?,
        slot,
        owner: Owner {
            incident_id: incident.into(),
            base: target.base.clone(),
            branch: target.branch.clone(),
            repair_branch,
        },
    };
    publish(&root.join(INTENT), &intent)?;
    complete(root, &intent)
}

pub(in crate::commands::supervisor_runtime) fn reconcile(root: &Path) -> Result<()> {
    let record = root.join(INTENT);
    let raw = match fs::read(&record) {
        Ok(raw) => raw,
        Err(e) if e.kind() == ErrorKind::NotFound => return Ok(()),
        Err(e) => return Err(io(&record, e)),
    };
    let intent = serde_json::from_slice(&raw).map_err(|e| {
        MaccError::Validation(format!(
            "Cannot read supervisor reservation {}: {e}",
            record.display()
        ))
    })?;
    complete(root, &intent).map(|_| ())
}

fn complete(root: &Path, intent: &Intent) -> Result<PathBuf> {
    let pool = root.join(".macc/worktree");
    let name = format!("supervisor-{:02}", intent.slot);
    let path = pool.join(&name);
    let owner = &intent.owner;
    let uncertain = |why: &str| {
        MaccError::Validation(format!(
        "Supervisor creation for incident {} at {} is unresolved: {why}. Inspect the worktree and reservation {} before retrying; files preserved.",
        owner.incident_id, path.display(), root.join(INTENT).display()
    ))
    };
    if !(1..=999).contains(&intent.slot)
        || root.canonicalize().map_err(|e| io(root, e))? != intent.project
        || pool.canonicalize().map_err(|e| io(&pool, e))? != intent.project.join(".macc/worktree")
        || !owner.repair_branch.starts_with("macc/supervisor/")
    {
        return Err(uncertain("project, slot or branch identity differs"));
    }
    let branch = &owner.repair_branch;
    let expected = intent.project.join(".macc/worktree").join(name);
    let registered = macc_core::git::worktrees_for_branch(root, branch)?;
    let dest = path
        .to_str()
        .ok_or_else(|| uncertain("unsupported slot path"))?;
    match fs::symlink_metadata(&path) {
        Err(e) if e.kind() == ErrorKind::NotFound && registered.is_empty() => {
            match macc_core::git::resolve_ref(root, &format!("refs/heads/{branch}")) {
                None => git(root, &["worktree", "add", "-b", branch, dest, &owner.base])?,
                Some(tip) if tip == owner.base => git(root, &["worktree", "add", dest, branch])?,
                Some(_) => return Err(uncertain("reserved branch moved")),
            };
        }
        Ok(meta)
            if meta.is_dir()
                && registered.len() == 1
                && registered[0].canonicalize().ok().as_ref() == Some(&expected) => {}
        Err(e) if e.kind() != ErrorKind::NotFound => return Err(io(&path, e)),
        _ => {
            return Err(uncertain(
                "Git registration does not match the reserved slot",
            ))
        }
    }
    super::baseline::verify_checkout(root, &path)?;
    if git(&path, &["rev-parse", "HEAD"])? != owner.base
        || git(&path, &["branch", "--show-current"])? != *branch
        || path.join(".macc").is_symlink()
    {
        return Err(uncertain("checkout identity or HEAD differs"));
    }
    let marker = path.join(OWNER);
    match fs::symlink_metadata(&marker) {
        Ok(meta) if meta.is_file() => {
            let raw = fs::read(&marker).map_err(|e| io(&marker, e))?;
            if serde_json::from_slice::<Owner>(&raw).ok().as_ref() != Some(owner) {
                return Err(uncertain("existing owner marker differs"));
            }
        }
        Err(e) if e.kind() == ErrorKind::NotFound => {
            if !git(
                &path,
                &["status", "--porcelain=v1", "--untracked-files=all"],
            )?
            .is_empty()
            {
                return Err(uncertain(
                    "checkout is incomplete or contains local changes",
                ));
            }
            publish(&marker, owner)?;
        }
        Ok(_) => return Err(uncertain("unexpected owner marker type")),
        Err(e) => return Err(io(&marker, e)),
    }
    macc_core::project_ownership::record_branch(root, branch)?;
    let record = root.join(INTENT);
    fs::remove_file(&record).map_err(|e| io(&record, e))?;
    Ok(path)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn reserve(root: &Path) -> (Intent, PathBuf) {
        let pool = root.join(".macc/worktree");
        fs::create_dir_all(&pool).unwrap();
        let target = super::super::target(root, None).unwrap();
        let intent = Intent {
            project: root.canonicalize().unwrap(),
            slot: 1,
            owner: Owner {
                incident_id: "incident".into(),
                repair_branch: super::super::repair_branch(root, "incident", &target.base).unwrap(),
                base: target.base,
                branch: target.branch,
            },
        };
        publish(&root.join(INTENT), &intent).unwrap();
        (intent, pool.join("supervisor-01"))
    }

    #[test]
    fn restart_completes_the_same_reservation_at_each_creation_boundary() {
        for stage in 0..3 {
            let fixture = super::super::tests::fixture();
            let root = fixture.path();
            let (intent, slot) = reserve(root);
            if stage >= 1 {
                git(
                    root,
                    &[
                        "worktree",
                        "add",
                        "-b",
                        &intent.owner.repair_branch,
                        slot.to_str().unwrap(),
                        &intent.owner.base,
                    ],
                )
                .unwrap();
            }
            if stage == 2 {
                publish(&slot.join(OWNER), &intent.owner).unwrap();
            }
            reconcile(root).unwrap();
            reconcile(root).unwrap();
            let target = super::super::target(root, None).unwrap();
            assert_eq!(
                super::super::prepare(root, "incident", &target, false).unwrap(),
                slot
            );
            assert!(!root.join(INTENT).exists());
            assert!(!slot.with_file_name("supervisor-02").exists());
        }
    }

    #[test]
    fn occupied_reserved_directory_is_reported_and_preserved() {
        let fixture = super::super::tests::fixture();
        let root = fixture.path();
        let (_, slot) = reserve(root);
        fs::create_dir(&slot).unwrap();
        fs::write(slot.join("personal"), b"keep").unwrap();
        let error = reconcile(root).unwrap_err().to_string();
        assert!(error.contains("Git registration") && error.contains("supervisor-01"));
        assert_eq!(fs::read(slot.join("personal")).unwrap(), b"keep");
        assert!(root.join(INTENT).exists());
        assert!(!slot.join(OWNER).exists());
    }
}
