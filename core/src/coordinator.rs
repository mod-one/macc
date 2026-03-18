use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::BTreeMap;
use std::str::FromStr;

pub mod args;
pub mod commit_reconciler;
pub mod control_plane;
pub mod engine;
pub mod error_normalizer;
pub mod helpers;
pub mod normalizers;
pub mod ipc;
pub mod logs;
pub mod model;
pub mod prd_auditor;
pub mod rate_limit;
pub mod runtime;
pub mod session_manager;
pub mod state;
pub mod state_runtime;
pub mod task_selector;
pub mod types;

pub const COORDINATOR_TASK_REGISTRY_REL_PATH: &str = ".macc/automation/task/task_registry.json";
pub const COORDINATOR_PAUSE_FILE_REL_PATH: &str = ".macc/automation/task/coordinator.pause.json";

pub const COORDINATOR_EVENT_SCHEMA_VERSION: &str = "1";
pub const COORDINATOR_EVENT_TYPES_V1: &[&str] = &[
    "command_start",
    "command_end",
    "command_error",
    "task_transition",
    "task_dispatched",
    "performer_complete",
    "task_blocked",
    "dispatch_complete",
    "started",
    "progress",
    "phase_result",
    "commit_created",
    "review_done",
    "integrate_done",
    "failed",
    "heartbeat",
    "task_runtime_retry",
    "task_runtime_requeue",
    "task_runtime_stale",
    "phase_retry",
    "phase_skipped",
    "events_rotated",
    "events_compacted",
    "storage_sync",
    "storage_sync_ok",
    "storage_sync_failed",
    "storage_sync_latency_ms",
    "storage_mismatch_count",
    "task_phase_duration_seconds",
    "task_retries_total",
    "stale_runtime_total",
    "merge_fail_total",
    "merge_fix_attempt_total",
    "task_retry_count",
    "task_slo_warning",
    "task_runtime_orphan",
    "local_merge_failed",
    "merge_worker_started",
    "merge_worker_complete",
    "tool_error_classified",
];

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WorkflowState {
    Todo,
    Claimed,
    InProgress,
    PrOpen,
    ChangesRequested,
    Queued,
    Merged,
    Blocked,
    Abandoned,
}

impl WorkflowState {
    pub fn as_str(self) -> &'static str {
        match self {
            WorkflowState::Todo => "todo",
            WorkflowState::Claimed => "claimed",
            WorkflowState::InProgress => "in_progress",
            WorkflowState::PrOpen => "pr_open",
            WorkflowState::ChangesRequested => "changes_requested",
            WorkflowState::Queued => "queued",
            WorkflowState::Merged => "merged",
            WorkflowState::Blocked => "blocked",
            WorkflowState::Abandoned => "abandoned",
        }
    }
}

impl FromStr for WorkflowState {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.trim().to_ascii_lowercase().as_str() {
            "todo" => Ok(WorkflowState::Todo),
            "claimed" => Ok(WorkflowState::Claimed),
            "in_progress" => Ok(WorkflowState::InProgress),
            "pr_open" => Ok(WorkflowState::PrOpen),
            "changes_requested" => Ok(WorkflowState::ChangesRequested),
            "queued" => Ok(WorkflowState::Queued),
            "merged" => Ok(WorkflowState::Merged),
            "blocked" => Ok(WorkflowState::Blocked),
            "abandoned" => Ok(WorkflowState::Abandoned),
            other => Err(format!("unknown workflow state: {}", other)),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RuntimeStatus {
    Idle,
    Dispatched,
    Running,
    WaitingForUser,
    PhaseDone,
    Failed,
    Stale,
    Paused,
}

impl RuntimeStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            RuntimeStatus::Idle => "idle",
            RuntimeStatus::Dispatched => "dispatched",
            RuntimeStatus::Running => "running",
            RuntimeStatus::WaitingForUser => "waiting_for_user",
            RuntimeStatus::PhaseDone => "phase_done",
            RuntimeStatus::Failed => "failed",
            RuntimeStatus::Stale => "stale",
            RuntimeStatus::Paused => "paused",
        }
    }
}

