use crate::coordinator::{RuntimeStatus, WorkflowState};
use crate::{MaccError, Result};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::{BTreeMap, HashSet};
use std::str::FromStr;

#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq)]
pub struct TaskRegistry {
    #[serde(default)]
    pub tasks: Vec<Task>,
    #[serde(default)]
    pub resource_locks: BTreeMap<String, ResourceLock>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub updated_at: Option<String>,
    #[serde(flatten)]
    pub extra: BTreeMap<String, Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq)]
pub struct Task {
    #[serde(default)]
    pub id: String,
    #[serde(default = "default_todo_state")]
    pub state: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub priority: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub category: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub scope: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub base_branch: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub coordinator_tool: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub assignee: Option<Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub claimed_at: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pr_url: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub review: Option<TaskReview>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub worktree: Option<TaskWorktree>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub dependencies: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub exclusive_resources: Vec<String>,
    #[serde(default, skip_serializing_if = "is_default_task_runtime")]
    pub task_runtime: TaskRuntime,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub state_changed_at: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub updated_at: Option<String>,
    #[serde(flatten)]
    pub extra: BTreeMap<String, Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq)]
pub struct TaskWorktree {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub worktree_path: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub branch: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub base_branch: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_commit: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub session_id: Option<String>,
    #[serde(flatten)]
    pub extra: BTreeMap<String, Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq)]
pub struct TaskReview {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub changed: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_reviewed_at: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reviewer: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
    #[serde(flatten)]
    pub extra: BTreeMap<String, Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq)]
pub struct TaskRuntime {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub status: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pid: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub current_phase: Option<String>,
    #[serde(default)]
    pub last_error: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_error_code: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_error_origin: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_error_message: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_heartbeat: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub started_at: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub phase_started_at: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub attempt: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub retries: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub completion_kind: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub metrics: Option<TaskRuntimeMetrics>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub slo_warnings: Option<BTreeMap<String, SloWarningRecord>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub merge_result_pending: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub merge_result_file: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub merge_worker_pid: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub merge_result_started_at: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_merge_result_file: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_merge_result_status: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_merge_result_rc: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_merge_result_at: Option<String>,
    #[serde(flatten)]
    pub extra: BTreeMap<String, Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq)]
pub struct TaskRuntimeMetrics {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub retries: Option<i64>,
    #[serde(flatten)]
    pub extra: BTreeMap<String, i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq)]
pub struct SloWarningRecord {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub metric: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub threshold: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub value: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub warned_at: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub suggestion: Option<String>,
    #[serde(flatten)]
    pub extra: BTreeMap<String, Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq)]
pub struct ResourceLock {
    #[serde(default)]
    pub task_id: String,
    #[serde(default)]
    pub worktree_path: String,
    #[serde(default)]
    pub locked_at: String,
    #[serde(flatten)]
    pub extra: BTreeMap<String, Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq)]
pub struct PrdInput {
    #[serde(default)]
    pub tasks: Vec<PrdTaskInput>,
    #[serde(flatten)]
    pub extra: BTreeMap<String, Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq)]
pub struct PrdTaskInput {
    #[serde(deserialize_with = "deserialize_stringish")]
    pub id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub priority: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub category: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub scope: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub base_branch: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub coordinator_tool: Option<String>,
    #[serde(
        default,
        deserialize_with = "deserialize_vec_stringish",
        skip_serializing_if = "Vec::is_empty"
    )]
    pub dependencies: Vec<String>,
    #[serde(
        default,
        deserialize_with = "deserialize_vec_stringish",
        skip_serializing_if = "Vec::is_empty"
    )]
    pub exclusive_resources: Vec<String>,
    #[serde(flatten)]
    pub extra: BTreeMap<String, Value>,
}

fn default_todo_state() -> String {
    "todo".to_string()
}

fn deserialize_stringish<'de, D>(deserializer: D) -> std::result::Result<String, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let value = Value::deserialize(deserializer)?;
    match value {
        Value::String(s) => Ok(s),
        Value::Number(n) => Ok(n.to_string()),
        other => Err(serde::de::Error::custom(format!(
            "expected string or number, got {}",
            other
        ))),
    }
}

