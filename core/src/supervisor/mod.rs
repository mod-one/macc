//! Supervisor agent contracts and communication protocol.
//!
//! This module defines the foundational types for the supervisor agent that
//! monitors the MACC coordinator process.
//!
//! # Communication protocol overview
//!
//! ```text
//! Supervisor ──[health_check()]──► Coordinator process (pid)
//!     │                               │
//!     │◄──[HealthCheckResult]──────────┘
//!     │
//!     │──[tail events.jsonl]──► Event stream
//!     │
//!     └──[SupervisorReport]──► report_output_path (JSON)
//! ```
//!
//! The supervisor operates on a watchdog interval, checking the coordinator
//! process health and analyzing the event log window for anomalies. Reports
//! are written as machine-readable JSON for downstream consumption.

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

pub mod mode_a;
pub mod mode_b;
pub mod mode_c;

// ── Configuration ────────────────────────────────────────────────────────────

/// Configuration for the supervisor watchdog process.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct SupervisorConfig {
    /// How often the supervisor checks coordinator health (seconds).
    #[serde(default = "default_watchdog_interval_seconds")]
    pub watchdog_interval_seconds: u64,

    /// Maximum consecutive restart attempts before supervisor gives up.
    #[serde(default = "default_max_restart_attempts")]
    pub max_restart_attempts: u32,

    /// Duration of the event log window to analyze on each cycle (seconds).
    #[serde(default = "default_log_analysis_window_seconds")]
    pub log_analysis_window_seconds: u64,

    /// Path where the supervisor writes its structured JSON report.
    #[serde(default = "default_report_output_path")]
    pub report_output_path: PathBuf,

    /// Path to the coordinator events log file to tail.
    #[serde(default = "default_events_log_path")]
    pub events_log_path: PathBuf,
}

fn default_watchdog_interval_seconds() -> u64 {
    30
}
fn default_max_restart_attempts() -> u32 {
    3
}
fn default_log_analysis_window_seconds() -> u64 {
    300
}
fn default_report_output_path() -> PathBuf {
    PathBuf::from(".macc/log/supervisor/report.json")
}
fn default_events_log_path() -> PathBuf {
    PathBuf::from(".macc/log/coordinator/events.jsonl")
}

impl Default for SupervisorConfig {
    fn default() -> Self {
        Self {
            watchdog_interval_seconds: default_watchdog_interval_seconds(),
            max_restart_attempts: default_max_restart_attempts(),
            log_analysis_window_seconds: default_log_analysis_window_seconds(),
            report_output_path: default_report_output_path(),
            events_log_path: default_events_log_path(),
        }
    }
}

// ── Health check ─────────────────────────────────────────────────────────────

/// Result of a supervisor health check against the coordinator process.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum HealthCheckResult {
    /// Coordinator is running and responding normally.
    Healthy,

    /// Coordinator is running but showing signs of problems.
    Degraded {
        /// Human-readable descriptions of observed degradation signals.
        reasons: Vec<String>,
    },

    /// Coordinator process exists but is not responding to checks.
    Unresponsive,

    /// Coordinator process has exited unexpectedly.
    Crashed {
        /// Exit code from the process, if available.
        exit_code: Option<i32>,
    },
}

impl HealthCheckResult {
    /// Returns `true` if the coordinator is healthy or degraded (still running).
    pub fn is_running(&self) -> bool {
        matches!(self, Self::Healthy | Self::Degraded { .. })
    }

    /// Returns `true` if the coordinator requires operator intervention.
    pub fn requires_restart(&self) -> bool {
        matches!(self, Self::Unresponsive | Self::Crashed { .. })
    }
}

// ── Finding ──────────────────────────────────────────────────────────────────

/// Severity level of a supervisor finding.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
#[serde(rename_all = "snake_case")]
pub enum Severity {
    Info,
    Warning,
    Error,
    Critical,
}

/// Category classifying what domain a finding relates to.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum FindingCategory {
    /// Coordinator process lifecycle (crash, restart, PID change).
    ProcessLifecycle,
    /// Task progression is stalled or below expected throughput.
    TaskProgress,
    /// Rate-limiting or quota signals from provider.
    RateLimit,
    /// Merge failures or conflicts blocking forward progress.
    MergeHealth,
    /// Log volume, event gaps, or missing heartbeats.
    Observability,
    /// Any finding that does not fit the above categories.
    Other,
}

