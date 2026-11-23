use crate::commands::AppContext;
use crate::commands::Command;
use crate::SupervisorCommands;
use macc_core::process_ownership::{ProcessHandle, ProcessKind};

use macc_core::supervisor::{
    instance::{self, Instance, Owner},
    SupervisorReport,
};
use macc_core::{MaccError, Result};
use serde_json::Value;
use std::fs;
#[cfg(unix)]
use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Command as ProcessCommand, Stdio};
use std::time::{Duration, Instant};

const SUPERVISOR_HEALTH_REL_PATH: &str = ".macc/state/supervisor-health.json";
const SUPERVISOR_DAEMON_CHILD_ENV: &str = "MACC_SUPERVISOR_DAEMON_CHILD";

pub struct SupervisorCommand<'a> {
    app: AppContext,
    command: &'a SupervisorCommands,
}

impl<'a> SupervisorCommand<'a> {
    pub fn new(app: AppContext, command: &'a SupervisorCommands) -> Self {
        Self { app, command }
    }
}

impl<'a> Command for SupervisorCommand<'a> {
    fn run(&self) -> Result<()> {
        match self.command {
            SupervisorCommands::Config { command } => {
                super::supervisor_settings::run(&self.app, command)
            }
            SupervisorCommands::Start {
                daemon,
                attach,
                coordinator_pid,
                retry,
            } => self.start(*daemon, *attach, *coordinator_pid, *retry),
            SupervisorCommands::Stop => self.stop(),
            SupervisorCommands::Prune { force } => self.prune(*force),
            SupervisorCommands::Status => self.status(),
            SupervisorCommands::Report => self.report(),
        }
    }
}

