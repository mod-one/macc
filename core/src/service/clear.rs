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
    ui.info("This will:");
    ui.info(
        "  1) Remove all non-root worktrees (equivalent to: macc worktree remove --all --force)",
    );
    ui.info("  2) Remove MACC-managed files/directories in this project (macc clear)");

    if dry_run {
        let entries = crate::list_worktrees(&paths.root).unwrap_or_default();
        let root_canon = paths.root.canonicalize().unwrap_or_else(|_| paths.root.clone());
        let mut removed_worktrees = 0usize;
        for entry in entries {
            if entry.path != root_canon {
                removed_worktrees += 1;
            }
        }

        let clear_report = crate::clear_dry_run(paths)?;
        ui.info(&format!("[Dry run] Would remove worktrees: {}", removed_worktrees));
        ui.info(&format!(
            "[Dry run] Would clear managed paths: removed={}, skipped=0",
            clear_report.removed
        ));

        return Ok(ClearExecutionReport {
            removed_worktrees,
            clear_report,
        });
    }

    if !force && !ui.confirm("Continue [y/N]? ")? {
        return Err(MaccError::Validation("Clear cancelled.".into()));
    }

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