/// An observed anomaly or noteworthy signal from the coordinator.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Finding {
    /// How serious this finding is.
    pub severity: Severity,

    /// Domain this finding relates to.
    pub category: FindingCategory,

    /// Human-readable description of what was observed.
    pub description: String,

    /// Supporting evidence: raw log lines, event JSON snippets, or other context.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub evidence: Vec<String>,
}

// ── Recommendation ───────────────────────────────────────────────────────────

/// A suggested corrective or preventive action.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Recommendation {
    /// Ordering hint (1 = highest priority).
    pub priority: u32,

    /// What should be done.
    pub description: String,

    /// Why this recommendation is being made.
    pub rationale: String,

    /// Source files in the MACC codebase relevant to this recommendation.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub affected_files: Vec<String>,

    /// A concise hint for how to implement the recommendation.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub implementation_hint: Option<String>,
}

// ── CodeChange ───────────────────────────────────────────────────────────────

/// A concrete suggested change to MACC source code.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct CodeChange {
    /// Repository-relative path of the file to change.
    pub file_path: String,

    /// Human-readable description of the change.
    pub description: String,

    /// Excerpt of the existing code at the relevant location (for context).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub before_context: Option<String>,

    /// The suggested replacement or patch (unified diff or prose description).
    pub suggested_change: String,
}

// ── Action ───────────────────────────────────────────────────────────────────

/// An action the supervisor has already taken (as opposed to a recommendation).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum SupervisorAction {
    /// Supervisor restarted the coordinator process.
    RestartedCoordinator { attempt: u32 },

    /// Supervisor wrote a pause signal to the coordinator pause file.
    PausedCoordinator { reason: String },

    /// Supervisor sent an alert / notification.
    AlertEmitted { channel: String, message: String },

    /// Supervisor took no automatic action (report only).
    NoAction { reason: String },

    // ── Mode B: worktree-level recovery actions ───────────────────────────

    /// Supervisor ran `git reset --hard HEAD` in the target worktree.
    WorktreeGitReset {
        worktree_path: String,
        /// Whether the reset command succeeded.
        succeeded: bool,
    },

    /// Supervisor ran `git clean -fd` in the target worktree.
    WorktreeGitClean {
        worktree_path: String,
        succeeded: bool,
    },

    /// AI analysis was dispatched for a stuck or failed worktree.
    AiAnalysisDispatched {
        task_id: String,
        worktree_path: String,
    },

    /// AI analysis completed and the report was written to disk.
    AiAnalysisCompleted {
        task_id: String,
        report_path: String,
    },

    /// AI analysis was skipped because the recovery attempt limit was reached.
    RecoveryAttemptLimitReached {
        task_id: String,
        limit: u32,
    },

    // ── Mode C: coordinator crash recovery actions ────────────────────────

    /// Task registry was scanned and orphaned tasks were found before restart.
    RegistryScanned {
        /// Total tasks in claimed/in_progress state at time of scan.
        total_active: usize,
        /// Tasks reset to todo because their performer process was not running.
        orphaned_reset: usize,
    },

    /// A specific task was reset to todo during pre-restart cleanup.
    OrphanedTaskReset {
        task_id: String,
        previous_state: String,
    },

    /// Session snapshot was saved before coordinator restart.
    SessionSnapshotSaved {
        snapshot_name: String,
    },

    /// Coordinator was restarted after a crash.
    CoordinatorRestarted {
        attempt: u32,
        max_attempts: u32,
    },

    /// Post-restart health verification confirmed coordinator is running.
    PostRestartHealthVerified {
        healthy_checks: u32,
    },

    /// Maximum restart attempts exceeded; supervisor gave up.
    MaxRestartsExceeded {
        max_attempts: u32,
    },

    /// Crash recovery report was written to disk.
    CrashRecoveryReportWritten {
        report_path: String,
    },
}

// ── Report ───────────────────────────────────────────────────────────────────

/// Machine-readable report produced by one supervisor cycle.
///
/// Written as JSON to `SupervisorConfig::report_output_path`.
/// Consumers (CI, dashboards, downstream agents) should parse this struct.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct SupervisorReport {
    /// RFC-3339 timestamp when this report was generated (UTC).
    pub timestamp: String,

    /// How many seconds of events were analysed for this report.
    pub analysis_window_seconds: u64,

    /// Current health status of the coordinator process.
    pub health: HealthCheckResult,

    /// All observations made during this cycle, ordered by severity descending.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub findings: Vec<Finding>,

    /// Ranked list of suggested corrective actions.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub recommendations: Vec<Recommendation>,

    /// Actions the supervisor has already taken this cycle.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub actions_taken: Vec<SupervisorAction>,

    /// Concrete source-level changes suggested to improve coordinator resilience.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub suggested_code_changes: Vec<CodeChange>,
}

