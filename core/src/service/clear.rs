use crate::service::interaction::InteractionHandler;
use crate::{clear_plan::Disposition, ClearReport, MaccError, ProjectPaths, Result};
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
    if !paths.config_path.is_file() && !paths.managed_paths_state_path().is_file() {
        return Err(MaccError::Validation(format!(
            "No initialized MACC project at {}; Clear does not reset shared user caches",
            paths.root.display()
        )));
    }
    if let Ok(primary) = crate::persistent::primary_root(&paths.root) {
        if paths.root.canonicalize().ok().as_ref() != Some(&primary) {
            return Err(MaccError::Validation(format!(
                "Run macc clear from the primary checkout: {}",
                primary.display()
            )));
        }
    }
    let plan = crate::clear_plan::preview(paths)?;
    ui.info("MACC Clear · reset generated runtime; keep setup, local durable authority and unfinished work");
    ui.info("Evidence, drafts and previous coordinator recovery state are archived under .macc/persistent/clear-history (local, ignored by Git).");
    for item in &plan.items {
        let action = match item.disposition {
            Disposition::Preserve => "Keep",
            Disposition::Archive => "Archive",
            Disposition::Remove => "Remove",
            Disposition::RemoveWorktree => "Remove worktree",
            Disposition::RetainWorktree => "Retain worktree",
            Disposition::RetainUser => "Retain user file",
            Disposition::RemoveBranch => "Delete merged branch",
            Disposition::RetainBranch => "Retain branch",
        };
        ui.info(&format!(
            "  {action}: {} · {} · {} bytes",
            item.path, item.reason, item.bytes
        ));
    }
    if dry_run {
        return Ok(ClearExecutionReport {
            removed_worktrees: plan
                .items
                .iter()
                .filter(|i| i.disposition == Disposition::RemoveWorktree)
                .count(),
            clear_report: plan.report(),
        });
    }
    if !force && !ui.confirm("Continue [y/N]? ")? {
        return Err(MaccError::Validation("Clear cancelled.".into()));
    }
    let report = crate::clear(paths)?;
    ui.info(&format!("Clear: {} removed paths, {} archived entries, {} removed worktrees, {} deleted merged branches, {} retained entries",report.removed,report.archived,report.removed_worktrees,report.removed_branches,report.skipped));
    if let Some(path) = &report.archive_path {
        ui.info(&format!("Recovery archive: {}", path.display()));
    }
    if !report.failures.is_empty() {
        return Err(MaccError::Validation(format!(
            "Clear incomplete; unresolved entries are retained for retry:\n{}",
            report.failures.join("\n")
        )));
    }
    Ok(ClearExecutionReport {
        removed_worktrees: report.removed_worktrees,
        clear_report: report,
    })
}
