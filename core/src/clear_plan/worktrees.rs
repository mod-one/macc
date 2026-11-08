//! Conservative slot and branch cleanup against the actual publication target.
use super::{Disposition, Item};
use crate::{ProjectPaths, Result};
use std::{collections::BTreeSet, path::Path};

pub(super) fn target(paths: &ProjectPaths) -> Result<String> {
    let config = if paths.config_path.exists() {
        crate::load_canonical_config(&paths.config_path)?
    } else {
        Default::default()
    };
    let reference = config
        .automation
        .coordinator
        .as_ref()
        .and_then(|c| c.reference_branch.as_ref())
        .map(|r| format!("refs/heads/{}", r.trim_start_matches("refs/heads/")))
        .unwrap_or_else(|| "HEAD".into());
    crate::git::resolve_ref(&paths.root, &reference).ok_or_else(|| {
        crate::persistent::error(format!(
            "Cannot verify cleanup target {reference}; no worktree or branch will be discarded"
        ))
    })
}
fn git(root: &Path, args: &[&str]) -> Result<Vec<u8>> {
    let out = crate::git::run_git_output_mapped(root, args, "inspect clear worktree")?;
    if !out.status.success() {
        return Err(crate::persistent::error(String::from_utf8_lossy(
            &out.stderr,
        )));
    }
    Ok(out.stdout)
}
fn owned(paths: &ProjectPaths, entry: &crate::WorktreeEntry, branches: &BTreeSet<String>) -> bool {
    if entry.path.parent() != Some(paths.macc_dir.join("worktree").as_path()) {
        return false;
    }
    let branch = entry
        .branch
        .as_deref()
        .unwrap_or("")
        .trim_start_matches("refs/heads/");
    if branches.contains(branch) {
        return true;
    }
    if let Ok(Some(metadata)) = crate::read_worktree_metadata(&entry.path) {
        if metadata.branch == branch {
            return true;
        }
    }
    let marker = entry.path.join(".macc/supervisor-owner.json");
    if let Ok(bytes) = std::fs::read(marker) {
        if let Ok(owner) = serde_json::from_slice::<serde_json::Value>(&bytes) {
            return branch.starts_with("macc/supervisor/")
                && owner["repair_branch"].as_str() == Some(branch);
        }
    }
    entry
        .path
        .file_name()
        .is_some_and(|name| name == "integration")
        && entry.branch.is_none()
}
pub(crate) fn inspect(paths: &ProjectPaths) -> Result<Vec<Item>> {
    if crate::git::git_common_dir(&paths.root).is_err() {
        return Ok(vec![]);
    }
    crate::persistent::no_symlink(&paths.macc_dir.join("worktree"))?;
    let inventory = crate::project_ownership::read(paths)?;
    let target = target(paths);
    let mut known_branches = inventory.branches;
    let entries = crate::list_worktrees(&paths.root)?;
    let mut checked = BTreeSet::new();
    let mut result = vec![];
    for entry in entries {
        if let Some(branch) = &entry.branch {
            checked.insert(branch.clone());
        }
        if !entry.path.starts_with(&paths.macc_dir) {
            continue;
        }
        let rel = crate::relative_to_root(paths, &entry.path)
            .ok_or_else(|| crate::persistent::error("Invalid worktree path"))?;
        let branch = entry
            .branch
            .as_deref()
            .map(|b| b.trim_start_matches("refs/heads/").to_owned());
        let mut reason = None;
        if branch
            .as_deref()
            .is_some_and(|b| publication_branch(paths, b).unwrap_or(true))
        {
            reason = Some("Publication branch is protected".into());
        } else if let Err(error) = &target {
            reason = Some(error.to_string());
        } else if !owned(paths, &entry, &known_branches) {
            reason = Some("Unverified MACC ownership".into());
        } else if entry.locked || entry.prunable {
            reason = Some("Locked or unavailable worktree".into());
        } else if entry.path.join(".macc/supervisor-restore-tree").exists() {
            reason = Some("Supervisor repair is parked during baseline validation".into());
        } else if !git(
            &entry.path,
            &[
                "status",
                "--porcelain",
                "--untracked-files=all",
                "--ignore-submodules=none",
            ],
        )?
        .is_empty()
        {
            reason = Some("Uncommitted or untracked work".into());
        } else if !crate::git::merge_base_is_ancestor(
            &paths.root,
            entry.head.as_deref().unwrap_or("HEAD"),
            target.as_ref().unwrap(),
        )? {
            reason = Some("Commits have not been integrated into the reference branch".into());
        } else {
            if let Some(name) =
                crate::project_ownership::unowned_ignored_files(&entry.path)?.first()
            {
                reason = Some(format!("Private/unowned or modified ignored file: {name}"));
            }
        }

        if let Some(b) = &branch {
            if owned(paths, &entry, &known_branches) {
                known_branches.insert(b.clone());
            }
        }
        let removable = reason.is_none();
        result.push(Item {
            path: rel,
            disposition: if removable {
                Disposition::RemoveWorktree
            } else {
                Disposition::RetainWorktree
            },
            reason: reason.unwrap_or_else(|| {
                "Clean, owned and integrated; evidence archived before removal".into()
            }),
            bytes: super::size(&entry.path),
            branch: branch.clone(),
            owned: owned(paths, &entry, &known_branches),
        });
        if removable {
            if let Some(branch) = branch {
                result.push(Item {
                    path: branch,
                    disposition: Disposition::RemoveBranch,
                    reason: "MACC-created branch integrated into reference".into(),
                    bytes: 0,
                    branch: None,
                    owned: true,
                });
            }
        }
    }
    for branch in known_branches {
        let reference = format!("refs/heads/{branch}");
        if checked.contains(&reference) || publication_branch(paths, &branch)? {
            continue;
        }
        if let Some(head) = crate::git::resolve_ref(&paths.root, &reference) {
            let integrated = match &target {
                Ok(target) => crate::git::merge_base_is_ancestor(&paths.root, &head, target)?,
                Err(_) => false,
            };
            result.push(Item {
                path: branch,
                disposition: if integrated {
                    Disposition::RemoveBranch
                } else {
                    Disposition::RetainBranch
                },
                reason: if integrated {
                    "Unoccupied MACC-created branch integrated into reference".into()
                } else {
                    "Unintegrated branch or unavailable publication target; retained".into()
                },
                bytes: 0,
                branch: None,
                owned: true,
            });
        }
    }

    Ok(result)
}
/// Copy only regular evidence files; never follow a symlink or traverse dependencies.
pub(crate) fn archive_slot(source: &Path, destination: &Path) -> Result<()> {
    fn copy(source: &Path, relative: &Path, dest: &Path) -> Result<()> {
        for entry in std::fs::read_dir(source).map_err(crate::persistent::error)? {
            let entry = entry.map_err(crate::persistent::error)?;
            let rel = relative.join(entry.file_name());
            let kind = entry.file_type().map_err(crate::persistent::error)?;
            if kind.is_symlink()
                || entry.file_name() == ".git"
                || entry.file_name() == "node_modules"
            {
                continue;
            }
            if kind.is_dir() {
                copy(&entry.path(), &rel, dest)?;
            } else if rel.starts_with(".macc")
                || matches!(
                    rel.extension().and_then(|s| s.to_str()),
                    Some("txt" | "log")
                )
            {
                let target = dest.join(&rel);
                std::fs::create_dir_all(target.parent().unwrap())
                    .map_err(crate::persistent::error)?;
                std::fs::copy(entry.path(), target).map_err(crate::persistent::error)?;
            }
        }
        Ok(())
    }
    copy(source, Path::new(""), destination)
}

