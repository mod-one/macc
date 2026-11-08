use crate::service::interaction::InteractionHandler;
use crate::{ClearReport, MaccError, ProjectPaths, Result};

#[derive(Debug, Clone, Default)]
pub struct ClearExecutionReport {
    pub removed_worktrees: usize,
    pub clear_report: ClearReport,
}

pub fn clear_project(
    paths: &ProjectPaths,
    force: bool,
    dry_run: bool,
    ui: &dyn InteractionHandler,
) -> Result<ClearExecutionReport> {
    if let Ok(primary) = crate::persistent::primary_root(&paths.root) {
        if paths.root.canonicalize().ok().as_ref() != Some(&primary) {
            return Err(MaccError::Validation(format!(
                "Run macc clear from the primary checkout: {}",
                primary.display()
            )));
        }
    }
    ui.info("This will:");
    ui.info("  1) Remove eligible clean, integrated worktrees; unfinished work is protected");
    ui.info("  2) Remove MACC-managed files/directories in this project (macc clear)");
    ui.info("  Configuration, catalogs, .macc/persistent authority, session authority and O2 memory/checkpoints are retained. A configuration-only restore does not restore historical O2 data; use a saved session bundle for that.");

    for path in crate::clear_policy::existing_preserved(paths) {
        ui.info(&format!("  Keep: {path}"));
    }

    if dry_run {
        let removed_worktrees = crate::domain::worktree::removable_worktrees(&paths.root)?.len();

        let clear_report = crate::clear_dry_run(paths)?;
        ui.info(&format!(
            "[Dry run] Would remove worktrees: {}",
            removed_worktrees
        ));
        ui.info(&format!(
            "[Dry run] Would clear managed paths: removed={}, skipped={}",
            clear_report.removed, clear_report.skipped
        ));

        return Ok(ClearExecutionReport {
            removed_worktrees,
            clear_report,
        });
    }

    if !force && !ui.confirm("Continue [y/N]? ")? {
        return Err(MaccError::Validation("Clear cancelled.".into()));
    }

    crate::clear_policy::require_stopped(&paths.root)?;
    let _authority_guards = crate::persistent::idle_guards(&paths.root)?;
    let removed_worktrees = crate::service::worktree::remove_all_worktrees(&paths.root, false)?;
    crate::prune_worktrees(&paths.root)?;
    ui.info(&format!("Removed worktrees: {}", removed_worktrees));

    let clear_report = crate::clear(paths)?;
    ui.info(&format!(
        "Cleared managed paths: removed={}, skipped={}",
        clear_report.removed, clear_report.skipped
    ));

    Ok(ClearExecutionReport {
        removed_worktrees,
        clear_report,
    })
}