impl<'a> SupervisorCommand<'a> {
    fn start(
        &self,
        daemon: bool,
        attach: bool,
        coordinator_pid: Option<u32>,
        retry: bool,
    ) -> Result<()> {
        let paths = self.app.ensure_initialized_paths()?;
        let canonical = self.app.canonical_config()?;
        let settings = canonical.automation.supervisor.clone().unwrap_or_default();
        settings.validate()?;
        if settings.validation_commands.is_empty() {
            eprintln!("Supervisor tests disabled: validation_commands is empty. Add at least one validation command to enable tests; AI verification keeps its configured setting.");
        }
        if settings.repair_enabled {
            if let Err(error) = macc_core::supervisor::validation::commands(&paths.root, &settings)
            {
                eprintln!("Supervisor can diagnose but cannot repair yet: {error}. Configure `macc supervisor config set validation_commands '<JSON array>'`, then `macc supervisor config check`.");
            }
        }

        if daemon {
            if Instance::running(&paths.root)?.is_some() {
                return Err(MaccError::Validation(
                    "Supervisor is already running".into(),
                ));
            }
            let current_exe = std::env::current_exe().map_err(|e| MaccError::Io {
                path: paths.root.to_string_lossy().into(),
                action: "resolve current executable for supervisor daemon".into(),
                source: e,
            })?;

            let log_dir = paths.root.join(".macc/log/supervisor");
            fs::create_dir_all(&log_dir).map_err(|e| MaccError::Io {
                path: log_dir.display().to_string(),
                action: "create supervisor log directory".into(),
                source: e,
            })?;
            let daemon_log = fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open(log_dir.join("daemon.log"))
                .map_err(|e| MaccError::Io {
                    path: log_dir.display().to_string(),
                    action: "open supervisor daemon log".into(),
                    source: e,
                })?;
            let mut daemon_cmd = ProcessCommand::new(current_exe);
            daemon_cmd
                .current_dir(&paths.root)
                .arg("--cwd")
                .arg(&paths.root)
                .arg("supervisor")
                .arg("start")
                .args(attach.then_some("--attach"))
                .args(retry.then_some("--retry"))
                .args(
                    coordinator_pid
                        .map(|pid| vec!["--coordinator-pid".to_string(), pid.to_string()])
                        .unwrap_or_default(),
                )
                .env(SUPERVISOR_DAEMON_CHILD_ENV, "1")
                .env("MACC_INTERNAL_INVOCATION", "1")
                .stdin(Stdio::null())
                .stdout(Stdio::from(daemon_log.try_clone().map_err(|e| {
                    MaccError::Io {
                        path: log_dir.display().to_string(),
                        action: "clone supervisor log".into(),
                        source: e,
                    }
                })?))
                .stderr(Stdio::from(daemon_log));

            // UNIX daemonization: call setsid() in the child after fork but before
            // exec so the daemon gets its own session with no controlling terminal.
            // Without this, SIGHUP from terminal close propagates to the entire
            // original session and kills the supervisor together with the coordinator.
            #[cfg(unix)]
            // SAFETY: setsid(2) is async-signal-safe. The only restriction is that
            // the calling process must not be a process-group leader, which is
            // always true for a freshly forked child.
            unsafe {
                daemon_cmd.pre_exec(|| {
                    if libc::setsid() == -1 {
                        return Err(std::io::Error::last_os_error());
                    }
                    Ok(())
                });
            }

            let mut child = daemon_cmd.spawn().map_err(|e| MaccError::Io {
                path: paths.root.to_string_lossy().into(),
                action: "spawn supervisor daemon".into(),
                source: e,
            })?;

            let deadline = Instant::now() + Duration::from_secs(10);
            loop {
                if Instance::running(&paths.root)?.is_some_and(|owner| owner.pid == child.id())
                    && paths.root.join(SUPERVISOR_HEALTH_REL_PATH).exists()
                {
                    break;
                }
                if child
                    .try_wait()
                    .map_err(|e| MaccError::Validation(e.to_string()))?
                    .is_some()
                    || Instant::now() >= deadline
                {
                    let _ = child.kill();
                    let _ = child.wait();
                    return Err(MaccError::Validation(format!("Supervisor did not become ready; inspect {}/.macc/log/supervisor/daemon.log", paths.root.display())));
                }
                std::thread::sleep(Duration::from_millis(50));
            }
            println!("Supervisor started in daemon mode (pid {}).", child.id());
            return Ok(());
        }

        let owner = Owner::acquire(&paths.root)?;
        if retry {
            super::supervisor_runtime::request_retry(&paths)?;
            let ledger = paths.root.join(".macc/state/supervisor-interventions.json");
            if ledger.exists() {
                let backup = ledger.with_extension(format!(
                    "{}.json",
                    chrono::Utc::now().timestamp_nanos_opt().unwrap_or_default()
                ));
                fs::rename(&ledger, &backup).map_err(|source| MaccError::Io {
                    path: ledger.display().to_string(),
                    action: "archive supervisor intervention ledger for retry".into(),
                    source,
                })?;
            }
        }
        let process_id = std::process::id();
        let supervisor_handle = ProcessHandle {
            kind: ProcessKind::Supervisor,
            project_root: paths.root.to_path_buf(),
            pid: Some(process_id as i32),
        };
        let _supervisor_process_guard = match self
            .app
            .engine
            .process_register(&paths.root, supervisor_handle)
        {
            Ok(guard) => {
                tracing::info!("supervisor: registered process handle");
                Some(guard)
            }
            Err(err) => {
                tracing::warn!("supervisor: failed to register process handle: {}", err);
                None
            }
        };

        if let Some(pid) = coordinator_pid {
            macc_core::atomic_write(
                &paths,
                &paths.root.join(".macc/state/coordinator.pid"),
                format!("{pid}\n").as_bytes(),
            )?;
        }
        let runtime = tokio::runtime::Runtime::new()
            .map_err(|e| MaccError::Validation(format!("Supervisor runtime: {e}")))?;
        let result = runtime.block_on(async {
            #[cfg(unix)]
            {
                let mut term = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
                    .map_err(|e| MaccError::Validation(format!("Supervisor SIGTERM handler: {e}")))?;
                tokio::select! {
                    result = super::supervisor_runtime::run(paths.clone(), canonical.clone(), attach, &owner.instance) => result,
                    _ = term.recv() => Ok(()),
                    result = tokio::signal::ctrl_c() => result.map_err(|e| MaccError::Validation(e.to_string())),
                }
            }
            #[cfg(not(unix))]
            { tokio::select! {
                result = super::supervisor_runtime::run(paths.clone(), canonical.clone(), attach, &owner.instance) => result,
                result = tokio::signal::ctrl_c() => result.map_err(|e| MaccError::Validation(e.to_string())),
            } }
        });

        super::supervisor_runtime::record_shutdown(&paths)?;
        if let Err(error) = &result {
            super::supervisor_runtime::record_failure(
                &paths,
                &canonical.automation.supervisor.clone().unwrap_or_default(),
                &error.to_string(),
            )?;
        }

        if std::env::var(SUPERVISOR_DAEMON_CHILD_ENV).is_ok() {
            result
        } else {
            if result.is_ok() {
                println!("Supervisor stopped.");
            }
            result
        }
    }