fn deserialize_vec_stringish<'de, D>(deserializer: D) -> std::result::Result<Vec<String>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let values = Vec::<Value>::deserialize(deserializer)?;
    values
        .into_iter()
        .map(|value| match value {
            Value::String(s) => Ok(s),
            Value::Number(n) => Ok(n.to_string()),
            other => Err(serde::de::Error::custom(format!(
                "expected string or number, got {}",
                other
            ))),
        })
        .collect()
}

fn is_default_task_runtime(runtime: &TaskRuntime) -> bool {
    runtime == &TaskRuntime::default()
}

impl TaskRegistry {
    pub fn from_value(value: &Value) -> Result<Self> {
        serde_json::from_value::<Self>(value.clone()).map_err(|e| {
            MaccError::Coordinator {
                code: "registry_parse",
                message: format!("Failed to parse typed coordinator registry: {}", e),
            }
        })
    }

    pub fn to_value(&self) -> Result<Value> {
        serde_json::to_value(self).map_err(|e| {
            MaccError::Validation(format!(
                "Failed to serialize typed coordinator registry: {}",
                e
            ))
        })
    }

    pub fn set_updated_at(&mut self, ts: String) {
        self.updated_at = Some(ts);
    }

    pub fn recompute_resource_locks(&mut self, now_iso: &str) {
        self.resource_locks.clear();
        for task in &self.tasks {
            if task.id.is_empty() {
                continue;
            }
            if !matches!(
                task.state.as_str(),
                "claimed" | "in_progress" | "pr_open" | "changes_requested" | "queued"
            ) {
                continue;
            }
            let worktree_path = task
                .worktree
                .as_ref()
                .and_then(|w| w.worktree_path.clone())
                .unwrap_or_default();
            for resource in &task.exclusive_resources {
                if resource.is_empty() {
                    continue;
                }
                self.resource_locks.insert(
                    resource.clone(),
                    ResourceLock {
                        task_id: task.id.clone(),
                        worktree_path: worktree_path.clone(),
                        locked_at: now_iso.to_string(),
                        extra: BTreeMap::new(),
                    },
                );
            }
        }
    }

    pub fn active_task_worktree_paths(&self) -> HashSet<String> {
        let mut out = HashSet::new();
        for task in &self.tasks {
            if !matches!(
                task.state.as_str(),
                "claimed" | "in_progress" | "pr_open" | "changes_requested" | "queued"
            ) {
                continue;
            }
            if let Some(path) = task
                .worktree
                .as_ref()
                .and_then(|w| w.worktree_path.as_ref())
            {
                if !path.is_empty() {
                    out.insert(path.to_string());
                }
            }
        }
        out
    }

    pub fn can_reuse_worktree_slot(&self, worktree_path: &str) -> bool {
        let mut seen = false;
        let mut all_merged = true;
        for task in &self.tasks {
            let Some(path) = task
                .worktree
                .as_ref()
                .and_then(|w| w.worktree_path.as_ref())
            else {
                continue;
            };
            if path != worktree_path {
                continue;
            }
            seen = true;
            if task.state != "merged" {
                all_merged = false;
            }
        }
        seen && all_merged
    }

    pub fn has_in_progress_or_queued_on_worktree(&self, worktree_path: &str) -> bool {
        self.tasks.iter().any(|task| {
            matches!(
                task.workflow_state(),
                Some(WorkflowState::InProgress | WorkflowState::Queued)
            ) && task.worktree_path().is_some_and(|p| p == worktree_path)
        })
    }

    pub fn find_task(&self, task_id: &str) -> Option<&Task> {
        self.tasks.iter().find(|task| task.id == task_id)
    }

    pub fn find_task_mut(&mut self, task_id: &str) -> Option<&mut Task> {
        self.tasks.iter_mut().find(|task| task.id == task_id)
    }

