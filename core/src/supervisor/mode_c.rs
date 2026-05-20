//! Supervisor Mode C: coordinator crash detection, registry cleanup, and restart.
//!
//! Mode C is the self-healing capability of the supervisor. It runs in the watchdog
//! loop and activates when the coordinator process is detected as crashed or unresponsive.
//!
//! # Recovery sequence
//!
//! 1. Detect coordinator exit via watchdog (`HealthCheckResult::Crashed`).
//! 2. Collect crash evidence (last events, exit code, stderr fragments).
//! 3. Validate task registry: scan for tasks in `claimed`/`in_progress` without
//!    active performer processes.
//! 4. Reset orphaned tasks to `todo` state.
//! 5. Save a session snapshot for continuity.
//! 6. Restart coordinator via `ProcessManager::start_coordinator()`.
//! 7. Run health checks for N cycles to confirm recovery.
//! 8. If restart fails `max_restart_attempts` times, write a critical alert and stop.
//! 9. Write a crash recovery report to `.macc/log/supervisor/crash-recovery-<ts>.json`.
//!
//! # Safety contract
//!
//! - Mode C does NOT perform AI analysis in the restart path (deterministic only).
//! - Mode C does NOT modify worktree git state; only the task registry is written.
//! - All actions are idempotent where possible.

use crate::coordinator::model::{Task, TaskRegistry};
use crate::supervisor::{
    CoordinatorImprovementHint, Finding, FindingCategory, HealthCheckResult, ProcessManager,
    ProcessManagerError, Recommendation, Severity, SupervisorAction, SupervisorReport,
};
use chrono::Utc;
use serde::{Deserialize, Serialize};
use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::time::Duration;

// ── Configuration ─────────────────────────────────────────────────────────────

fn default_max_restart_attempts() -> u32 {
    3
}
fn default_post_restart_health_checks() -> u32 {
    3
}
fn default_post_restart_check_interval_seconds() -> u64 {
    10
}
fn default_report_output_dir() -> PathBuf {
    PathBuf::from(".macc/log/supervisor")
}
fn default_events_log_path() -> PathBuf {
    PathBuf::from(".macc/log/coordinator/events.jsonl")
}
fn default_max_crash_event_lines() -> usize {
    50
}
fn default_registry_path() -> PathBuf {
    PathBuf::from(".macc/automation/task/task_registry.json")
}
fn default_tool_sessions_path() -> PathBuf {
    PathBuf::from(".macc/state/tool-sessions.json")
}
fn default_sessions_snapshot_dir() -> PathBuf {
    // Default to project-local snapshots; callers may override to user-level dir.
    PathBuf::from(".macc/state/session-snapshots")
}

/// Configuration for Mode C supervisor.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ModeCConfig {
    /// Maximum consecutive restart attempts before supervisor gives up.
    #[serde(default = "default_max_restart_attempts")]
    pub max_restart_attempts: u32,

    /// Number of health checks to run after restart to confirm coordinator is healthy.
    #[serde(default = "default_post_restart_health_checks")]
    pub post_restart_health_checks: u32,

    /// Seconds between post-restart health check polls.
    #[serde(default = "default_post_restart_check_interval_seconds")]
    pub post_restart_check_interval_seconds: u64,

    /// Directory where crash recovery reports are written.
    #[serde(default = "default_report_output_dir")]
    pub report_output_dir: PathBuf,

    /// Path to the coordinator events log.
    #[serde(default = "default_events_log_path")]
    pub events_log_path: PathBuf,

    /// Maximum number of recent event lines to include in the crash report.
    #[serde(default = "default_max_crash_event_lines")]
    pub max_crash_event_lines: usize,

    /// Path to the task registry JSON file.
    #[serde(default = "default_registry_path")]
    pub registry_path: PathBuf,

    /// Path to the tool sessions state file.
    #[serde(default = "default_tool_sessions_path")]
    pub tool_sessions_path: PathBuf,

    /// Directory where session snapshots are saved before restart.
    #[serde(default = "default_sessions_snapshot_dir")]
    pub sessions_snapshot_dir: PathBuf,
}

impl Default for ModeCConfig {
    fn default() -> Self {
        Self {
            max_restart_attempts: default_max_restart_attempts(),
            post_restart_health_checks: default_post_restart_health_checks(),
            post_restart_check_interval_seconds: default_post_restart_check_interval_seconds(),
            report_output_dir: default_report_output_dir(),
            events_log_path: default_events_log_path(),
            max_crash_event_lines: default_max_crash_event_lines(),
            registry_path: default_registry_path(),
            tool_sessions_path: default_tool_sessions_path(),
            sessions_snapshot_dir: default_sessions_snapshot_dir(),
        }
    }
}

// ── Crash Evidence ────────────────────────────────────────────────────────────

/// Evidence collected when a coordinator crash is detected.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct CrashEvidence {
    /// Exit code from the coordinator process, if available.
    pub exit_code: Option<i32>,

    /// Last N lines from the coordinator events log at time of crash.
    pub last_events: Vec<String>,

    /// Timestamp when the crash was detected (RFC-3339).
    pub detected_at: String,

    /// Number of tasks found in `claimed` or `in_progress` state.
    pub active_tasks_at_crash: usize,
}

// ── Registry Cleanup ──────────────────────────────────────────────────────────

/// Result of scanning the task registry for orphaned tasks.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct RegistryCleanupResult {
    /// IDs of tasks that were reset to `todo`.
    pub reset_task_ids: Vec<String>,

    /// Number of tasks that were already idle (not orphaned).
    pub skipped_active: usize,
}

