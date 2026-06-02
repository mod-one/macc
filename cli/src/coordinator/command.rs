use crate::coordinator::legacy_helpers::{
    stop_coordinator_process_groups, NativeCoordinatorLogger,
};
use crate::coordinator::render::print_status_summary;
use macc_core::coordinator::engine as coordinator_engine;
use macc_core::coordinator::types::CoordinatorEnvConfig;
use macc_core::coordinator_storage::CoordinatorStorageMode;
use macc_core::process_ownership::{ProcessHandle, ProcessKind};
use macc_core::service::coordinator_workflow::{
    coordinator_command_emits_runtime_events, coordinator_command_from_name, CoordinatorCommand,
    CoordinatorCommandRequest,
};
use macc_core::service::process_ownership_gate::{gate_owner_action, ClientContext};
use macc_core::{find_project_root, load_canonical_config, MaccError, Result};
use std::path::Path;
use std::process::{Command as ProcessCommand, Stdio};
#[cfg(unix)]
use libc;

const SUPERVISOR_PID_REL_PATH: &str = ".macc/state/supervisor.pid";
const COORDINATOR_SUPERVISOR_REL_PATH: &str = ".macc/state/coordinator-supervisor.json";

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

/// Which client the user wants to open alongside the coordinator run.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CoordinatorClientMode {
    /// Show the launch review and prompt interactively (default in a TTY).
    Interactive,
    /// Open the Ratatui TUI coordinator live screen.
    Tui,
    /// Start the local web server and print the dashboard URL.
    Web,
    /// Run headless, no client.
    None,
}

#[derive(Clone)]
pub struct CoordinatorCommandInput {
    pub command_name: String,
    pub client_id: String,
    /// Client to open after the coordinator starts.
    pub client_mode: CoordinatorClientMode,
    pub supervisor: bool,
    pub drain: bool,
    pub graceful: bool,
    pub force: bool,
    pub remove_worktrees: bool,
    pub remove_branches: bool,
    pub env_cfg: CoordinatorEnvConfig,
    pub extra_args: Vec<String>,
    // ── Reference branch preflight flags (spec §8.6) ──────────────────────
    /// Exit after preflight checks without starting coordinator.
    pub preflight_only: bool,
    /// Override dirty-branch block for this run.
    pub allow_dirty_reference: bool,
    /// Create the reference branch if it is missing.
    pub create_reference_branch: bool,
    /// Base branch/revision used when creating the reference branch.
    pub reference_branch_base: Option<String>,
}

struct ProjectContext {
    paths: macc_core::ProjectPaths,
    canonical: macc_core::config::CanonicalConfig,
    coordinator_cfg: Option<macc_core::config::CoordinatorConfig>,
}

impl ProjectContext {
    fn load(
        absolute_cwd: &Path,
        _engine: &crate::services::engine_provider::SharedEngine,
    ) -> Result<Self> {
        // Use find_project_root (no auto-create fallback): coordinator commands must
        // run against an existing MACC project. Silent auto-init at the wrong CWD
        // is what caused spurious project creation at $HOME and similar locations.
        let paths = find_project_root(absolute_cwd).map_err(|_| {
            MaccError::Validation(format!(
                "No MACC project found in '{}' or any parent directory. \
             Run 'macc init' in your repository root to initialize.",
                absolute_cwd.display()
            ))
        })?;
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
        input.drain,
        input.graceful,
        input.force,
        input.remove_worktrees,
        input.remove_branches,
    )?;
    let client_ctx = ClientContext {
        client_id: input.client_id.clone(),
        project_root: paths.root.clone(),
    };

    if command_requires_owner_gate(&command) {
        gate_owner_action(&client_ctx, &coordinator_process_handle(&paths.root))?;
    }

