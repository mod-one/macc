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

#[derive(Debug, Clone)]
pub struct CoordinatorCommandInput {
    pub command_name: String,
    pub client_id: String,
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
        input.graceful,
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
                graceful: false,
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
            no_tui: true,
            supervisor: false,
            graceful: false,
            remove_worktrees: false,
            remove_branches: false,
            env_cfg: CoordinatorEnvConfig::default(),
            extra_args: Vec::new(),
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