    fn stop(&self) -> Result<()> {
        let paths = self.app.project_paths()?;

        if instance::stop(&paths.root, None)? {
            if let Some(_idle) = instance::lock_if_idle(&paths.root)? {
                if let Err(error) =
                    macc_core::git::prune_merged_supervisor_branches(&paths.root, None, false)
                {
                    eprintln!("Supervisor branches retained: {error}");
                }
            }
            println!("Supervisor stopped.");
        } else {
            println!("Supervisor is not running.");
        }
        Ok(())
    }

    fn prune(&self, force: bool) -> Result<()> {
        let paths = self.app.project_paths()?;
        crate::commands::gate_cli_mutation(&paths.root)?;
        let canonical = self.app.canonical_config()?;
        let reference_branch = canonical
            .automation
            .coordinator
            .as_ref()
            .and_then(|c| c.reference_branch.as_deref());
        let deleted =
            macc_core::git::prune_merged_supervisor_branches(&paths.root, reference_branch, force)?;
        if deleted.is_empty() {
            println!("No eligible merged supervisor branches to prune; retained, unmerged and checked-out branches are preserved.");
        } else {
            for b in &deleted {
                println!("Pruned supervisor branch: {b}");
            }
            println!("Pruned {} supervisor branch(es).", deleted.len());
        }
        Ok(())
    }

    fn status(&self) -> Result<()> {
        let paths = self.app.project_paths()?;
        let supervisor_health_path = paths.root.join(SUPERVISOR_HEALTH_REL_PATH);

        let pid = Instance::running(&paths.root)?.map(|owner| owner.pid);
        let running = pid.is_some();

        println!("Supervisor:");
        if running {
            println!("  status: running");
            if let Some(pid) = pid {
                println!("  pid: {}", pid);
            }
        } else {
            println!("  status: stopped");
        }

        if supervisor_health_path.exists() {
            let raw = fs::read_to_string(&supervisor_health_path).map_err(|e| MaccError::Io {
                path: supervisor_health_path.to_string_lossy().into(),
                action: "read supervisor health file".into(),
                source: e,
            })?;
            let health: Value = serde_json::from_str(&raw)
                .map_err(|e| MaccError::Validation(format!("Invalid supervisor health: {e}")))?;
            println!(
                "  health: {}",
                health
                    .pointer("/health/status")
                    .and_then(Value::as_str)
                    .unwrap_or("unknown")
            );
            println!(
                "  tool: {}",
                health
                    .get("tool")
                    .and_then(Value::as_str)
                    .unwrap_or("unconfigured")
            );
            println!(
                "  checked_at: {}",
                health
                    .get("checked_at")
                    .and_then(Value::as_str)
                    .unwrap_or("unknown")
            );
            println!(
                "  last_intervention: {}",
                health
                    .get("last_intervention")
                    .and_then(Value::as_str)
                    .unwrap_or("none")
            );
            if let Some(detail) = health.get("detail").and_then(Value::as_str) {
                println!("  detail: {detail}");
            }
        } else {
            println!(
                "  health: unavailable ({})",
                supervisor_health_path.display()
            );
        }

        let latest = paths.root.join(".macc/log/supervisor/latest.json");
        if let Some(value) = fs::read_to_string(&latest)
            .ok()
            .and_then(|raw| serde_json::from_str::<Value>(&raw).ok())
        {
            for line in intervention_summary(&value) {
                println!("  {line}");
            }
        }

        println!("Coordinator:");
        match self.app.engine.get_coordinator_status(&paths) {
            Ok(status) => {
                println!(
                    "  total={} todo={} active={} blocked={} merged={}",
                    status.total, status.todo, status.active, status.blocked, status.merged
                );
                println!("  paused={}", status.paused);
                if let Some(err) = status.latest_error {
                    println!("  latest_error={}", err);
                }
            }
            Err(err) => {
                println!("  health: unavailable ({})", err);
            }
        }

        Ok(())
    }