impl FromStr for RuntimeStatus {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.trim().to_ascii_lowercase().as_str() {
            "idle" => Ok(RuntimeStatus::Idle),
            "dispatched" => Ok(RuntimeStatus::Dispatched),
            "running" => Ok(RuntimeStatus::Running),
            "waiting_for_user" => Ok(RuntimeStatus::WaitingForUser),
            "phase_done" => Ok(RuntimeStatus::PhaseDone),
            "failed" => Ok(RuntimeStatus::Failed),
            "stale" => Ok(RuntimeStatus::Stale),
            "paused" => Ok(RuntimeStatus::Paused),
            other => Err(format!("unknown runtime status: {}", other)),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PerformerCompletionKind {
    SuccessWithChanges,
    SuccessWithoutChanges,
    AlreadySatisfied,
}

impl PerformerCompletionKind {
    pub fn as_str(self) -> &'static str {
        match self {
            PerformerCompletionKind::SuccessWithChanges => "success_with_changes",
            PerformerCompletionKind::SuccessWithoutChanges => "success_without_changes",
            PerformerCompletionKind::AlreadySatisfied => "already_satisfied",
        }
    }
}

impl FromStr for PerformerCompletionKind {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.trim().to_ascii_lowercase().as_str() {
            "success_with_changes" => Ok(PerformerCompletionKind::SuccessWithChanges),
            "success_without_changes" => Ok(PerformerCompletionKind::SuccessWithoutChanges),
            "already_satisfied" | "already_done" | "noop_success" => {
                Ok(PerformerCompletionKind::AlreadySatisfied)
            }
            other => Err(format!("unknown performer completion kind: {}", other)),
        }
    }
}

pub fn is_valid_workflow_transition(from: WorkflowState, to: WorkflowState) -> bool {
    matches!(
        (from, to),
        (WorkflowState::Todo, WorkflowState::Claimed)
            | (WorkflowState::Claimed, WorkflowState::InProgress)
            | (WorkflowState::Claimed, WorkflowState::Blocked)
            | (WorkflowState::Claimed, WorkflowState::Abandoned)
            | (WorkflowState::InProgress, WorkflowState::PrOpen)
            | (WorkflowState::InProgress, WorkflowState::ChangesRequested)
            | (WorkflowState::InProgress, WorkflowState::Blocked)
            | (WorkflowState::InProgress, WorkflowState::Abandoned)
            | (WorkflowState::PrOpen, WorkflowState::ChangesRequested)
            | (WorkflowState::PrOpen, WorkflowState::Queued)
            | (WorkflowState::PrOpen, WorkflowState::Blocked)
            | (WorkflowState::PrOpen, WorkflowState::Abandoned)
            | (WorkflowState::ChangesRequested, WorkflowState::PrOpen)
            | (WorkflowState::ChangesRequested, WorkflowState::Blocked)
            | (WorkflowState::ChangesRequested, WorkflowState::Abandoned)
            | (WorkflowState::Queued, WorkflowState::Merged)
            | (WorkflowState::Queued, WorkflowState::PrOpen)
            | (WorkflowState::Queued, WorkflowState::Blocked)
            | (WorkflowState::Queued, WorkflowState::Abandoned)
            | (WorkflowState::Blocked, WorkflowState::Todo)
            | (WorkflowState::Blocked, WorkflowState::Claimed)
            | (WorkflowState::Blocked, WorkflowState::InProgress)
            | (WorkflowState::Blocked, WorkflowState::PrOpen)
            | (WorkflowState::Blocked, WorkflowState::ChangesRequested)
            | (WorkflowState::Blocked, WorkflowState::Queued)
            | (WorkflowState::Blocked, WorkflowState::Abandoned)
            | (WorkflowState::Abandoned, WorkflowState::Todo)
    )
}

