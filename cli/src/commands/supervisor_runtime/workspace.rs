//! Owned supervisor slots and durable evidence, independent of attempt log directories.
use super::{io, repair::git, report::write_json};
use macc_core::{MaccError, Result};
use serde_json::{json, Value};
use std::path::{Path, PathBuf};

const OWNER: &str = ".macc/supervisor-owner.json";

pub fn prepare(root: &Path, incident: &str) -> Result<(PathBuf, String)> {
    super::repair::clean(root)?;
    let head = git(root, &["rev-parse", "HEAD"])?;
    let pool = root.join(".macc/worktree");
    std::fs::create_dir_all(&pool).map_err(|e| io(&pool, e))?;
    let mut vacant = None;
    for index in 1..=999 {
        let path = pool.join(format!("supervisor-{index:02}"));
        if !path.exists() {
            vacant.get_or_insert(path);
            continue;
        }
        let owner: Value = match std::fs::read_to_string(path.join(OWNER)) {
            Ok(text) => {
                serde_json::from_str(&text).map_err(|e| MaccError::Validation(e.to_string()))?
            }
            Err(_) => continue, // Unowned or retained by another incident: never recycle it.
        };
        if owner["incident_id"].as_str() != Some(incident) {
            continue;
        }
        let base = owner["base"]
            .as_str()
            .ok_or_else(|| MaccError::Validation("Supervisor slot has no base".into()))?;
        if base != head {
            if !safe_to_remove(root, &path)? {
                return Err(MaccError::Validation(
                    "Project base changed; supervisor work preserved for operator recovery".into(),
                ));
            }
            git(&path, &["checkout", "--detach", &head])?;
        }
        write_json(
            &path.join(OWNER),
            &json!({"incident_id":incident,"base":head}),
        )?;
        sync_config(root, &path)?;
        return Ok((path, head));
    }
    let path = vacant.ok_or_else(|| MaccError::Validation("No vacant supervisor slot".into()))?;
    git(
        root,
        &[
            "worktree",
            "add",
            "--detach",
            path.to_str()
                .ok_or_else(|| MaccError::Validation("Invalid supervisor path".into()))?,
            &head,
        ],
    )?;
    std::fs::create_dir_all(path.join(".macc")).map_err(|e| io(&path, e))?;
    // This repository-wide exclude keeps local evidence out of repair commits.
    let exclude = PathBuf::from(git(&path, &["rev-parse", "--git-path", "info/exclude"])?);
    let exclude = if exclude.is_absolute() {
        exclude
    } else {
        path.join(exclude)
    };
    use std::io::Write;
    std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&exclude)
        .and_then(|mut file| file.write_all(b"\n.macc/\n"))
        .map_err(|e| io(&exclude, e))?;
    write_json(
        &path.join(OWNER),
        &json!({"incident_id":incident,"base":head}),
    )?;
    sync_config(root, &path)?;
    Ok((path, head))
}
fn sync_config(root: &Path, path: &Path) -> Result<()> {
    let cfg = root.join(".macc/macc.yaml");
    if cfg.exists() {
        std::fs::copy(&cfg, path.join(".macc/macc.yaml")).map_err(|e| io(&cfg, e))?;
    }
    Ok(())
}

pub fn safe_to_remove(root: &Path, path: &Path) -> Result<bool> {
    if !git(path, &["status", "--porcelain"])?.is_empty() {
        return Ok(false);
    }
    let head = git(path, &["rev-parse", "HEAD"])?;
    let integrated = git(root, &["rev-parse", "HEAD"])?;
    let result = macc_core::git::run_git_output_mapped(
        root,
        &["merge-base", "--is-ancestor", &head, &integrated],
        "inspect supervisor integration",
    )?;
    match result.status.code() {
        Some(0) => Ok(true),
        Some(1) => Ok(false),
        _ => Err(MaccError::Validation(
            "Cannot verify supervisor commit integration; worktree retained".into(),
        )),
    }
}

/// Copy every supervisor evidence file, plus generated text/log files, before removal.
/// Dependency trees and Git internals are not intervention evidence. Symlinks are never followed.
pub fn archive(path: &Path, dir: &Path) -> Result<()> {
    fn copy(source: &Path, relative: &Path, target: &Path) -> Result<()> {
        for entry in std::fs::read_dir(source).map_err(|e| io(source, e))? {
            let entry = entry.map_err(|e| io(source, e))?;
            let kind = entry.file_type().map_err(|e| io(&entry.path(), e))?;
            let name = entry.file_name();
            let rel = relative.join(&name);
            if kind.is_symlink() {
                continue;
            }
            if kind.is_dir() {
                if name == ".git" || name == "node_modules" {
                    continue;
                }
                copy(&entry.path(), &rel, target)?;
            } else if kind.is_file()
                && (rel.starts_with(".macc")
                    || matches!(
                        rel.extension().and_then(|s| s.to_str()),
                        Some("txt" | "log")
                    ))
            {
                let dest = target.join(&rel);
                std::fs::create_dir_all(dest.parent().unwrap()).map_err(|e| io(&dest, e))?;
                std::fs::copy(entry.path(), &dest).map_err(|e| io(&dest, e))?;
            }
        }
        Ok(())
    }
    copy(path, Path::new(""), &dir.join("evidence"))?;
    std::fs::write(
        dir.join("worktree-status.txt"),
        git(path, &["status", "--porcelain"])?,
    )
    .map_err(|e| io(dir, e))?;
    std::fs::write(
        dir.join("worktree-diff.txt"),
        git(path, &["diff", "HEAD", "--binary"])?,
    )
    .map_err(|e| io(dir, e))?;
    Ok(())
}

