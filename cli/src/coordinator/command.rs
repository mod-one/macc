use crate::coordinator::legacy_helpers::{
    stop_coordinator_process_groups, NativeCoordinatorLogger,
};
use crate::coordinator::render::print_status_summary;
use macc_core::coordinator::engine as coordinator_engine;
use macc_core::coordinator::types::CoordinatorEnvConfig;
use macc_core::coordinator_storage::CoordinatorStorageMode;
use macc_core::service::coordinator_workflow::{
    coordinator_command_emits_runtime_events, coordinator_command_from_name, CoordinatorCommand,
    CoordinatorCommandRequest,
};
use macc_core::{load_canonical_config, MaccError, Result};
use std::path::Path;
use std::process::{Command as ProcessCommand, Stdio};

fn build_native_logger(
    repo_root: &Path,
    command_name: &str,
    env_cfg: &CoordinatorEnvConfig,
    coordinator_cfg: Option<&macc_core::config::CoordinatorConfig>,
) -> Result<NativeCoordinatorLogger> {
    NativeCoordinatorLogger::new_with_flush(
        repo_root,
        command_name,
        env_cfg
            .log_flush_lines
            .or_else(|| coordinator_cfg.and_then(|c| c.log_flush_lines)),
        env_cfg
            .log_flush_ms
            .or_else(|| coordinator_cfg.and_then(|c| c.log_flush_ms)),
    )
}

struct LoggerAdapter<'a>(&'a NativeCoordinatorLogger);

impl macc_core::coordinator::control_plane::CoordinatorLog for LoggerAdapter<'_> {
    fn note(&self, line: String) -> Result<()> {
        self.0.note(line)
    }
}

#[derive(Debug, Clone)]
pub struct CoordinatorCommandInput {
    pub command_name: String,
    pub no_tui: bool,
    pub supervisor: bool,
    pub graceful: bool,
    pub remove_worktrees: bool,
    pub remove_branches: bool,
    pub env_cfg: CoordinatorEnvConfig,
    pub extra_args: Vec<String>,
}

struct ProjectContext {
    paths: macc_core::ProjectPaths,
    canonical: macc_core::config::CanonicalConfig,
    coordinator_cfg: Option<macc_core::config::CoordinatorConfig>,
}

impl ProjectContext {
    fn load(
        absolute_cwd: &Path,
        engine: &crate::services::engine_provider::SharedEngine,
    ) -> Result<Self> {
        let paths = engine.project_ensure_initialized_paths(absolute_cwd)?;
        let canonical = load_canonical_config(&paths.config_path)?;
        let coordinator_cfg = canonical.automation.coordinator.clone();
        Ok(Self {
            paths,
            canonical,
            coordinator_cfg,
        })
    }
}