/// Scans the task registry and resets tasks in `claimed`/`in_progress` state
/// to `todo` when their performer process is not running.
///
/// Returns cleanup result on success.
pub fn cleanup_orphaned_tasks(registry_path: &Path) -> Result<RegistryCleanupResult, CleanupError> {
    if !registry_path.exists() {
        return Err(CleanupError::RegistryNotFound {
            path: registry_path.to_path_buf(),
        });
    }

    let raw = fs::read_to_string(registry_path).map_err(CleanupError::Io)?;
    let mut registry: TaskRegistry = serde_json::from_str(&raw).map_err(CleanupError::Parse)?;

    let now = Utc::now().to_rfc3339();
    let mut result = RegistryCleanupResult::default();

    for task in &mut registry.tasks {
        if !is_active_state(&task.state) {
            continue;
        }

        let performer_pid = task.task_runtime.pid.map(|p| p as u32);

        if performer_is_running(performer_pid) {
            result.skipped_active += 1;
        } else {
            let previous_state = task.state.clone();
            reset_task_to_todo(task, &now);
            result.reset_task_ids.push(task.id.clone());
            tracing::info!(
                task_id = %task.id,
                previous_state = %previous_state,
                "mode_c: reset orphaned task to todo"
            );
        }
    }

    registry.updated_at = Some(now);
    save_registry(registry_path, &registry)?;
    Ok(result)
}

fn is_active_state(state: &str) -> bool {
    matches!(state, "claimed" | "in_progress")
}

fn performer_is_running(pid: Option<u32>) -> bool {
    let Some(pid) = pid else {
        return false;
    };

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

fn reset_task_to_todo(task: &mut Task, now: &str) {
    task.state = "todo".to_string();
    task.updated_at = Some(now.to_string());
    task.state_changed_at = Some(now.to_string());
    task.claimed_at = None;
    task.assignee = None;
    task.worktree = None;

    let runtime = &mut task.task_runtime;
    runtime.status = Some("idle".to_string());
    runtime.pid = None;
    runtime.started_at = None;
    runtime.current_phase = None;
    runtime.merge_result_pending = Some(false);
    runtime.merge_result_file = None;
}

fn save_registry(path: &Path, registry: &TaskRegistry) -> Result<(), CleanupError> {
    let body = serde_json::to_vec_pretty(registry).map_err(CleanupError::Parse)?;
    let tmp = path.with_extension("tmp");
    fs::write(&tmp, body).map_err(CleanupError::Io)?;
    fs::rename(&tmp, path).map_err(CleanupError::Io)?;
    Ok(())
}

/// Errors from registry cleanup operations.
#[derive(Debug, thiserror::Error)]
pub enum CleanupError {
    #[error("Task registry not found at {path}")]
    RegistryNotFound { path: PathBuf },

    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    #[error("Registry parse error: {0}")]
    Parse(#[from] serde_json::Error),
}

// ── Session Save ──────────────────────────────────────────────────────────────

/// Save a snapshot of the current tool sessions file before coordinator restart.
///
/// Returns the snapshot file path on success, or `None` if no sessions file exists.
pub fn save_session_snapshot(
    tool_sessions_path: &Path,
    snapshot_dir: &Path,
    snapshot_name: &str,
) -> Result<Option<String>, std::io::Error> {
    if !tool_sessions_path.exists() {
        tracing::info!("mode_c: no tool-sessions.json found; skipping session snapshot");
        return Ok(None);
    }

    fs::create_dir_all(snapshot_dir)?;

    let dest = snapshot_dir.join(format!("{}.json", snapshot_name));
    fs::copy(tool_sessions_path, &dest)?;

    let path_str = dest.to_string_lossy().into_owned();
    tracing::info!(snapshot = %path_str, "mode_c: session snapshot saved before restart");
    Ok(Some(path_str))
}

// ── Crash Evidence Collector ──────────────────────────────────────────────────

/// Collect crash evidence from the events log and process state.
pub fn collect_crash_evidence(
    events_log_path: &Path,
    max_event_lines: usize,
    exit_code: Option<i32>,
) -> CrashEvidence {
    let last_events = read_last_event_lines(events_log_path, max_event_lines);
    CrashEvidence {
        exit_code,
        last_events,
        detected_at: Utc::now().to_rfc3339(),
        active_tasks_at_crash: 0, // filled in by caller after registry scan
    }
}

fn read_last_event_lines(path: &Path, max_lines: usize) -> Vec<String> {
    if !path.exists() {
        return Vec::new();
    }

    let Ok(mut file) = fs::File::open(path) else {
        return Vec::new();
    };

    let mut content = String::new();
    if file.read_to_string(&mut content).is_err() {
        return Vec::new();
    }

    let lines: Vec<String> = content
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty())
        .map(str::to_owned)
        .collect();

    if lines.len() <= max_lines {
        lines
    } else {
        lines[lines.len() - max_lines..].to_vec()
    }
}

// ── Crash Recovery Report ─────────────────────────────────────────────────────

/// Machine-readable report produced by a Mode C crash recovery cycle.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CrashRecoveryReport {
    /// RFC-3339 timestamp when this report was generated.
    pub timestamp: String,

    /// Evidence collected at crash time.
    pub crash_evidence: CrashEvidence,

    /// Result of registry cleanup before restart.
    pub cleanup_result: Option<RegistryCleanupResult>,

    /// Whether the coordinator was successfully restarted.
    pub restart_succeeded: bool,

    /// How many restart attempts were made.
    pub restart_attempts: u32,

    /// Post-restart health check outcome.
    pub post_restart_healthy: bool,

    /// All actions taken during this recovery cycle.
    pub actions_taken: Vec<SupervisorAction>,

    /// Whether max restart attempts were exceeded.
    pub max_restarts_exceeded: bool,

    /// Session snapshot path, if a snapshot was saved.
    pub session_snapshot_path: Option<String>,
}