pub fn is_valid_runtime_transition(from: RuntimeStatus, to: RuntimeStatus) -> bool {
    matches!(
        (from, to),
        (RuntimeStatus::Idle, RuntimeStatus::Dispatched)
            | (RuntimeStatus::Idle, RuntimeStatus::Running)
            | (RuntimeStatus::Dispatched, RuntimeStatus::Running)
            | (RuntimeStatus::Dispatched, RuntimeStatus::Failed)
            | (RuntimeStatus::Dispatched, RuntimeStatus::Stale)
            | (RuntimeStatus::Running, RuntimeStatus::PhaseDone)
            | (RuntimeStatus::Running, RuntimeStatus::Failed)
            | (RuntimeStatus::Running, RuntimeStatus::Stale)
            | (RuntimeStatus::Running, RuntimeStatus::Paused)
            | (RuntimeStatus::Running, RuntimeStatus::WaitingForUser)
            | (RuntimeStatus::WaitingForUser, RuntimeStatus::Running)
            | (RuntimeStatus::WaitingForUser, RuntimeStatus::Failed)
            | (RuntimeStatus::WaitingForUser, RuntimeStatus::Paused)
            | (RuntimeStatus::WaitingForUser, RuntimeStatus::Idle)
            | (RuntimeStatus::PhaseDone, RuntimeStatus::Running)
            | (RuntimeStatus::PhaseDone, RuntimeStatus::Idle)
            | (RuntimeStatus::PhaseDone, RuntimeStatus::Failed)
            | (RuntimeStatus::Failed, RuntimeStatus::Dispatched)
            | (RuntimeStatus::Failed, RuntimeStatus::Paused)
            | (RuntimeStatus::Failed, RuntimeStatus::Idle)
            | (RuntimeStatus::Stale, RuntimeStatus::Dispatched)
            | (RuntimeStatus::Stale, RuntimeStatus::Failed)
            | (RuntimeStatus::Stale, RuntimeStatus::Paused)
            | (RuntimeStatus::Paused, RuntimeStatus::Dispatched)
            | (RuntimeStatus::Paused, RuntimeStatus::Running)
            | (RuntimeStatus::Paused, RuntimeStatus::Failed)
            | (RuntimeStatus::Paused, RuntimeStatus::Idle)
    )
}

pub fn runtime_status_from_event(event_type: &str, status: &str) -> RuntimeStatus {
    let status_norm = status.trim().to_ascii_lowercase();
    let event_norm = event_type.trim().to_ascii_lowercase();
    match status_norm.as_str() {
        "started" | "dispatched" => RuntimeStatus::Dispatched,
        "running" | "progress" | "heartbeat" => RuntimeStatus::Running,
        "waiting_for_user" | "input_required" => RuntimeStatus::WaitingForUser,
        "done" | "phase_done" | "already_satisfied" | "success_without_changes" => {
            RuntimeStatus::PhaseDone
        }
        "failed" | "error" => RuntimeStatus::Failed,
        "stale" => RuntimeStatus::Stale,
        "paused" => RuntimeStatus::Paused,
        _ => match event_norm.as_str() {
            "started" => RuntimeStatus::Dispatched,
            "progress" | "heartbeat" => RuntimeStatus::Running,
            "input_required" => RuntimeStatus::WaitingForUser,
            "phase_result" => RuntimeStatus::Running,
            "failed" => RuntimeStatus::Failed,
            _ => RuntimeStatus::Running,
        },
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq)]
pub struct CoordinatorEventRecord {
    #[serde(default = "default_event_schema_version")]
    pub schema_version: String,
    #[serde(default)]
    pub event_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub run_id: Option<String>,
    #[serde(default)]
    pub seq: i64,
    #[serde(default)]
    pub ts: String,
    #[serde(default)]
    pub source: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub task_id: Option<String>,
    #[serde(rename = "type", default)]
    pub event_type: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub phase: Option<String>,
    #[serde(default)]
    pub status: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub msg: Option<String>,
    #[serde(default)]
    pub payload: Value,
    #[serde(flatten)]
    pub extra: BTreeMap<String, Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq)]
