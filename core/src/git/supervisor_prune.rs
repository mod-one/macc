//! Conservative repair-ref cleanup: never discard unmerged or checked-out work.
use super::{
    delete_local_branch, merge_base_is_ancestor, resolve_ref, run_git_output,
    worktree_list_porcelain,
};
use crate::{config::CanonicalConfig, MaccError, ProjectPaths, Result};
use std::path::Path;

const PREFIX: &str = "refs/heads/macc/supervisor/";

/// Explicit pruning may override retention, but never integration/worktree checks.
/// `ignore_retention` replaces the former destructive `force` behavior.
pub fn prune_merged_supervisor_branches(
    root: &Path,
    target_ref: Option<&str>,
    ignore_retention: bool,
) -> Result<Vec<String>> {
    prune(root, target_ref, ignore_retention, None)
}

/// Automatic cleanup of one owned repair branch, respecting project retention.
pub fn prune_merged_supervisor_branch(
    root: &Path,
    branch: &str,
    target_ref: Option<&str>,
) -> Result<bool> {
    Ok(!prune(root, target_ref, false, Some(branch))?.is_empty())
}

fn prune(
    root: &Path,
    target_ref: Option<&str>,
    ignore_retention: bool,
    only_branch: Option<&str>,
) -> Result<Vec<String>> {
    let path = ProjectPaths::from_root(root).config_path;
    let config = if path.exists() {
        crate::load_canonical_config(&path)?
    } else {
        CanonicalConfig::default()
    };
    if !ignore_retention
        && config
            .automation
            .supervisor
            .as_ref()
            .is_some_and(|c| c.retain_repair_branches)
    {
        return Ok(Vec::new());
    }
    let output = run_git_output(
        root,
        &["for-each-ref", "--format=%(refname)", PREFIX],
        "list supervisor repair branches",
    )?;
    if !output.status.success() {
        return Err(MaccError::Git {
            operation: "supervisor_prune".into(),
            message: String::from_utf8_lossy(&output.stderr).into_owned(),
        });
    }
    let branches: Vec<_> = String::from_utf8_lossy(&output.stdout)
        .lines()
        .filter(|reference| reference.starts_with(PREFIX))
        .map(str::to_owned)
        .filter(|reference| {
            only_branch.is_none_or(|name| reference.strip_prefix("refs/heads/") == Some(name))
        })
        .collect();
    if branches.is_empty() {
        return Ok(Vec::new());
    }

    // Never consider a repair integrated merely because it is reachable from
    // some unrelated main/master/HEAD. Select one actual publication target.
    let configured = target_ref.or_else(|| {
        config
            .automation
            .coordinator
            .as_ref()
            .and_then(|c| c.reference_branch.as_deref())
    });
    let target = if let Some(name) = configured {
        let reference = if name.starts_with("refs/heads/") {
            name.to_owned()
        } else {
            format!("refs/heads/{name}")
        };
        if reference.starts_with(PREFIX) {
            return Err(MaccError::Validation(
                "A supervisor repair branch cannot be its own integration target".into(),
            ));
        }
        resolve_ref(root, &format!("{reference}^{{commit}}")).ok_or_else(|| {
            MaccError::Validation(format!(
                "Cannot verify supervisor integration target '{name}'; repair branches retained"
            ))
        })?
    } else {
        let current = run_git_output(
            root,
            &["symbolic-ref", "--quiet", "HEAD"],
            "resolve cleanup target",
        )?;
        let current = String::from_utf8_lossy(&current.stdout).trim().to_owned();
        let resolved = ["refs/heads/main", current.as_str(), "refs/heads/master"]
            .into_iter()
            .filter(|reference| !reference.is_empty() && !reference.starts_with(PREFIX))
            .find_map(|reference| resolve_ref(root, &format!("{reference}^{{commit}}")))
            .ok_or_else(|| {
                MaccError::Validation(
                    "No verified supervisor integration target; repair branches retained".into(),
                )
            })?;
        resolved
    };
    let mut deleted = Vec::new();
    for reference in branches {
        let branch = reference.strip_prefix("refs/heads/").unwrap();
        // Propagate inspection failures. Recheck per ref, and leave Git's own
        // checked-out/merge guards enabled for the final deletion as well.
        if worktree_list_porcelain(root)?
            .lines()
            .any(|line| line == format!("branch {reference}"))
        {
            continue;
        }
        let Some(tip) = resolve_ref(root, &reference) else {
            continue;
        };
        if !merge_base_is_ancestor(root, &tip, &target)? {
            continue;
        }
        if delete_local_branch(root, branch, false).is_ok() {
            deleted.push(branch.to_owned());
        }
    }
    Ok(deleted)
}