    pub fn counts(&self) -> (usize, usize, usize, usize, usize) {
        let total = self.tasks.len();
        let mut todo = 0usize;
        let mut active = 0usize;
        let mut blocked = 0usize;
        let mut merged = 0usize;
        for task in &self.tasks {
            match task.workflow_state().unwrap_or(WorkflowState::Todo) {
                WorkflowState::Todo => todo += 1,
                WorkflowState::Blocked => blocked += 1,
                WorkflowState::Merged => merged += 1,
                WorkflowState::Claimed
                | WorkflowState::InProgress
                | WorkflowState::PrOpen
                | WorkflowState::ChangesRequested
                | WorkflowState::Queued => active += 1,
                WorkflowState::Abandoned => {}
            }
        }
        (total, todo, active, blocked, merged)
    }
}

impl Task {
    pub fn workflow_state(&self) -> Option<WorkflowState> {
        WorkflowState::from_str(self.state.as_str()).ok()
    }

    pub fn is_active(&self) -> bool {
        matches!(
            self.workflow_state(),
            Some(
                WorkflowState::Claimed
                    | WorkflowState::InProgress
                    | WorkflowState::PrOpen
                    | WorkflowState::ChangesRequested
                    | WorkflowState::Queued
            )
        )
    }

    pub fn is_merged(&self) -> bool {
        matches!(self.workflow_state(), Some(WorkflowState::Merged))
    }

    pub fn worktree_path(&self) -> Option<&str> {
        self.worktree
            .as_ref()
            .and_then(|worktree| worktree.worktree_path.as_deref())
            .filter(|path| !path.is_empty())
    }

    pub fn has_worktree_attached(&self) -> bool {
        self.worktree.as_ref().is_some_and(|worktree| {
            worktree.worktree_path.is_some()
                || worktree.branch.is_some()
                || worktree.base_branch.is_some()
                || worktree.last_commit.is_some()
                || worktree.session_id.is_some()
                || !worktree.extra.is_empty()
        })
    }

    pub fn task_tool(&self) -> Option<&str> {
        self.tool.as_deref().filter(|tool| !tool.is_empty())
    }

    pub fn coordinator_tool(&self) -> Option<&str> {
        self.coordinator_tool
            .as_deref()
            .filter(|tool| !tool.is_empty())
    }

    pub fn category(&self) -> Option<&str> {
        self.category
            .as_deref()
            .filter(|category| !category.is_empty())
    }

    pub fn scope(&self) -> Option<&str> {
        self.scope.as_deref().filter(|scope| !scope.is_empty())
    }

    pub fn base_branch(&self, default: &str) -> String {
        self.base_branch
            .as_deref()
            .filter(|value| !value.is_empty())
            .or_else(|| {
                self.worktree
                    .as_ref()
                    .and_then(|worktree| worktree.base_branch.as_deref())
                    .filter(|value| !value.is_empty())
            })
            .unwrap_or(default)
            .to_string()
    }

    pub fn priority_rank(&self) -> i32 {
        parse_priority_value(self.priority.as_deref())
    }

    pub fn dependency_ids(&self) -> Vec<String> {
        self.dependencies.clone()
    }

    pub fn branch(&self) -> Option<&str> {
        self.worktree
            .as_ref()
            .and_then(|worktree| worktree.branch.as_deref())
            .filter(|branch| !branch.is_empty())
    }

    pub fn last_commit(&self) -> Option<&str> {
        self.worktree
            .as_ref()
            .and_then(|worktree| worktree.last_commit.as_deref())
            .filter(|value| !value.is_empty())
    }

    pub fn session_id(&self) -> Option<&str> {
        self.worktree
            .as_ref()
            .and_then(|worktree| worktree.session_id.as_deref())
            .filter(|value| !value.is_empty())
    }

    pub fn current_phase(&self) -> &str {
        self.task_runtime
            .current_phase
            .as_deref()
            .filter(|phase| !phase.is_empty())
            .unwrap_or("dev")
    }

    pub fn runtime_status(&self) -> RuntimeStatus {
        self.task_runtime
            .status
            .as_deref()
            .unwrap_or(RuntimeStatus::Idle.as_str())
            .parse::<RuntimeStatus>()
            .unwrap_or(RuntimeStatus::Idle)
    }

    pub fn runtime_pid(&self) -> Option<i64> {
        self.task_runtime.pid
    }

