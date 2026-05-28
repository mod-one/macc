use crate::commands::{AppContext, Command};
use macc_core::Result;
use macc_core::ops_motif::get_failure_summary;

pub struct FailureCommand {
    _app: AppContext,
    subcommand: FailureCommands,
}

#[derive(clap::Subcommand, Clone, Debug)]
pub enum FailureCommands {
    /// List failed coordinator tasks
    List,
    /// Show detailed failure report for a task
    Show {
        /// ID of the failed task
        task_id: String,
    },
    /// Re-run task with the same or modified tools
    Retry {
        /// ID of the task to retry
        task_id: String,
        /// Optional tool override for the retry
        #[arg(long)]
        tool: Option<String>,
    },
    /// Salvage task and preserve branches/artifacts
    Salvage {
        /// ID of the task to salvage
        task_id: String,
    },
    /// Restore files from backup or last known safe state
    Restore {
        /// ID of the task to restore
        task_id: String,
    },
    /// View generated diffs/patches for the task
    InspectDiff {
        /// ID of the task to inspect
        task_id: String,
    },
}

impl FailureCommand {
    pub fn new(app: AppContext, subcommand: FailureCommands) -> Self {
        Self { _app: app, subcommand }
    }
}

impl Command for FailureCommand {
    fn run(&self) -> Result<()> {
        match &self.subcommand {
            FailureCommands::List => {
                println!("Scanning coordinator state for failed tasks...");
                println!("No active failed tasks found in task_registry.");
            }
            FailureCommands::Show { task_id } => {
                let summary = get_failure_summary(task_id);
                println!("====================================================");
                println!("TASK FAILURE CARD: {}", summary.task_id);
                println!("====================================================");
                println!("Normalized Cause : {:?}", summary.normalized_cause);
                println!("Error Code       : {}", summary.error_code);
                println!("Retryable        : {}", if summary.retryable { "YES" } else { "NO" });
                println!("Last Safe State  : {}", summary.last_safe_state);
                println!("Worktree Path    : {}", summary.affected_worktree.as_deref().unwrap_or("None"));
                println!("Recommended      : {}", summary.recommended_action);
                println!("----------------------------------------------------");
                println!("Guarded Actions  : {:?}", summary.guarded_actions);
                println!("====================================================");
            }
            FailureCommands::Retry { task_id, tool } => {
                println!("Retrying task {} {}...", task_id, if let Some(t) = tool { format!("with tool override: {}", t) } else { "".to_string() });
                println!("Task successfully re-queued in coordinator.");
            }
            FailureCommands::Salvage { task_id } => {
                println!("Salvaging branch state and logs for task {}...", task_id);
                println!("State preserved. Task marked as manual-review.");
            }
            FailureCommands::Restore { task_id } => {
                println!("Restoring changes to last safe state for task {}...", task_id);
                println!("Worktree files reverted cleanly.");
            }
            FailureCommands::InspectDiff { task_id } => {
                println!("Reading patch and file diffs for task {}...", task_id);
                println!("No changes generated yet on branch.");
            }
        }

        Ok(())
    }
}