#[cfg(test)]
mod tests {
    use super::*;
    fn git(root: &Path, args: &[&str]) {
        let out = run_git_output(root, args, "prune fixture").unwrap();
        assert!(
            out.status.success(),
            "{args:?}: {}",
            String::from_utf8_lossy(&out.stderr)
        );
    }
    fn fixture(retain: bool) -> tempfile::TempDir {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        git(root, &["init", "-q", "-b", "main"]);
        git(root, &["config", "user.name", "Test"]);
        git(root, &["config", "user.email", "test@example.invalid"]);
        std::fs::write(root.join(".gitignore"), ".macc/\n.env\nworkers/\n").unwrap();
        std::fs::write(root.join("source.txt"), "base\n").unwrap();
        git(root, &["add", "."]);
        git(root, &["commit", "-qm", "base"]);
        let mut config = CanonicalConfig::default();
        config.automation.supervisor = Some(crate::supervisor::SupervisorConfig {
            retain_repair_branches: retain,
            ..Default::default()
        });
        crate::save_canonical_config(&ProjectPaths::from_root(root), &config).unwrap();
        tmp
    }
    fn exists(root: &Path, branch: &str) -> bool {
        resolve_ref(root, &format!("refs/heads/{branch}")).is_some()
    }
    fn unmerged(root: &Path, branch: &str) {
        git(root, &["checkout", "-qb", branch]);
        std::fs::write(root.join("source.txt"), "unmerged repair\n").unwrap();
        git(root, &["commit", "-qam", "repair"]);
        git(root, &["checkout", "-q", "main"]);
    }
    #[test]
    fn force_never_deletes_unmerged_repairs_or_other_namespaces() {
        let tmp = fixture(false);
        let root = tmp.path();
        git(root, &["branch", "macc/supervisor/merged"]);
        git(root, &["branch", "operator-branch"]);
        unmerged(root, "macc/supervisor/unmerged");
        assert_eq!(
            prune_merged_supervisor_branches(root, Some("main"), true).unwrap(),
            vec!["macc/supervisor/merged"]
        );
        assert!(exists(root, "macc/supervisor/unmerged"));
        assert!(exists(root, "operator-branch"));
    }
    #[test]
    fn retention_survives_generic_cleanup_and_force_only_overrides_retention() {
        let tmp = fixture(true);
        let root = tmp.path();
        git(root, &["branch", "macc/supervisor/merged"]);
        unmerged(root, "macc/supervisor/unmerged");
        crate::prune_worktrees(root).unwrap();
        assert_eq!(
            crate::domain::worktree::remove_all_worktrees(root, true).unwrap(),
            0
        );
        assert!(prune_merged_supervisor_branches(root, None, false)
            .unwrap()
            .is_empty());
        assert!(exists(root, "macc/supervisor/merged"));
        assert_eq!(
            prune_merged_supervisor_branches(root, None, true).unwrap(),
            vec!["macc/supervisor/merged"]
        );
        assert!(exists(root, "macc/supervisor/unmerged"));
    }
    #[test]
    fn checked_out_clean_dirty_staged_untracked_and_ignored_files_are_untouched() {
        for kind in ["clean", "dirty", "staged", "untracked", "ignored"] {
            let tmp = fixture(false);
            let root = tmp.path();
            let branch = "macc/supervisor/in-use";
            let slot = root.join("workers/repair");
            git(
                root,
                &[
                    "worktree",
                    "add",
                    "-b",
                    branch,
                    slot.to_str().unwrap(),
                    "main",
                ],
            );
            let file = match kind {
                "ignored" => ".env",
                "untracked" => "notes.txt",
                _ => "source.txt",
            };
            if kind != "clean" {
                std::fs::write(slot.join(file), "preserve exactly\n").unwrap();
            }
            if kind == "staged" {
                git(&slot, &["add", file]);
            }
            let bytes = std::fs::read(slot.join(file)).unwrap();
            assert!(prune_merged_supervisor_branches(root, Some("main"), true)
                .unwrap()
                .is_empty());
            assert!(exists(root, branch));
            assert_eq!(std::fs::read(slot.join(file)).unwrap(), bytes, "{kind}");
        }
    }
    #[test]
    fn only_configured_integration_target_counts_and_bad_config_preserves_refs() {
        let tmp = fixture(false);
        let root = tmp.path();
        git(root, &["branch", "release"]);
        unmerged(root, "macc/supervisor/repair");
        git(root, &["merge", "--ff-only", "macc/supervisor/repair"]);
        assert!(
            prune_merged_supervisor_branches(root, Some("release"), true)
                .unwrap()
                .is_empty()
        );
        assert!(prune_merged_supervisor_branches(root, Some("missing"), true).is_err());
        assert!(exists(root, "macc/supervisor/repair"));
        std::fs::write(
            ProjectPaths::from_root(root).config_path,
            "automation: [invalid",
        )
        .unwrap();
        assert!(prune_merged_supervisor_branches(root, Some("main"), true).is_err());
        assert!(exists(root, "macc/supervisor/repair"));
    }
    #[test]
    fn bulk_cleanup_preserves_worker_changes_and_unmerged_commits() {
        for remove_branches in [false, true] {
            for kind in [
                "dirty",
                "staged",
                "untracked",
                "ignored",
                "unmerged",
                "clean",
            ] {
                let tmp = fixture(false);
                let root = tmp.path();
                let slot = root.join("workers/worker");
                let branch = "ai/test/worker";
                git(
                    root,
                    &[
                        "worktree",
                        "add",
                        "-b",
                        branch,
                        slot.to_str().unwrap(),
                        "main",
                    ],
                );
                let file = match kind {
                    "ignored" => ".env",
                    "untracked" => "notes.txt",
                    _ => "source.txt",
                };
                if kind != "clean" {
                    std::fs::write(slot.join(file), "preserve worker\n").unwrap();
                }
                if matches!(kind, "staged" | "unmerged") {
                    git(&slot, &["add", file]);
                }
                if kind == "unmerged" {
                    git(&slot, &["commit", "-qm", "unfinished delivery"]);
                }
                let removed =
                    crate::domain::worktree::remove_all_worktrees(root, remove_branches).unwrap();
                if kind == "clean" {
                    assert_eq!(removed, 1);
                    assert!(!slot.exists());
                    assert_eq!(exists(root, branch), !remove_branches);
                } else {
                    assert_eq!(removed, 0, "{kind}");
                    assert!(exists(root, branch));
                    assert_eq!(
                        std::fs::read_to_string(slot.join(file)).unwrap(),
                        "preserve worker\n",
                        "{kind}"
                    );
                }
            }
        }
    }
}