pub fn handle(
    absolute_cwd: &Path,
    engine: &crate::services::engine_provider::SharedEngine,
    input: CoordinatorCommandInput,
) -> Result<()> {
    // Intercept "sessions" subcommand before normal coordinator dispatch.
    if input.command_name == "sessions" {
        let context = ProjectContext::load(absolute_cwd, engine)?;
        return handle_sessions_command(&context.paths.root, &input.extra_args);
    }

    let context = ProjectContext::load(absolute_cwd, engine)?;
    let paths = &context.paths;
    let canonical = &context.canonical;
    let coordinator_cfg = context.coordinator_cfg.as_ref();
    let command = coordinator_command_from_name(
        &input.command_name,
        &input.extra_args,
        input.graceful,
        input.remove_worktrees,
        input.remove_branches,
    )?;

    if input.supervisor && input.command_name == "run" {
        spawn_attached_supervisor(&context.paths.root)?;
    }

    if matches!(command, CoordinatorCommand::Run) && !input.no_tui {
        return macc_tui::run_tui_with_launch(macc_tui::LaunchMode::CoordinatorRun).map_err(|e| {
            MaccError::Io {
                path: "tui".into(),
                action: "run_tui coordinator live".into(),
                source: std::io::Error::other(e.to_string()),
            }
        });
    }

    let _ = macc_core::ensure_embedded_automation_scripts(paths)?;

    if let Ok(effective_storage_mode) =
        coordinator_engine::resolve_storage_mode(&input.env_cfg, coordinator_cfg)
    {
        let mode_raw = match effective_storage_mode {
            CoordinatorStorageMode::Json => "json",
            CoordinatorStorageMode::DualWrite => "dual-write",
            CoordinatorStorageMode::Sqlite => "sqlite",
        };
        std::env::set_var("COORDINATOR_STORAGE_MODE", mode_raw);
    }
    if let Some(debounce_ms) = input
        .env_cfg
        .mirror_json_debounce_ms
        .or_else(|| coordinator_cfg.and_then(|c| c.mirror_json_debounce_ms))
    {
        std::env::set_var(
            "COORDINATOR_JSON_EXPORT_DEBOUNCE_MS",
            debounce_ms.to_string(),
        );
    }
    if coordinator_command_emits_runtime_events(&command) {
        let _ = engine.project_ensure_coordinator_run_id();
    }

    // Preflight: abort before dispatching workers if git identity is not configured.
    // A missing user.email / user.name causes every performer's `git commit` to fail,
    // leaving changes uncommitted and the coordinator stuck.
    if matches!(
        command,
        CoordinatorCommand::RunControlPlane | CoordinatorCommand::DispatchReadyTasks
    ) {
        let missing = macc_core::git::missing_git_identity_fields(&paths.root);
        if !missing.is_empty() {
            let fields = missing.join(", ");
            return Err(MaccError::Validation(format!(
                "Git identity is not configured ({fields}). \
Performers cannot commit without it. Fix this first:\n\
  git config --global user.email \"you@example.com\"\n\
  git config --global user.name \"Your Name\""
            )));
        }
    }

    if matches!(command, CoordinatorCommand::Stop { .. }) {
        let coordinator_path = paths.automation_coordinator_path();
        let stopped =
            stop_coordinator_process_groups(&paths.root, &coordinator_path, input.graceful)?;
        println!("Coordinator process groups signaled: {}", stopped);
    }

    let logger_action = match command {
        CoordinatorCommand::RunControlPlane => Some("run"),
        CoordinatorCommand::DispatchReadyTasks => Some("dispatch"),
        CoordinatorCommand::AdvanceTasks => Some("advance"),
        CoordinatorCommand::SyncRegistry => Some("sync"),
        CoordinatorCommand::SyncPrd => Some("sync-prd"),
        CoordinatorCommand::AuditPrd { .. } => Some("audit-prd"),
        CoordinatorCommand::ReconcileRuntime => Some("reconcile"),
        CoordinatorCommand::CleanupMaintenance => Some("cleanup"),
        _ => None,
    };
    let native_logger = if let Some(action_name) = logger_action {
        let logger =
            build_native_logger(&paths.root, action_name, &input.env_cfg, coordinator_cfg)?;
        println!("Coordinator log file: {}", logger.file.display());
        Some(logger)
    } else {
        None
    };
    let logger_adapter = native_logger.as_ref().map(LoggerAdapter);
    let core_logger = logger_adapter
        .as_ref()
        .map(|adapter| adapter as &dyn macc_core::coordinator::control_plane::CoordinatorLog);

    let response = engine.coordinator_execute_command(
        paths,
        command.clone(),
        CoordinatorCommandRequest {
            canonical: Some(canonical),
            coordinator_cfg,
            env_cfg: &input.env_cfg,
            logger: core_logger,
        },
    )?;

    if let Some(status) = response.status {
        print_status_summary(&paths.root, &status);
    }
    if let Some(runtime) = response.runtime_status {
        println!("{}", runtime);
    }
    if let Some(copied) = response.aggregated_performer_logs {
        println!("Aggregated {} performer log file(s).", copied);
    }
    if let Some(resumed) = response.resumed {
        if resumed {
            println!("Coordinator resume signal applied.");
        } else {
            println!("Coordinator is not paused.");
        }
    }
    if let Some(path) = response.exported_events_path {
        println!(
            "Coordinator storage export complete (sqlite -> json): {}",
            path.display()
        );
    } else if matches!(command, CoordinatorCommand::ImportStorageJsonToSqlite) {
        println!("Coordinator storage import complete (json -> sqlite).");
    } else if matches!(command, CoordinatorCommand::VerifyStorageParity) {
        println!("Coordinator storage parity OK (json == sqlite).");
    }
    if let Some(removed) = response.removed_worktrees {
        println!("Removed {} worktree(s).", removed);
        println!("Pruned git worktrees.");
    }
    if let Some(selected) = response.selected_task {
        println!(
            "{}\t{}\t{}\t{}",
            selected.id, selected.title, selected.tool, selected.base_branch
        );
    }
    if let Some(audit) = response.audit_prd_report {
        println!(
            "Audit PRD: {} completed task(s) with context, {} todo task(s)",
            audit.completed_with_context, audit.todo_tasks
        );
        if audit.prompt_generated {
            if matches!(command, CoordinatorCommand::AuditPrd { dry_run: true, .. })
                || matches!(command, CoordinatorCommand::AuditPrd { tool: None, .. })
            {
                println!("--- BEGIN AUDIT PROMPT ---");
                if let Some(ref prompt) = audit.prompt {
                    println!("{}", prompt);
                }
                println!("--- END AUDIT PROMPT ---");
            } else {
                println!("Audit prompt sent to tool.");
            }
        } else {
            println!("No tasks to audit.");
        }
    }
    if let Some(cooldowns) = response.tool_cooldowns {
        if cooldowns.is_empty() {
            println!("No tool cooldowns active.");
        } else {
            println!("{:<16} {:>12} {:>14}", "TOOL", "REMAINING", "BACKOFF");
            for entry in &cooldowns {
                let remaining = if entry.remaining_seconds > 0 {
                    format_duration_human(entry.remaining_seconds as u64)
                } else {
                    "expired".to_string()
                };
                println!(
                    "{:<16} {:>12} {:>12}s",
                    entry.tool_id, remaining, entry.backoff_seconds
                );
            }
        }
    }

    // Auto-save sessions on graceful stop so they can be restored in future runs.
    if matches!(command, CoordinatorCommand::Stop { graceful: true, .. }) {
        match macc_core::coordinator::session_manager::save_sessions(&paths.root, None) {
            Ok(meta) => {
                println!(
                    "Sessions auto-saved as '{}' ({} active, {} archived).",
                    meta.name, meta.active_session_count, meta.archived_session_count
                );
            }
            Err(e) => {
                eprintln!("Warning: could not auto-save sessions: {}", e);
            }
        }
    }

    Ok(())
}

