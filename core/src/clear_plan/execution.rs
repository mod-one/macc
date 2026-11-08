use super::*;

pub fn execute(paths: &ProjectPaths) -> Result<ClearReport> {
    execute_with_remover(paths, remove)
}
pub(super) fn execute_with_remover(
    paths: &ProjectPaths,
    mut remove: impl FnMut(&Path) -> std::io::Result<()>,
) -> Result<ClearReport> {
    crate::clear_policy::require_stopped(&paths.root)?;
    let _guards = crate::persistent::idle_guards(&paths.root)?;
    let plan = preview(paths)?;
    let archive = paths
        .macc_dir
        .join("persistent/clear-history")
        .join(uuid::Uuid::new_v4().to_string());
    crate::project_ownership::validate_path(
        paths,
        &crate::relative_to_root(paths, &archive).unwrap(),
    )?;
    std::fs::create_dir_all(&archive).map_err(crate::persistent::error)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&archive, std::fs::Permissions::from_mode(0o700))
            .map_err(crate::persistent::error)?;
    }
    crate::atomic_write(
        paths,
        &archive.join("plan.json"),
        &serde_json::to_vec_pretty(&plan).map_err(crate::persistent::error)?,
    )?;
    let mut report = plan.report();
    report.removed = 0;
    report.archived = 0;
    report.removed_worktrees = 0;
    report.removed_branches = 0;
    report.archive_path = Some(archive.clone());
    // Archive all recovery/evidence before removing any files or slots. Any failure
    // leaves the immutable plan for diagnosis and aborts deletion.
    for i in &plan.items {
        if i.disposition == Disposition::Archive {
            let dest = archive.join("project").join(&i.path);
            std::fs::create_dir_all(dest.parent().unwrap()).map_err(crate::persistent::error)?;
            std::fs::rename(paths.root.join(&i.path), &dest).map_err(|e| {
                crate::persistent::error(format!(
                    "Archive {}: {e}; cleanup stopped, inspect {}",
                    i.path,
                    archive.display()
                ))
            })?;
            report.archived += 1;
        } else if i.disposition == Disposition::RemoveWorktree {
            worktrees::archive_slot(
                &paths.root.join(&i.path),
                &archive
                    .join("worktrees")
                    .join(Path::new(&i.path).file_name().unwrap()),
            )
            .map_err(|error| {
                crate::persistent::error(format!(
                    "Cannot archive {} into {}: {error}; slot retained",
                    i.path,
                    archive.display()
                ))
            })?;
        }
    }
    for i in &plan.items {
        let result = match i.disposition {
            Disposition::Remove => {
                if !i.path.starts_with(".macc/") {
                    let inventory = crate::project_ownership::read(paths)?;
                    if !inventory.files.get(&i.path).is_some_and(|expected| {
                        crate::project_ownership::digest(&paths.root.join(&i.path))
                            .is_ok_and(|actual| &actual == expected)
                    }) {
                        report
                            .failures
                            .push(format!("{} changed during cleanup; retained", i.path));
                        continue;
                    }
                }
                remove(&paths.root.join(&i.path)).map_err(crate::persistent::error)
            }
            Disposition::RemoveWorktree => {
                // Recheck after archiving: a concurrent edit never becomes disposable.
                let current = worktrees::inspect(paths)?;
                if !current
                    .iter()
                    .any(|c| c.path == i.path && c.disposition == Disposition::RemoveWorktree)
                {
                    report
                        .failures
                        .push(format!("{} changed during cleanup; retained", i.path));
                    continue;
                }
                crate::remove_worktree(&paths.root, &paths.root.join(&i.path), true)
            }
            Disposition::RemoveBranch => {
                let target = worktrees::target(paths)?;
                let head = crate::git::resolve_ref(&paths.root, &format!("refs/heads/{}", i.path));
                if head.is_none() {
                    continue;
                }
                if !crate::git::merge_base_is_ancestor(
                    &paths.root,
                    head.as_deref().unwrap(),
                    &target,
                )? {
                    report
                        .failures
                        .push(format!("{} is no longer integrated; retained", i.path));
                    continue;
                }
                worktrees::delete_integrated_branch(paths, &i.path, head.as_deref().unwrap())
            }
            _ => continue,
        };
        match result {
            Ok(()) => match i.disposition {
                Disposition::RemoveWorktree => report.removed_worktrees += 1,
                Disposition::RemoveBranch => report.removed_branches += 1,
                _ => report.removed += 1,
            },
            Err(e) => report.failures.push(format!("{}: {e}", i.path)),
        }
    }
    let _inventory_guard = crate::project_ownership::lock(paths)?;
    let mut inventory = crate::project_ownership::read(paths)?;
    let protected: Vec<_> = plan
        .items
        .iter()
        .filter(|i| {
            i.disposition == Disposition::RetainUser && !inventory.files.contains_key(&i.path)
        })
        .map(|i| i.path.clone())
        .collect();
    inventory.protected.extend(protected);
    inventory
        .protected
        .retain(|name| paths.root.join(name).exists());
    inventory
        .files
        .retain(|name, _| paths.root.join(name).exists());
    let mut dirs: Vec<_> = inventory.directories.iter().cloned().collect();
    dirs.sort_by_key(|p| std::cmp::Reverse(p.len()));
    for rel in dirs {
        crate::project_ownership::validate_path(paths, &rel)?;
        let _ = std::fs::remove_dir(paths.root.join(&rel));
    }
    inventory
        .directories
        .retain(|name| paths.root.join(name).exists());
    inventory.branches.extend(
        plan.items
            .iter()
            .filter(|i| i.owned)
            .filter_map(|i| i.branch.clone()),
    );
    inventory
        .branches
        .retain(|b| crate::git::resolve_ref(&paths.root, &format!("refs/heads/{b}")).is_some());
    crate::project_ownership::save(paths, &inventory)?;
    if let Err(error) = crate::gitignore_policy::after_clear(paths, &inventory, &archive) {
        report.failures.push(error.to_string());
    }
    for dir in [".macc/worktree", ".macc/state"] {
        let _ = std::fs::remove_dir(paths.root.join(dir));
    }
    // Durable result and residual provenance make I/O failures retryable.
    crate::atomic_write(
        paths,
        &archive.join("result.json"),
        &serde_json::to_vec_pretty(&report).map_err(crate::persistent::error)?,
    )?;
    Ok(report)
}
fn remove(path: &Path) -> std::io::Result<()> {
    let metadata = match std::fs::symlink_metadata(path) {
        Ok(m) => m,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(e) => return Err(e),
    };
    if metadata.is_dir() {
        std::fs::remove_dir_all(path)
    } else {
        std::fs::remove_file(path)
    }
}