    fn report(&self) -> Result<()> {
        let paths = self.app.project_paths()?;
        let canonical = self.app.canonical_config()?;
        let supervisor_cfg = canonical.automation.supervisor.unwrap_or_default();
        let report_path = resolve_project_path(&paths.root, &supervisor_cfg.report_output_path);

        if !report_path.exists() {
            return Err(MaccError::Validation(format!(
                "supervisor report file not found: {}",
                report_path.display()
            )));
        }

        let raw = fs::read_to_string(&report_path).map_err(|e| MaccError::Io {
            path: report_path.to_string_lossy().into(),
            action: "read supervisor report file".into(),
            source: e,
        })?;
        let report: SupervisorReport = serde_json::from_str(&raw).map_err(|e| {
            MaccError::Validation(format!(
                "parse supervisor report {}: {}",
                report_path.display(),
                e
            ))
        })?;

        println!("Supervisor report: {}", report_path.display());
        println!("  timestamp: {}", report.timestamp);
        println!("  health: {:?}", report.health);
        println!("  findings: {}", report.findings.len());
        println!("  recommendations: {}", report.recommendations.len());
        println!("  actions_taken: {}", report.actions_taken.len());
        let value: Value =
            serde_json::from_str(&raw).map_err(|e| MaccError::Validation(e.to_string()))?;
        // Readable outcome first; the full record follows for tooling.
        for line in intervention_summary(&value) {
            println!("{line}");
        }
        if let Some(intervention) = value.get("intervention") {
            println!(
                "{}",
                serde_json::to_string_pretty(intervention)
                    .map_err(|e| MaccError::Validation(e.to_string()))?
            );
        }

        Ok(())
    }
}

/// Operator-facing lines for the latest intervention: why it stopped, what to
/// do next, and the MACC improvements the diagnosis recommended.
fn intervention_summary(report: &Value) -> Vec<String> {
    let Some(intervention) = report.get("intervention") else {
        return Vec::new();
    };
    let text = |key: &str| {
        intervention
            .get(key)
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty())
    };
    let mut lines = vec![format!(
        "intervention: {} (attempt {})",
        text("status").unwrap_or("unknown"),
        intervention
            .get("attempt")
            .and_then(Value::as_u64)
            .unwrap_or(0)
    )];
    if let Some(reason) = text("detail") {
        lines.push(format!("reason: {reason}"));
    }
    if let Some(next) = text("next_action") {
        lines.push(format!("next: {next}"));
    }
    if let Some(branch) = text("target_branch") {
        lines.push(format!("target branch: {branch}"));
    }
    let rounds = intervention
        .get("repair_rounds")
        .and_then(Value::as_u64)
        .unwrap_or(0);
    if rounds > 1 {
        lines.push(format!("repair rounds: {rounds}"));
    }
    let list = |pointer: &str| -> Vec<&str> {
        intervention
            .pointer(pointer)
            .and_then(Value::as_array)
            .map(|items| items.iter().filter_map(Value::as_str).collect())
            .unwrap_or_default()
    };
    let preexisting = list("/preexisting_failures");
    if !preexisting.is_empty() {
        lines.push(format!(
            "already failing on the unmodified base (not a gate): {}",
            preexisting.join(", ")
        ));
    }
    let findings = list("/diagnosis/macc_findings");
    if !findings.is_empty() {
        lines.push(format!("MACC findings ({}):", findings.len()));
        for (index, finding) in findings.iter().enumerate() {
            lines.push(format!("  {}. {finding}", index + 1));
        }
    }
    lines
}

fn resolve_project_path(root: &Path, path: &Path) -> PathBuf {
    if path.is_absolute() {
        return path.to_path_buf();
    }
    root.join(path)
}