impl SupervisorReport {
    /// Returns `true` if the report contains any finding at or above `severity`.
    pub fn has_severity(&self, severity: &Severity) -> bool {
        self.findings.iter().any(|f| &f.severity >= severity)
    }

    /// Returns `true` if no findings and no actions were taken.
    pub fn is_clean(&self) -> bool {
        self.findings.is_empty() && self.actions_taken.is_empty()
    }
}

// ── ProcessManager trait ─────────────────────────────────────────────────────

/// Interface for managing and probing the coordinator OS process.
///
/// Implementations may be real (spawning via tokio::process) or test doubles.
#[async_trait]
pub trait ProcessManager: Send + Sync {
    /// Spawn the coordinator process. Returns `Ok(())` if start was requested;
    /// callers should follow with `coordinator_pid()` to confirm liveness.
    async fn start_coordinator(&self) -> Result<(), ProcessManagerError>;

    /// Send a graceful stop signal to the coordinator process.
    /// Callers are expected to follow with a `health_check()` to confirm exit.
    async fn stop_coordinator(&self) -> Result<(), ProcessManagerError>;

    /// Check whether the coordinator is alive and responsive.
    async fn health_check(&self) -> Result<HealthCheckResult, ProcessManagerError>;

    /// Return the PID of the running coordinator, if known.
    async fn coordinator_pid(&self) -> Option<u32>;
}

/// Errors from `ProcessManager` operations.
#[derive(Debug, thiserror::Error)]
pub enum ProcessManagerError {
    #[error("Coordinator process already running (pid {pid})")]
    AlreadyRunning { pid: u32 },

    #[error("Coordinator process is not running")]
    NotRunning,

    #[error("Failed to spawn coordinator: {0}")]
    SpawnFailed(String),

    #[error("Signal delivery failed: {0}")]
    SignalFailed(String),

    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
}

// ── EventConsumer trait ──────────────────────────────────────────────────────

/// Interface for reading coordinator events from the event log.
///
/// The canonical event log format is newline-delimited JSON at
/// `.macc/log/coordinator/events.jsonl`.
#[async_trait]
pub trait EventConsumer: Send + Sync {
    /// Return event lines from the log within the given time window.
    ///
    /// - `since_rfc3339`: RFC-3339 string; include only events at or after this timestamp.
    /// - Returns raw JSON strings (one per event line) in arrival order.
    async fn events_since(
        &self,
        since_rfc3339: &str,
    ) -> Result<Vec<String>, EventConsumerError>;
}

/// Errors from `EventConsumer` operations.
#[derive(Debug, thiserror::Error)]
pub enum EventConsumerError {
    #[error("Event log not found at {path}")]
    LogNotFound { path: PathBuf },

    #[error("Failed to parse event line: {raw}")]
    ParseError { raw: String },

    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_config() -> SupervisorConfig {
        SupervisorConfig {
            watchdog_interval_seconds: 60,
            max_restart_attempts: 5,
            log_analysis_window_seconds: 600,
            report_output_path: PathBuf::from("/tmp/report.json"),
            events_log_path: PathBuf::from("/tmp/events.jsonl"),
        }
    }

    fn sample_report() -> SupervisorReport {
        SupervisorReport {
            timestamp: "1970-01-01T00:00:00Z".to_string(),
            analysis_window_seconds: 300,
            health: HealthCheckResult::Healthy,
            findings: vec![Finding {
                severity: Severity::Warning,
                category: FindingCategory::TaskProgress,
                description: "No task transitions in the last 5 minutes".to_string(),
                evidence: vec!["last event: heartbeat at T-310s".to_string()],
            }],
            recommendations: vec![Recommendation {
                priority: 1,
                description: "Investigate stalled task queue".to_string(),
                rationale: "No progress events observed in the analysis window".to_string(),
                affected_files: vec!["core/src/coordinator/engine.rs".to_string()],
                implementation_hint: Some(
                    "Check task_selector for tasks stuck in claimed state".to_string(),
                ),
            }],
            actions_taken: vec![SupervisorAction::NoAction {
                reason: "Below restart threshold".to_string(),
            }],
            suggested_code_changes: vec![CodeChange {
                file_path: "core/src/coordinator/engine.rs".to_string(),
                description: "Add stale-task watchdog timeout".to_string(),
                before_context: None,
                suggested_change: "Add a periodic check that requeues claimed tasks older than stale_claimed_seconds".to_string(),
            }],
        }
    }

