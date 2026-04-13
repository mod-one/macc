use crate::supervisor::{HealthCheckResult, ProcessManager, ProcessManagerError};
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::fs;
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::time::Duration;

fn default_stall_threshold_seconds() -> u64 {
    90
}

fn default_watchdog_interval_seconds() -> u64 {
    30
}

fn default_events_log_path() -> PathBuf {
    PathBuf::from(".macc/log/coordinator/events.jsonl")
}

fn default_error_burst_threshold() -> usize {
    3
}

fn default_pid_file_path() -> PathBuf {
    PathBuf::from(".macc/state/coordinator.pid")
}

fn default_health_status_path() -> PathBuf {
    PathBuf::from(".macc/state/supervisor-health.json")
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct WatchdogConfig {
    #[serde(default = "default_watchdog_interval_seconds")]
    pub watchdog_interval_seconds: u64,
    #[serde(default = "default_stall_threshold_seconds")]
    pub stall_threshold_seconds: u64,
    #[serde(default = "default_error_burst_threshold")]
    pub error_burst_threshold: usize,
    #[serde(default = "default_events_log_path")]
    pub events_log_path: PathBuf,
    #[serde(default = "default_pid_file_path")]
    pub pid_file_path: PathBuf,
    #[serde(default = "default_health_status_path")]
    pub health_status_path: PathBuf,
}

impl Default for WatchdogConfig {
    fn default() -> Self {
        Self {
            watchdog_interval_seconds: default_watchdog_interval_seconds(),
            stall_threshold_seconds: default_stall_threshold_seconds(),
            error_burst_threshold: default_error_burst_threshold(),
            events_log_path: default_events_log_path(),
            pid_file_path: default_pid_file_path(),
            health_status_path: default_health_status_path(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct SupervisorHealthStatus {
    pub checked_at: String,
    pub health: HealthCheckResult,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub coordinator_pid: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_event_ts: Option<String>,
    pub error_events_in_window: usize,
    pub watchdog_interval_seconds: u64,
    pub stall_threshold_seconds: u64,
}

#[derive(Debug, thiserror::Error)]
pub enum WatchdogError {
    #[error("process manager error: {0}")]
    ProcessManager(#[from] ProcessManagerError),
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
    #[error("serde error: {0}")]
    Serde(#[from] serde_json::Error),
}

#[derive(Debug, Clone)]
pub struct CoordinatorProcessManager {
    pid_file_path: PathBuf,
    start_command: Option<Vec<String>>,
}

impl CoordinatorProcessManager {
    pub fn new(pid_file_path: PathBuf) -> Self {
        Self {
            pid_file_path,
            start_command: None,
        }
    }

    pub fn with_start_command(mut self, command: Vec<String>) -> Self {
        self.start_command = Some(command);
        self
    }

    pub fn pid_file_path(&self) -> &Path {
        &self.pid_file_path
    }

    fn read_pid_from_file(&self) -> Result<Option<u32>, ProcessManagerError> {
        if !self.pid_file_path.exists() {
            return Ok(None);
        }
        let raw = fs::read_to_string(&self.pid_file_path)?;
        let trimmed = raw.trim();
        if trimmed.is_empty() {
            return Ok(None);
        }
        let pid = trimmed.parse::<u32>().map_err(|err| {
            ProcessManagerError::SpawnFailed(format!(
                "invalid pid in {}: {}",
                self.pid_file_path.display(),
                err
            ))
        })?;
        Ok(Some(pid))
    }

    fn write_pid_to_file(&self, pid: u32) -> Result<(), ProcessManagerError> {
        if let Some(parent) = self.pid_file_path.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::write(&self.pid_file_path, format!("{}\n", pid))?;
        Ok(())
    }

    fn is_pid_running(pid: u32) -> bool {
        #[cfg(unix)]
        {
            let status = std::process::Command::new("kill")
                .arg("-0")
                .arg(pid.to_string())
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .status();
            return matches!(status, Ok(s) if s.success());
        }
        #[cfg(not(unix))]
        {
            let _ = pid;
            false
        }
    }

    fn send_signal(pid: u32, signal: &str) -> Result<(), ProcessManagerError> {
        #[cfg(unix)]
        {
            let status = std::process::Command::new("kill")
                .arg(signal)
                .arg(pid.to_string())
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .status()
                .map_err(|err| ProcessManagerError::SignalFailed(err.to_string()))?;
            if status.success() {
                return Ok(());
            }
            Err(ProcessManagerError::SignalFailed(format!(
                "kill {} {} returned status {}",
                signal, pid, status
            )))
        }
        #[cfg(not(unix))]
        {
            let _ = (pid, signal);
            Err(ProcessManagerError::SignalFailed(
                "signals not supported on this platform".to_string(),
            ))
        }
    }
}

#[async_trait]
impl ProcessManager for CoordinatorProcessManager {
    async fn start_coordinator(&self) -> Result<(), ProcessManagerError> {
        if let Some(pid) = self.read_pid_from_file()? {
            if Self::is_pid_running(pid) {
                return Err(ProcessManagerError::AlreadyRunning { pid });
            }
        }

        let command = self.start_command.as_ref().ok_or_else(|| {
            ProcessManagerError::SpawnFailed(
                "start command is not configured for CoordinatorProcessManager".to_string(),
            )
        })?;

        let Some(program) = command.first() else {
            return Err(ProcessManagerError::SpawnFailed(
                "start command must include program name".to_string(),
            ));
        };

        let mut cmd = tokio::process::Command::new(program);
        if command.len() > 1 {
            cmd.args(&command[1..]);
        }
        cmd.stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null());
        let child = cmd
            .spawn()
            .map_err(|err| ProcessManagerError::SpawnFailed(err.to_string()))?;

        let pid = child.id().ok_or_else(|| {
            ProcessManagerError::SpawnFailed("spawned process has no pid".to_string())
        })?;
        self.write_pid_to_file(pid)?;
        Ok(())
    }

    async fn stop_coordinator(&self) -> Result<(), ProcessManagerError> {
        let Some(pid) = self.read_pid_from_file()? else {
            return Err(ProcessManagerError::NotRunning);
        };

        if !Self::is_pid_running(pid) {
            return Err(ProcessManagerError::NotRunning);
        }

        Self::send_signal(pid, "-TERM")
    }

    async fn health_check(&self) -> Result<HealthCheckResult, ProcessManagerError> {
        let Some(pid) = self.read_pid_from_file()? else {
            return Ok(HealthCheckResult::Crashed { exit_code: None });
        };

        if Self::is_pid_running(pid) {
            Ok(HealthCheckResult::Healthy)
        } else {
            Ok(HealthCheckResult::Crashed { exit_code: None })
        }
    }

    async fn coordinator_pid(&self) -> Option<u32> {
        self.read_pid_from_file().ok().flatten()
    }
}

#[derive(Debug, Clone, PartialEq)]
struct EventSnapshot {
    ts: Option<DateTime<Utc>>,
    is_error: bool,
}

#[derive(Debug, Clone)]
pub struct EventStreamTailer {
    log_path: PathBuf,
    cursor: u64,
}

impl EventStreamTailer {
    pub fn new(log_path: PathBuf) -> Self {
        Self {
            log_path,
            cursor: 0,
        }
    }

    fn parse_event_line(line: &str) -> Option<EventSnapshot> {
        let parsed: Value = serde_json::from_str(line).ok()?;
        let ts = parsed
            .get("ts")
            .and_then(Value::as_str)
            .or_else(|| parsed.get("timestamp").and_then(Value::as_str))
            .and_then(|v| DateTime::parse_from_rfc3339(v).ok())
            .map(|dt| dt.with_timezone(&Utc));

        let event_type = parsed.get("type").and_then(Value::as_str).unwrap_or("");
        let status = parsed.get("status").and_then(Value::as_str).unwrap_or("");
        let severity = parsed
            .get("severity")
            .and_then(Value::as_str)
            .or_else(|| parsed.get("level").and_then(Value::as_str))
            .unwrap_or("");

        let is_error = event_type.eq_ignore_ascii_case("failed")
            || status.eq_ignore_ascii_case("failed")
            || severity.eq_ignore_ascii_case("error")
            || severity.eq_ignore_ascii_case("critical");

        Some(EventSnapshot { ts, is_error })
    }

    fn tail_new_events(&mut self) -> Result<Vec<EventSnapshot>, std::io::Error> {
        if !self.log_path.exists() {
            return Ok(Vec::new());
        }

        let mut file = fs::File::open(&self.log_path)?;
        let len = file.metadata()?.len();
        if self.cursor > len {
            self.cursor = 0;
        }

        file.seek(SeekFrom::Start(self.cursor))?;
        let mut buf = String::new();
        file.read_to_string(&mut buf)?;
        self.cursor = len;

        let mut out = Vec::new();
        for line in buf.lines().map(str::trim).filter(|line| !line.is_empty()) {
            if let Some(event) = Self::parse_event_line(line) {
                out.push(event);
            } else {
                tracing::warn!("supervisor mode_a: skipping invalid event line");
            }
        }
        Ok(out)
    }
}

#[derive(Debug, Clone, Default)]
struct WatchdogState {
    last_event_at: Option<DateTime<Utc>>,
    consecutive_error_events: usize,
}

pub struct SupervisorWatchdog<P> {
    config: WatchdogConfig,
    process_manager: P,
    tailer: EventStreamTailer,
    state: WatchdogState,
}

impl<P> SupervisorWatchdog<P>
where
    P: ProcessManager,
{
    pub fn new(config: WatchdogConfig, process_manager: P) -> Self {
        let tailer = EventStreamTailer::new(config.events_log_path.clone());
        Self {
            config,
            process_manager,
            tailer,
            state: WatchdogState::default(),
        }
    }

    pub async fn check_once(&mut self) -> Result<SupervisorHealthStatus, WatchdogError> {
        let now = Utc::now();
        self.check_once_at(now).await
    }

    pub async fn check_once_at(
        &mut self,
        now: DateTime<Utc>,
    ) -> Result<SupervisorHealthStatus, WatchdogError> {
        let process_health = self.process_manager.health_check().await?;
        let coordinator_pid = self.process_manager.coordinator_pid().await;

        let events = self.tailer.tail_new_events()?;
        for event in events {
            if let Some(ts) = event.ts {
                if self.state.last_event_at.map(|last| ts > last).unwrap_or(true) {
                    self.state.last_event_at = Some(ts);
                }
            }

            if event.is_error {
                self.state.consecutive_error_events += 1;
            } else {
                self.state.consecutive_error_events = 0;
            }
        }

        let health = evaluate_health(
            process_health,
            now,
            self.state.last_event_at,
            self.state.consecutive_error_events,
            self.config.stall_threshold_seconds,
            self.config.error_burst_threshold,
        );

        if !matches!(health, HealthCheckResult::Healthy) {
            tracing::warn!(
                "supervisor mode_a detected unhealthy coordinator: {:?}",
                health
            );
        }

        let status = SupervisorHealthStatus {
            checked_at: now.to_rfc3339(),
            health,
            coordinator_pid,
            last_event_ts: self.state.last_event_at.map(|v| v.to_rfc3339()),
            error_events_in_window: self.state.consecutive_error_events,
            watchdog_interval_seconds: self.config.watchdog_interval_seconds,
            stall_threshold_seconds: self.config.stall_threshold_seconds,
        };

        write_health_status(&self.config.health_status_path, &status)?;
        Ok(status)
    }

    pub async fn run_forever(&mut self) -> Result<(), WatchdogError> {
        let interval = Duration::from_secs(self.config.watchdog_interval_seconds.max(1));
        loop {
            if let Err(err) = self.check_once().await {
                tracing::warn!("supervisor mode_a cycle failed: {}", err);
            }
            tokio::time::sleep(interval).await;
        }
    }
}

fn evaluate_health(
    process_health: HealthCheckResult,
    now: DateTime<Utc>,
    last_event_at: Option<DateTime<Utc>>,
    consecutive_error_events: usize,
    stall_threshold_seconds: u64,
    error_burst_threshold: usize,
) -> HealthCheckResult {
    if matches!(
        process_health,
        HealthCheckResult::Crashed { .. } | HealthCheckResult::Unresponsive
    ) {
        return process_health;
    }

    let mut reasons = Vec::new();

    match last_event_at {
        Some(last_ts) => {
            let age = now.signed_duration_since(last_ts).num_seconds();
            if age >= stall_threshold_seconds as i64 {
                reasons.push(format!(
                    "no events emitted for {}s (threshold {}s)",
                    age, stall_threshold_seconds
                ));
            }
        }
        None => {
            reasons.push("no parseable coordinator events observed".to_string());
        }
    }

    if consecutive_error_events >= error_burst_threshold {
        reasons.push(format!(
            "repeated error events detected ({} consecutive, threshold {})",
            consecutive_error_events, error_burst_threshold
        ));
    }

    if reasons.is_empty() {
        process_health
    } else {
        match process_health {
            HealthCheckResult::Degraded {
                reasons: mut existing,
            } => {
                existing.extend(reasons);
                HealthCheckResult::Degraded { reasons: existing }
            }
            HealthCheckResult::Healthy => HealthCheckResult::Degraded { reasons },
            other => other,
        }
    }
}

fn write_health_status(path: &Path, status: &SupervisorHealthStatus) -> Result<(), WatchdogError> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }

    let tmp = path.with_extension("tmp");
    let bytes = serde_json::to_vec_pretty(status)?;
    fs::write(&tmp, bytes)?;
    fs::rename(&tmp, path)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    struct MockProcessManager {
        health: HealthCheckResult,
        pid: Option<u32>,
    }

    #[async_trait]
    impl ProcessManager for MockProcessManager {
        async fn start_coordinator(&self) -> Result<(), ProcessManagerError> {
            Ok(())
        }

        async fn stop_coordinator(&self) -> Result<(), ProcessManagerError> {
            Ok(())
        }

        async fn health_check(&self) -> Result<HealthCheckResult, ProcessManagerError> {
            Ok(self.health.clone())
        }

        async fn coordinator_pid(&self) -> Option<u32> {
            self.pid
        }
    }

    fn temp_dir(prefix: &str) -> PathBuf {
        let nanos = Utc::now().timestamp_nanos_opt().unwrap_or_default();
        let path = std::env::temp_dir().join(format!("macc-supervisor-{}-{}", prefix, nanos));
        fs::create_dir_all(&path).expect("create temp dir");
        path
    }

    fn write_event(path: &Path, ts: &str, event_type: &str, status: &str) {
        let line = serde_json::json!({
            "ts": ts,
            "type": event_type,
            "status": status
        });
        fs::write(path, format!("{}\n", line)).expect("write event");
    }

    #[tokio::test]
    async fn healthy_coordinator_is_reported_healthy() {
        let root = temp_dir("healthy");
        let events_path = root.join("events.jsonl");
        let health_path = root.join("supervisor-health.json");
        write_event(&events_path, "2026-04-13T00:00:10Z", "heartbeat", "ok");

        let config = WatchdogConfig {
            watchdog_interval_seconds: 30,
            stall_threshold_seconds: 120,
            error_burst_threshold: 3,
            events_log_path: events_path,
            pid_file_path: root.join("coordinator.pid"),
            health_status_path: health_path.clone(),
        };

        let mut watchdog = SupervisorWatchdog::new(
            config,
            MockProcessManager {
                health: HealthCheckResult::Healthy,
                pid: Some(4242),
            },
        );

        let now = DateTime::parse_from_rfc3339("2026-04-13T00:01:00Z")
            .expect("parse now")
            .with_timezone(&Utc);
        let status = watchdog.check_once_at(now).await.expect("watchdog check");

        assert_eq!(status.health, HealthCheckResult::Healthy);
        assert_eq!(status.coordinator_pid, Some(4242));
        assert!(health_path.exists());
    }

    #[tokio::test]
    async fn stalled_coordinator_is_reported_degraded() {
        let root = temp_dir("stalled");
        let events_path = root.join("events.jsonl");
        write_event(&events_path, "2026-04-13T00:00:10Z", "heartbeat", "ok");

        let config = WatchdogConfig {
            watchdog_interval_seconds: 30,
            stall_threshold_seconds: 30,
            error_burst_threshold: 3,
            events_log_path: events_path,
            pid_file_path: root.join("coordinator.pid"),
            health_status_path: root.join("supervisor-health.json"),
        };

        let mut watchdog = SupervisorWatchdog::new(
            config,
            MockProcessManager {
                health: HealthCheckResult::Healthy,
                pid: Some(7001),
            },
        );

        let now = DateTime::parse_from_rfc3339("2026-04-13T00:01:00Z")
            .expect("parse now")
            .with_timezone(&Utc);
        let status = watchdog.check_once_at(now).await.expect("watchdog check");

        match status.health {
            HealthCheckResult::Degraded { reasons } => {
                assert!(reasons.iter().any(|r| r.contains("no events emitted")));
            }
            other => panic!("expected degraded, got {:?}", other),
        }
    }

    #[tokio::test]
    async fn crashed_coordinator_is_reported_crashed() {
        let root = temp_dir("crashed");

        let config = WatchdogConfig {
            watchdog_interval_seconds: 30,
            stall_threshold_seconds: 30,
            error_burst_threshold: 3,
            events_log_path: root.join("events.jsonl"),
            pid_file_path: root.join("coordinator.pid"),
            health_status_path: root.join("supervisor-health.json"),
        };

        let mut watchdog = SupervisorWatchdog::new(
            config,
            MockProcessManager {
                health: HealthCheckResult::Crashed { exit_code: Some(1) },
                pid: None,
            },
        );

        let now = DateTime::parse_from_rfc3339("2026-04-13T00:01:00Z")
            .expect("parse now")
            .with_timezone(&Utc);
        let status = watchdog.check_once_at(now).await.expect("watchdog check");

        assert_eq!(
            status.health,
            HealthCheckResult::Crashed { exit_code: Some(1) }
        );
    }
}
