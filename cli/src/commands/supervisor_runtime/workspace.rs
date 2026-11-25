//! Owned supervisor slots and durable evidence, independent of attempt log directories.
mod baseline;
mod config_copy;
mod creation;
use super::{io, repair::git, report::write_json};
pub use baseline::{enter_baseline, restore_repair};
pub(super) use creation::reconcile;
use macc_core::supervisor::cleanup::{Cleanup, Retention};
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
    macc_core::gitignore_policy::ensure_local_exclusions(root)?;
    reconcile(root)?;
    let head = target.base.as_str();
    let pool = root.join(".macc/worktree");
    std::fs::create_dir_all(&pool).map_err(|e| io(&pool, e))?;
    let mut vacant = None;
    let mut unidentified = None;
    for index in 1..=999 {
        let path = pool.join(format!("supervisor-{index:02}"));
        if !path.exists() {
            vacant.get_or_insert(index);
            continue;
        }
        let marker = path.join(OWNER);
        let owner: Value = match std::fs::read(&marker)
            .and_then(|bytes| serde_json::from_slice(&bytes).map_err(std::io::Error::from))
        {
            Ok(owner) => owner,
            Err(error) => {
                unidentified.get_or_insert_with(|| MaccError::Validation(format!(
                    "Cannot identify supervisor worktree {}: {error}. Inspect {} and its Git registration before allocating a new slot. Files preserved.",
                    path.display(), marker.display()
                )));
                continue;
            }
        };
        if owner["incident_id"].as_str() != Some(incident) {
            continue;
        }
        restore_repair(root, &path, incident)?;
        let base = owner["base"]
            .as_str()
            .ok_or_else(|| MaccError::Validation("Supervisor slot has no base".into()))?;
        let branch = if base != head {
            if let Some(reason) = retention_reason(root, &path)? {
                return Err(MaccError::Validation(format!(
                    "Project base changed; supervisor work preserved: {reason}"
                )));
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
        macc_core::project_ownership::record_branch(root, &branch)?;
        let marker = json!({
            "incident_id": incident,
            "base": head,
            "branch": target.branch,
            "repair_branch": &branch,
        });
        write_json(&path.join(OWNER), &marker)?;
        config_copy::sync(root, &path)?;
        return Ok(path);
    }
    if let Some(error) = unidentified {
        return Err(error);
    }
    let slot = vacant.ok_or_else(|| MaccError::Validation("No vacant supervisor slot".into()))?;
    let branch = repair_branch(root, incident, head)?;
    let path = creation::create(root, slot, incident, target, branch)?;
    config_copy::sync(root, &path)?;
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
            Some(head)
                if head == base
                    && macc_core::git::worktrees_for_branch(root, &branch)?.is_empty() =>
            {
                return Ok(branch)
            }
            _ => continue,
        }
    }
    Err(MaccError::Validation(
        "No vacant supervisor repair branch".into(),
    ))
}

/// Content identity of a slot (tracked, staged and untracked files), computed
/// without touching the slot's own index.
pub fn snapshot(path: &Path) -> Result<String> {
    macc_core::git::worktree_content_tree(path)
}

