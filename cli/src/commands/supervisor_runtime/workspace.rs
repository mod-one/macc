//! Owned supervisor slots and durable evidence, independent of attempt log directories.
use super::{io, repair::git, report::write_json};
use macc_core::{MaccError, Result};
use serde_json::{json, Value};
use std::path::{Path, PathBuf};

const OWNER: &str = ".macc/supervisor-owner.json";

/// Present while a slot temporarily holds the unmodified base for a baseline
/// validation run; names the tree that restores the repair.
const RESTORE: &str = ".macc/supervisor-restore-tree";

/// Where a repair is based and published: the coordinator's reference branch,
/// so re-dispatched tasks start from the repaired source.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Target {
    /// `None` only for a detached project without a resolvable reference branch.
    pub branch: Option<String>,
    pub base: String,
}

pub fn target(
    root: &Path,
    coordinator: Option<&macc_core::config::CoordinatorConfig>,
) -> Result<Target> {
    let reference =
        macc_core::config::CoordinatorConfigResolved::resolve(coordinator).reference_branch;
    let current = git(root, &["branch", "--show-current"])?;
    for branch in [reference, current] {
        if branch.is_empty() {
            continue;
        }
        if let Some(base) = macc_core::git::resolve_ref(root, &format!("refs/heads/{branch}")) {
            return Ok(Target {
                branch: Some(branch),
                base,
            });
        }
    }
    Ok(Target {
        branch: None,
        base: git(root, &["rev-parse", "HEAD"])?,
    })
}

pub fn prepare(
    root: &Path,
    incident: &str,
    target: &Target,
    retain_branch: bool,
) -> Result<PathBuf> {
    let head = target.base.as_str();
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
        restore_repair(&path)?;
        let base = owner["base"]
            .as_str()
            .ok_or_else(|| MaccError::Validation("Supervisor slot has no base".into()))?;
        let branch = if base != head {
            if !safe_to_remove(root, &path)? {
                return Err(MaccError::Validation(
                    "Project base changed; supervisor work preserved for operator recovery".into(),
                ));
            }
            let old_repair_branch = owner["repair_branch"].as_str().map(String::from);
            let branch = repair_branch(root, incident, head)?;
            if macc_core::git::resolve_ref(root, &format!("refs/heads/{branch}")).is_some() {
                git(&path, &["checkout", &branch])?;
            } else {
                git(&path, &["checkout", "-b", &branch, head])?;
            }
            if !retain_branch {
                if let Some(old_branch) = old_repair_branch {
                    if old_branch != branch && old_branch.starts_with("macc/supervisor/") {
                        macc_core::git::prune_merged_supervisor_branch(
                            root,
                            &old_branch,
                            target.branch.as_deref(),
                        )?;
                    }
                }
            }
            branch
        } else {
            // Older markers did not record the repair ref. Use the actual
            // checkout rather than inventing a new, unrelated branch name.
            git(&path, &["branch", "--show-current"])?
        };
        let marker = json!({
            "incident_id": incident,
            "base": head,
            "branch": target.branch,
            "repair_branch": &branch,
        });
        write_json(&path.join(OWNER), &marker)?;
        sync_config(root, &path)?;
        return Ok(path);
    }
    let path = vacant.ok_or_else(|| MaccError::Validation("No vacant supervisor slot".into()))?;
    let branch = repair_branch(root, incident, head)?;
    let slot = path
        .to_str()
        .ok_or_else(|| MaccError::Validation("Invalid supervisor path".into()))?;
    if macc_core::git::resolve_ref(root, &format!("refs/heads/{branch}")).is_some() {
        git(root, &["worktree", "add", slot, &branch])?;
    } else {
        git(root, &["worktree", "add", "-b", &branch, slot, head])?;
    }
    std::fs::create_dir_all(path.join(".macc")).map_err(|e| io(&path, e))?;
    // This repository-wide exclude keeps local evidence out of repair commits.
    let exclude = PathBuf::from(git(&path, &["rev-parse", "--git-path", "info/exclude"])?);
    let exclude = if exclude.is_absolute() {
        exclude
    } else {
        path.join(exclude)
    };
    let excluded = std::fs::read_to_string(&exclude)
        .is_ok_and(|text| text.lines().any(|line| line.trim() == ".macc/"));
    if !excluded {
        use std::io::Write;
        if let Some(parent) = exclude.parent() {
            std::fs::create_dir_all(parent).map_err(|e| io(&exclude, e))?;
        }
        std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&exclude)
            .and_then(|mut file| file.write_all(b"\n.macc/\n"))
            .map_err(|e| io(&exclude, e))?;
    }
    let marker = json!({
        "incident_id": incident,
        "base": head,
        "branch": target.branch,
        "repair_branch": &branch,
    });
    write_json(&path.join(OWNER), &marker)?;
    sync_config(root, &path)?;
    Ok(path)
}
/// Named repair refs isolate and validate fixes. When retain_branch is false,
/// cleanly integrated or no-op repair branches are pruned upon slot removal.
fn repair_branch(root: &Path, incident: &str, base: &str) -> Result<String> {
    let prefix = format!(
        "macc/supervisor/{}-{}",
        super::report::slug(incident),
        &base[..12.min(base.len())]
    );
    for n in 0..1000 {
        let branch = if n == 0 {
            prefix.clone()
        } else {
            format!("{prefix}-{n}")
        };
        match macc_core::git::resolve_ref(root, &format!("refs/heads/{branch}")) {
            None => return Ok(branch),
            Some(head) if head == base => return Ok(branch),
            _ => continue,
        }
    }
    Err(MaccError::Validation(
        "No vacant supervisor repair branch".into(),
    ))
}