pub struct CoordinatorCursor {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub path: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub inode: Option<i64>,
    #[serde(default)]
    pub offset: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_event_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub updated_at: Option<String>,
    #[serde(flatten)]
    pub extra: BTreeMap<String, Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq)]
#[serde(transparent)]
pub struct CoordinatorEventPayload(pub Value);

#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq)]
pub struct CoordinatorProgressPayload {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub code: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub origin: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub attempt: Option<i64>,
    #[serde(flatten)]
    pub extra: BTreeMap<String, Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq)]
pub struct CoordinatorPhaseResultPayload {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub code: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error_code: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub origin: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub attempt: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub result_kind: Option<PerformerCompletionKind>,
    #[serde(flatten)]
    pub extra: BTreeMap<String, Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq)]
pub struct CoordinatorFailedPayload {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub code: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error_code: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub origin: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub attempt: Option<i64>,
    #[serde(flatten)]
    pub extra: BTreeMap<String, Value>,
}

fn default_event_schema_version() -> String {
    COORDINATOR_EVENT_SCHEMA_VERSION.to_string()
}

impl CoordinatorEventRecord {
    pub fn from_value(raw: Value) -> Result<Self, String> {
        serde_json::from_value(raw)
            .map_err(|e| format!("failed to parse coordinator event record: {}", e))
    }

    pub fn to_value(&self) -> Result<Value, String> {
        serde_json::to_value(self)
            .map_err(|e| format!("failed to serialize coordinator event record: {}", e))
    }

    pub fn severity(&self) -> Option<&str> {
        self.extra.get("severity").and_then(Value::as_str)
    }

    pub fn is_performer_runtime_event(&self) -> bool {
        matches!(
            self.event_type.as_str(),
            "started" | "heartbeat" | "progress" | "phase_result" | "failed" | "commit_created"
        ) && matches!(
            self.source.as_str(),
            source if source.starts_with("coordinator-worktree:")
                || source.starts_with("worktree-run:")
                || source.starts_with("performer:")
        )
    }

    pub fn validate_performer_runtime_event(&self) -> Result<(), String> {
        if !self.is_performer_runtime_event() {
            return Ok(());
        }
        if self.schema_version != COORDINATOR_EVENT_SCHEMA_VERSION {
            return Err(format!(
                "invalid performer event schema_version '{}'",
                self.schema_version
            ));
        }
        if self.event_id.trim().is_empty() {
            return Err("invalid performer event: missing event_id".to_string());
        }
        if self.ts.trim().is_empty() {
            return Err("invalid performer event: missing ts".to_string());
        }
        if self.source.trim().is_empty() {
            return Err("invalid performer event: missing source".to_string());
        }
        if self
            .task_id
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .is_none()
        {
            return Err("invalid performer event: missing task_id".to_string());
        }
        let normalized_payload = self.normalized_payload();
        if !normalized_payload.is_object() {
            return Err(format!(
                "invalid performer event '{}': payload must be an object (got {:?})",
                self.event_type, normalized_payload
            ));
        }
        match self.event_type.as_str() {
            "started" => {
                if !matches!(self.status.as_str(), "started" | "running") {
                    return Err(format!(
                        "invalid performer event 'started': unexpected status '{}'",
                        self.status
                    ));
                }
                let has_tool =
                    payload_has_non_empty_string_in_sources(self, &normalized_payload, "tool");
                let has_worktree =
                    payload_has_non_empty_string_in_sources(self, &normalized_payload, "worktree");
                if !has_tool || !has_worktree {
                    return Err(format!(
                        "invalid performer event 'started': payload.tool and payload.worktree are required. has_tool={} has_worktree={} payload={:?}",
                        has_tool, has_worktree, normalized_payload
                    ));
                }
            }
            "heartbeat" | "progress" => {
                if self.status != "running" {
                    return Err(format!(
                        "invalid performer event '{}': unexpected status '{}'",
                        self.event_type, self.status
                    ));
                }
            }
            "phase_result" => {
                if self
                    .phase
                    .as_deref()
                    .map(str::trim)
                    .filter(|value| !value.is_empty())
                    .is_none()
                {
                    return Err("invalid performer event 'phase_result': missing phase".to_string());
                }
                if !matches!(self.status.as_str(), "done" | "failed") {
                    return Err(format!(
                        "invalid performer event 'phase_result': unexpected status '{}'",
                        self.status
                    ));
                }
                let kind = self.payload_result_kind();
                if self.status == "done" && kind.is_none() {
                    let norm = self.normalized_payload();
                    return Err(format!(
                        "invalid performer event 'phase_result': payload.result_kind is required for successful terminal events. status={} payload={:?} normalized={:?}",
                        self.status, self.payload, norm
                    ));
                }
            }
            "commit_created" => {
                if self.status != "done"
                    || !payload_has_non_empty_string_in_sources(self, &normalized_payload, "sha")
                {
                    return Err(
                        "invalid performer event 'commit_created': status=done and payload.sha are required"
                            .to_string(),
                    );
                }
            }
            "failed" => {}
            _ => {}
        }
        Ok(())
    }