    #[test]
    fn supervisor_config_round_trips_json() {
        let config = sample_config();
        let json = serde_json::to_string(&config).expect("serialize");
        let back: SupervisorConfig = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(config, back);
    }

    #[test]
    fn supervisor_config_defaults() {
        let config = SupervisorConfig::default();
        assert_eq!(config.watchdog_interval_seconds, 30);
        assert_eq!(config.max_restart_attempts, 3);
        assert_eq!(config.log_analysis_window_seconds, 300);
    }

    #[test]
    fn health_check_result_round_trips_json() {
        let cases = vec![
            HealthCheckResult::Healthy,
            HealthCheckResult::Degraded {
                reasons: vec!["high error rate".to_string()],
            },
            HealthCheckResult::Unresponsive,
            HealthCheckResult::Crashed { exit_code: Some(1) },
            HealthCheckResult::Crashed { exit_code: None },
        ];
        for h in cases {
            let json = serde_json::to_string(&h).expect("serialize");
            let back: HealthCheckResult = serde_json::from_str(&json).expect("deserialize");
            assert_eq!(h, back);
        }
    }

    #[test]
    fn health_check_is_running_semantics() {
        assert!(HealthCheckResult::Healthy.is_running());
        assert!(HealthCheckResult::Degraded {
            reasons: vec![]
        }
        .is_running());
        assert!(!HealthCheckResult::Unresponsive.is_running());
        assert!(!HealthCheckResult::Crashed { exit_code: None }.is_running());
    }

    #[test]
    fn health_check_requires_restart_semantics() {
        assert!(!HealthCheckResult::Healthy.requires_restart());
        assert!(HealthCheckResult::Unresponsive.requires_restart());
        assert!(HealthCheckResult::Crashed { exit_code: Some(137) }.requires_restart());
    }

    #[test]
    fn supervisor_report_round_trips_json() {
        let report = sample_report();
        let json = serde_json::to_string_pretty(&report).expect("serialize");
        let back: SupervisorReport = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(report, back);
    }

    #[test]
    fn supervisor_report_has_severity() {
        let report = sample_report();
        assert!(report.has_severity(&Severity::Warning));
        assert!(!report.has_severity(&Severity::Error));
        assert!(!report.has_severity(&Severity::Critical));
    }

    #[test]
    fn supervisor_report_is_clean() {
        let report = SupervisorReport {
            timestamp: "1970-01-01T00:00:00Z".to_string(),
            analysis_window_seconds: 300,
            health: HealthCheckResult::Healthy,
            findings: vec![],
            recommendations: vec![],
            actions_taken: vec![],
            suggested_code_changes: vec![],
        };
        assert!(report.is_clean());
        assert!(!sample_report().is_clean());
    }

    #[test]
    fn supervisor_action_round_trips_json() {
        let actions = vec![
            SupervisorAction::RestartedCoordinator { attempt: 1 },
            SupervisorAction::PausedCoordinator {
                reason: "quota exhausted".to_string(),
            },
            SupervisorAction::AlertEmitted {
                channel: "slack".to_string(),
                message: "coordinator crashed".to_string(),
            },
            SupervisorAction::NoAction {
                reason: "healthy".to_string(),
            },
        ];
        for action in actions {
            let json = serde_json::to_string(&action).expect("serialize");
            let back: SupervisorAction = serde_json::from_str(&json).expect("deserialize");
            assert_eq!(action, back);
        }
    }

    #[test]
    fn severity_ordering() {
        assert!(Severity::Critical > Severity::Error);
        assert!(Severity::Error > Severity::Warning);
        assert!(Severity::Warning > Severity::Info);
    }

    #[test]
    fn code_change_round_trips_json() {
        let change = CodeChange {
            file_path: "core/src/coordinator/engine.rs".to_string(),
            description: "fix stale task detection".to_string(),
            before_context: Some("let tasks = registry.tasks.clone();".to_string()),
            suggested_change: "add staleness check before dispatch".to_string(),
        };
        let json = serde_json::to_string(&change).expect("serialize");
        let back: CodeChange = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(change, back);
    }
}