/// Compare-and-delete a verified owned ref without using HEAD as an alternative
/// integration target. A changed branch tip or checked-out branch is retained.
pub(super) fn delete_integrated_branch(
    paths: &ProjectPaths,
    branch: &str,
    expected: &str,
) -> Result<()> {
    let reference = format!("refs/heads/{branch}");
    if publication_branch(paths, branch)? {
        return Err(crate::persistent::error("Publication branch is protected"));
    }
    if crate::list_worktrees(&paths.root)?
        .iter()
        .any(|w| w.branch.as_deref() == Some(&reference))
    {
        return Err(crate::persistent::error(
            "Branch is still checked out; retained",
        ));
    }
    git(&paths.root, &["update-ref", "-d", &reference, expected])?;
    Ok(())
}

fn publication_branch(paths: &ProjectPaths, branch: &str) -> Result<bool> {
    if paths.config_path.exists() {
        let config = crate::load_canonical_config(&paths.config_path)?;
        if config
            .automation
            .coordinator
            .as_ref()
            .and_then(|c| c.reference_branch.as_deref())
            .is_some_and(|r| r.trim_start_matches("refs/heads/") == branch)
        {
            return Ok(true);
        }
    }
    let current = crate::git::run_git_output_mapped(
        &paths.root,
        &["symbolic-ref", "--quiet", "HEAD"],
        "inspect publication branch",
    )?;
    if !current.status.success() && current.status.code() != Some(1) {
        return Err(crate::persistent::error(
            "Cannot verify current publication branch",
        ));
    }
    Ok(String::from_utf8_lossy(&current.stdout).trim() == format!("refs/heads/{branch}"))
}
