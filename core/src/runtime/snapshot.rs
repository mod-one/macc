use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RuntimeSnapshot {
    pub generated_at: String,
    pub project: ProjectSummary,
    pub coordinator: CoordinatorStatus,
    pub queue: QueueSummary,
    pub workers: Vec<WorkerRuntime>,
    pub tasks: Vec<TaskRuntimeSummary>,
    pub active_runs: Vec<SkillRunSummary>,
    pub throttled_tools: Vec<ToolThrottleStatus>,
    pub recent_events: Vec<RuntimeEvent>,
    pub git: GitRuntimeSummary,
    pub diagnostics: RuntimeDiagnostics,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ProjectSummary {
    pub name: String,
    pub root: PathBuf,
    pub config_version: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct CoordinatorStatus {
    pub running: bool,
    pub paused: bool,
    pub pause_reason: Option<String>,
    pub pause_task_id: Option<String>,
    pub pause_phase: Option<String>,
    pub run_id: Option<String>,
    pub epoch: Option<i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct QueueSummary {
    pub todo: usize,
    pub ready: usize,
    pub claimed: usize,
    pub in_progress: usize,
    pub testing: usize,
    pub reviewing: usize,
    pub changes_requested: usize,
    pub blocked: usize,
    pub merged: usize,
    pub failed: usize,
    pub total: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkerRuntime {
    pub id: String,
    pub worktree_path: PathBuf,
    pub tool: String,
    pub task_id: Option<String>,
    pub branch: Option<String>,
    pub base_branch: Option<String>,
    pub phase: Option<String>,
    pub runtime_status: String,
    pub last_heartbeat: Option<String>,
    pub git_status: GitStatusSummary,
    pub retry_count: u32,
    pub delayed_until: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct GitStatusSummary {
    pub clean: bool,
    pub modified_files: usize,
    pub branch: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct TaskRuntimeSummary {
    pub task_id: String,
    pub title: String,
    pub workflow_state: String,
    pub runtime_status: String,
    pub tool: Option<String>,
    pub phase: Option<String>,
    pub worker_id: Option<String>,
    pub worktree: Option<String>,
    pub branch: Option<String>,
    pub message: Option<String>,
    pub last_heartbeat: Option<String>,
    pub started_at: Option<String>,
    pub last_error: Option<String>,
    pub last_error_code: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolThrottleStatus {
    pub tool: String,
    pub reason: String,
    pub error_code: String,
    pub retryable: bool,
    pub delayed_until: Option<String>,
    pub backoff_seconds: u64,
    pub effective_parallelism_delta: i32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SkillRunSummary {
    pub id: String,
    pub skill_id: String,
    pub tool: Option<String>,
    pub status: String,
    pub started_at: String,
    pub duration_ms: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RuntimeEvent {
    pub ts: Option<String>,
    pub event_type: String,
    pub task_id: Option<String>,
    pub phase: Option<String>,
    pub status: Option<String>,
    pub message: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct GitRuntimeSummary {
    pub repo_root: Option<PathBuf>,
    pub current_branch: Option<String>,
    pub clean: bool,
    pub worktrees_count: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct RuntimeDiagnostics {
    pub issues_count: usize,
    pub warnings_count: usize,
    pub critical_count: usize,
}