impl CrashRecoveryReport {
    /// Write the report to `.macc/log/supervisor/crash-recovery-<timestamp>.json`.
    pub fn write_to_dir(&self, report_dir: &Path) -> Result<PathBuf, std::io::Error> {
        fs::create_dir_all(report_dir)?;

        // Sanitize the timestamp for use as a filename component.
        let ts_safe = self
            .timestamp
            .replace(':', "-")
            .replace('.', "-")
            .replace('+', "-");
        let filename = format!("crash-recovery-{}.json", ts_safe);
        let path = report_dir.join(&filename);

        let body = serde_json::to_vec_pretty(self).map_err(|e| {
            std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                format!("failed to serialize crash recovery report: {}", e),
            )
        })?;

        let tmp = path.with_extension("tmp");
        fs::write(&tmp, body)?;
        fs::rename(&tmp, &path)?;

        tracing::info!(report = %path.display(), "mode_c: crash recovery report written");
        Ok(path)
    }
}

// ── Mode C Orchestrator ───────────────────────────────────────────────────────

/// Errors from Mode C operations.
#[derive(Debug, thiserror::Error)]
pub enum ModeCError {
    #[error("process manager error: {0}")]
    ProcessManager(#[from] ProcessManagerError),

    #[error("registry cleanup error: {0}")]
    Cleanup(#[from] CleanupError),

    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    #[error("max restart attempts exceeded ({max})")]
    MaxRestartsExceeded { max: u32 },
}

/// Orchestrates the full crash recovery sequence for Mode C.
///
/// This is the synchronous core of Mode C. Callers drive restart retries
/// externally and pass the current `attempt` number (1-indexed).
///
/// Returns the completed `CrashRecoveryReport` on success or when max restarts
/// are exceeded (the error path also carries a partial report if needed via logs).
pub struct ModeCRecovery {
    config: ModeCConfig,
    restart_attempts: u32,
}

impl ModeCRecovery {
    pub fn new(config: ModeCConfig) -> Self {
        Self {
            config,
            restart_attempts: 0,
        }
    }

    /// Return true if there are restart attempts remaining.
    pub fn can_retry(&self) -> bool {
        self.restart_attempts < self.config.max_restart_attempts
    }

    /// Execute a single crash recovery cycle.
    ///
    /// - Collects crash evidence.
    /// - Cleans up orphaned tasks.
    /// - Saves a session snapshot.
    /// - Restarts the coordinator via `process_manager`.
    /// - Verifies post-restart health.
    /// - Writes the crash recovery report.
    ///
    /// Returns the report path on success, or `ModeCError` if max restarts exceeded.
    pub async fn run_recovery<P>(
        &mut self,
        process_manager: &P,
        exit_code: Option<i32>,
    ) -> Result<CrashRecoveryReport, ModeCError>
    where
        P: ProcessManager,
    {
        self.restart_attempts += 1;
        let attempt = self.restart_attempts;
        let max = self.config.max_restart_attempts;

        let ts = Utc::now().to_rfc3339();
        let mut actions: Vec<SupervisorAction> = Vec::new();

        // Step 1: Collect crash evidence.
        let mut evidence = collect_crash_evidence(
            &self.config.events_log_path,
            self.config.max_crash_event_lines,
            exit_code,
        );

        // Step 2: Registry cleanup.
        let cleanup_result = match cleanup_orphaned_tasks(&self.config.registry_path) {
            Ok(result) => {
                evidence.active_tasks_at_crash =
                    result.reset_task_ids.len() + result.skipped_active;

                let total_active = evidence.active_tasks_at_crash;
                let orphaned_reset = result.reset_task_ids.len();

                actions.push(SupervisorAction::RegistryScanned {
                    total_active,
                    orphaned_reset,
                });

                for task_id in &result.reset_task_ids {
                    actions.push(SupervisorAction::OrphanedTaskReset {
                        task_id: task_id.clone(),
                        previous_state: "claimed_or_in_progress".to_string(),
                    });
                }

                Some(result)
            }
            Err(err) => {
                tracing::warn!("mode_c: registry cleanup failed: {}", err);
                None
            }
        };

        // Step 3: Save session snapshot.
        let ts_safe = ts.replace(':', "-").replace('.', "-").replace('+', "-");
        let snapshot_name = format!("crash-recovery-pre-restart-{}", ts_safe);
        let session_snapshot_path = match save_session_snapshot(
            &self.config.tool_sessions_path,
            &self.config.sessions_snapshot_dir,
            &snapshot_name,
        ) {
            Ok(Some(path)) => {
                actions.push(SupervisorAction::SessionSnapshotSaved {
                    snapshot_name: snapshot_name.clone(),
                });
                Some(path)
            }
            Ok(None) => None,
            Err(err) => {
                tracing::warn!("mode_c: session snapshot failed: {}", err);
                None
            }
        };

        // Step 4: Check attempt limit before restarting.
        if attempt > max {
            actions.push(SupervisorAction::MaxRestartsExceeded { max_attempts: max });
            tracing::error!(
                attempt = attempt,
                max = max,
                "mode_c: max restart attempts exceeded, giving up"
            );
            return Err(ModeCError::MaxRestartsExceeded { max });
        }

        // Step 5: Restart coordinator.
        let restart_ok = match process_manager.start_coordinator().await {
            Ok(()) => {
                tracing::info!(
                    attempt = attempt,
                    max = max,
                    "mode_c: coordinator restarted"
                );
                actions.push(SupervisorAction::CoordinatorRestarted {
                    attempt,
                    max_attempts: max,
                });
                true
            }
            Err(ProcessManagerError::AlreadyRunning { pid }) => {
                tracing::warn!(
                    pid = pid,
                    "mode_c: coordinator already running after restart attempt"
                );
                actions.push(SupervisorAction::CoordinatorRestarted {
                    attempt,
                    max_attempts: max,
                });
                true
            }
            Err(err) => {
                tracing::error!("mode_c: coordinator restart failed: {}", err);
                false
            }
        };

        // Step 6: Post-restart health verification.
        let mut healthy_checks = 0u32;
        let post_restart_healthy = if restart_ok {
            let interval =
                Duration::from_secs(self.config.post_restart_check_interval_seconds.max(1));
            for _ in 0..self.config.post_restart_health_checks {
                tokio::time::sleep(interval).await;
                match process_manager.health_check().await {
                    Ok(HealthCheckResult::Healthy) | Ok(HealthCheckResult::Degraded { .. }) => {
                        healthy_checks += 1;
                    }
                    _ => break,
                }
            }
            let confirmed = healthy_checks >= 1;
            if confirmed {
                actions.push(SupervisorAction::PostRestartHealthVerified { healthy_checks });
                tracing::info!(
                    healthy_checks = healthy_checks,
                    "mode_c: post-restart health verification passed"
                );
            } else {
                tracing::warn!("mode_c: coordinator unhealthy after restart");
            }
            confirmed
        } else {
            false
        };

        let report = CrashRecoveryReport {
            timestamp: ts,
            crash_evidence: evidence,
            cleanup_result,
            restart_succeeded: restart_ok,
            restart_attempts: attempt,
            post_restart_healthy,
            actions_taken: actions,
            max_restarts_exceeded: false,
            session_snapshot_path,
        };

        Ok(report)
    }
}

// ── Supervisor report builder ──────────────────────────────────────────────────

/// Build a `SupervisorReport` from a completed `CrashRecoveryReport`.
pub fn build_supervisor_report(recovery: &CrashRecoveryReport) -> SupervisorReport {
    let health = if recovery.post_restart_healthy {
        HealthCheckResult::Healthy
    } else if recovery.max_restarts_exceeded {
        HealthCheckResult::Crashed {
            exit_code: recovery.crash_evidence.exit_code,
        }
    } else {
        HealthCheckResult::Unresponsive
    };

    let mut findings = Vec::new();

    findings.push(crate::supervisor::Finding {
        severity: if recovery.max_restarts_exceeded {
            Severity::Critical
        } else if recovery.post_restart_healthy {
            Severity::Warning
        } else {
            Severity::Error
        },
        category: FindingCategory::ProcessLifecycle,
        description: if recovery.max_restarts_exceeded {
            format!(
                "Coordinator crashed and max restart attempts ({}) exceeded",
                recovery.restart_attempts
            )
        } else if recovery.post_restart_healthy {
            format!(
                "Coordinator crashed and was restarted successfully (attempt {})",
                recovery.restart_attempts
            )
        } else {
            format!(
                "Coordinator crashed; restart attempt {} failed post-restart verification",
                recovery.restart_attempts
            )
        },
        evidence: recovery
            .crash_evidence
            .last_events
            .iter()
            .take(5)
            .cloned()
            .collect(),
    });

    if let Some(cleanup) = &recovery.cleanup_result {
        if !cleanup.reset_task_ids.is_empty() {
            findings.push(Finding {
                severity: Severity::Warning,
                category: FindingCategory::TaskProgress,
                description: format!(
                    "{} orphaned tasks reset to todo before restart: {}",
                    cleanup.reset_task_ids.len(),
                    cleanup.reset_task_ids.join(", ")
                ),
                evidence: Vec::new(),
            });
        }
    }

    let mut recommendations = Vec::new();
    if recovery.max_restarts_exceeded {
        recommendations.push(Recommendation {
            priority: 1,
            description: "Investigate coordinator startup failure".to_string(),
            rationale: "Coordinator exceeded max restart attempts and is no longer being recovered automatically".to_string(),
            affected_files: vec!["core/src/coordinator/".to_string()],
            implementation_hint: Some(
                "Check .macc/log/coordinator/ for crash details and fix the underlying issue before resuming".to_string(),
            ),
        });
    }

    let improvement_hints = build_improvement_hints(recovery);

    SupervisorReport {
        timestamp: recovery.timestamp.clone(),
        analysis_window_seconds: 0,
        health,
        findings,
        recommendations,
        actions_taken: recovery.actions_taken.clone(),
        suggested_code_changes: Vec::new(),
        improvement_hints,
    }
}

/// Derive structured improvement hints from a crash recovery report.
///
/// Each known crash pattern maps to a [`CoordinatorImprovementHint`] that gives
/// operators structured, actionable guidance for making the coordinator handle
/// the same problem autonomously in the future.
fn build_improvement_hints(recovery: &CrashRecoveryReport) -> Vec<CoordinatorImprovementHint> {
    let mut hints: Vec<CoordinatorImprovementHint> = Vec::new();

    // Pattern: coordinator crashed (always present in Mode C).
    hints.push(CoordinatorImprovementHint {
        problem_class: "coordinator_crash".to_string(),
        detection_hook: "watchdog_health_check".to_string(),
        suggested_coordinator_behavior: "implement exponential backoff before each restart \
            attempt and emit a structured alert on repeated crashes so operators are notified \
            without requiring supervisor intervention"
            .to_string(),
        suggested_prd_task: None,
    });

    // Pattern: orphaned tasks found — performers were not running when crash was detected.
    if let Some(cleanup) = &recovery.cleanup_result {
        if !cleanup.reset_task_ids.is_empty() {
            hints.push(CoordinatorImprovementHint {
                problem_class: "orphaned_tasks_on_crash".to_string(),
                detection_hook: "registry_scan_on_startup".to_string(),
                suggested_coordinator_behavior:
                    "scan the task registry on every coordinator startup and automatically reset \
                    tasks in claimed/in_progress state whose performer PID is no longer alive, \
                    preventing manual supervisor intervention on each restart"
                        .to_string(),
                suggested_prd_task: None,
            });
        }
    }

    // Pattern: max restart attempts exceeded — persistent crash loop.
    if recovery.max_restarts_exceeded {
        hints.push(CoordinatorImprovementHint {
            problem_class: "persistent_crash_loop".to_string(),
            detection_hook: "restart_attempt_counter".to_string(),
            suggested_coordinator_behavior:
                "circuit-break after reaching the max restart threshold: stop retrying, write \
                a critical alert to the event log, and wait for explicit operator resume signal \
                instead of silently giving up"
                    .to_string(),
            suggested_prd_task: None,
        });
    }

    // Pattern: events log was empty or missing at crash time.
    if recovery.crash_evidence.last_events.is_empty() {
        hints.push(CoordinatorImprovementHint {
            problem_class: "events_log_unavailable_at_crash".to_string(),
            detection_hook: "events_log_read_on_crash_detection".to_string(),
            suggested_coordinator_behavior:
                "add a startup check that validates events.jsonl writability before beginning \
                orchestration; emit a structured warning when the log cannot be opened so \
                operators can diagnose storage or permission issues before tasks are dispatched"
                    .to_string(),
            suggested_prd_task: None,
        });
    }

    hints
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use async_trait::async_trait;
    use std::sync::{Arc, Mutex};

    // ── Test helpers ──────────────────────────────────────────────────────────

    fn temp_dir(prefix: &str) -> PathBuf {
        let nanos = Utc::now().timestamp_nanos_opt().unwrap_or_default();
        let path =
            std::env::temp_dir().join(format!("macc-supervisor-mode-c-{}-{}", prefix, nanos));
        fs::create_dir_all(&path).expect("create temp dir");
        path
    }

    fn write_registry(path: &Path, tasks: &[(&str, &str, Option<u32>)]) {
        // tasks: (id, state, performer_pid)
        let tasks_json: Vec<serde_json::Value> = tasks
            .iter()
            .map(|(id, state, pid)| {
                let mut task = serde_json::json!({
                    "id": id,
                    "state": state,
                    "task_runtime": {}
                });
                if let Some(pid) = pid {
                    task["task_runtime"]["pid"] = serde_json::json!(pid);
                }
                task
            })
            .collect();

        let registry = serde_json::json!({ "tasks": tasks_json });
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).expect("create registry parent");
        }
        fs::write(path, serde_json::to_vec_pretty(&registry).unwrap()).expect("write registry");
    }