fn spawn_attached_supervisor(project_root: &Path) -> Result<()> {
    let current_exe = std::env::current_exe().map_err(|e| MaccError::Io {
        path: project_root.to_string_lossy().into(),
        action: "resolve current executable for coordinator supervisor bootstrap".into(),
        source: e,
    })?;
    let coordinator_pid = std::process::id();
    let status = ProcessCommand::new(current_exe)
        .current_dir(project_root)
        .arg("--cwd")
        .arg(project_root)
        .arg("supervisor")
        .arg("start")
        .arg("--daemon")
        .arg("--attach")
        .arg("--coordinator-pid")
        .arg(coordinator_pid.to_string())
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .map_err(|e| MaccError::Io {
            path: project_root.to_string_lossy().into(),
            action: "spawn supervisor from coordinator run --supervisor".into(),
            source: e,
        })?;

    if status.success() {
        return Ok(());
    }

    Err(MaccError::Validation(format!(
        "failed to start supervisor for coordinator run (exit code: {:?})",
        status.code()
    )))
}

fn handle_sessions_command(repo_root: &Path, extra_args: &[String]) -> Result<()> {
    use macc_core::coordinator::session_manager;

    let action = extra_args.first().map(|s| s.as_str()).unwrap_or("list");

    match action {
        "save" => {
            let name = extra_args.get(1).map(|s| s.as_str());
            let meta = session_manager::save_sessions(repo_root, name)?;
            println!("Session snapshot saved:");
            println!("  Name:             {}", meta.name);
            println!("  Saved at:         {}", meta.saved_at);
            println!("  Tools:            {}", meta.tool_count);
            println!("  Active sessions:  {}", meta.active_session_count);
            println!("  Archived sessions:{}", meta.archived_session_count);
            Ok(())
        }
        "restore" => {
            let name = extra_args.get(1).ok_or_else(|| {
                MaccError::Validation(
                    "Usage: macc coordinator sessions restore <name> [--dry-run]".into(),
                )
            })?;
            let dry_run = extra_args.iter().any(|a| a == "--dry-run");
            let meta = session_manager::restore_sessions(repo_root, name, dry_run)?;
            if dry_run {
                println!("Dry-run: would restore from snapshot '{}':", meta.name);
            } else {
                println!("Sessions restored from snapshot '{}':", meta.name);
            }
            println!("  Saved at:         {}", meta.saved_at);
            println!("  Tools:            {}", meta.tool_count);
            println!("  Active sessions:  {}", meta.active_session_count);
            println!("  Archived sessions:{}", meta.archived_session_count);
            Ok(())
        }
        "list" => {
            let snapshots = session_manager::list_saved_sessions(repo_root)?;
            if snapshots.is_empty() {
                println!("No saved session snapshots for this project.");
            } else {
                println!(
                    "{:<30} {:<24} {:>6} {:>8} {:>10}",
                    "NAME", "SAVED_AT", "TOOLS", "ACTIVE", "ARCHIVED"
                );
                for snap in &snapshots {
                    println!(
                        "{:<30} {:<24} {:>6} {:>8} {:>10}",
                        snap.name,
                        snap.saved_at,
                        snap.tool_count,
                        snap.active_session_count,
                        snap.archived_session_count,
                    );
                }
            }
            Ok(())
        }
        "delete" => {
            let name = extra_args.get(1).ok_or_else(|| {
                MaccError::Validation("Usage: macc coordinator sessions delete <name>".into())
            })?;
            session_manager::delete_saved_session(repo_root, name)?;
            println!("Deleted session snapshot '{}'.", name);
            Ok(())
        }
        other => Err(MaccError::Validation(format!(
            "Unknown sessions action '{}'. Available: save, restore, list, delete",
            other
        ))),
    }
}

fn format_duration_human(secs: u64) -> String {
    if secs >= 3600 {
        let h = secs / 3600;
        let m = (secs % 3600) / 60;
        format!("{}h{}m", h, m)
    } else if secs >= 60 {
        let m = secs / 60;
        let s = secs % 60;
        format!("{}m{}s", m, s)
    } else {
        format!("{}s", secs)
    }
}