    fn parse_payload<T>(&self) -> Option<T>
    where
        T: for<'de> Deserialize<'de>,
    {
        let payload = self.normalized_payload();
        serde_json::from_value(payload).ok()
    }

    pub fn progress_payload(&self) -> Option<CoordinatorProgressPayload> {
        (self.event_type == "progress")
            .then(|| self.parse_payload())
            .flatten()
    }

    pub fn phase_result_payload(&self) -> Option<CoordinatorPhaseResultPayload> {
        (self.event_type == "phase_result")
            .then(|| self.parse_payload())
            .flatten()
    }

    pub fn failed_payload(&self) -> Option<CoordinatorFailedPayload> {
        (self.event_type == "failed")
            .then(|| self.parse_payload())
            .flatten()
    }

    pub fn payload_attempt(&self) -> Option<i64> {
        self.phase_result_payload()
            .and_then(|payload| payload.attempt)
            .or_else(|| self.failed_payload().and_then(|payload| payload.attempt))
            .or_else(|| self.progress_payload().and_then(|payload| payload.attempt))
            .or_else(|| self.payload.get("attempt").and_then(Value::as_i64))
            .or_else(|| self.extra.get("attempt").and_then(Value::as_i64))
    }

    pub fn payload_error_code(&self) -> Option<String> {
        self.failed_payload()
            .and_then(|payload| payload.error_code.or(payload.code))
            .or_else(|| {
                self.phase_result_payload()
                    .and_then(|payload| payload.error_code.or(payload.code))
            })
            .or_else(|| {
                self.payload
                    .get("error_code")
                    .and_then(Value::as_str)
                    .map(ToString::to_string)
            })
            .or_else(|| {
                self.extra
                    .get("error_code")
                    .and_then(Value::as_str)
                    .map(ToString::to_string)
            })
            .or_else(|| {
                self.payload
                    .get("code")
                    .and_then(Value::as_str)
                    .map(ToString::to_string)
            })
            .or_else(|| {
                self.extra
                    .get("code")
                    .and_then(Value::as_str)
                    .map(ToString::to_string)
            })
    }

    pub fn payload_origin(&self) -> Option<String> {
        self.failed_payload()
            .and_then(|payload| payload.origin)
            .or_else(|| {
                self.phase_result_payload()
                    .and_then(|payload| payload.origin)
            })
            .or_else(|| self.progress_payload().and_then(|payload| payload.origin))
            .or_else(|| {
                self.payload
                    .get("origin")
                    .and_then(Value::as_str)
                    .map(ToString::to_string)
            })
            .or_else(|| {
                self.extra
                    .get("origin")
                    .and_then(Value::as_str)
                    .map(ToString::to_string)
            })
    }

