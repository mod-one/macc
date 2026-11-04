//! Serialize file promotion with first plan registration and protect all retained plans.
use super::*;
use std::{
    path::{Path, PathBuf},
    time::Duration,
};

/// Resolve an output path even before its parent directory exists. Existing
/// aliases resolve to their actual repository; dangling symlinks are refused.
pub(crate) fn promotion_destination(path: &Path) -> crate::Result<PathBuf> {
    let absolute = if path.is_absolute() {
        path.to_owned()
    } else {
        std::env::current_dir()
            .map_err(|e| error(e.to_string()))?
            .join(path)
    };
    if std::fs::symlink_metadata(&absolute).is_ok_and(|m| m.file_type().is_symlink()) {
        return Err(error("PRD promotion destination cannot be a symlink"));
    }
    let mut existing = absolute.as_path();
    let mut suffix = Vec::new();
    while !existing.exists() {
        let name = existing
            .file_name()
            .ok_or_else(|| error("Unresolvable promotion destination"))?;
        suffix.push(name.to_owned());
        existing = existing
            .parent()
            .ok_or_else(|| error("Unresolvable promotion destination"))?;
    }
    let mut resolved = existing.canonicalize().map_err(|e| error(e.to_string()))?;
    for name in suffix.into_iter().rev() {
        resolved.push(name);
    }
    Ok(resolved)
}

/// Hold through the final file replacement. This applies even if decision
/// management is currently disabled or the runtime .macc directory was cleared.
pub(crate) fn guard_prd_promotion(
    destination: &Path,
) -> crate::Result<Option<crate::fs_lock::AdvisoryLock>> {
    let mut ancestor = destination
        .parent()
        .ok_or_else(|| error("PRD promotion needs a parent directory"))?;
    while !ancestor.exists() {
        ancestor = ancestor
            .parent()
            .ok_or_else(|| error("PRD promotion needs an existing ancestor"))?;
    }
    let output = crate::git::run_git_output_mapped(
        ancestor,
        &["rev-parse", "--show-toplevel"],
        "resolve promotion repository",
    )?;
    if !output.status.success() {
        if ancestor.ancestors().any(|p| p.join(".git").exists()) {
            return Err(error("Cannot resolve Git authority before PRD promotion; repair repository identity first"));
        }
        return Ok(None);
    }
    let root = PathBuf::from(String::from_utf8_lossy(&output.stdout).trim());
    let location = Store::location(&root)?;
    if std::fs::symlink_metadata(location.parent().unwrap())
        .is_ok_and(|m| m.file_type().is_symlink())
    {
        return Err(error("Control directory cannot be a symlink"));
    }
    let guard = crate::fs_lock::AdvisoryLock::acquire(
        &location.with_file_name("mutation.lock"),
        Duration::from_secs(5),
        "decision mutation/publication",
    )?;
    if std::fs::symlink_metadata(&location).is_ok() {
        let store = Store::open(&root)?;
        for plan in store.plans()? {
            let target = promotion_destination(&root.join(&plan.source))?;
            if target == destination {
                return Err(error(format!("PRD-GEN-PROMOTE-MANAGED: '{}' is managed by native plan {}. Use `macc decisions graph preview` and `macc decisions graph apply` (or reconcile inspected drift); --yes cannot bypass graph version/history checks",plan.source,plan.id)));
            }
        }
    }
    Ok(Some(guard))
}