    // Supervisor is intentionally not spawned here. When command == "run",
    // the supervisor is started AFTER the coordinator child is spawned so it
    // can watch the actual coordinator child PID rather than the CLI process PID.
    // (See the launch_coordinator_with_client call below.)

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
    //
    // CoordinatorCommand::Run is included so that `macc coordinator run --no-tui`
    // catches errors in the parent process before the child is spawned with muted
    // stdio (control-plane-run). Without this, preflight errors in the child are
    // silently swallowed and the parent only sees a generic non-zero exit code.
    if matches!(
        command,
        CoordinatorCommand::Run
            | CoordinatorCommand::RunControlPlane
            | CoordinatorCommand::DispatchReadyTasks
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

    // Reference branch preflight gate (spec §7, §11.1).
    // Must run before any registry/worktree mutation.
    // CoordinatorCommand::Run is included for the same reason as above.
    if matches!(
        command,
        CoordinatorCommand::Run
            | CoordinatorCommand::RunControlPlane
            | CoordinatorCommand::DispatchReadyTasks
    ) {
        run_reference_branch_preflight(engine, paths, coordinator_cfg, &input)?;
    }

    // ── Client selection and launch review (motif §2) ─────────────────────────
    //
    // For `macc coordinator run`, after preflight passes, show a launch review
    // and ask which client to open (TUI / Web / None). In non-interactive mode
    // (--client flag set, or stdout is not a TTY), skip the prompt.
    if matches!(command, CoordinatorCommand::Run) {
        let chosen_mode = resolve_client_mode(&input, coordinator_cfg, paths);
        return launch_coordinator_with_client(
            chosen_mode,
            &input,
            paths,
            coordinator_cfg,
        );
    }

    if let CoordinatorCommand::Stop { drain, .. } = &command {
        if !*drain {
            // Stop any supervisor that was launched by this coordinator first.
            // Doing this before killing the coordinator prevents the supervisor's
            // --attach loop from detecting the coordinator is gone and triggering
            // an unwanted recovery/restart.
            stop_attached_supervisor_if_present(&paths.root);
            let coordinator_path = paths.automation_coordinator_path();
            let stopped =
                stop_coordinator_process_groups(&paths.root, &coordinator_path, input.graceful)?;
            println!("Coordinator process groups signaled: {}", stopped);
        }
    }

    let logger_action = match command {
        CoordinatorCommand::RunControlPlane => Some("run"),
        CoordinatorCommand::DispatchReadyTasks => Some("dispatch"),
        CoordinatorCommand::AdvanceTasks => Some("advance"),
        CoordinatorCommand::SyncRegistry => Some("sync"),
        CoordinatorCommand::SyncPrd => Some("sync-prd"),
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
    if let Some(processes) = response.processes {
        println!("{:<12} {:<12} {:<12} {:>8} {:>8} {:<10} {:<12} {}", "TASK ID", "CLAIM ID", "TOOL", "PID", "PGID", "STATUS", "HEARTBEAT", "WORKTREE");
        println!("{}", "-".repeat(100));
        for p in processes {
            println!(
                "{:<12} {:<12} {:<12} {:>8} {:>8} {:<10} {:<12} {}",
                p.task_id, p.claim_id, p.tool, p.pid, p.pgid, p.status, p.heartbeat, p.worktree
            );
        }
    }
    if let Some(report) = response.recovery_report {
        println!("{:<12} {:<30} {:<20} {:<30} {}", "TASK ID", "SITUATION", "CLASSIFICATION", "ACTION", "MUTATED");
        println!("{}", "-".repeat(100));
        for r in report {
            println!(
                "{:<12} {:<30} {:<20} {:<30} {}",
                r.task_id, r.situation, r.classification, r.action, r.mutated
            );
        }
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

    // Auto-save sessions after a full coordinator run completes and on graceful
    // stop, so 'macc init' can offer them on the next fresh checkout.
    let should_autosave_sessions =
        matches!(command, CoordinatorCommand::Stop { graceful: true, .. })
            || matches!(command, CoordinatorCommand::RunControlPlane);
    if should_autosave_sessions {
        match macc_core::coordinator::session_manager::save_sessions(&paths.root, None) {
            Ok(meta) => {
                println!(
                    "Sessions auto-saved as '{}' ({} session(s) across {} tool(s)).",
                    meta.name, meta.active_session_count, meta.tool_count
                );
            }
            Err(e) if e.to_string().contains("Nothing to save") => {}
            Err(e) => {
                eprintln!("Note: session auto-save failed: {}", e);
            }
        }
    }

    Ok(())
}

fn command_requires_owner_gate(command: &CoordinatorCommand) -> bool {
    matches!(
        command,
        CoordinatorCommand::Run
            | CoordinatorCommand::Stop { .. }
            | CoordinatorCommand::ResumePausedRun
            | CoordinatorCommand::Unlock { .. }
            | CoordinatorCommand::DispatchReadyTasks
            | CoordinatorCommand::AdvanceTasks
            | CoordinatorCommand::CleanupMaintenance
    )
}

fn coordinator_process_handle(project_root: &Path) -> ProcessHandle {
    ProcessHandle {
        kind: ProcessKind::Coordinator,
        project_root: project_root.to_path_buf(),
        pid: None,
    }
}

/// Spawn a supervisor daemon attached to the given coordinator child process.
///
/// `coordinator_pid` must be the actual coordinator child PID (not the CLI
/// process ID). The supervisor watches this PID and restarts the coordinator
/// if it crashes. Because the supervisor itself uses `setsid()` it also
/// survives SSH session close independently.
fn spawn_attached_supervisor(project_root: &Path, coordinator_pid: u32) -> Result<()> {
    let current_exe = std::env::current_exe().map_err(|e| MaccError::Io {
        path: project_root.to_string_lossy().into(),
        action: "resolve current executable for coordinator supervisor bootstrap".into(),
        source: e,
    })?;
    let status = ProcessCommand::new(current_exe)
        .current_dir(project_root)
        .env("MACC_INTERNAL_INVOCATION", "1")
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

    if !status.success() {
        return Err(MaccError::Validation(format!(
            "failed to start supervisor for coordinator run (exit code: {:?})",
            status.code()
        )));
    }

    // Poll for the supervisor daemon child to write its PID file (up to 2 s).
    // Once found, record the binding so `macc coordinator stop` can stop the
    // supervisor along with the coordinator.
    let supervisor_pid_path = project_root.join(SUPERVISOR_PID_REL_PATH);
    let mut supervisor_pid: Option<u32> = None;
    for _ in 0..20 {
        if let Ok(raw) = std::fs::read_to_string(&supervisor_pid_path) {
            if let Ok(pid) = raw.trim().parse::<u32>() {
                supervisor_pid = Some(pid);
                break;
            }
        }
        std::thread::sleep(std::time::Duration::from_millis(100));
    }

    if let Some(spid) = supervisor_pid {
        let marker = serde_json::json!({
            "coordinator_pid": coordinator_pid,
            "supervisor_pid": spid
        });
        let marker_path = project_root.join(COORDINATOR_SUPERVISOR_REL_PATH);
        if let Some(parent) = marker_path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        let _ = std::fs::write(&marker_path, format!("{}\n", marker));
    }

    Ok(())
}

/// Stop the supervisor that was spawned by `coordinator run --supervisor`, if
/// one is recorded in the marker file and is still alive.  Does nothing when
/// the marker is absent or stale (i.e., the coordinator that created it is no
/// longer running, meaning it already exited on its own).
fn stop_attached_supervisor_if_present(project_root: &Path) {
    let marker_path = project_root.join(COORDINATOR_SUPERVISOR_REL_PATH);
    let Ok(raw) = std::fs::read_to_string(&marker_path) else {
        return;
    };
    let Ok(marker) = serde_json::from_str::<serde_json::Value>(&raw) else {
        let _ = std::fs::remove_file(&marker_path);
        return;
    };
    let coordinator_pid = marker
        .get("coordinator_pid")
        .and_then(|v| v.as_u64())
        .map(|v| v as u32);
    let supervisor_pid = marker
        .get("supervisor_pid")
        .and_then(|v| v.as_u64())
        .map(|v| v as u32);

    let (Some(cpid), Some(spid)) = (coordinator_pid, supervisor_pid) else {
        let _ = std::fs::remove_file(&marker_path);
        return;
    };

    // Guard: only act if the coordinator that wrote this marker is currently
    // running.  A stale marker (coordinator already exited naturally) must not
    // cause an independently-started supervisor to be killed.
    if !pid_is_alive(cpid) {
        let _ = std::fs::remove_file(&marker_path);
        return;
    }

    // Send SIGTERM; give the supervisor up to 3 s to exit cleanly.
    let _ = ProcessCommand::new("kill")
        .arg("-TERM")
        .arg(spid.to_string())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status();

    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(3);
    while std::time::Instant::now() < deadline {
        if !pid_is_alive(spid) {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(100));
    }

    let _ = std::fs::remove_file(&marker_path);
    println!("Attached supervisor (pid {}) stopped.", spid);
}

fn pid_is_alive(pid: u32) -> bool {
    ProcessCommand::new("kill")
        .arg("-0")
        .arg(pid.to_string())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
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

/// Run the reference branch preflight gate and handle the result interactively (spec §11).
///
/// Returns `Ok(())` to proceed, or an `Err` to cancel the coordinator run.
fn run_reference_branch_preflight(
    engine: &crate::services::engine_provider::SharedEngine,
    paths: &macc_core::ProjectPaths,
    coordinator_cfg: Option<&macc_core::config::CoordinatorConfig>,
    input: &CoordinatorCommandInput,
) -> Result<()> {
    use macc_core::coordinator::preflight::{self, BranchCreateSource, ReferencePreflightAction};
    use macc_core::Engine as _;

    let repo_root = &paths.root;

    // Resolve preflight config from project config + CLI overrides.
    let raw = coordinator_cfg
        .and_then(|c| c.reference_branch_preflight.clone())
        .unwrap_or_default();
    let require_clean = coordinator_cfg.and_then(|c| c.require_clean_reference_branch);
    let mut cfg = raw.resolve(require_clean);

    // --allow-dirty-reference CLI flag overrides dirty_policy.
    if input.allow_dirty_reference {
        cfg.dirty_policy = preflight::DirtyReferencePolicy::Allow;
    }
    // --create-reference-branch enables non-interactive creation.
    if input.create_reference_branch {
        cfg.missing_branch_policy = preflight::MissingBranchPolicy::Create;
        cfg.allow_non_interactive_create = true;
    }

    if !cfg.enabled {
        return Ok(());
    }

    // Resolve reference_branch from env_cfg > coordinator_cfg > default.
    let reference_branch = input
        .env_cfg
        .reference_branch
        .clone()
        .or_else(|| coordinator_cfg.and_then(|c| c.reference_branch.clone()))
        .unwrap_or_else(|| "main".to_string());

    let report = engine
        .inspect_reference_preflight(paths, &reference_branch, &cfg)
        .map_err(|e| MaccError::Validation(e.to_string()))?;

    // Log preflight result to coordinator log if available.
    let log_event =
        preflight::build_preflight_log_event(&report, "pending", input.allow_dirty_reference);
    if cfg.log_clean_result
        || !matches!(report.status, preflight::ReferencePreflightStatus::Clean | preflight::ReferencePreflightStatus::NotCheckedOut)
    {
        if let Ok(log_json) = serde_json::to_string(&log_event) {
            let log_path = repo_root.join(".macc/log/coordinator/preflight-latest.json");
            if let Some(parent) = log_path.parent() {
                let _ = std::fs::create_dir_all(parent);
            }
            let _ = std::fs::write(&log_path, log_json);
        }
    }

    // --preflight-only: print result and exit.
    if input.preflight_only {
        println!("{}", preflight::format_report_cli(&report));
        return if matches!(report.recommended_action, ReferencePreflightAction::Proceed) {
            Ok(())
        } else {
            Err(MaccError::Validation(format!(
                "Preflight failed for reference branch \"{}\".",
                reference_branch
            )))
        };
    }

    match &report.status {
        preflight::ReferencePreflightStatus::Clean
        | preflight::ReferencePreflightStatus::NotCheckedOut => {
            if cfg.log_clean_result {
                println!("Reference branch: {}\nPreflight: OK", reference_branch);
            }
            Ok(())
        }

        preflight::ReferencePreflightStatus::BranchMissing => {
            // Non-interactive: fail fast unless --create-reference-branch was given.
            if input.create_reference_branch {
                let base = input
                    .reference_branch_base
                    .clone()
                    .or_else(|| report.remote_tracking_branches.first().cloned())
                    .unwrap_or_else(|| "HEAD".to_string());

                let source = if report.remote_tracking_branches.contains(&base) {
                    BranchCreateSource::RemoteTracking(base.clone())
                } else {
                    BranchCreateSource::LocalBranch(base.clone())
                };

                engine.create_reference_branch_via_engine(paths, &reference_branch, source)
                    .map_err(|e| MaccError::Validation(format!("{}", e)))?;

                println!(
                    "Created local branch \"{}\" from \"{}\".\nPreflight: OK",
                    reference_branch, base
                );
                return Ok(());
            }

            // Interactive: prompt the user.
            println!("{}", preflight::format_report_cli(&report));
            println!();
            println!("Options:");
            println!("  [1] Create from current HEAD");
            if !report.remote_tracking_branches.is_empty() {
                for (i, remote) in report.remote_tracking_branches.iter().enumerate() {
                    println!("  [{}] Create from {}", i + 2, remote);
                }
            }
            println!("  [c] Cancel");
            print!("Selection [c]: ");
            use std::io::Write;
            let _ = std::io::stdout().flush();
            let mut line = String::new();
            let _ = std::io::stdin().read_line(&mut line);
            let choice = line.trim().to_lowercase();

            if choice == "1" {
                engine.create_reference_branch_via_engine(paths, &reference_branch, BranchCreateSource::CurrentHead)
                    .map_err(|e| MaccError::Validation(format!("{}", e)))?;
                println!("Created local branch \"{}\" from HEAD.\nPreflight: OK", reference_branch);
                return Ok(());
            }

            if let Ok(idx) = choice.parse::<usize>() {
                let remote_idx = idx.saturating_sub(2);
                if let Some(remote) = report.remote_tracking_branches.get(remote_idx) {
                    engine.create_reference_branch_via_engine(paths, &reference_branch, BranchCreateSource::RemoteTracking(remote.clone()))
                        .map_err(|e| MaccError::Validation(format!("{}", e)))?;
                    println!(
                        "Created local tracking branch \"{}\" from \"{}\".\nPreflight: OK",
                        reference_branch, remote
                    );
                    return Ok(());
                }
            }

            Err(MaccError::Validation(format!(
                "Coordinator cancelled: reference branch \"{}\" does not exist.\n\
                 Use --create-reference-branch to create it automatically.",
                reference_branch
            )))
        }

        preflight::ReferencePreflightStatus::Dirty => {
            // Non-interactive (allow_dirty_reference already set cfg.dirty_policy to Allow).
            if matches!(cfg.dirty_policy, preflight::DirtyReferencePolicy::Allow) {
                eprintln!(
                    "WARNING: Reference branch \"{}\" has uncommitted changes.\n\
                     Override accepted because --allow-dirty-reference was provided.",
                    reference_branch
                );
                return Ok(());
            }
            if matches!(cfg.dirty_policy, preflight::DirtyReferencePolicy::Warn) {
                eprintln!("{}", preflight::format_report_cli(&report));
                return Ok(());
            }

            // Interactive prompt.
            println!("{}", preflight::format_report_cli(&report));
            println!();
            println!("Options:");
            println!("  [1] Cancel  (recommended)");
            println!("  [2] Continue once");
            print!("Selection [1]: ");
            use std::io::Write;
            let _ = std::io::stdout().flush();
            let mut line = String::new();
            let _ = std::io::stdin().read_line(&mut line);
            if line.trim() == "2" {
                eprintln!(
                    "WARNING: Continuing with dirty reference branch \"{}\".",
                    reference_branch
                );
                Ok(())
            } else {
                Err(MaccError::Validation(format!(
                    "{}: Reference branch \"{}\" has uncommitted changes.\n\
                     Commit, stash, discard, or rerun with --allow-dirty-reference.",
                    preflight::E702,
                    reference_branch
                )))
            }
        }

        preflight::ReferencePreflightStatus::InvalidBranchName => Err(MaccError::Validation(
            preflight::format_report_cli(&report),
        )),

        preflight::ReferencePreflightStatus::BareRepository => Err(MaccError::Validation(
            preflight::format_report_cli(&report),
        )),

        preflight::ReferencePreflightStatus::GitInspectionFailed => {
            Err(MaccError::Validation(preflight::format_report_cli(&report)))
        }
    }
}

// ── Client selection and launch (motif §2–5) ──────────────────────────────────

/// Full 8-step flow: load config → resolve → preflight → summary → client →
/// confirm → start engine → attach client.
///
/// Called only for `CoordinatorCommand::Run` with interactive or explicit client.
fn resolve_client_mode(
    input: &CoordinatorCommandInput,
    coordinator_cfg: Option<&macc_core::config::CoordinatorConfig>,
    paths: &macc_core::ProjectPaths,
) -> CoordinatorClientMode {
    // Explicit CLI flag wins — skip config and prompt.
    if input.client_mode != CoordinatorClientMode::Interactive {
        return input.client_mode.clone();
    }

    // Read client preferences from config.
    let client_cfg = coordinator_cfg
        .and_then(|c| c.client.as_ref())
        .cloned()
        .unwrap_or_default();

    let is_tty = std::io::IsTerminal::is_terminal(&std::io::stdout());

    // Resolve default mode from config (falls through to Interactive/None).
    let config_default_mode = match client_cfg.default.as_deref() {
        Some("tui") => Some(CoordinatorClientMode::Tui),
        Some("web") => Some(CoordinatorClientMode::Web),
        Some("none") => Some(CoordinatorClientMode::None),
        Some("auto") => {
            if is_tty { Some(CoordinatorClientMode::Tui) } else { Some(CoordinatorClientMode::None) }
        }
        // "prompt" or absent: fall through to interactive or headless below
        _ => None,
    };

    // Non-interactive terminal: use config default or fall back to headless.
    if !is_tty {
        return config_default_mode.unwrap_or(CoordinatorClientMode::None);
    }

    // If config says non-prompt mode AND show_preflight is false AND no confirmation needed:
    // skip the review entirely and return the configured mode.
    let show_preflight = client_cfg.show_preflight.unwrap_or(true);
    let require_confirmation = client_cfg.require_confirmation.unwrap_or(true);
    if let Some(ref forced_mode) = config_default_mode {
        if !show_preflight && !require_confirmation {
            return forced_mode.clone();
        }
    }

    // ── Steps 4+5+6: display full launch summary, ask client, confirm ─────────
    let warnings = collect_launch_warnings(input, coordinator_cfg, paths);

    if show_preflight {
        print_launch_review(input, coordinator_cfg, paths, &warnings, &client_cfg);
    }

    // Step 5 — choose client (or use config default).
    let mode = if let Some(forced) = config_default_mode {
        // Config has a non-prompt default: skip the prompt but still confirm.
        let label = match &forced {
            CoordinatorClientMode::Tui => "TUI",
            CoordinatorClientMode::Web => "Web",
            _ => "headless",
        };
        println!("Client:");
        println!("  Using configured default: {} (change with --client or automation.coordinator.client.default)", label);
        println!();
        forced
    } else {
        println!("Client:");
        println!("  Choose how to monitor this run:");
        println!();
        println!("  [1] TUI client (default)");
        println!("  [2] Web client");
        println!("  [3] No client / headless");
        println!();
        let client_choice = prompt("  Selection [1]: ");
        match client_choice.trim() {
            "2" => CoordinatorClientMode::Web,
            "3" => CoordinatorClientMode::None,
            _ => CoordinatorClientMode::Tui,
        }
    };

    // Step 6 — final confirmation (respects require_confirmation config).
    if require_confirmation {
        println!();
        let mode_label = match &mode {
            CoordinatorClientMode::Tui => "TUI",
            CoordinatorClientMode::Web => "Web",
            _ => "headless",
        };
        if !warnings.is_empty() {
            println!("  {} warning(s) noted above.", warnings.len());
        }
        let confirm = prompt(&format!("  Start coordinator ({})? [Y/n]: ", mode_label));
        if matches!(confirm.trim().to_lowercase().as_str(), "n" | "no") {
            println!("Cancelled.");
            std::process::exit(0);
        }
        println!();
    }

    mode
}

fn prompt(msg: &str) -> String {
    use std::io::Write;
    print!("{}", msg);
    let _ = std::io::stdout().flush();
    let mut line = String::new();
    let _ = std::io::stdin().read_line(&mut line);
    line
}

/// Collect safety warnings to surface in the launch review.
fn collect_launch_warnings(
    input: &CoordinatorCommandInput,
    coordinator_cfg: Option<&macc_core::config::CoordinatorConfig>,
    paths: &macc_core::ProjectPaths,
) -> Vec<String> {
    use macc_core::config::CoordinatorConfigResolved;
    let resolved = CoordinatorConfigResolved::resolve(coordinator_cfg);
    let mut warnings: Vec<String> = Vec::new();

    // Web host is non-localhost.
    let client_cfg = coordinator_cfg.and_then(|c| c.client.as_ref());
    let web_host = client_cfg
        .and_then(|c| c.web_host.as_deref())
        .unwrap_or("127.0.0.1");
    if web_host != "127.0.0.1" && web_host != "localhost" {
        warnings.push(format!(
            "Web client will bind to {}, not localhost.",
            web_host
        ));
    }

    // High max_parallel — rate-limit risk.
    let max_parallel = input.env_cfg.max_parallel.unwrap_or(resolved.max_parallel);
    if max_parallel > 8 {
        warnings.push(format!(
            "Max parallel is {}; this may increase provider rate-limit risk.",
            max_parallel
        ));
    }

    // Reference branch behind remote.
    let ref_branch = input
        .env_cfg
        .reference_branch
        .as_deref()
        .unwrap_or(&resolved.reference_branch);
    let remote_ref = format!("origin/{}", ref_branch);
    let behind = std::process::Command::new("git")
        .args(["rev-list", "--count", &format!("{}..{}", ref_branch, remote_ref)])
        .current_dir(&paths.root)
        .output()
        .ok()
        .and_then(|o| if o.status.success() { Some(o) } else { None })
        .and_then(|o| String::from_utf8(o.stdout).ok())
        .and_then(|s| s.trim().parse::<u64>().ok())
        .unwrap_or(0);
    if behind > 0 {
        warnings.push(format!(
            "Reference branch \"{}\" is {} commit(s) behind {}.",
            ref_branch, behind, remote_ref
        ));
    }

    // Dirty working tree in reference branch worktrees.
    // (Already caught by preflight — only warn if preflight was overridden.)
    if input.allow_dirty_reference {
        warnings.push("Working tree has uncommitted changes (--allow-dirty-reference active).".into());
    }

    // Tool priority contains unavailable tools.
    for tool_id in &resolved.tool_priority {
        let available = std::process::Command::new("which")
            .arg(tool_id)
            .output()
            .map(|o| o.status.success())
            .unwrap_or(false);
        if !available {
            warnings.push(format!(
                "Tool \"{}\" is in tool_priority but not available in PATH.",
                tool_id
            ));
        }
    }

    warnings
}

/// Print the full Coordinator Launch Review (spec §4).
fn print_launch_review(
    input: &CoordinatorCommandInput,
    coordinator_cfg: Option<&macc_core::config::CoordinatorConfig>,
    paths: &macc_core::ProjectPaths,
    warnings: &[String],
    client_cfg: &macc_core::config::CoordinatorClientConfig,
) {
    use macc_core::config::CoordinatorConfigResolved;
    let resolved = CoordinatorConfigResolved::resolve(coordinator_cfg);

    let prd = resolved.prd_file.as_deref().unwrap_or("prd.json");
    let ref_branch = input
        .env_cfg
        .reference_branch
        .as_deref()
        .unwrap_or(&resolved.reference_branch);

    println!();
    println!("MACC Coordinator Launch Review");
    println!();

    // ── Project and task source ───────────────────────────────────────────────
    println!("Project:");
    println!("  Root:              {}", paths.root.display());
    println!("  PRD:               {}", prd);
    println!("  Reference branch:  {}", ref_branch);
    println!();

    // ── Dispatch policy ───────────────────────────────────────────────────────
    let max_parallel = input.env_cfg.max_parallel.unwrap_or(resolved.max_parallel);
    let max_dispatch = input.env_cfg.max_dispatch.unwrap_or(resolved.max_dispatch);
    let tool_priority = if !resolved.tool_priority.is_empty() {
        resolved.tool_priority.join(", ")
    } else {
        "(any enabled)".to_string()
    };
    println!("Dispatch:");
    println!("  Max parallel:      {}", max_parallel);
    println!("  Max dispatch:      {}", max_dispatch);
    println!("  Tool priority:     {}", tool_priority);
    println!();

    // ── Stale and recovery policy ─────────────────────────────────────────────
    let stale_action = input
        .env_cfg
        .stale_action
        .as_deref()
        .or_else(|| coordinator_cfg.and_then(|c| c.stale_action.as_deref()))
        .unwrap_or("block");
    let stale_claimed = coordinator_cfg
        .and_then(|c| c.stale_claimed_seconds)
        .map(|s| format!("{}s", s))
        .unwrap_or_else(|| "default".into());
    let stale_in_progress = coordinator_cfg
        .and_then(|c| c.stale_in_progress_seconds)
        .map(|s| format!("{}s", s))
        .unwrap_or_else(|| "default".into());
    println!("Stale / Recovery:");
    println!("  Stale action:      {}", stale_action);
    println!("  Stale claimed:     {}", stale_claimed);
    println!("  Stale in-progress: {}", stale_in_progress);
    println!();

    // ── Merge and retry policy ────────────────────────────────────────────────
    let merge_ai_fix = coordinator_cfg
        .and_then(|c| c.merge_ai_fix)
        .map(|v| if v { "enabled" } else { "disabled" })
        .unwrap_or("disabled");
    let max_review_cycles = coordinator_cfg
        .and_then(|c| c.max_review_cycles)
        .map(|n| n.to_string())
        .unwrap_or_else(|| "default".into());
    let retry_codes = coordinator_cfg
        .and_then(|c| c.error_code_retry_list.as_deref())
        .unwrap_or("(none)");
    let retry_max = coordinator_cfg
        .and_then(|c| c.error_code_retry_max)
        .map(|n| n.to_string())
        .unwrap_or_else(|| "default".into());
    let rl_backoff = coordinator_cfg
        .and_then(|c| c.rate_limit_backoff_base_seconds)
        .map(|n| format!("{}s base", n))
        .unwrap_or_else(|| "default".into());
    println!("Merge / Retry:");
    println!("  Merge AI fix:      {}", merge_ai_fix);
    println!("  Max review cycles: {}", max_review_cycles);
    println!("  Retry error codes: {}", retry_codes);
    println!("  Retry max:         {}", retry_max);
    println!("  RL backoff:        {}", rl_backoff);
    println!();

    // ── Client and observability ──────────────────────────────────────────────
    let web_port = client_cfg.web_port.unwrap_or(3450);
    let web_host = client_cfg.web_host.as_deref().unwrap_or("127.0.0.1");
    let log_dir = paths.macc_dir.join("log/coordinator");
    println!("Observability:");
    println!("  TUI available:     yes");
    println!("  Web port:          {}", web_port);
    println!("  Web host:          {}", web_host);
    println!("  Log directory:     {}", log_dir.display());
    println!("  SSE event stream:  /api/v1/sse (when web is running)");
    println!("  Ops audit log:     .macc/log/ops.jsonl");
    println!();

    // ── Safety warnings ───────────────────────────────────────────────────────
    if !warnings.is_empty() {
        println!("Warnings:");
        for w in warnings {
            println!("  - {}", w);
        }
        println!();
    }
}

/// Start the coordinator and open the chosen client.
fn launch_coordinator_with_client(
    mode: CoordinatorClientMode,
    input: &CoordinatorCommandInput,
    paths: &macc_core::ProjectPaths,
    coordinator_cfg: Option<&macc_core::config::CoordinatorConfig>,
) -> Result<()> {
    let phase_overrides = build_phase_overrides_label(input);

    match mode {
        CoordinatorClientMode::Tui => {
            // TUI path: the TUI itself starts the coordinator daemon and
            // connects to it. Once the TUI exits the coordinator keeps running
            // in the background (coordinator child has setsid() — terminal-independent).
            // If --supervisor was requested, start the supervisor AFTER the TUI
            // has launched the coordinator so we have the real child PID.
            let result = macc_tui::run_tui_with_launch(macc_tui::LaunchMode::CoordinatorRun {
                phase_overrides,
            })
            .map_err(|e| MaccError::Io {
                path: "tui".into(),
                action: "run_tui coordinator live".into(),
                source: std::io::Error::other(e.to_string()),
            });
            // Best-effort supervisor start after TUI (coordinator child already running).
            if input.supervisor {
                if let Ok(coord_pid) = coordinator_child_pid_from_registry(&paths.root) {
                    let _ = spawn_attached_supervisor(&paths.root, coord_pid);
                }
            }
            result
        }

        CoordinatorClientMode::Web => {
            let client_cfg = coordinator_cfg
                .and_then(|c| c.client.as_ref())
                .cloned()
                .unwrap_or_default();
            let port = client_cfg.web_port.unwrap_or(3450);
            let host = client_cfg.web_host.as_deref().unwrap_or("127.0.0.1");
            let url = format!("http://{}:{}/ops/console", host, port);

            // Start coordinator as background daemon.
            let coord_pid = run_coordinator_daemon(paths, coordinator_cfg)?;

            // Start supervisor with coordinator child PID (not CLI PID).
            if input.supervisor {
                let _ = spawn_attached_supervisor(&paths.root, coord_pid as u32);
            }

            // Launch web server as a background daemon so the CLI can return.
            if let Err(e) = spawn_web_daemon(&paths.root) {
                println!("Note: could not start web server daemon: {}", e);
                println!("Start it manually: macc web");
            } else {
                // Give the server a moment to bind before printing the URL.
                std::thread::sleep(std::time::Duration::from_millis(300));
            }

            if client_cfg.open_browser.unwrap_or(false) {
                let _ = ProcessCommand::new("sh")
                    .args(["-c", &format!("open '{url}' 2>/dev/null || xdg-open '{url}' 2>/dev/null || true")])
                    .spawn();
                println!("Dashboard opening in browser: {}", url);
            } else {
                println!("Dashboard: {}", url);
            }
            Ok(())
        }

        CoordinatorClientMode::None | CoordinatorClientMode::Interactive => {
            // Start coordinator as background daemon; return immediately.
            let coord_pid = run_coordinator_daemon(paths, coordinator_cfg)?;
            if input.supervisor {
                let _ = spawn_attached_supervisor(&paths.root, coord_pid as u32);
            }
            Ok(())
        }
    }
}

fn build_phase_overrides_label(input: &CoordinatorCommandInput) -> Option<String> {
    let mut parts: Vec<String> = Vec::new();
    let testing_off = input.env_cfg.disable_testing == Some(true)
        || input.env_cfg.testing_mode.as_deref() == Some("disabled");
    let review_off = input.env_cfg.disable_review == Some(true)
        || input.env_cfg.review_mode.as_deref() == Some("disabled");
    if testing_off {
        parts.push("[testing:off]".to_string());
    } else if let Some(ref mode) = input.env_cfg.testing_mode {
        parts.push(format!("[testing:{}]", mode));
    }
    if review_off {
        parts.push("[review:off]".to_string());
    } else if let Some(ref mode) = input.env_cfg.review_mode {
        parts.push(format!("[review:{}]", mode));
    }
    if parts.is_empty() { None } else { Some(parts.join(" ")) }
}

/// Start the coordinator as a background daemon and return immediately.
///
/// The coordinator child process has `setsid()` applied (in `coordinator.rs`)
/// so it runs in its own session, independent of any controlling terminal or
/// SSH session. Closing the terminal / SSH that ran this command does not
/// affect the coordinator or any of its performers/workers.
fn run_coordinator_daemon(
    paths: &macc_core::ProjectPaths,
    coordinator_cfg: Option<&macc_core::config::CoordinatorConfig>,
) -> Result<i32> {
    use macc_core::service::coordinator::coordinator_start_managed_command_process_with_pid;
    use macc_core::service::coordinator_workflow::coordinator_command_invocation;

    let invocation = coordinator_command_invocation(
        &macc_core::service::coordinator_workflow::CoordinatorCommand::Run,
    )?;
    let pid = coordinator_start_managed_command_process_with_pid(
        paths,
        invocation.action,
        &invocation.args,
        coordinator_cfg,
    )?;

    println!("Coordinator started (pid {}).", pid);
    println!("  Monitor : macc status");
    println!("  Live TUI: macc tui");
    println!("  Stop    : macc coordinator stop");

    Ok(pid)
}

/// Read the coordinator child PID from the managed command registry.
fn coordinator_child_pid_from_registry(project_root: &Path) -> Result<u32> {
    use macc_core::coordinator::managed_command_registry::get_managed_command;
    let paths = macc_core::ProjectPaths::from_root(project_root);
    get_managed_command(&paths, "run")?
        .map(|r| r.pid as u32)
        .ok_or_else(|| MaccError::Validation("coordinator is not running".into()))
}

/// Spawn the web server as a background daemon (setsid + null stdio).
fn spawn_web_daemon(project_root: &Path) -> Result<()> {
    let current_exe = std::env::current_exe().map_err(|e| MaccError::Io {
        path: project_root.to_string_lossy().into(),
        action: "resolve current exe for web daemon".into(),
        source: e,
    })?;
    let mut cmd = ProcessCommand::new(current_exe);
    cmd.arg("--cwd")
        .arg(project_root)
        .arg("web")
        .env("MACC_INTERNAL_INVOCATION", "1")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());

    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        unsafe {
            cmd.pre_exec(|| {
                libc::setsid();
                Ok(())
            });
        }
    }

    cmd.spawn().map_err(|e| MaccError::Io {
        path: project_root.to_string_lossy().into(),
        action: "spawn web server daemon".into(),
        source: e,
    })?;
    Ok(())
}

// Legacy alias kept so the large polling loop below compiles; unused now.
#[allow(dead_code)]
fn run_headless_coordinator(
    paths: &macc_core::ProjectPaths,
    coordinator_cfg: Option<&macc_core::config::CoordinatorConfig>,
) -> Result<()> {
    run_coordinator_daemon(paths, coordinator_cfg).map(|_| ())
}

/// Dead code: the blocking poll loop below is retained only for reference.
/// `run_coordinator_daemon` replaces it.
#[allow(dead_code)]
fn _poll_coordinator_until_done(
    paths: &macc_core::ProjectPaths,
) -> Result<()> {
    use macc_core::service::coordinator::{
        coordinator_poll_managed_command_process,
        CoordinatorManagedCommandPoll,
    };
    loop {
        match coordinator_poll_managed_command_process(paths)? {
            CoordinatorManagedCommandPoll::Idle => return Ok(()),
            CoordinatorManagedCommandPoll::Running { elapsed_secs, .. } => {
                if elapsed_secs % 30 == 0 && elapsed_secs > 0 {
                    println!("Coordinator running… {}s elapsed", elapsed_secs);
                }
                std::thread::sleep(std::time::Duration::from_millis(500));
            }
            CoordinatorManagedCommandPoll::Exited { success, code, command, .. } => {
                if success {
                    println!("Coordinator '{}' completed.", command);
                    return Ok(());
                }
                return Err(MaccError::Validation(format!(
                    "Coordinator '{}' exited with code {:?}.",
                    command, code
                )));
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{handle, CoordinatorCommandInput};
    use crate::services::engine_provider::SharedEngine;
    use macc_core::config::CanonicalConfig;
    use macc_core::coordinator::types::CoordinatorEnvConfig;
    use macc_core::plan::{ActionPlan, PlannedOp};
    use macc_core::process_ownership::{ClientIdentity, ClientKind, ProcessHandle, ProcessKind};
    use macc_core::resolve::{CliOverrides, MaterializedFetchUnit};
    use macc_core::service::coordinator_workflow::{
        CoordinatorCommand, CoordinatorCommandRequest, CoordinatorCommandResult,
    };
    use macc_core::service::process_ownership::{claim_owner, register_process};
    use macc_core::tool::{ToolDescriptor, ToolDiagnostic};
    use macc_core::{ApplyReport, Engine, MaccError, ProjectPaths, TestEngine};
    use std::fs;
    use std::path::Path;
    use std::sync::{Arc, Mutex};
    use std::time::{SystemTime, UNIX_EPOCH};

    #[test]
    fn stop_rejects_non_owner_before_engine_dispatch() {
        let dir = temp_project_root();
        let handle_record = coordinator_record_handle(&dir, Some(4242));
        register_process(&dir, handle_record.clone()).expect("register");
        claim_owner(&dir, &handle_record, sample_client("client-A")).expect("claim owner");

        let engine = Arc::new(RecordingEngine::default());
        let err = handle(
            &dir,
            &(Arc::clone(&engine) as SharedEngine),
            coordinator_input("stop", "client-B"),
        )
        .expect_err("viewer should be rejected");

        match err {
            MaccError::NotProcessOwner {
                handle,
                current_owner,
            } => {
                // Gate echoes the caller-supplied handle (kind matches; pid may be None).
                assert_eq!(handle.kind, handle_record.kind);
                assert_eq!(handle.project_root, handle_record.project_root);
                assert_eq!(current_owner.as_deref(), Some("client-A"));
            }
            other => panic!("expected NotProcessOwner, got {other:?}"),
        }
        assert_eq!(engine.execute_calls(), 0);
    }

    #[test]
    fn stop_allows_owner_and_dispatches_engine_command() {
        let dir = temp_project_root();
        let handle_record = coordinator_record_handle(&dir, Some(4242));
        register_process(&dir, handle_record.clone()).expect("register");
        claim_owner(&dir, &handle_record, sample_client("client-A")).expect("claim owner");

        let engine = Arc::new(RecordingEngine::default());
        handle(
            &dir,
            &(Arc::clone(&engine) as SharedEngine),
            coordinator_input("stop", "client-A"),
        )
        .expect("owner should pass");

        assert_eq!(engine.execute_calls(), 1);
        assert_eq!(
            engine.last_command(),
            Some(CoordinatorCommand::Stop {
                drain: false,
                graceful: false,
                force: false,
                remove_worktrees: false,
                remove_branches: false,
                reason: "manual stop".to_string(),
            })
        );
    }

    struct RecordingEngine {
        inner: TestEngine,
        execute_calls: Mutex<usize>,
        last_command: Mutex<Option<CoordinatorCommand>>,
    }

    impl Default for RecordingEngine {
        fn default() -> Self {
            Self {
                inner: TestEngine::with_fixtures(),
                execute_calls: Mutex::new(0),
                last_command: Mutex::new(None),
            }
        }
    }

    impl RecordingEngine {
        fn execute_calls(&self) -> usize {
            *self.execute_calls.lock().expect("lock")
        }

        fn last_command(&self) -> Option<CoordinatorCommand> {
            self.last_command.lock().expect("lock").clone()
        }
    }

    impl Engine for RecordingEngine {
        fn list_tools(&self, paths: &ProjectPaths) -> (Vec<ToolDescriptor>, Vec<ToolDiagnostic>) {
            self.inner.list_tools(paths)
        }

        fn doctor(&self, paths: &ProjectPaths) -> Vec<macc_core::doctor::ToolCheck> {
            self.inner.doctor(paths)
        }

        fn plan(
            &self,
            paths: &ProjectPaths,
            config: &CanonicalConfig,
            materialized_units: &[MaterializedFetchUnit],
            overrides: &CliOverrides,
        ) -> macc_core::Result<ActionPlan> {
            self.inner
                .plan(paths, config, materialized_units, overrides)
        }

        fn plan_operations(&self, paths: &ProjectPaths, plan: &ActionPlan) -> Vec<PlannedOp> {
            self.inner.plan_operations(paths, plan)
        }

        fn apply(
            &self,
            paths: &ProjectPaths,
            plan: &mut ActionPlan,
            allow_user_scope: bool,
        ) -> macc_core::Result<ApplyReport> {
            self.inner.apply(paths, plan, allow_user_scope)
        }

        fn builtin_skills(&self) -> Vec<macc_core::catalog::Skill> {
            self.inner.builtin_skills()
        }

        fn builtin_agents(&self) -> Vec<macc_core::catalog::Agent> {
            self.inner.builtin_agents()
        }

        fn coordinator_execute_command(
            &self,
            _paths: &ProjectPaths,
            command: CoordinatorCommand,
            _request: CoordinatorCommandRequest<'_>,
        ) -> macc_core::Result<CoordinatorCommandResult> {
            *self.execute_calls.lock().expect("lock") += 1;
            *self.last_command.lock().expect("lock") = Some(command);
            Ok(CoordinatorCommandResult::default())
        }
    }

    fn coordinator_input(command_name: &str, client_id: &str) -> CoordinatorCommandInput {
        CoordinatorCommandInput {
            command_name: command_name.to_string(),
            client_id: client_id.to_string(),
            client_mode: CoordinatorClientMode::None,
            supervisor: false,
            drain: false,
            graceful: false,
            force: false,
            remove_worktrees: false,
            remove_branches: false,
            env_cfg: CoordinatorEnvConfig::default(),
            extra_args: Vec::new(),
            preflight_only: false,
            allow_dirty_reference: false,
            create_reference_branch: false,
            reference_branch_base: None,
        }
    }

    fn coordinator_record_handle(project_root: &Path, pid: Option<i32>) -> ProcessHandle {
        ProcessHandle {
            kind: ProcessKind::Coordinator,
            project_root: project_root.to_path_buf(),
            pid,
        }
    }

    fn sample_client(client_id: &str) -> ClientIdentity {
        let now = chrono::Utc::now().to_rfc3339();
        ClientIdentity {
            client_id: client_id.to_string(),
            kind: ClientKind::Cli,
            connected_at: now.clone(),
            last_heartbeat: now,
        }
    }

    fn temp_project_root() -> std::path::PathBuf {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("time")
            .as_nanos();
        let dir = std::env::temp_dir().join(format!("macc-cli-owner-gate-{unique}"));
        let macc_dir = dir.join(".macc");
        fs::create_dir_all(&macc_dir).expect("create .macc");
        fs::write(macc_dir.join("macc.yaml"), "tools:\n  enabled: []\n").expect("write config");
        dir
    }
}