fn sync_config(root: &Path, path: &Path) -> Result<()> {
    let cfg = root.join(".macc/macc.yaml");
    if cfg.exists() {
        std::fs::copy(&cfg, path.join(".macc/macc.yaml")).map_err(|e| io(&cfg, e))?;
    }
    Ok(())
}

/// Content identity of a slot (tracked, staged and untracked files), computed
/// without touching the slot's own index.
pub fn snapshot(path: &Path) -> Result<String> {
    macc_core::git::worktree_content_tree(path)
}

/// Swap the slot to the unmodified base, keeping the repair as a tree object.
/// The marker is written before any file changes so an interrupted baseline run
/// is always restored by [`restore_repair`].
pub fn enter_baseline(path: &Path) -> Result<()> {
    git(path, &["add", "--all"])?;
    let tree = git(path, &["write-tree"])?;
    std::fs::write(path.join(RESTORE), &tree).map_err(|e| io(path, e))?;
    git(path, &["read-tree", "--reset", "-u", "HEAD"])?;
    Ok(())
}

/// Return the slot from the baseline state to the staged repair. Everything
/// discarded here was produced by the baseline run: the repair lives in the tree.
pub fn restore_repair(path: &Path) -> Result<()> {
    let marker = path.join(RESTORE);
    let tree = match std::fs::read_to_string(&marker) {
        Ok(tree) => tree.trim().to_owned(),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(e) => return Err(io(&marker, e)),
    };
    git(path, &["reset", "--hard", "HEAD"])?;
    git(path, &["clean", "-fd"])?;
    git(path, &["read-tree", "--reset", "-u", &tree])?;
    std::fs::remove_file(&marker).map_err(|e| io(&marker, e))
}

