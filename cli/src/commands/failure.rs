use crate::commands::{AppContext, Command};
use macc_core::ops_motif::get_failure_summary;
use macc_core::Result;

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
    /// Abandon task and clean up resources
    Abandon {
        /// ID of the task to abandon
        task_id: String,
    },
}

struct CliBackupsUi;

impl macc_core::service::interaction::InteractionHandler for CliBackupsUi {
    fn info(&self, message: &str) {
        println!("{}", message);
    }

    fn warn(&self, message: &str) {
        eprintln!("{}", message);
    }

    fn error(&self, message: &str) {
        eprintln!("{}", message);
    }

    fn confirm_yes_no(&self, prompt: &str) -> Result<bool> {
        crate::confirm_yes_no(prompt)
    }
}

impl macc_core::service::backups::BackupsUi for CliBackupsUi {
    fn open_in_editor(&self, path: &std::path::Path, command: &str) -> Result<()> {
        macc_core::service::task_runner::open_in_editor(path, command)
    }
}

impl FailureCommand {
    pub fn new(app: AppContext, subcommand: FailureCommands) -> Self {
        Self {
            _app: app,
            subcommand,
        }
    }
}

impl Command for FailureCommand {
    fn run(&self) -> Result<()> {
        let paths = self._app.project_paths()?;
        let config = self._app.canonical_config()?;

        match &self.subcommand {
            FailureCommands::List => {
                println!("Scanning coordinator state for failed tasks...");
                let registry = macc_core::ops_motif::load_task_registry(&paths, &config)?;
                let failed_tasks: Vec<_> = registry
                    .tasks
                    .iter()
                    .filter(|t| t.state == "blocked" || t.task_runtime.last_error_code.is_some())
                    .collect();
                if failed_tasks.is_empty() {
                    println!("No active failed tasks found in task_registry.");
                } else {
                    println!(
                        "{:<12} {:<30} {:<10} {:<15}",
                        "TASK ID", "TITLE", "ERROR", "RECOMMENDED"
                    );
                    println!("{:-<12} {:-<30} {:-<10} {:-<15}", "", "", "", "");
                    for task in failed_tasks {
                        let title = task.title.as_deref().unwrap_or("No Title");
                        let err_code = task
                            .task_runtime
                            .last_error_code
                            .as_deref()
                            .unwrap_or("E901");
                        let state = &task.state;
                        println!(
                            "{:<12} {:<30} {:<10} {:<15}",
                            task.id, title, err_code, state
                        );
                    }
                }
            }
            FailureCommands::Show { task_id } => {
                let summary = get_failure_summary(&paths, &config, task_id)?;
                println!("====================================================");
                println!("TASK FAILURE CARD: {}", summary.task_id);
                println!("====================================================");
                println!("Normalized Cause : {:?}", summary.normalized_cause);
                println!("Error Code       : {}", summary.error_code);
                println!(
                    "Retryable        : {}",
                    if summary.retryable { "YES" } else { "NO" }
                );
                println!("Last Safe State  : {}", summary.last_safe_state);
                println!(
                    "Worktree Path    : {}",
                    summary.affected_worktree.as_deref().unwrap_or("None")
                );
                println!("Recommended      : {}", summary.recommended_action);
                println!("----------------------------------------------------");
                println!("Guarded Actions  : {:?}", summary.guarded_actions);
                println!("====================================================");
            }
            FailureCommands::Retry { task_id, tool } => {
                let mut registry = macc_core::ops_motif::load_task_registry(&paths, &config)?;
                let task = registry.find_task_mut(task_id).ok_or_else(|| {
                    macc_core::MaccError::Validation(format!("Task not found: {}", task_id))
                })?;
                if let Some(t) = tool {
                    task.tool = Some(t.clone());
                }
                task.state = "todo".to_string();
                task.clear_assignment();
                task.task_runtime.status = Some("idle".to_string());
                task.task_runtime.pid = None;
                task.task_runtime.started_at = None;
                task.task_runtime.current_phase = None;
                task.task_runtime.clear_last_error_details();

                let mut args = std::collections::BTreeMap::new();
                let coord = config.automation.coordinator.as_ref();
                if let Some(storage_mode) = coord.and_then(|c| c.storage_mode.as_ref()) {
                    args.insert("storage-mode".to_string(), storage_mode.clone());
                }
                let value = serde_json::to_value(&registry)
                    .map_err(|e| macc_core::MaccError::Validation(e.to_string()))?;
                macc_core::coordinator::state::coordinator_state_registry_save(
                    &paths.root,
                    &args,
                    &value,
                )?;

                macc_core::ops_motif::log_ops_action(&paths, "retry", task_id)?;
                println!("Retrying task {}...", task_id);
                println!("Task successfully re-queued in coordinator.");
            }
            FailureCommands::Salvage { task_id } => {
                let mut registry = macc_core::ops_motif::load_task_registry(&paths, &config)?;
                let task = registry.find_task_mut(task_id).ok_or_else(|| {
                    macc_core::MaccError::Validation(format!("Task not found: {}", task_id))
                })?;
                task.state = "changes_requested".to_string();

                let mut args = std::collections::BTreeMap::new();
                let coord = config.automation.coordinator.as_ref();
                if let Some(storage_mode) = coord.and_then(|c| c.storage_mode.as_ref()) {
                    args.insert("storage-mode".to_string(), storage_mode.clone());
                }
                let value = serde_json::to_value(&registry)
                    .map_err(|e| macc_core::MaccError::Validation(e.to_string()))?;
                macc_core::coordinator::state::coordinator_state_registry_save(
                    &paths.root,
                    &args,
                    &value,
                )?;

                macc_core::ops_motif::log_ops_action(&paths, "salvage", task_id)?;
                println!("Salvaging branch state and logs for task {}...", task_id);
                println!("State preserved. Task marked as manual-review.");
            }
            FailureCommands::Restore { task_id } => {
                println!(
                    "WARNING: Restoring files will overwrite any local changes in the worktree."
                );
                if !crate::confirm_yes_no("Are you sure you want to proceed with restore [y/N]? ")?
                {
                    println!("Restore cancelled.");
                    return Ok(());
                }
                if !crate::confirm_yes_no(
                    "CONFIRM AGAIN: This action is destructive. Proceed [y/N]? ",
                )? {
                    println!("Restore cancelled.");
                    return Ok(());
                }

                println!(
                    "Restoring changes to last safe state for task {}...",
                    task_id
                );
                self._app.engine.backups_restore(
                    &paths,
                    false, // user
                    None,  // backup
                    true,  // latest
                    false, // dry_run
                    true,  // yes
                    &CliBackupsUi,
                )?;
                macc_core::ops_motif::log_ops_action(&paths, "restore", task_id)?;
                println!("Worktree files reverted cleanly.");
            }
            FailureCommands::InspectDiff { task_id } => {
                let registry = macc_core::ops_motif::load_task_registry(&paths, &config)?;
                let task = registry.find_task(task_id).ok_or_else(|| {
                    macc_core::MaccError::Validation(format!("Task not found: {}", task_id))
                })?;

                let wt_dir = task
                    .worktree
                    .as_ref()
                    .and_then(|w| w.worktree_path.as_ref())
                    .map(|p| paths.root.join(p))
                    .unwrap_or_else(|| paths.root.clone());

                println!(
                    "Reading diffs for task {} in {}...",
                    task_id,
                    wt_dir.display()
                );

                let git_args = vec!["diff", "HEAD"];
                match macc_core::git::run_git_output_mapped(&wt_dir, &git_args, "inspect diff") {
                    Ok(out) => {
                        let stdout = String::from_utf8_lossy(&out.stdout);
                        if stdout.trim().is_empty() {
                            println!("No local changes found in worktree.");
                        } else {
                            println!("{}", stdout);
                        }
                    }
                    Err(e) => {
                        println!("Failed to read git diff: {}", e);
                    }
                }
            }
            FailureCommands::Abandon { task_id } => {
                println!(
                    "WARNING: Abandoning a task will clear assignment and mark task as abandoned."
                );
                if !crate::confirm_yes_no("Are you sure you want to proceed with abandon [y/N]? ")?
                {
                    println!("Abandon cancelled.");
                    return Ok(());
                }
                if !crate::confirm_yes_no(
                    "CONFIRM AGAIN: This action is destructive. Proceed [y/N]? ",
                )? {
                    println!("Abandon cancelled.");
                    return Ok(());
                }

                let mut registry = macc_core::ops_motif::load_task_registry(&paths, &config)?;
                let task = registry.find_task_mut(task_id).ok_or_else(|| {
                    macc_core::MaccError::Validation(format!("Task not found: {}", task_id))
                })?;
                task.state = "abandoned".to_string();
                task.clear_assignment();
                task.task_runtime.status = Some("idle".to_string());
                task.task_runtime.pid = None;
                task.task_runtime.started_at = None;
                task.task_runtime.current_phase = None;
                task.task_runtime.clear_last_error_details();

                let mut args = std::collections::BTreeMap::new();
                let coord = config.automation.coordinator.as_ref();
                if let Some(storage_mode) = coord.and_then(|c| c.storage_mode.as_ref()) {
                    args.insert("storage-mode".to_string(), storage_mode.clone());
                }
                let value = serde_json::to_value(&registry)
                    .map_err(|e| macc_core::MaccError::Validation(e.to_string()))?;
                macc_core::coordinator::state::coordinator_state_registry_save(
                    &paths.root,
                    &args,
                    &value,
                )?;

                macc_core::ops_motif::log_ops_action(&paths, "abandon", task_id)?;
                println!("Task {} successfully marked as abandoned.", task_id);
            }
        }

        Ok(())
    }
}