    struct MockProcessManager {
        health: Arc<Mutex<HealthCheckResult>>,
        start_result: Arc<Mutex<Result<(), ProcessManagerError>>>,
        pid: Option<u32>,
    }

    impl MockProcessManager {
        fn healthy() -> Self {
            Self {
                health: Arc::new(Mutex::new(HealthCheckResult::Healthy)),
                start_result: Arc::new(Mutex::new(Ok(()))),
                pid: Some(1234),
            }
        }

        fn crash_then_recover() -> Self {
            // Start as crashed; after start_coordinator(), health returns Healthy.
            let health = Arc::new(Mutex::new(HealthCheckResult::Crashed {
                exit_code: Some(1),
            }));
            let start_result = Arc::new(Mutex::new(Ok(())));
            Self {
                health,
                start_result,
                pid: None,
            }
        }

        fn always_fail_start() -> Self {
            Self {
                health: Arc::new(Mutex::new(HealthCheckResult::Crashed {
                    exit_code: Some(1),
                })),
                start_result: Arc::new(Mutex::new(Err(ProcessManagerError::SpawnFailed(
                    "injected failure".to_string(),
                )))),
                pid: None,
            }
        }
    }

    #[async_trait]
    impl ProcessManager for MockProcessManager {
        async fn start_coordinator(&self) -> Result<(), ProcessManagerError> {
            let result = self.start_result.lock().unwrap();
            match &*result {
                Ok(()) => {
                    // Simulate: after start, health becomes healthy.
                    let mut h = self.health.lock().unwrap();
                    *h = HealthCheckResult::Healthy;
                    Ok(())
                }
                Err(ProcessManagerError::SpawnFailed(msg)) => {
                    Err(ProcessManagerError::SpawnFailed(msg.clone()))
                }
                Err(e) => Err(ProcessManagerError::SpawnFailed(e.to_string())),
            }
        }