    pub fn payload_result_kind(&self) -> Option<PerformerCompletionKind> {
        let norm = self.normalized_payload();
        self.phase_result_payload()
            .and_then(|payload| payload.result_kind)
            .or_else(|| {
                norm.get("result_kind")
                    .and_then(Value::as_str)
                    .and_then(|value| PerformerCompletionKind::from_str(value).ok())
            })
            .or_else(|| {
                self.extra
                    .get("result_kind")
                    .and_then(Value::as_str)
                    .and_then(|value| PerformerCompletionKind::from_str(value).ok())
            })
    }

    pub fn is_terminal_success(&self) -> bool {
        self.event_type == "commit_created"
            || (self.event_type == "phase_result"
                && matches!(
                    self.status.as_str(),
                    "done" | "phase_done" | "already_satisfied" | "success_without_changes"
                )
                && self.payload_result_kind().is_some())
    }

    pub fn message(&self) -> Option<&str> {
        self.detail
            .as_deref()
            .or(self.msg.as_deref())
            .or_else(|| self.payload.get("reason").and_then(Value::as_str))
            .or_else(|| self.payload.get("message").and_then(Value::as_str))
            .or_else(|| self.payload.get("error").and_then(Value::as_str))
            .or_else(|| self.extra.get("reason").and_then(Value::as_str))
            .or_else(|| self.extra.get("message").and_then(Value::as_str))
            .or_else(|| self.extra.get("error").and_then(Value::as_str))
    }

    pub fn normalized_payload(&self) -> Value {
        if let Some(val) = self.extra.get("payload") {
            if val.is_object() {
                return val.clone();
            } else if let Some(s) = val.as_str() {
                if let Ok(parsed) = serde_json::from_str::<Value>(s) {
                    if parsed.is_object() {
                        return parsed;
                    }
                }
            }
        }
        if self.payload.is_object() {
            if let Some(val_str) = self.payload.get("value").and_then(Value::as_str) {
                if let Ok(parsed) = serde_json::from_str::<Value>(val_str) {
                    if parsed.is_object() {
                        return parsed;
                    }
                }
            }
            return self.payload.clone();
        }
        if let Some(raw) = self.payload.as_str() {
            if let Ok(parsed) = serde_json::from_str::<Value>(raw) {
                if parsed.is_object() {
                    return parsed;
                }
            }
        }
        serde_json::json!({})
    }
}

fn payload_has_non_empty_string(payload: &Value, key: &str) -> bool {
    payload
        .get(key)
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .is_some()
}

fn payload_has_non_empty_string_in_sources(
    event: &CoordinatorEventRecord,
    payload: &Value,
    key: &str,
) -> bool {
    payload_has_non_empty_string(payload, key)
        || event
            .extra
            .get(key)
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .is_some()
}

impl CoordinatorEventPayload {
    pub fn as_value(&self) -> &Value {
        &self.0
    }

    pub fn into_value(self) -> Value {
        self.0
    }
}

impl From<Value> for CoordinatorEventPayload {
    fn from(value: Value) -> Self {
        Self(value)
    }
}

impl From<CoordinatorProgressPayload> for CoordinatorEventPayload {
    fn from(value: CoordinatorProgressPayload) -> Self {
        Self(serde_json::to_value(value).unwrap_or_else(|_| serde_json::json!({})))
    }
}

impl From<CoordinatorPhaseResultPayload> for CoordinatorEventPayload {
    fn from(value: CoordinatorPhaseResultPayload) -> Self {
        Self(serde_json::to_value(value).unwrap_or_else(|_| serde_json::json!({})))
    }
}