    pub fn set_workflow_state(&mut self, state: WorkflowState) {
        self.state = state.as_str().to_string();
    }

    pub fn ensure_worktree(&mut self) -> &mut TaskWorktree {
        self.worktree.get_or_insert_with(TaskWorktree::default)
    }

    pub fn clear_assignment(&mut self) {
        self.assignee = None;
        self.claimed_at = None;
        self.worktree = None;
    }

    pub fn ensure_runtime(&mut self) -> &mut TaskRuntime {
        &mut self.task_runtime
    }

    pub fn touch_state_changed(&mut self, now: &str) {
        self.state_changed_at = Some(now.to_string());
        self.updated_at = Some(now.to_string());
    }
}

impl TaskRuntime {
    pub fn status(&self) -> RuntimeStatus {
        self.status
            .as_deref()
            .unwrap_or(RuntimeStatus::Idle.as_str())
            .parse::<RuntimeStatus>()
            .unwrap_or(RuntimeStatus::Idle)
    }

    pub fn set_status(&mut self, status: RuntimeStatus) {
        self.status = Some(status.as_str().to_string());
    }

    pub fn set_last_error_details(
        &mut self,
        code: impl Into<String>,
        origin: impl Into<String>,
        message: impl Into<String>,
    ) {
        self.last_error_code = Some(code.into());
        self.last_error_origin = Some(origin.into());
        self.last_error_message = Some(message.into());
    }

    pub fn clear_last_error_details(&mut self) {
        self.last_error_code = None;
        self.last_error_origin = None;
        self.last_error_message = None;
    }

    pub fn ensure_metrics(&mut self) -> &mut TaskRuntimeMetrics {
        self.metrics.get_or_insert_with(TaskRuntimeMetrics::default)
    }

    pub fn metric_i64(&self, metric_name: &str) -> Option<i64> {
        match metric_name {
            "retries" => self
                .retries
                .or_else(|| self.metrics.as_ref().and_then(|metrics| metrics.retries)),
            other => self
                .metrics
                .as_ref()
                .and_then(|metrics| metrics.extra.get(other).copied()),
        }
    }

    pub fn set_metric_i64(&mut self, metric_name: &str, value: i64) {
        if metric_name == "retries" {
            self.retries = Some(value);
            self.ensure_metrics().retries = Some(value);
            return;
        }
        self.ensure_metrics()
            .extra
            .insert(metric_name.to_string(), value);
    }

    pub fn ensure_slo_warnings(&mut self) -> &mut BTreeMap<String, SloWarningRecord> {
        self.slo_warnings.get_or_insert_with(BTreeMap::new)
    }

    pub fn has_slo_warning(&self, metric_name: &str) -> bool {
        self.slo_warnings
            .as_ref()
            .is_some_and(|warnings| warnings.contains_key(metric_name))
    }

    pub fn upsert_slo_warning(
        &mut self,
        metric: &str,
        threshold: i64,
        value: i64,
        suggestion: &str,
        warned_at: &str,
    ) {
        self.ensure_slo_warnings().insert(
            metric.to_string(),
            SloWarningRecord {
                metric: Some(metric.to_string()),
                threshold: Some(threshold),
                value: Some(value),
                warned_at: Some(warned_at.to_string()),
                suggestion: Some(suggestion.to_string()),
                extra: BTreeMap::new(),
            },
        );
    }

    pub fn retries_count(&self) -> usize {
        self.metric_i64("retries")
            .and_then(|v| usize::try_from(v).ok())
            .unwrap_or(0)
    }

    pub fn increment_retries(&mut self) -> usize {
        let next = self.retries_count().saturating_add(1);
        self.set_metric_i64("retries", next as i64);
        next
    }
}

fn parse_priority_value(priority: Option<&str>) -> i32 {
    match priority.map(|value| value.trim().to_ascii_lowercase()) {
        Some(value) if value == "p0" => 0,
        Some(value) if value == "p1" => 1,
        Some(value) if value == "p2" => 2,
        Some(value) if value == "p3" => 3,
        Some(value) if value == "p4" => 4,
        Some(value) => value.parse::<i32>().unwrap_or(99),
        None => 99,
    }
}