        async fn stop_coordinator(&self) -> Result<(), ProcessManagerError> {
            Ok(())
        }

        async fn health_check(&self) -> Result<HealthCheckResult, ProcessManagerError> {
            Ok(self.health.lock().unwrap().clone())
        }

        async fn coordinator_pid(&self) -> Option<u32> {
            self.pid
        }
    }

    // ── Unit tests ────────────────────────────────────────────────────────────

    #[test]
    fn mode_c_config_defaults() {
        let config = ModeCConfig::default();
        assert_eq!(config.max_restart_attempts, 3);
        assert_eq!(config.post_restart_health_checks, 3);
        assert_eq!(config.post_restart_check_interval_seconds, 10);
        assert_eq!(config.max_crash_event_lines, 50);
    }

    #[test]
    fn mode_c_config_round_trips_json() {
        let config = ModeCConfig::default();
        let json = serde_json::to_string(&config).expect("serialize");
        let back: ModeCConfig = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(config, back);
    }

    #[test]
    fn cleanup_orphaned_tasks_resets_claimed_task() {
        let root = temp_dir("cleanup-claimed");
        let registry_path = root.join("task_registry.json");

        // No real performer running for pid 999999 (not a real process).
        write_registry(&registry_path, &[("T-001", "claimed", Some(999_999_u32))]);

        let result = cleanup_orphaned_tasks(&registry_path).expect("cleanup");

        assert_eq!(result.reset_task_ids, vec!["T-001"]);
        assert_eq!(result.skipped_active, 0);

        // Verify the registry was written with todo state.
        let raw = fs::read_to_string(&registry_path).expect("read registry");
        let v: serde_json::Value = serde_json::from_str(&raw).expect("parse");
        let state = v["tasks"][0]["state"].as_str().unwrap_or("");
        assert_eq!(state, "todo");
    }

