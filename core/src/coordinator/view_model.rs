use crate::coordinator::model::Task;
use crate::coordinator::{RuntimeStatus, WorkflowState};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::str::FromStr;
use std::time::Duration;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TaskHealth {
    Healthy,
    Waiting,
    Warning,
    Stale,
    Completed,
    Idle,
}

impl TaskHealth {
    pub fn symbol(self) -> &'static str {
        match self {
            TaskHealth::Healthy => "●",
            TaskHealth::Waiting => "◐",
            TaskHealth::Warning => "▲",
            TaskHealth::Stale => "!",
            TaskHealth::Completed => "✓",
            TaskHealth::Idle => "·",
        }
    }

    pub fn compute(last_error: &str, runtime_status: &str, last_error_code: &str) -> Self {
        if !last_error.is_empty() || runtime_status == "failed" {
            TaskHealth::Warning
        } else if last_error_code.starts_with("E601") || runtime_status == "stale" {
            TaskHealth::Stale
        } else if matches!(
            runtime_status,
            "waiting" | "waiting_for_user" | "phase_done" | "paused"
        ) {
            TaskHealth::Waiting
        } else if runtime_status == "completed" {
            TaskHealth::Completed
        } else if runtime_status.is_empty() || runtime_status == "-" || runtime_status == "idle" {
            TaskHealth::Idle
        } else {
            TaskHealth::Healthy
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(from = "String", into = "String")]
pub enum TaskPhase {
    ReadingContext,
    Planning,
    Implementing,
    Editing,
    Testing,
    Fixing,
    Reviewing,
    Committing,
    OpeningPr,
    WaitingCi,
    Merging,
    Cleanup,
    Other(String),
}

impl From<String> for TaskPhase {
    fn from(s: String) -> Self {
        match s.as_str() {
            "reading_context" | "reading-context" => TaskPhase::ReadingContext,
            "planning" => TaskPhase::Planning,
            "implementing" | "implementation" => TaskPhase::Implementing,
            "editing" => TaskPhase::Editing,
            "testing" => TaskPhase::Testing,
            "fixing" => TaskPhase::Fixing,
            "reviewing" => TaskPhase::Reviewing,
            "committing" => TaskPhase::Committing,
            "opening_pr" | "opening-pr" | "opening_pull_request" => TaskPhase::OpeningPr,
            "waiting_ci" | "waiting-ci" | "waiting_for_ci" => TaskPhase::WaitingCi,
            "merging" => TaskPhase::Merging,
            "cleanup" => TaskPhase::Cleanup,
            _ => TaskPhase::Other(s),
        }
    }
}

impl From<TaskPhase> for String {
    fn from(phase: TaskPhase) -> Self {
        match phase {
            TaskPhase::ReadingContext => "reading_context".to_string(),
            TaskPhase::Planning => "planning".to_string(),
            TaskPhase::Implementing => "implementing".to_string(),
            TaskPhase::Editing => "editing".to_string(),
            TaskPhase::Testing => "testing".to_string(),
            TaskPhase::Fixing => "fixing".to_string(),
            TaskPhase::Reviewing => "reviewing".to_string(),
            TaskPhase::Committing => "committing".to_string(),
            TaskPhase::OpeningPr => "opening_pr".to_string(),
            TaskPhase::WaitingCi => "waiting_ci".to_string(),
            TaskPhase::Merging => "merging".to_string(),
            TaskPhase::Cleanup => "cleanup".to_string(),
            TaskPhase::Other(s) => s,
        }
    }
}

impl TaskPhase {
    pub fn compact_label(&self) -> &str {
        match self {
            TaskPhase::ReadingContext => "ctx",
            TaskPhase::Planning => "plan",
            TaskPhase::Implementing => "dev",
            TaskPhase::Editing => "edit",
            TaskPhase::Testing => "test",
            TaskPhase::Fixing => "fix",
            TaskPhase::Reviewing => "review",
            TaskPhase::Committing => "commit",
            TaskPhase::OpeningPr => "pr",
            TaskPhase::WaitingCi => "ci",
            TaskPhase::Merging => "merge",
            TaskPhase::Cleanup => "clean",
            TaskPhase::Other(s) => s.as_str(),
        }
    }
}

pub type TaskState = WorkflowState;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct LiveTaskRow {
    pub health: TaskHealth,
    pub worker_id: String,
    pub task_id: String,
    pub workflow_state: TaskState,
    pub runtime_status: RuntimeStatus,
    pub phase: TaskPhase,
    pub tool: String,
    pub model: String,
    #[serde(with = "serde_duration_secs")]
    pub age: Duration,
    #[serde(with = "serde_opt_duration_secs")]
    pub heartbeat_age: Option<Duration>,
    #[serde(with = "serde_opt_duration_secs")]
    pub last_event_age: Option<Duration>,
    pub current_message: Option<String>,
    pub worktree: Option<PathBuf>,
    pub branch: Option<String>,
    pub last_error_code: Option<String>,
    pub last_error: Option<String>,
}

impl LiveTaskRow {
    pub fn from_task(task: &Task, now: DateTime<Utc>, model: String) -> Self {
        let last_error = task.task_runtime.last_error.as_deref().unwrap_or("");
        let runtime_status_str = task.task_runtime.status.as_deref().unwrap_or("");
        let last_error_code_str = task.task_runtime.last_error_code.as_deref().unwrap_or("");

        let health = TaskHealth::compute(last_error, runtime_status_str, last_error_code_str);

        let workflow_state = WorkflowState::from_str(&task.state).unwrap_or(WorkflowState::Todo);
        let runtime_status = task
            .task_runtime
            .status
            .as_deref()
            .map(|s| match s {
                "starting" | "dispatched" => RuntimeStatus::Dispatched,
                "running" => RuntimeStatus::Running,
                "waiting" | "waiting_for_user" => RuntimeStatus::WaitingForUser,
                "phase_done" | "completed" => RuntimeStatus::PhaseDone,
                "failed" => RuntimeStatus::Failed,
                "stale" => RuntimeStatus::Stale,
                "paused" => RuntimeStatus::Paused,
                _ => RuntimeStatus::Idle,
            })
            .unwrap_or(RuntimeStatus::Idle);

        let phase = task
            .task_runtime
            .current_phase
            .clone()
            .map(TaskPhase::from)
            .unwrap_or(TaskPhase::Other(String::new()));

        let mut worker_id = task.task_runtime.worker_id.clone().unwrap_or_default();
        let task_id = task.id.clone();
        let tool = task.tool.clone().unwrap_or_else(|| "-".to_string());

        let age = parse_age_to_duration(task.task_runtime.started_at.as_deref(), now)
            .unwrap_or(Duration::from_secs(0));

        let heartbeat_age = parse_age_to_duration(task.task_runtime.last_heartbeat.as_deref(), now);
        let last_event_age = parse_age_to_duration(task.task_runtime.last_event_at.as_deref(), now);

        let current_message = task.task_runtime.message.clone().filter(|m| !m.is_empty());

        let worktree = task
            .worktree
            .as_ref()
            .and_then(|w| w.worktree_path.clone())
            .filter(|p| !p.is_empty() && p != "-")
            .map(PathBuf::from);

        let branch = task
            .worktree
            .as_ref()
            .and_then(|w| w.branch.clone())
            .filter(|b| !b.is_empty() && b != "-");

        let last_error_code = task
            .task_runtime
            .last_error_code
            .clone()
            .filter(|c| !c.is_empty());
        let last_error = task
            .task_runtime
            .last_error
            .clone()
            .filter(|e| !e.is_empty());

        // Worker fallback: resolve from worktree directory name if worker_id is empty
        if worker_id.is_empty() {
            if let Some(ref path) = worktree {
                if let Some(name) = path.file_name().and_then(|n| n.to_str()) {
                    if name.starts_with("worker-") {
                        worker_id = name.to_string();
                    }
                }
            }
        }

        Self {
            health,
            worker_id,
            task_id,
            workflow_state,
            runtime_status,
            phase,
            tool,
            model,
            age,
            heartbeat_age,
            last_event_age,
            current_message,
            worktree,
            branch,
            last_error_code,
            last_error,
        }
    }

    pub fn format_duration(d: Duration) -> String {
        let secs = d.as_secs();
        if secs < 60 {
            format!("{}s", secs)
        } else if secs < 3600 {
            format!("{}m", secs / 60)
        } else {
            format!("{}h", secs / 3600)
        }
    }

    pub fn age_label(&self) -> String {
        Self::format_duration(self.age)
    }

    pub fn heartbeat_age_label(&self) -> String {
        self.heartbeat_age
            .map(Self::format_duration)
            .unwrap_or_default()
    }

    pub fn status_label(&self) -> String {
        if let Some(ref code) = self.last_error_code {
            if code.starts_with("E601") {
                return "RATE".to_string();
            }
        }
        match self.runtime_status {
            RuntimeStatus::Running | RuntimeStatus::Dispatched => "RUN".to_string(),
            RuntimeStatus::WaitingForUser => "WAIT".to_string(),
            RuntimeStatus::Stale => "STALE".to_string(),
            RuntimeStatus::Failed => "ERR".to_string(),
            RuntimeStatus::PhaseDone => "DONE".to_string(),
            RuntimeStatus::Paused => "PAUSED".to_string(),
            RuntimeStatus::Idle => "IDLE".to_string(),
        }
    }
}

fn parse_age_to_duration(iso_str: Option<&str>, now: DateTime<Utc>) -> Option<Duration> {
    let s = iso_str?;
    if s.is_empty() || s == "-" {
        return None;
    }
    let dt = chrono::DateTime::parse_from_rfc3339(s).ok()?;
    let diff = now.signed_duration_since(dt.with_timezone(&Utc));
    let secs = diff.num_seconds().max(0) as u64;
    Some(Duration::from_secs(secs))
}

pub mod serde_duration_secs {
    use serde::{self, Deserialize, Deserializer, Serializer};
    use std::time::Duration;

    pub fn serialize<S>(d: &Duration, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_u64(d.as_secs())
    }

    pub fn deserialize<'de, D>(deserializer: D) -> Result<Duration, D::Error>
    where
        D: Deserializer<'de>,
    {
        let secs = u64::deserialize(deserializer)?;
        Ok(Duration::from_secs(secs))
    }
}

pub mod serde_opt_duration_secs {
    use serde::{self, Deserialize, Deserializer, Serializer};
    use std::time::Duration;

    pub fn serialize<S>(opt: &Option<Duration>, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        match opt {
            Some(d) => serializer.serialize_some(&d.as_secs()),
            None => serializer.serialize_none(),
        }
    }

    pub fn deserialize<'de, D>(deserializer: D) -> Result<Option<Duration>, D::Error>
    where
        D: Deserializer<'de>,
    {
        let opt_secs: Option<u64> = Option::deserialize(deserializer)?;
        Ok(opt_secs.map(Duration::from_secs))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::coordinator::model::{Task, TaskRuntime, TaskWorktree};

    #[test]
    fn test_task_health_compute() {
        assert_eq!(TaskHealth::compute("", "failed", ""), TaskHealth::Warning);
        assert_eq!(
            TaskHealth::compute("err", "running", ""),
            TaskHealth::Warning
        );
        assert_eq!(
            TaskHealth::compute("", "running", "E601"),
            TaskHealth::Stale
        );
        assert_eq!(TaskHealth::compute("", "stale", ""), TaskHealth::Stale);
        assert_eq!(TaskHealth::compute("", "waiting", ""), TaskHealth::Waiting);
        assert_eq!(TaskHealth::compute("", "paused", ""), TaskHealth::Waiting);
        assert_eq!(
            TaskHealth::compute("", "completed", ""),
            TaskHealth::Completed
        );
        assert_eq!(TaskHealth::compute("", "idle", ""), TaskHealth::Idle);
        assert_eq!(TaskHealth::compute("", "running", ""), TaskHealth::Healthy);
    }

    #[test]
    fn test_task_phase_mapping() {
        let phase = TaskPhase::from("reading_context".to_string());
        assert_eq!(phase, TaskPhase::ReadingContext);
        assert_eq!(phase.compact_label(), "ctx");

        let custom = TaskPhase::from("my_custom_phase".to_string());
        assert_eq!(custom, TaskPhase::Other("my_custom_phase".to_string()));
        assert_eq!(custom.compact_label(), "my_custom_phase");
    }

    #[test]
    fn test_live_task_row_conversion() {
        let now = chrono::Utc::now();
        let started_at = (now - chrono::Duration::seconds(45)).to_rfc3339();

        let task = Task {
            id: "task-123".to_string(),
            state: "in_progress".to_string(),
            tool: Some("performer".to_string()),
            task_runtime: TaskRuntime {
                status: Some("running".to_string()),
                current_phase: Some("implementing".to_string()),
                started_at: Some(started_at),
                worker_id: Some("worker-1".to_string()),
                ..Default::default()
            },
            worktree: Some(TaskWorktree {
                worktree_path: Some("wt/task-123".to_string()),
                branch: Some("feature/task-123".to_string()),
                ..Default::default()
            }),
            ..Default::default()
        };

        let row = LiveTaskRow::from_task(&task, now, "sonnet".to_string());
        assert_eq!(row.task_id, "task-123");
        assert_eq!(row.model, "sonnet");
        assert_eq!(row.health, TaskHealth::Healthy);
        assert_eq!(row.runtime_status, RuntimeStatus::Running);
        assert_eq!(row.phase, TaskPhase::Implementing);
        assert_eq!(row.tool, "performer");
        assert_eq!(row.worker_id, "worker-1");
        assert!(row.age.as_secs() >= 45 && row.age.as_secs() <= 47);
        assert_eq!(row.worktree, Some(PathBuf::from("wt/task-123")));
        assert_eq!(row.branch, Some("feature/task-123".to_string()));
    }
}
