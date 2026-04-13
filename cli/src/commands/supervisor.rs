use crate::commands::AppContext;
use crate::commands::Command;
use crate::SupervisorCommands;
use macc_core::supervisor::mode_a::{
    CoordinatorProcessManager, SupervisorHealthStatus, SupervisorWatchdog, WatchdogConfig,
    WatchdogError,
};
use macc_core::supervisor::SupervisorReport;
use macc_core::{MaccError, Result};
use std::fs;
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
            SupervisorCommands::Start { daemon } => self.start(*daemon),
            SupervisorCommands::Stop => self.stop(),
            SupervisorCommands::Status => self.status(),
            SupervisorCommands::Report => self.report(),
        }
    }
}

impl<'a> SupervisorCommand<'a> {
    fn start(&self, daemon: bool) -> Result<()> {
        let paths = self.app.ensure_initialized_paths()?;
        let canonical = self.app.canonical_config()?;
        let supervisor_cfg = canonical.automation.supervisor.unwrap_or_default();
        let supervisor_pid_path = paths.root.join(SUPERVISOR_PID_REL_PATH);

        if daemon {
            ensure_not_running(&supervisor_pid_path)?;
            let current_exe = std::env::current_exe().map_err(|e| MaccError::Io {
                path: paths.root.to_string_lossy().into(),
                action: "resolve current executable for supervisor daemon".into(),
                source: e,
            })?;

            let child = ProcessCommand::new(current_exe)
                .current_dir(&paths.root)
                .arg("--cwd")
                .arg(&paths.root)
                .arg("supervisor")
                .arg("start")
                .env(SUPERVISOR_DAEMON_CHILD_ENV, "1")
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .spawn()
                .map_err(|e| MaccError::Io {
                    path: paths.root.to_string_lossy().into(),
                    action: "spawn supervisor daemon".into(),
                    source: e,
                })?;

            println!("Supervisor started in daemon mode (pid {}).", child.id());
            return Ok(());
        }

        ensure_not_running(&supervisor_pid_path)?;

        let process_id = std::process::id();
        write_pid_file(&supervisor_pid_path, process_id)?;

        let mut watchdog_cfg = WatchdogConfig {
            watchdog_interval_seconds: supervisor_cfg.watchdog_interval_seconds.max(1),
            stall_threshold_seconds: supervisor_cfg.log_analysis_window_seconds.max(1),
            events_log_path: resolve_project_path(&paths.root, &supervisor_cfg.events_log_path),
            ..WatchdogConfig::default()
        };

        watchdog_cfg.health_status_path = paths.root.join(SUPERVISOR_HEALTH_REL_PATH);

        let coordinator_pid_path = resolve_project_path(&paths.root, &watchdog_cfg.pid_file_path);
        watchdog_cfg.pid_file_path = coordinator_pid_path;

        let coordinator_start_cmd = vec![
            std::env::current_exe()
                .map_err(|e| MaccError::Io {
                    path: paths.root.to_string_lossy().into(),
                    action: "resolve current executable for supervisor".into(),
                    source: e,
                })?
                .to_string_lossy()
                .into_owned(),
            "--cwd".to_string(),
            paths.root.to_string_lossy().into_owned(),
            "coordinator".to_string(),
            "run".to_string(),
            "--no-tui".to_string(),
        ];

        let process_manager = CoordinatorProcessManager::new(watchdog_cfg.pid_file_path.clone())
            .with_start_command(coordinator_start_cmd);
        let mut watchdog = SupervisorWatchdog::new(watchdog_cfg.clone(), process_manager);

        println!(
            "Supervisor started (watchdog={}s, health={}).",
            watchdog_cfg.watchdog_interval_seconds,
            watchdog_cfg.health_status_path.display()
        );

        let runtime = tokio::runtime::Runtime::new().map_err(|e| {
            MaccError::Validation(format!("build runtime for supervisor watchdog: {}", e))
        })?;

        let result = runtime.block_on(async {
            tokio::select! {
                res = watchdog.run_forever() => map_watchdog_error(res),
                sig = tokio::signal::ctrl_c() => {
                    sig.map_err(|e| MaccError::Io {
                        path: "signal".into(),
                        action: "wait for ctrl-c".into(),
                        source: e,
                    })?;
                    Ok(())
                }
            }
        });

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
            let health: SupervisorHealthStatus = serde_json::from_str(&raw).map_err(|e| {
                MaccError::Validation(format!(
                    "parse supervisor health file {}: {}",
                    supervisor_health_path.display(),
                    e
                ))
            })?;
            println!("  health: {:?}", health.health);
            println!("  checked_at: {}", health.checked_at);
            if let Some(last_event) = health.last_event_ts {
                println!("  last_event_ts: {}", last_event);
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

fn map_watchdog_error(result: std::result::Result<(), WatchdogError>) -> Result<()> {
    result.map_err(|err| MaccError::Validation(format!("supervisor watchdog failed: {}", err)))
}