fn retention_reason(root: &Path, path: &Path) -> Result<Option<Retention>> {
    // A slot parked on the base still owns an unrestored repair.
    if path.join(RESTORE).exists() {
        return Ok(Some(Retention::PendingRestore));
    }
    if config_copy::customized(path) {
        return Ok(Some(Retention::CustomizedConfig {
            path: config_copy::CONFIG.into(),
        }));
    }
    let status = git(
        path,
        &[
            "status",
            "--porcelain=v1",
            "--no-renames",
            "--untracked-files=all",
            "-z",
        ],
    )?;
    for row in status.split_terminator('\0') {
        let name = row.get(3..).ok_or_else(|| {
            MaccError::Validation("Invalid Git status entry during supervisor cleanup".into())
        })?;
        if name == config_copy::CONFIG && config_copy::disposable(root, path)? {
            continue;
        }
        return Ok(Some(if row.starts_with("?? ") {
            Retention::UntrackedFile { path: name.into() }
        } else {
            Retention::ChangedFile { path: name.into() }
        }));
    }
    if let Some(path) = macc_core::project_ownership::unowned_ignored_files(path)?
        .into_iter()
        .next()
    {
        return Ok(Some(Retention::IgnoredFile { path }));
    }
    let head = git(path, &["rev-parse", "HEAD"])?;
    // Integrated means reachable from the branch the slot publishes to.
    let marker = path.join(OWNER);
    let owner: Value = serde_json::from_slice(&std::fs::read(&marker).map_err(|e| io(&marker, e))?)
        .map_err(|e| MaccError::Validation(e.to_string()))?;
    let integrated = match owner["branch"].as_str() {
        Some(branch) => match macc_core::git::resolve_ref(root, &format!("refs/heads/{branch}")) {
            Some(tip) => tip,
            None => {
                return Ok(Some(Retention::MissingTarget {
                    branch: branch.into(),
                }))
            }
        },
        None => git(root, &["rev-parse", "HEAD"])?,
    };
    let result = macc_core::git::run_git_output_mapped(
        root,
        &["merge-base", "--is-ancestor", &head, &integrated],
        "inspect supervisor integration",
    )?;
    match result.status.code() {
        Some(0) => Ok(None),
        Some(1) => Ok(Some(Retention::UnmergedCommit { commit: head })),
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
    incident: &str,
    path: &Path,
    dir: &Path,
    close: bool,
    retain_branch: bool,
) -> Result<Cleanup> {
    // An interrupted baseline run leaves the base checked out: archive the repair.
    restore_repair(root, path, incident)?;
    archive(path, dir)?;
    let reason = if close {
        retention_reason(root, path)?
    } else {
        Some(Retention::RetryScheduled)
    };
    if let Some(reason) = reason {
        return Ok(Cleanup::Retained { reason });
    }
    let owner = baseline::owner(root, path, incident)?;
    let branch = git(path, &["branch", "--show-current"])?;
    let target = owner["branch"].as_str();
    config_copy::restore_before_removal(root, path)?;

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
    Ok(Cleanup::Removed)
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
    pub(super) fn fixture() -> tempfile::TempDir {
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
    fn a_supervisor_slot_keeps_portable_setup_visible_before_apply() {
        let temp = fixture();
        let root = temp.path();
        git(root, &["rm", ".gitignore"]).unwrap();
        git(root, &["commit", "-qm", "project without shared policy"]).unwrap();
        let (slot, _) = open(root, "visibility").unwrap();
        for path in [root, slot.as_path()] {
            for (file, expected) in [
                (OWNER, true),
                (RESTORE, true),
                (".macc/log/diagnosis.log", true),
                (".macc/macc.yaml", false),
                (".macc/catalog/nested/item.json", false),
                (".macc/skills.lock.json", false),
            ] {
                let status = macc_core::git::run_git_output_mapped(
                    path,
                    &["check-ignore", "-q", "--no-index", "--", file],
                    "verify visibility",
                )
                .unwrap()
                .status;
                assert_eq!(status.code(), Some(if expected { 0 } else { 1 }), "{file}");
            }
        }
        let policy = std::fs::read_to_string(root.join(".git/info/exclude")).unwrap();
        assert!(!policy.lines().any(|line| line == ".macc/"));
    }

    #[test]
    fn only_the_unchanged_portable_config_copy_is_disposable() {
        let temp = fixture();
        let root = temp.path();
        git(root, &["rm", ".gitignore"]).unwrap();
        git(root, &["commit", "-qm", "no portable setup in HEAD"]).unwrap();
        std::fs::create_dir_all(root.join(".macc")).unwrap();
        let primary = macc_core::config::CanonicalConfig::default()
            .to_yaml()
            .unwrap();
        std::fs::write(root.join(config_copy::CONFIG), &primary).unwrap();
        let (slot, _) = open(root, "untracked-config").unwrap();
        assert!(retention_reason(root, &slot).unwrap().is_none());
        std::fs::write(slot.join(config_copy::CONFIG), "customized").unwrap();
        assert!(retention_reason(root, &slot).unwrap().is_some());
        assert!(open(root, "untracked-config").is_err());
        assert_eq!(
            std::fs::read_to_string(slot.join(config_copy::CONFIG)).unwrap(),
            "customized"
        );
        std::fs::write(slot.join(config_copy::CONFIG), &primary).unwrap();
        assert!(matches!(
            finish(
                root,
                "untracked-config",
                &slot,
                &root.join(".macc/log/receipt"),
                true,
                false
            )
            .unwrap(),
            Cleanup::Removed
        ));
        assert!(!slot.exists());
        assert_eq!(
            std::fs::read_to_string(root.join(config_copy::CONFIG)).unwrap(),
            primary
        );
    }

    #[test]
    fn report_distinguishes_scheduled_retry_from_untracked_file() {
        let temp = fixture();
        let root = temp.path();
        let (slot, _) = open(root, "retained").unwrap();
        let dir = root.join(".macc/log/supervisor/retained");
        std::fs::create_dir_all(slot.join(".codex")).unwrap();
        std::fs::write(slot.join(".codex/config.toml"), "personal").unwrap();
        assert_eq!(
            finish(root, "retained", &slot, &dir, false, false).unwrap(),
            Cleanup::Retained {
                reason: Retention::RetryScheduled
            }
        );
        let cleanup = finish(root, "retained", &slot, &dir, true, false).unwrap();
        assert_eq!(
            serde_json::json!(cleanup),
            serde_json::json!({"status":"retained","reason":{"kind":"untracked_file","path":".codex/config.toml"}})
        );
        assert_eq!(
            std::fs::read(slot.join(".codex/config.toml")).unwrap(),
            b"personal"
        );
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
        assert!(matches!(
            finish(root, "incident-a", &slot, &logs, true, false).unwrap(),
            Cleanup::Retained { .. }
        ));
        assert!(slot.exists());
        assert_eq!(
            std::fs::read_to_string(logs.join("evidence/.macc/diagnosis.log")).unwrap(),
            "evidence\n"
        );
        assert!(logs.join("evidence/untracked.txt").exists());
        let (other, _) = open(root, "incident-b").unwrap();
        assert_eq!(other, root.join(".macc/worktree/supervisor-02"));
        assert!(matches!(
            finish(
                root,
                "incident-b",
                &other,
                &root.join(".macc/log/supervisor/other"),
                true,
                false
            )
            .unwrap(),
            Cleanup::Removed
        ));
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
        assert!(matches!(
            finish(root, "incident-c", &slot, &logs, true, true).unwrap(),
            Cleanup::Removed
        ));
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
        assert!(matches!(
            finish(
                root,
                "ignored",
                &slot,
                &root.join(".macc/log/supervisor/ignored"),
                true,
                false
            )
            .unwrap(),
            Cleanup::Retained {
                reason: Retention::IgnoredFile { .. }
            }
        ));
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
        assert!(matches!(
            finish(
                root,
                "legacy",
                &slot,
                &root.join(".macc/log/supervisor/legacy"),
                true,
                false
            )
            .unwrap(),
            Cleanup::Removed
        ));
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
        assert!(finish(root, "a", &slot, &logs, true, false).is_err());
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
        assert!(matches!(
            finish(
                root,
                "a",
                &slot,
                &root.join(".macc/log/supervisor/a"),
                true,
                false
            )
            .unwrap(),
            Cleanup::Retained {
                reason: Retention::UnmergedCommit { .. }
            }
        ));
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
        assert!(matches!(
            finish(root, "a", &slot, &logs, true, false).unwrap(),
            Cleanup::Removed
        ));
        assert!(!slot.exists());
        assert_eq!(
            std::fs::read_to_string(logs.join("evidence/.macc/repair-notes.md")).unwrap(),
            "proof"
        );
        let branches = git(root, &["branch", "--list", "macc/supervisor/*"]).unwrap();
        assert!(branches.is_empty());
    }
}

#[cfg(test)]
mod generated_cleanup_tests {
    use super::*;
    #[test]
    fn proven_generated_ignored_configuration_no_longer_keeps_a_finished_slot() {
        let temp = super::tests::fixture();
        let root = temp.path();
        let target = target(root, None).unwrap();
        let slot = prepare(root, "generated-config", &target, false).unwrap();
        let path = slot.join(".codex/config.toml");
        macc_core::write_if_changed(
            &macc_core::ProjectPaths::from_root(&slot),
            ".codex/config.toml",
            &path,
            b"generated",
            |_| Ok(()),
        )
        .unwrap();
        let common = macc_core::git::git_common_dir(root).unwrap();
        use std::io::Write;
        std::fs::OpenOptions::new()
            .append(true)
            .open(common.join("info/exclude"))
            .unwrap()
            .write_all(b"\n.codex/\n")
            .unwrap();
        assert!(matches!(
            finish(
                root,
                "generated-config",
                &slot,
                &root.join(".macc/log/supervisor/generated-config"),
                true,
                false
            )
            .unwrap(),
            Cleanup::Removed
        ));
        assert!(!slot.exists());
    }
}