pub fn finish(root: &Path, path: &Path, dir: &Path, close: bool) -> Result<bool> {
    archive(path, dir)?;
    if !close || !safe_to_remove(root, path)? {
        return Ok(false);
    }
    if path.parent() != Some(root.join(".macc/worktree").as_path()) || !path.join(OWNER).is_file() {
        return Err(MaccError::Validation(
            "Refusing cleanup of an unowned supervisor slot".into(),
        ));
    }
    git(
        root,
        &[
            "worktree",
            "remove",
            path.to_str()
                .ok_or_else(|| MaccError::Validation("Invalid supervisor path".into()))?,
        ],
    )?;
    Ok(true)
}

#[cfg(test)]
mod tests {
    use super::*;
    fn fixture() -> tempfile::TempDir {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path();
        git(root, &["init", "-q", "-b", "main"]).unwrap();
        git(root, &["config", "user.name", "Test"]).unwrap();
        git(root, &["config", "user.email", "test@example.invalid"]).unwrap();
        std::fs::write(root.join(".gitignore"), ".macc/\n").unwrap();
        std::fs::write(root.join("source.txt"), "original\n").unwrap();
        git(root, &["add", "."]).unwrap();
        git(root, &["commit", "-qm", "initial"]).unwrap();
        temp
    }
    #[test]
    fn same_incident_reuses_dirty_slot_and_archive_precedes_clean_cleanup() {
        let temp = fixture();
        let root = temp.path();
        let (slot, base) = prepare(root, "incident-a").unwrap();
        assert_eq!(slot, root.join(".macc/worktree/supervisor-01"));
        std::fs::write(slot.join("source.txt"), "partial\n").unwrap();
        git(&slot, &["add", "source.txt"]).unwrap();
        std::fs::write(slot.join("untracked.txt"), "unfinished\n").unwrap();
        std::fs::write(slot.join(".macc/diagnosis.log"), "evidence\n").unwrap();
        let before = git(&slot, &["diff", "--cached", "--binary"]).unwrap();
        let (same, new_base) = prepare(root, "incident-a").unwrap();
        assert_eq!(same, slot);
        assert_eq!(new_base, base);
        assert_eq!(
            git(&slot, &["diff", "--cached", "--binary"]).unwrap(),
            before
        );
        let logs = root.join(".macc/log/supervisor/attempt-1");
        assert!(!finish(root, &slot, &logs, true).unwrap());
        assert!(slot.exists());
        assert_eq!(
            std::fs::read_to_string(logs.join("evidence/.macc/diagnosis.log")).unwrap(),
            "evidence\n"
        );
        assert!(logs.join("evidence/untracked.txt").exists());
        let (other, _) = prepare(root, "incident-b").unwrap();
        assert_eq!(other, root.join(".macc/worktree/supervisor-02"));
        assert!(finish(root, &other, &root.join(".macc/log/supervisor/other"), true).unwrap());
        assert!(!other.exists());
    }
    #[test]
    fn archive_failure_never_removes_the_slot() {
        let temp = fixture();
        let root = temp.path();
        let (slot, _) = prepare(root, "a").unwrap();
        let logs = root.join("archive-blocker");
        std::fs::write(&logs, "occupied").unwrap();
        assert!(finish(root, &slot, &logs, true).is_err());
        assert!(slot.exists());
    }
    #[test]
    fn unmerged_commits_and_changed_base_keep_repairs_reserved() {
        let temp = fixture();
        let root = temp.path();
        let (slot, _) = prepare(root, "a").unwrap();
        std::fs::write(slot.join("source.txt"), "repair\n").unwrap();
        git(&slot, &["add", "."]).unwrap();
        git(&slot, &["commit", "-qm", "repair"]).unwrap();
        assert!(!finish(root, &slot, &root.join(".macc/log/supervisor/a"), true).unwrap());
        std::fs::write(root.join("source.txt"), "new base\n").unwrap();
        git(root, &["add", "."]).unwrap();
        git(root, &["commit", "-qm", "advance"]).unwrap();
        assert!(prepare(root, "a").is_err());
        assert!(slot.exists());
    }
    #[test]
    fn clean_slot_updates_base_and_archived_evidence_survives_removal() {
        let temp = fixture();
        let root = temp.path();
        let (slot, _) = prepare(root, "a").unwrap();
        std::fs::write(slot.join(".macc/repair-notes.md"), "proof").unwrap();
        std::fs::write(root.join("source.txt"), "new base\n").unwrap();
        git(root, &["add", "."]).unwrap();
        git(root, &["commit", "-qm", "advance"]).unwrap();
        let (same, base) = prepare(root, "a").unwrap();
        assert_eq!(same, slot);
        assert_eq!(git(&same, &["rev-parse", "HEAD"]).unwrap(), base);
        let logs = root.join(".macc/log/supervisor/a");
        assert!(finish(root, &slot, &logs, true).unwrap());
        assert!(!slot.exists());
        assert_eq!(
            std::fs::read_to_string(logs.join("evidence/.macc/repair-notes.md")).unwrap(),
            "proof"
        );
    }
}