pub fn safe_to_remove(root: &Path, path: &Path) -> Result<bool> {
    // A slot parked on the base still owns an unrestored repair.
    if path.join(RESTORE).exists() {
        return Ok(false);
    }
    if !git(path, &["status", "--porcelain"])?.is_empty() {
        return Ok(false);
    }
    let ignored = macc_core::git::run_git_output_mapped(
        path,
        &[
            "ls-files",
            "--others",
            "--ignored",
            "--exclude-standard",
            "-z",
        ],
        "inspect unarchived ignored repair files",
    )?;
    // .macc evidence is archived before removal. Any other ignored files
    // (credentials, generated outputs, dependencies, etc.) keep the slot.
    if !ignored.status.success()
        || ignored
            .stdout
            .split(|b| *b == 0)
            .any(|name| !name.is_empty() && !name.starts_with(b".macc/"))
    {
        return Ok(false);
    }
    let head = git(path, &["rev-parse", "HEAD"])?;
    // Integrated means reachable from the branch the slot publishes to.
    let branch = std::fs::read_to_string(path.join(OWNER))
        .ok()
        .and_then(|text| serde_json::from_str::<Value>(&text).ok())
        .and_then(|owner| owner["branch"].as_str().map(str::to_owned))
        .and_then(|branch| macc_core::git::resolve_ref(root, &format!("refs/heads/{branch}")));
    let integrated = match branch {
        Some(tip) => tip,
        None => git(root, &["rev-parse", "HEAD"])?,
    };
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

pub fn finish(
    root: &Path,
    path: &Path,
    dir: &Path,
    close: bool,
    retain_branch: bool,
) -> Result<bool> {
    // An interrupted baseline run leaves the base checked out: archive the repair.
    restore_repair(path)?;
    archive(path, dir)?;
    if !close || !safe_to_remove(root, path)? {
        return Ok(false);
    }
    if path.parent() != Some(root.join(".macc/worktree").as_path()) || !path.join(OWNER).is_file() {
        return Err(MaccError::Validation(
            "Refusing cleanup of an unowned supervisor slot".into(),
        ));
    }
    let owner: Value =
        serde_json::from_str(&std::fs::read_to_string(path.join(OWNER)).map_err(|e| io(path, e))?)
            .map_err(|e| MaccError::Validation(e.to_string()))?;
    let branch = git(path, &["branch", "--show-current"])?;
    if owner["repair_branch"]
        .as_str()
        .is_some_and(|expected| expected != branch)
    {
        return Err(MaccError::Validation(
            "Supervisor checkout differs from its owner marker; worktree and branches retained"
                .into(),
        ));
    }
    let target = owner["branch"].as_str();

    git(
        root,
        &[
            "worktree",
            "remove",
            path.to_str()
                .ok_or_else(|| MaccError::Validation("Invalid supervisor path".into()))?,
        ],
    )?;

    if !retain_branch {
        if branch.starts_with("macc/supervisor/") {
            macc_core::git::prune_merged_supervisor_branch(root, &branch, target)?;
        }
    }
    Ok(true)
}

#[cfg(test)]
mod tests {
    use super::*;
    fn open(root: &Path, incident: &str) -> Result<(PathBuf, String)> {
        let target = target(root, None)?;
        Ok((prepare(root, incident, &target, false)?, target.base))
    }
    fn open_with_retention(root: &Path, incident: &str, retain: bool) -> Result<(PathBuf, String)> {
        let target = target(root, None)?;
        Ok((prepare(root, incident, &target, retain)?, target.base))
    }
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
        let (slot, base) = open(root, "incident-a").unwrap();
        assert_eq!(slot, root.join(".macc/worktree/supervisor-01"));
        std::fs::write(slot.join("source.txt"), "partial\n").unwrap();
        git(&slot, &["add", "source.txt"]).unwrap();
        std::fs::write(slot.join("untracked.txt"), "unfinished\n").unwrap();
        std::fs::write(slot.join(".macc/diagnosis.log"), "evidence\n").unwrap();
        let before = git(&slot, &["diff", "--cached", "--binary"]).unwrap();
        let (same, new_base) = open(root, "incident-a").unwrap();
        assert_eq!(same, slot);
        assert_eq!(new_base, base);
        assert_eq!(
            git(&slot, &["diff", "--cached", "--binary"]).unwrap(),
            before
        );
        let logs = root.join(".macc/log/supervisor/attempt-1");
        assert!(!finish(root, &slot, &logs, true, false).unwrap());
        assert!(slot.exists());
        assert_eq!(
            std::fs::read_to_string(logs.join("evidence/.macc/diagnosis.log")).unwrap(),
            "evidence\n"
        );
        assert!(logs.join("evidence/untracked.txt").exists());
        let (other, _) = open(root, "incident-b").unwrap();
        assert_eq!(other, root.join(".macc/worktree/supervisor-02"));
        assert!(finish(
            root,
            &other,
            &root.join(".macc/log/supervisor/other"),
            true,
            false
        )
        .unwrap());
        assert!(!other.exists());
        let branches = git(root, &["branch", "--list", "macc/supervisor/*"]).unwrap();
        assert!(!branches.contains("incident-b"));
    }
    #[test]
    fn retained_repair_branch_survives_cleanup_when_configured() {
        let temp = fixture();
        let root = temp.path();
        let (slot, _) = open_with_retention(root, "incident-c", true).unwrap();
        assert!(slot.exists());
        let logs = root.join(".macc/log/supervisor/c");
        assert!(finish(root, &slot, &logs, true, true).unwrap());
        assert!(!slot.exists());
        let branches = git(root, &["branch", "--list", "macc/supervisor/*"]).unwrap();
        assert!(branches.contains("incident-c"));
    }
    #[test]
    fn ignored_files_outside_archived_evidence_keep_the_slot_and_branch() {
        let temp = fixture();
        let root = temp.path();
        let (slot, _) = open(root, "ignored").unwrap();
        let exclude = git(root, &["rev-parse", "--git-path", "info/exclude"]).unwrap();
        let exclude = root.join(exclude);
        use std::io::Write;
        std::fs::OpenOptions::new()
            .append(true)
            .open(exclude)
            .unwrap()
            .write_all(b"\n.env\n")
            .unwrap();
        std::fs::write(slot.join(".env"), "private work\n").unwrap();
        assert!(!finish(
            root,
            &slot,
            &root.join(".macc/log/supervisor/ignored"),
            true,
            false
        )
        .unwrap());
        assert_eq!(
            std::fs::read_to_string(slot.join(".env")).unwrap(),
            "private work\n"
        );
        assert!(git(root, &["branch", "--list", "macc/supervisor/*"])
            .unwrap()
            .contains("ignored"));
    }
    #[test]
    fn legacy_owner_marker_reuses_the_actual_branch() {
        let temp = fixture();
        let root = temp.path();
        let (slot, _) = open(root, "legacy").unwrap();
        let branch = git(&slot, &["branch", "--show-current"]).unwrap();
        let mut owner: Value =
            serde_json::from_str(&std::fs::read_to_string(slot.join(OWNER)).unwrap()).unwrap();
        owner.as_object_mut().unwrap().remove("repair_branch");
        write_json(&slot.join(OWNER), &owner).unwrap();
        open(root, "legacy").unwrap();
        let owner: Value =
            serde_json::from_str(&std::fs::read_to_string(slot.join(OWNER)).unwrap()).unwrap();
        assert_eq!(owner["repair_branch"].as_str(), Some(branch.as_str()));
        assert!(finish(
            root,
            &slot,
            &root.join(".macc/log/supervisor/legacy"),
            true,
            false
        )
        .unwrap());
        assert!(git(root, &["branch", "--list", "macc/supervisor/*"])
            .unwrap()
            .is_empty());
    }
    #[test]
    fn archive_failure_never_removes_the_slot() {
        let temp = fixture();
        let root = temp.path();
        let (slot, _) = open(root, "a").unwrap();
        let logs = root.join("archive-blocker");
        std::fs::write(&logs, "occupied").unwrap();
        assert!(finish(root, &slot, &logs, true, false).is_err());
        assert!(slot.exists());
    }
    #[test]
    fn unmerged_commits_and_changed_base_keep_repairs_reserved() {
        let temp = fixture();
        let root = temp.path();
        let (slot, _) = open(root, "a").unwrap();
        std::fs::write(slot.join("source.txt"), "repair\n").unwrap();
        git(&slot, &["add", "."]).unwrap();
        git(&slot, &["commit", "-qm", "repair"]).unwrap();
        assert!(!finish(
            root,
            &slot,
            &root.join(".macc/log/supervisor/a"),
            true,
            false
        )
        .unwrap());
        std::fs::write(root.join("source.txt"), "new base\n").unwrap();
        git(root, &["add", "."]).unwrap();
        git(root, &["commit", "-qm", "advance"]).unwrap();
        assert!(open(root, "a").is_err());
        assert!(slot.exists());
    }
    #[test]
    fn clean_slot_updates_base_and_archived_evidence_survives_removal() {
        let temp = fixture();
        let root = temp.path();
        let (slot, _) = open(root, "a").unwrap();
        std::fs::write(slot.join(".macc/repair-notes.md"), "proof").unwrap();
        std::fs::write(root.join("source.txt"), "new base\n").unwrap();
        git(root, &["add", "."]).unwrap();
        git(root, &["commit", "-qm", "advance"]).unwrap();
        let (same, base) = open(root, "a").unwrap();
        assert_eq!(same, slot);
        assert_eq!(git(&same, &["rev-parse", "HEAD"]).unwrap(), base);
        let logs = root.join(".macc/log/supervisor/a");
        assert!(finish(root, &slot, &logs, true, false).unwrap());
        assert!(!slot.exists());
        assert_eq!(
            std::fs::read_to_string(logs.join("evidence/.macc/repair-notes.md")).unwrap(),
            "proof"
        );
        let branches = git(root, &["branch", "--list", "macc/supervisor/*"]).unwrap();
        assert!(branches.is_empty());
    }
}