impl From<CoordinatorFailedPayload> for CoordinatorEventPayload {
    fn from(value: CoordinatorFailedPayload) -> Self {
        Self(serde_json::to_value(value).unwrap_or_else(|_| serde_json::json!({})))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeSet;

    #[test]
    fn workflow_transition_table_has_expected_edges() {
        assert!(is_valid_workflow_transition(
            WorkflowState::Todo,
            WorkflowState::Claimed
        ));
        assert!(is_valid_workflow_transition(
            WorkflowState::Queued,
            WorkflowState::Merged
        ));
        assert!(!is_valid_workflow_transition(
            WorkflowState::Todo,
            WorkflowState::Merged
        ));
    }

    #[test]
    fn runtime_transition_table_has_expected_edges() {
        assert!(is_valid_runtime_transition(
            RuntimeStatus::Idle,
            RuntimeStatus::Dispatched
        ));
        assert!(is_valid_runtime_transition(
            RuntimeStatus::Running,
            RuntimeStatus::PhaseDone
        ));
        assert!(is_valid_runtime_transition(
            RuntimeStatus::Failed,
            RuntimeStatus::Dispatched
        ));
        assert!(!is_valid_runtime_transition(
            RuntimeStatus::Idle,
            RuntimeStatus::PhaseDone
        ));
    }

    #[test]
    fn runtime_status_parsing_roundtrips() {
        let status = "phase_done".parse::<RuntimeStatus>().unwrap();
        assert_eq!(status, RuntimeStatus::PhaseDone);
        assert_eq!(status.as_str(), "phase_done");
    }

    #[test]
    fn performer_completion_kind_parsing_roundtrips() {
        let kind = "already_satisfied"
            .parse::<PerformerCompletionKind>()
            .unwrap();
        assert_eq!(kind, PerformerCompletionKind::AlreadySatisfied);
        assert_eq!(kind.as_str(), "already_satisfied");
        assert_eq!(
            "noop_success".parse::<PerformerCompletionKind>().unwrap(),
            PerformerCompletionKind::AlreadySatisfied
        );
    }

    #[test]
    fn runtime_status_from_event_maps_stable_values() {
        assert_eq!(
            runtime_status_from_event("heartbeat", "running"),
            RuntimeStatus::Running
        );
        assert_eq!(
            runtime_status_from_event("input_required", "waiting_for_user"),
            RuntimeStatus::WaitingForUser
        );
        assert_eq!(
            runtime_status_from_event("phase_result", "already_satisfied"),
            RuntimeStatus::PhaseDone
        );
        assert_eq!(
            runtime_status_from_event("phase_result", "phase_done"),
            RuntimeStatus::PhaseDone
        );
        assert_eq!(
            runtime_status_from_event("failed", "error"),
            RuntimeStatus::Failed
        );
        assert_eq!(
            runtime_status_from_event("unknown", ""),
            RuntimeStatus::Running
        );
    }

    #[test]
    fn workflow_state_parsing_roundtrips() {
        let state = "in_progress".parse::<WorkflowState>().unwrap();
        assert_eq!(state, WorkflowState::InProgress);
        assert_eq!(state.as_str(), "in_progress");
    }

    #[test]
    fn event_schema_matches_core_event_types() {
        let schema_path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../docs/schemas/coordinator-event.v1.schema.json");
        let schema_raw = std::fs::read_to_string(&schema_path).expect("read schema");
        let schema: serde_json::Value = serde_json::from_str(&schema_raw).expect("parse schema");

        let schema_version = schema
            .get("properties")
            .and_then(|p| p.get("schema_version"))
            .and_then(|s| s.get("const"))
            .and_then(|v| v.as_str())
            .expect("schema_version const");
        assert_eq!(schema_version, COORDINATOR_EVENT_SCHEMA_VERSION);

        let schema_types: BTreeSet<String> = schema
            .get("properties")
            .and_then(|p| p.get("type"))
            .and_then(|t| t.get("enum"))
            .and_then(|e| e.as_array())
            .expect("type enum")
            .iter()
            .filter_map(|v| v.as_str())
            .map(|s| s.to_string())
            .collect();
        let core_types: BTreeSet<String> = COORDINATOR_EVENT_TYPES_V1
            .iter()
            .map(|s| s.to_string())
            .collect();
        assert_eq!(schema_types, core_types);
    }

    #[test]
    fn phase_result_payload_exposes_completion_kind() {
        let event = CoordinatorEventRecord {
            schema_version: COORDINATOR_EVENT_SCHEMA_VERSION.to_string(),
            event_id: "evt-1".to_string(),
            run_id: Some("run-1".to_string()),
            seq: 1,
            ts: "2026-03-15T00:00:00Z".to_string(),
            source: "performer:test".to_string(),
            task_id: Some("TASK-1".to_string()),
            event_type: "phase_result".to_string(),
            phase: Some("dev".to_string()),
            status: "done".to_string(),
            payload: serde_json::json!({
                "message": "Task already satisfied",
                "result_kind": "already_satisfied"
            }),
            detail: None,
            msg: None,
            extra: BTreeMap::new(),
        };
        assert_eq!(
            event.payload_result_kind(),
            Some(PerformerCompletionKind::AlreadySatisfied)
        );
    }

    #[test]
    fn performer_phase_result_event_schema_requires_result_kind() {
        let event = CoordinatorEventRecord {
            schema_version: COORDINATOR_EVENT_SCHEMA_VERSION.to_string(),
            event_id: "evt-1".to_string(),
            ts: "2026-03-15T00:00:00Z".to_string(),
            source: "coordinator-worktree:T1:1".to_string(),
            task_id: Some("T1".to_string()),
            event_type: "phase_result".to_string(),
            phase: Some("dev".to_string()),
            status: "done".to_string(),
            payload: serde_json::json!({"attempt": 1}),
            ..CoordinatorEventRecord::default()
        };
        assert!(event
            .validate_performer_runtime_event()
            .expect_err("missing result_kind should fail")
            .contains("payload.result_kind"));
    }

    #[test]
    fn performer_phase_result_event_schema_accepts_success_without_attempt() {
        let event = CoordinatorEventRecord {
            schema_version: COORDINATOR_EVENT_SCHEMA_VERSION.to_string(),
            event_id: "evt-2".to_string(),
            ts: "2026-03-15T00:00:00Z".to_string(),
            source: "coordinator-worktree:T1:1".to_string(),
            task_id: Some("T1".to_string()),
            event_type: "phase_result".to_string(),
            phase: Some("dev".to_string()),
            status: "done".to_string(),
            payload: serde_json::json!({
                "result_kind": "already_satisfied",
                "message": "Task already satisfied"
            }),
            ..CoordinatorEventRecord::default()
        };
        event
            .validate_performer_runtime_event()
            .expect("successful phase_result without attempt should be accepted");
    }

    #[test]
    fn performer_phase_result_with_attempt_is_terminal_success() {
        let event = CoordinatorEventRecord {
            schema_version: COORDINATOR_EVENT_SCHEMA_VERSION.to_string(),
            event_id: "evt-3".to_string(),
            ts: "2026-03-15T00:00:00Z".to_string(),
            source: "coordinator-worktree:T1:1".to_string(),
            task_id: Some("T1".to_string()),
            event_type: "phase_result".to_string(),
            phase: Some("dev".to_string()),
            status: "done".to_string(),
            payload: serde_json::json!({
                "attempt": 1,
                "result_kind": "already_satisfied",
                "message": "Task already satisfied"
            }),
            ..CoordinatorEventRecord::default()
        };
        assert!(event.is_terminal_success());
    }

    #[test]
    fn performer_started_event_schema_is_accepted() {
        let event = CoordinatorEventRecord {
            schema_version: COORDINATOR_EVENT_SCHEMA_VERSION.to_string(),
            event_id: "evt-4".to_string(),
            ts: "2026-03-15T00:00:00Z".to_string(),
            source: "coordinator-worktree:T1:1".to_string(),
            task_id: Some("T1".to_string()),
            event_type: "started".to_string(),
            phase: Some("dev".to_string()),
            status: "started".to_string(),
            payload: serde_json::json!({
                "tool": "codex",
                "worktree": "/tmp/worktree"
            }),
            ..CoordinatorEventRecord::default()
        };
        event
            .validate_performer_runtime_event()
            .expect("valid started event");
    }
}