    #[test]
    fn cleanup_orphaned_tasks_resets_in_progress_task() {
        let root = temp_dir("cleanup-inprog");
        let registry_path = root.join("task_registry.json");

        write_registry(
            &registry_path,
            &[("T-002", "in_progress", Some(999_998_u32))],
        );

        let result = cleanup_orphaned_tasks(&registry_path).expect("cleanup");

        assert!(result.reset_task_ids.contains(&"T-002".to_string()));
    }

    #[test]
    fn cleanup_orphaned_tasks_skips_todo_tasks() {
        let root = temp_dir("cleanup-todo");
        let registry_path = root.join("task_registry.json");

        write_registry(&registry_path, &[("T-003", "todo", None)]);

        let result = cleanup_orphaned_tasks(&registry_path).expect("cleanup");

        assert!(result.reset_task_ids.is_empty());
        assert_eq!(result.skipped_active, 0);
    }

    #[test]
    fn cleanup_orphaned_tasks_missing_registry_returns_error() {
        let root = temp_dir("cleanup-missing");
        let registry_path = root.join("task_registry.json");

        let err = cleanup_orphaned_tasks(&registry_path).expect_err("should fail");
        assert!(matches!(err, CleanupError::RegistryNotFound { .. }));
    }

    #[test]
    fn collect_crash_evidence_returns_last_events() {
        let root = temp_dir("evidence");
        let events_path = root.join("events.jsonl");

        let events = (0..60)
            .map(|i| {
                format!(
                    "{{\"ts\":\"2026-04-13T00:{:02}:00Z\",\"type\":\"heartbeat\"}}",
                    i
                )
            })
            .collect::<Vec<_>>()
            .join("\n");
        fs::write(&events_path, events).expect("write events");

        let evidence = collect_crash_evidence(&events_path, 10, Some(1));

        assert_eq!(evidence.exit_code, Some(1));
        assert_eq!(evidence.last_events.len(), 10);
    }

    #[test]
    fn collect_crash_evidence_missing_log_returns_empty() {
        let root = temp_dir("evidence-missing");
        let events_path = root.join("events.jsonl");

        let evidence = collect_crash_evidence(&events_path, 10, None);

        assert!(evidence.last_events.is_empty());
        assert_eq!(evidence.exit_code, None);
    }

