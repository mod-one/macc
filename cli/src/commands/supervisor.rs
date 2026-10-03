use crate::commands::AppContext;
use crate::commands::Command;
use crate::SupervisorCommands;
use macc_core::process_ownership::{ProcessHandle, ProcessKind};

use macc_core::supervisor::SupervisorReport;
use macc_core::{MaccError, Result};
use serde_json::Value;
use std::fs;
#[cfg(unix)]
use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Command as ProcessCommand, Stdio};
use std::time::{Duration, Instant};

const SUPERVISOR_PID_REL_PATH: &str = ".macc/state/supervisor.pid";
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
            SupervisorCommands::Start {
                daemon,
                attach,
                coordinator_pid,
                retry,
            } => self.start(*daemon, *attach, *coordinator_pid, *retry),
            SupervisorCommands::Stop => self.stop(),
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
        let supervisor_pid_path = paths.root.join(SUPERVISOR_PID_REL_PATH);

        if daemon {
            ensure_not_running(&supervisor_pid_path)?;
            if paths.root.join(SUPERVISOR_HEALTH_REL_PATH).exists() {
                fs::remove_file(paths.root.join(SUPERVISOR_HEALTH_REL_PATH)).map_err(|e| {
                    MaccError::Validation(format!("Reset supervisor readiness: {e}"))
                })?;
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
                if read_pid_file(&supervisor_pid_path)? == Some(child.id())
                    && paths.root.join(SUPERVISOR_HEALTH_REL_PATH).exists()
                {
                    break;
                }
                if !is_pid_running(child.id()) || Instant::now() >= deadline {
                    let _ = child.kill();
                    let _ = child.wait();
                    return Err(MaccError::Validation(format!("Supervisor did not become ready; inspect {}/.macc/log/supervisor/daemon.log", paths.root.display())));
                }
                std::thread::sleep(Duration::from_millis(50));
            }
            println!("Supervisor started in daemon mode (pid {}).", child.id());
            return Ok(());
        }

        ensure_not_running(&supervisor_pid_path)?;

        let _instance_lock = macc_core::fs_lock::AdvisoryLock::acquire(
            &paths.root.join(".macc/state/supervisor.lock"),
            Duration::ZERO,
            "supervisor",
        )?;
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
        let stop_request = paths.root.join(".macc/state/supervisor-stop.json");
        if stop_request.exists() {
            fs::remove_file(&stop_request).map_err(|source| MaccError::Io {
                path: stop_request.display().to_string(),
                action: "clear previous supervisor stop request".into(),
                source,
            })?;
        }
        write_pid_file(&supervisor_pid_path, process_id)?;

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
            write_pid_file(&paths.root.join(".macc/state/coordinator.pid"), pid)?;
        }
        let runtime = tokio::runtime::Runtime::new()
            .map_err(|e| MaccError::Validation(format!("Supervisor runtime: {e}")))?;
        let result = runtime.block_on(async {
            #[cfg(unix)]
            {
                let mut term = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
                    .map_err(|e| MaccError::Validation(format!("Supervisor SIGTERM handler: {e}")))?;
                tokio::select! {
                    result = super::supervisor_runtime::run(paths.clone(), canonical.clone(), attach) => result,
                    _ = term.recv() => Ok(()),
                    result = tokio::signal::ctrl_c() => result.map_err(|e| MaccError::Validation(e.to_string())),
                }
            }
            #[cfg(not(unix))]
            { tokio::select! {
                result = super::supervisor_runtime::run(paths.clone(), canonical.clone(), attach) => result,
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
        cleanup_pid_file_if_matches(&supervisor_pid_path, process_id)?;

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
        let supervisor_pid_path = paths.root.join(SUPERVISOR_PID_REL_PATH);

        let Some(pid) = read_pid_file(&supervisor_pid_path)? else {
            println!("Supervisor is not running.");
            return Ok(());
        };

        if !is_pid_running(pid) {
            let _ = fs::remove_file(&supervisor_pid_path);
            println!("Supervisor is not running (stale pid file removed).");
            return Ok(());
        }

        super::supervisor_runtime::request_stop(&paths, pid)?;
        send_signal(pid, "-TERM")?;

        let deadline = Instant::now() + Duration::from_secs(5);
        while Instant::now() < deadline {
            if !is_pid_running(pid) {
                break;
            }
            std::thread::sleep(Duration::from_millis(150));
        }

        if is_pid_running(pid) {
            send_signal(pid, "-KILL")?;
        }

        let _ = fs::remove_file(&supervisor_pid_path);
        println!("Supervisor stopped.");
        Ok(())
    }

    fn status(&self) -> Result<()> {
        let paths = self.app.project_paths()?;
        let supervisor_pid_path = paths.root.join(SUPERVISOR_PID_REL_PATH);
        let supervisor_health_path = paths.root.join(SUPERVISOR_HEALTH_REL_PATH);

        let pid = read_pid_file(&supervisor_pid_path)?;
        let running = pid.map(is_pid_running).unwrap_or(false);

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

fn resolve_project_path(root: &Path, path: &Path) -> PathBuf {
    if path.is_absolute() {
        return path.to_path_buf();
    }
    root.join(path)
}

fn read_pid_file(path: &Path) -> Result<Option<u32>> {
    if !path.exists() {
        return Ok(None);
    }

    let raw = fs::read_to_string(path).map_err(|e| MaccError::Io {
        path: path.to_string_lossy().into(),
        action: "read supervisor pid file".into(),
        source: e,
    })?;
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return Ok(None);
    }

    let pid = trimmed.parse::<u32>().map_err(|e| {
        MaccError::Validation(format!(
            "invalid supervisor pid in {}: {}",
            path.display(),
            e
        ))
    })?;
    Ok(Some(pid))
}

fn write_pid_file(path: &Path, pid: u32) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|e| MaccError::Io {
            path: parent.to_string_lossy().into(),
            action: "create supervisor state directory".into(),
            source: e,
        })?;
    }
    fs::write(path, format!("{}\n", pid)).map_err(|e| MaccError::Io {
        path: path.to_string_lossy().into(),
        action: "write supervisor pid file".into(),
        source: e,
    })
}

fn cleanup_pid_file_if_matches(path: &Path, pid: u32) -> Result<()> {
    if let Some(existing) = read_pid_file(path)? {
        if existing == pid {
            let _ = fs::remove_file(path);
        }
    }
    Ok(())
}

fn ensure_not_running(path: &Path) -> Result<()> {
    if let Some(pid) = read_pid_file(path)? {
        if is_pid_running(pid) {
            return Err(MaccError::Validation(format!(
                "supervisor is already running (pid {})",
                pid
            )));
        }
        let _ = fs::remove_file(path);
    }
    Ok(())
}

fn is_pid_running(pid: u32) -> bool {
    ProcessCommand::new("kill")
        .arg("-0")
        .arg(pid.to_string())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .map(|status| status.success())
        .unwrap_or(false)
}

fn send_signal(pid: u32, signal: &str) -> Result<()> {
    let status = ProcessCommand::new("kill")
        .arg(signal)
        .arg(pid.to_string())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .map_err(|e| MaccError::Io {
            path: "kill".into(),
            action: format!("send {} to supervisor pid {}", signal, pid),
            source: e,
        })?;

    if status.success() {
        return Ok(());
    }

    Err(MaccError::Validation(format!(
        "failed to send {} to supervisor pid {}",
        signal, pid
    )))
}