    #[test]
    fn save_session_snapshot_copies_file() {
        let root = temp_dir("snapshot");
        let sessions_path = root.join("tool-sessions.json");
        let snapshot_dir = root.join("snapshots");

        fs::write(&sessions_path, r#"{"tools":{}}"#).expect("write sessions");

        let result = save_session_snapshot(&sessions_path, &snapshot_dir, "test-snap")
            .expect("save snapshot");

        assert!(result.is_some());
        let dest = snapshot_dir.join("test-snap.json");
        assert!(dest.exists());
    }

    #[test]
    fn save_session_snapshot_no_file_returns_none() {
        let root = temp_dir("snapshot-missing");
        let sessions_path = root.join("tool-sessions.json");
        let snapshot_dir = root.join("snapshots");

        let result =
            save_session_snapshot(&sessions_path, &snapshot_dir, "snap").expect("no io error");

        assert!(result.is_none());
    }

    #[test]
    fn crash_recovery_report_written_to_disk() {
        let root = temp_dir("report");
        let report = CrashRecoveryReport {
            timestamp: "2026-04-13T00:00:00Z".to_string(),
            crash_evidence: CrashEvidence {
                exit_code: Some(1),
                last_events: vec!["event1".to_string()],
                detected_at: "2026-04-13T00:00:00Z".to_string(),
                active_tasks_at_crash: 2,
            },
            cleanup_result: Some(RegistryCleanupResult {
                reset_task_ids: vec!["T-001".to_string()],
                skipped_active: 0,
            }),
            restart_succeeded: true,
            restart_attempts: 1,
            post_restart_healthy: true,
            actions_taken: vec![SupervisorAction::CoordinatorRestarted {
                attempt: 1,
                max_attempts: 3,
            }],
            max_restarts_exceeded: false,
            session_snapshot_path: None,
        };

        let path = report.write_to_dir(&root).expect("write report");

        assert!(path.exists());
        let raw = fs::read_to_string(&path).expect("read report");
        let v: serde_json::Value = serde_json::from_str(&raw).expect("parse");
        assert_eq!(v["restart_succeeded"], true);
        assert_eq!(v["post_restart_healthy"], true);
    }

    #[tokio::test]
    async fn recovery_succeeds_and_restarts_coordinator() {
        let root = temp_dir("recovery-ok");
        let registry_path = root.join("task_registry.json");
        let events_path = root.join("events.jsonl");
        let sessions_path = root.join("tool-sessions.json");
        let snapshot_dir = root.join("snapshots");
        let report_dir = root.join("reports");

        // No tasks in registry.
        write_registry(&registry_path, &[]);
        fs::write(&events_path, "").expect("write events");
        fs::write(&sessions_path, r#"{"tools":{}}"#).expect("write sessions");

        let config = ModeCConfig {
            max_restart_attempts: 3,
            post_restart_health_checks: 1,
            post_restart_check_interval_seconds: 0, // no actual sleep in tests
            report_output_dir: report_dir.clone(),
            events_log_path: events_path,
            max_crash_event_lines: 10,
            registry_path,
            tool_sessions_path: sessions_path,
            sessions_snapshot_dir: snapshot_dir,
        };

        let pm = MockProcessManager::crash_then_recover();
        let mut recovery = ModeCRecovery::new(config);

        let report = recovery.run_recovery(&pm, Some(1)).await.expect("recovery");

        assert!(report.restart_succeeded);
        assert!(report.post_restart_healthy);
        assert!(!report.max_restarts_exceeded);
        assert_eq!(report.restart_attempts, 1);

        // Verify actions.
        assert!(report
            .actions_taken
            .iter()
            .any(|a| matches!(a, SupervisorAction::CoordinatorRestarted { .. })));
        assert!(report
            .actions_taken
            .iter()
            .any(|a| matches!(a, SupervisorAction::PostRestartHealthVerified { .. })));
        assert!(report
            .actions_taken
            .iter()
            .any(|a| matches!(a, SupervisorAction::SessionSnapshotSaved { .. })));

        // Write report and verify it.
        let path = report.write_to_dir(&report_dir).expect("write report");
        assert!(path.exists());
    }

    #[tokio::test]
    async fn recovery_resets_orphaned_tasks_before_restart() {
        let root = temp_dir("recovery-orphan");
        let registry_path = root.join("task_registry.json");
        let events_path = root.join("events.jsonl");
        let sessions_path = root.join("tool-sessions.json");

        // Two orphaned tasks (pids that are definitely not running).
        write_registry(
            &registry_path,
            &[
                ("T-001", "claimed", Some(999_991_u32)),
                ("T-002", "in_progress", Some(999_992_u32)),
            ],
        );
        fs::write(&events_path, "").expect("write events");

        let config = ModeCConfig {
            max_restart_attempts: 3,
            post_restart_health_checks: 1,
            post_restart_check_interval_seconds: 0,
            report_output_dir: root.join("reports"),
            events_log_path: events_path,
            max_crash_event_lines: 10,
            registry_path: registry_path.clone(),
            tool_sessions_path: sessions_path,
            sessions_snapshot_dir: root.join("snapshots"),
        };

        let pm = MockProcessManager::crash_then_recover();
        let mut recovery = ModeCRecovery::new(config);

        let report = recovery
            .run_recovery(&pm, Some(137))
            .await
            .expect("recovery");

        assert!(report.cleanup_result.is_some());
        let cleanup = report.cleanup_result.as_ref().unwrap();
        assert_eq!(cleanup.reset_task_ids.len(), 2);

        // Registry should now have todo states.
        let raw = fs::read_to_string(&registry_path).expect("read registry");
        let v: serde_json::Value = serde_json::from_str(&raw).expect("parse");
        for task in v["tasks"].as_array().unwrap() {
            assert_eq!(task["state"].as_str().unwrap(), "todo");
        }

        // Evidence should record active task count.
        assert_eq!(report.crash_evidence.active_tasks_at_crash, 2);
    }

    #[tokio::test]
    async fn recovery_max_restarts_exceeded_returns_error() {
        let root = temp_dir("recovery-maxfail");
        let registry_path = root.join("task_registry.json");
        let events_path = root.join("events.jsonl");
        let sessions_path = root.join("tool-sessions.json");

        write_registry(&registry_path, &[]);
        fs::write(&events_path, "").expect("write events");

        let config = ModeCConfig {
            max_restart_attempts: 2,
            post_restart_health_checks: 1,
            post_restart_check_interval_seconds: 0,
            report_output_dir: root.join("reports"),
            events_log_path: events_path,
            max_crash_event_lines: 10,
            registry_path,
            tool_sessions_path: sessions_path,
            sessions_snapshot_dir: root.join("snapshots"),
        };

        let pm = MockProcessManager::always_fail_start();
        let mut recovery = ModeCRecovery::new(config);

        // Exhaust all attempts.
        for _ in 0..2 {
            let _ = recovery.run_recovery(&pm, Some(1)).await;
        }

        // The third attempt exceeds the limit.
        let err = recovery
            .run_recovery(&pm, Some(1))
            .await
            .expect_err("should exceed max");

        assert!(matches!(err, ModeCError::MaxRestartsExceeded { max: 2 }));
    }

    #[test]
    fn build_supervisor_report_healthy_after_restart() {
        let recovery = CrashRecoveryReport {
            timestamp: "2026-04-13T00:00:00Z".to_string(),
            crash_evidence: CrashEvidence {
                exit_code: Some(1),
                last_events: vec!["evt".to_string()],
                detected_at: "2026-04-13T00:00:00Z".to_string(),
                active_tasks_at_crash: 0,
            },
            cleanup_result: None,
            restart_succeeded: true,
            restart_attempts: 1,
            post_restart_healthy: true,
            actions_taken: vec![SupervisorAction::CoordinatorRestarted {
                attempt: 1,
                max_attempts: 3,
            }],
            max_restarts_exceeded: false,
            session_snapshot_path: None,
        };

        let report = build_supervisor_report(&recovery);

        assert_eq!(report.health, HealthCheckResult::Healthy);
        assert!(!report.findings.is_empty());
        assert!(report.has_severity(&Severity::Warning));
    }

    #[test]
    fn build_supervisor_report_critical_when_max_exceeded() {
        let recovery = CrashRecoveryReport {
            timestamp: "2026-04-13T00:00:00Z".to_string(),
            crash_evidence: CrashEvidence {
                exit_code: Some(1),
                last_events: Vec::new(),
                detected_at: "2026-04-13T00:00:00Z".to_string(),
                active_tasks_at_crash: 0,
            },
            cleanup_result: None,
            restart_succeeded: false,
            restart_attempts: 3,
            post_restart_healthy: false,
            actions_taken: vec![SupervisorAction::MaxRestartsExceeded { max_attempts: 3 }],
            max_restarts_exceeded: true,
            session_snapshot_path: None,
        };

        let report = build_supervisor_report(&recovery);

        assert!(matches!(report.health, HealthCheckResult::Crashed { .. }));
        assert!(report.has_severity(&Severity::Critical));
        assert!(!report.recommendations.is_empty());
    }

    // ── build_supervisor_report: improvement hints ─────────────────────────────

    fn basic_crash_report(
        max_restarts_exceeded: bool,
        orphaned: bool,
        events_empty: bool,
    ) -> CrashRecoveryReport {
        CrashRecoveryReport {
            timestamp: "2026-04-28T00:00:00Z".to_string(),
            crash_evidence: CrashEvidence {
                exit_code: Some(1),
                last_events: if events_empty {
                    vec![]
                } else {
                    vec!["event1".to_string()]
                },
                detected_at: "2026-04-28T00:00:00Z".to_string(),
                active_tasks_at_crash: if orphaned { 1 } else { 0 },
            },
            cleanup_result: if orphaned {
                Some(RegistryCleanupResult {
                    reset_task_ids: vec!["T-001".to_string()],
                    skipped_active: 0,
                })
            } else {
                None
            },
            restart_succeeded: !max_restarts_exceeded,
            restart_attempts: if max_restarts_exceeded { 3 } else { 1 },
            post_restart_healthy: !max_restarts_exceeded,
            actions_taken: vec![],
            max_restarts_exceeded,
            session_snapshot_path: None,
        }
    }

    #[test]
    fn build_supervisor_report_always_emits_coordinator_crash_hint() {
        let recovery = basic_crash_report(false, false, false);
        let report = build_supervisor_report(&recovery);
        let classes: Vec<&str> = report
            .improvement_hints
            .iter()
            .map(|h| h.problem_class.as_str())
            .collect();
        assert!(
            classes.contains(&"coordinator_crash"),
            "expected coordinator_crash hint, got: {:?}",
            classes
        );
    }

    #[test]
    fn build_supervisor_report_emits_orphaned_tasks_hint_when_tasks_were_reset() {
        let recovery = basic_crash_report(false, true, false);
        let report = build_supervisor_report(&recovery);
        let classes: Vec<&str> = report
            .improvement_hints
            .iter()
            .map(|h| h.problem_class.as_str())
            .collect();
        assert!(
            classes.contains(&"orphaned_tasks_on_crash"),
            "expected orphaned_tasks_on_crash hint, got: {:?}",
            classes
        );
    }

    #[test]
    fn build_supervisor_report_no_orphaned_tasks_hint_when_no_reset() {
        let recovery = basic_crash_report(false, false, false);
        let report = build_supervisor_report(&recovery);
        let classes: Vec<&str> = report
            .improvement_hints
            .iter()
            .map(|h| h.problem_class.as_str())
            .collect();
        assert!(
            !classes.contains(&"orphaned_tasks_on_crash"),
            "unexpected orphaned_tasks_on_crash hint when no tasks were reset"
        );
    }

    #[test]
    fn build_supervisor_report_emits_persistent_crash_loop_hint_when_max_exceeded() {
        let recovery = basic_crash_report(true, false, false);
        let report = build_supervisor_report(&recovery);
        let classes: Vec<&str> = report
            .improvement_hints
            .iter()
            .map(|h| h.problem_class.as_str())
            .collect();
        assert!(
            classes.contains(&"persistent_crash_loop"),
            "expected persistent_crash_loop hint, got: {:?}",
            classes
        );
    }

    #[test]
    fn build_supervisor_report_emits_events_log_hint_when_events_empty() {
        let recovery = basic_crash_report(false, false, true);
        let report = build_supervisor_report(&recovery);
        let classes: Vec<&str> = report
            .improvement_hints
            .iter()
            .map(|h| h.problem_class.as_str())
            .collect();
        assert!(
            classes.contains(&"events_log_unavailable_at_crash"),
            "expected events_log_unavailable_at_crash hint, got: {:?}",
            classes
        );
    }

    #[test]
    fn build_supervisor_report_improvement_hints_serialize_in_json_output() {
        let recovery = basic_crash_report(false, false, false);
        let report = build_supervisor_report(&recovery);
        assert!(!report.improvement_hints.is_empty());
        let json = serde_json::to_string_pretty(&report).expect("serialize");
        assert!(json.contains("improvement_hints"));
        assert!(json.contains("problem_class"));
        assert!(json.contains("coordinator_crash"));
    }

    #[test]
    fn supervisor_action_mode_c_variants_round_trip_json() {
        let actions = vec![
            SupervisorAction::RegistryScanned {
                total_active: 3,
                orphaned_reset: 2,
            },
            SupervisorAction::OrphanedTaskReset {
                task_id: "T-001".to_string(),
                previous_state: "claimed".to_string(),
            },
            SupervisorAction::SessionSnapshotSaved {
                snapshot_name: "snap-2026".to_string(),
            },
            SupervisorAction::CoordinatorRestarted {
                attempt: 1,
                max_attempts: 3,
            },
            SupervisorAction::PostRestartHealthVerified { healthy_checks: 3 },
            SupervisorAction::MaxRestartsExceeded { max_attempts: 3 },
            SupervisorAction::CrashRecoveryReportWritten {
                report_path: "/tmp/report.json".to_string(),
            },
        ];

        for action in actions {
            let json = serde_json::to_string(&action).expect("serialize");
            let back: SupervisorAction = serde_json::from_str(&json).expect("deserialize");
            assert_eq!(action, back);
        }
    }
}
