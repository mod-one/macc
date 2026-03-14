use super::{RuntimeStatus, WorkflowState};
use crate::config::{CanonicalConfig, CoordinatorConfig};
use crate::coordinator::control_plane::CoordinatorLog;
use crate::coordinator::model::{Task, TaskRegistry};
use crate::coordinator::runtime::{
    process_branch_cleanup_queue, terminate_active_jobs, CoordinatorRunState,
};
use crate::coordinator::state_runtime::{
    cleanup_dead_runtime_tasks, clear_coordinator_pause_file, coordinator_pause_file_path,
    resume_paused_task_integrate, set_task_paused_for_integrate, write_coordinator_pause_file,
};
use crate::coordinator_storage::{
    coordinator_storage_bootstrap_sqlite_from_json, coordinator_storage_export_sqlite_to_json,
    CoordinatorStorageMode,
};
use crate::{MaccError, Result};
use async_trait::async_trait;
use serde_json::{json, Value};
use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PhaseTransition {
    pub mode: &'static str,
    pub next_state: WorkflowState,
    pub runtime_phase: &'static str,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AdvancePlan {
    RunPhase(PhaseTransition),
    Merge,
    Noop,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReviewVerdict {
    Ok,
    ChangesRequested,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum WorkflowEvent {
    PhaseSucceeded(&'static str),
    PhaseFailed(&'static str),
    ReviewChangesRequested,
    MergeSucceeded,
    MergeFailed,
}

#[derive(Debug, Clone)]
pub struct AdvanceResult {
    pub progressed: bool,
    pub blocked_merge: Option<(String, String)>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CoordinatorCounts {
    pub total: usize,
    pub todo: usize,
    pub active: usize,
    pub blocked: usize,
    pub merged: usize,
}

#[derive(Debug, Clone)]
pub struct DispatchClaimUpdate {
    pub task_id: String,
    pub tool: String,
    pub worktree_path: String,
    pub branch: String,
    pub base_branch: String,
    pub last_commit: String,
    pub session_id: String,
    pub pid: Option<i64>,
    pub phase: String,
    pub now: String,
}

#[derive(Debug, Clone)]
pub struct JobCompletionInput {
    pub success: bool,
    pub attempt: usize,
    pub max_attempts: usize,
    pub timed_out: bool,
    pub phase_timeout_seconds: usize,
    pub elapsed_seconds: u64,
    pub status_text: String,
    pub error_code: Option<String>,
    pub error_origin: Option<String>,
    pub error_message: Option<String>,
    pub auto_retry_error_codes: Vec<String>,
    pub auto_retry_max: usize,
}

#[derive(Debug, Clone)]
pub struct JobCompletionResult {
    pub should_retry: bool,
    pub status_label: &'static str,
    pub detail: String,
}

#[derive(Debug, Clone)]
pub enum AdvanceTaskAction {
    RunPhase {
        task_id: String,
        mode: &'static str,
        transition: PhaseTransition,
    },
    QueueMerge {
        task_id: String,
        branch: String,
        base: String,
    },
}

#[derive(Debug, Clone)]
pub struct DeadRuntimeCleanupEntry {
    pub task_id: String,
    pub old_state: String,
    pub phase: String,
    pub pid: i64,
    pub new_state: String,
}

#[derive(Debug, Clone)]
pub struct ControlPlaneLoopConfig {
    pub timeout: Option<Duration>,
    pub max_no_progress_cycles: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ControlPlaneDecision {
    Continue,
    Complete,
}

pub struct CoordinatorRunController {
    cfg: ControlPlaneLoopConfig,
    started: Instant,
    no_progress_cycles: usize,
    previous_counts: Option<CoordinatorCounts>,
}

#[async_trait]
pub trait ControlPlaneBackend {
    async fn on_cycle_start(&mut self, cycle: usize) -> Result<()>;
    async fn monitor_active_jobs(&mut self) -> Result<()>;
    async fn monitor_merge_jobs(&mut self) -> Result<Option<(String, String)>>;
    async fn on_blocked_merge(&mut self, task_id: &str, reason: &str) -> Result<()>;
    async fn advance_tasks(&mut self) -> Result<AdvanceResult>;
    async fn dispatch_ready_tasks(&mut self) -> Result<usize>;
    async fn on_cycle_end(
        &mut self,
        cycle: usize,
        advance: &AdvanceResult,
        dispatched: usize,
    ) -> Result<CoordinatorCounts>;
    async fn sleep_between_cycles(&mut self) -> Result<()>;
    fn should_terminate_run(&self, _counts: &CoordinatorCounts) -> bool {
        false
    }
}

pub fn plan_advance(state: WorkflowState) -> AdvancePlan {
    match state {
        WorkflowState::InProgress => AdvancePlan::RunPhase(PhaseTransition {
            mode: "review",
            next_state: WorkflowState::PrOpen,
            runtime_phase: "review",
        }),
        WorkflowState::PrOpen => AdvancePlan::RunPhase(PhaseTransition {
            mode: "integrate",
            next_state: WorkflowState::Queued,
            runtime_phase: "integrate",
        }),
        WorkflowState::ChangesRequested => AdvancePlan::RunPhase(PhaseTransition {
            mode: "fix",
            next_state: WorkflowState::PrOpen,
            runtime_phase: "fix",
        }),
        WorkflowState::Queued => AdvancePlan::Merge,
        _ => AdvancePlan::Noop,
    }
}

fn transition_workflow_state(from: WorkflowState, event: WorkflowEvent) -> Result<WorkflowState> {
    let to = match (from, event) {
        (WorkflowState::InProgress, WorkflowEvent::PhaseSucceeded("review")) => {
            WorkflowState::PrOpen
        }
        (WorkflowState::InProgress, WorkflowEvent::ReviewChangesRequested) => {
            WorkflowState::ChangesRequested
        }
        (WorkflowState::PrOpen, WorkflowEvent::PhaseSucceeded("integrate")) => {
            WorkflowState::Queued
        }
        (WorkflowState::ChangesRequested, WorkflowEvent::PhaseSucceeded("fix")) => {
            WorkflowState::PrOpen
        }
        (WorkflowState::InProgress, WorkflowEvent::PhaseFailed("review"))
        | (WorkflowState::PrOpen, WorkflowEvent::PhaseFailed("integrate"))
        | (WorkflowState::ChangesRequested, WorkflowEvent::PhaseFailed("fix"))
        | (WorkflowState::Queued, WorkflowEvent::MergeFailed) => WorkflowState::Blocked,
        (WorkflowState::Queued, WorkflowEvent::MergeSucceeded) => WorkflowState::Merged,
        _ => {
            return Err(MaccError::Validation(format!(
                "Invalid coordinator FSM transition: from={} event={:?}",
                from.as_str(),
                event
            )));
        }
    };

    if !super::is_valid_workflow_transition(from, to) {
        return Err(MaccError::Validation(format!(
            "Coordinator FSM produced invalid workflow transition {} -> {}",
            from.as_str(),
            to.as_str()
        )));
    }

    Ok(to)
}

pub fn apply_dispatch_claim_in_registry(
    registry: &mut Value,
    update: &DispatchClaimUpdate,
) -> Result<()> {
    let mut typed = TaskRegistry::from_value(registry)?;
    let task = typed.find_task_mut(&update.task_id).ok_or_else(|| {
        MaccError::Validation(format!("Task '{}' not found in registry", update.task_id))
    })?;
    apply_dispatch_claim_typed(task, update);
    *registry = typed.to_value()?;
    Ok(())
}

pub fn apply_dispatch_pid_in_registry(
    registry: &mut Value,
    task_id: &str,
    pid: Option<i64>,
) -> Result<()> {
    let mut typed = TaskRegistry::from_value(registry)?;
    let task = typed.find_task_mut(task_id).ok_or_else(|| {
        MaccError::Validation(format!("Task '{}' not found in registry", task_id))
    })?;
    apply_dispatch_pid_typed(task, pid);
    *registry = typed.to_value()?;
    Ok(())
}

pub fn build_advance_actions(
    registry: &Value,
    active_merge_jobs: &HashSet<String>,
) -> Result<Vec<AdvanceTaskAction>> {
    let typed = TaskRegistry::from_value(registry)?;
    let mut actions = Vec::new();
    for task in &typed.tasks {
        let task_id = task.id.clone();
        let workflow_state = task.workflow_state();
        match workflow_state
            .map(plan_advance)
            .unwrap_or(AdvancePlan::Noop)
        {
            AdvancePlan::RunPhase(transition) => {
                actions.push(AdvanceTaskAction::RunPhase {
                    task_id,
                    mode: transition.mode,
                    transition,
                });
            }
            AdvancePlan::Merge => {
                if active_merge_jobs.contains(&task_id) {
                    continue;
                }
                let branch = task.branch().unwrap_or_default().to_string();
                if branch.is_empty() {
                    continue;
                }
                let base = task.base_branch("master");
                actions.push(AdvanceTaskAction::QueueMerge {
                    task_id,
                    branch,
                    base,
                });
            }
            AdvancePlan::Noop => {}
        }
    }
    Ok(actions)
}

pub fn apply_phase_outcome_in_registry(
    registry: &mut Value,
    task_id: &str,
    mode: &'static str,
    transition: PhaseTransition,
    review_verdict: Option<ReviewVerdict>,
    phase_error: Option<&str>,
    now: &str,
) -> Result<()> {
    let mut typed = TaskRegistry::from_value(registry)?;
    let task = typed.find_task_mut(task_id).ok_or_else(|| {
        MaccError::Validation(format!("Task '{}' not found in registry", task_id))
    })?;
    if let Some(reason) = phase_error {
        apply_phase_failure_typed(task, mode, reason, now)?;
        *registry = typed.to_value()?;
        return Ok(());
    }
    if mode == "review" {
        let verdict = review_verdict.ok_or_else(|| {
            MaccError::Validation(format!(
                "Missing review verdict for task '{}' during review phase",
                task_id
            ))
        })?;
        let next = apply_review_phase_success_typed(task, verdict, now)?;
        if next == WorkflowState::PrOpen && task.pr_url.as_deref().unwrap_or_default().is_empty() {
            let branch = task.branch().unwrap_or("unknown");
            task.pr_url = Some(format!("local://{}", branch));
        }
        *registry = typed.to_value()?;
        return Ok(());
    }
    apply_phase_success_typed(task, transition, now)?;
    if transition.next_state == WorkflowState::PrOpen
        && task.pr_url.as_deref().unwrap_or_default().is_empty()
    {
        let branch = task.branch().unwrap_or("unknown");
        task.pr_url = Some(format!("local://{}", branch));
    }
    *registry = typed.to_value()?;
    Ok(())
}

pub fn apply_job_completion_in_registry(
    registry: &mut Value,
    task_id: &str,
    input: &JobCompletionInput,
    now: &str,
) -> Result<JobCompletionResult> {
    let mut typed = TaskRegistry::from_value(registry)?;
    let task = typed.find_task_mut(task_id).ok_or_else(|| {
        MaccError::Validation(format!("Task '{}' not found in registry", task_id))
    })?;
    let out = apply_job_completion_typed(task, input, now);
    *registry = typed.to_value()?;
    Ok(out)
}

pub fn apply_merge_result_in_registry(
    registry: &mut Value,
    task_id: &str,
    success: bool,
    reason: &str,
    now: &str,
) -> Result<()> {
    let mut typed = TaskRegistry::from_value(registry)?;
    let task = typed.find_task_mut(task_id).ok_or_else(|| {
        MaccError::Validation(format!("Task '{}' not found in registry", task_id))
    })?;
    if success {
        apply_merge_success_typed(task, now)?
    } else {
        apply_merge_failure_typed(task, reason, now)?
    }
    *registry = typed.to_value()?;
    Ok(())
}

pub fn ensure_runtime_object(task: &mut Value) {
    if !task
        .get("task_runtime")
        .map(Value::is_object)
        .unwrap_or(false)
    {
        task["task_runtime"] = json!({});
    }
}

fn parse_compat_task(task: &Value) -> Task {
    serde_json::from_value::<Task>(task.clone()).unwrap_or_default()
}

fn write_compat_task(task: &mut Value, typed: &Task) {
    if let Ok(serialized) = serde_json::to_value(typed) {
        *task = serialized;
    }
}

pub fn apply_dispatch_claim(task: &mut Value, update: &DispatchClaimUpdate) {
    let mut typed = parse_compat_task(task);
    apply_dispatch_claim_typed(&mut typed, update);
    write_compat_task(task, &typed);
}

pub fn apply_dispatch_pid(task: &mut Value, pid: Option<i64>) {
    let mut typed = parse_compat_task(task);
    apply_dispatch_pid_typed(&mut typed, pid);
    write_compat_task(task, &typed);
}

fn apply_dispatch_claim_typed(task: &mut Task, update: &DispatchClaimUpdate) {
    task.set_workflow_state(WorkflowState::Claimed);
    task.tool = Some(update.tool.clone());
    let worktree = task.ensure_worktree();
    worktree.worktree_path = Some(update.worktree_path.clone());
    worktree.branch = Some(update.branch.clone());
    worktree.base_branch = Some(update.base_branch.clone());
    worktree.last_commit = Some(update.last_commit.clone());
    worktree.session_id = Some(update.session_id.clone());
    let runtime = task.ensure_runtime();
    runtime.set_status(RuntimeStatus::Running);
    runtime.current_phase = Some(update.phase.clone());
    runtime.started_at = Some(update.now.clone());
    runtime.pid = update.pid;
    task.touch_state_changed(&update.now);
}

fn apply_dispatch_pid_typed(task: &mut Task, pid: Option<i64>) {
    task.ensure_runtime().pid = pid;
}

pub fn apply_phase_success(task: &mut Value, transition: PhaseTransition, now: &str) -> Result<()> {
    let mut typed = parse_compat_task(task);
    apply_phase_success_typed(&mut typed, transition, now)?;
    write_compat_task(task, &typed);
    Ok(())
}

pub fn apply_review_phase_success(
    task: &mut Value,
    verdict: ReviewVerdict,
    now: &str,
) -> Result<WorkflowState> {
    let mut typed = parse_compat_task(task);
    let to = apply_review_phase_success_typed(&mut typed, verdict, now)?;
    write_compat_task(task, &typed);
    Ok(to)
}

pub fn apply_phase_failure(
    task: &mut Value,
    phase_mode: &'static str,
    reason: &str,
    now: &str,
) -> Result<()> {
    let mut typed = parse_compat_task(task);
    apply_phase_failure_typed(&mut typed, phase_mode, reason, now)?;
    write_compat_task(task, &typed);
    Ok(())
}

pub fn apply_merge_success(task: &mut Value, now: &str) -> Result<()> {
    let mut typed = parse_compat_task(task);
    apply_merge_success_typed(&mut typed, now)?;
    write_compat_task(task, &typed);
    Ok(())
}

pub fn apply_merge_failure(task: &mut Value, reason: &str, now: &str) -> Result<()> {
    let mut typed = parse_compat_task(task);
    apply_merge_failure_typed(&mut typed, reason, now)?;
    write_compat_task(task, &typed);
    Ok(())
}

pub fn apply_job_completion(
    task: &mut Value,
    input: &JobCompletionInput,
    now: &str,
) -> JobCompletionResult {
    let mut typed = parse_compat_task(task);
    let result = apply_job_completion_typed(&mut typed, input, now);
    write_compat_task(task, &typed);
    result
}

fn task_workflow_state_typed(task: &Task) -> Result<WorkflowState> {
    task.workflow_state().ok_or_else(|| {
        MaccError::Validation(format!(
            "Invalid coordinator workflow state '{}'",
            task.state
        ))
    })
}

pub(crate) fn apply_phase_success_typed(
    task: &mut Task,
    transition: PhaseTransition,
    now: &str,
) -> Result<()> {
    let from = task_workflow_state_typed(task)?;
    let to = transition_workflow_state(from, WorkflowEvent::PhaseSucceeded(transition.mode))?;
    if to != transition.next_state {
        return Err(MaccError::Validation(format!(
            "Coordinator FSM mismatch for mode='{}': expected next={} got {}",
            transition.mode,
            transition.next_state.as_str(),
            to.as_str()
        )));
    }
    task.set_workflow_state(to);
    let runtime = task.ensure_runtime();
    runtime.set_status(RuntimeStatus::PhaseDone);
    runtime.current_phase = Some(transition.runtime_phase.to_string());
    runtime.pid = None;
    task.touch_state_changed(now);
    Ok(())
}

pub(crate) fn apply_review_phase_success_typed(
    task: &mut Task,
    verdict: ReviewVerdict,
    now: &str,
) -> Result<WorkflowState> {
    let from = task_workflow_state_typed(task)?;
    let to = match verdict {
        ReviewVerdict::Ok => {
            transition_workflow_state(from, WorkflowEvent::PhaseSucceeded("review"))?
        }
        ReviewVerdict::ChangesRequested => {
            transition_workflow_state(from, WorkflowEvent::ReviewChangesRequested)?
        }
    };
    task.set_workflow_state(to);
    let runtime = task.ensure_runtime();
    runtime.set_status(RuntimeStatus::PhaseDone);
    runtime.current_phase = Some("review".to_string());
    runtime.pid = None;
    task.touch_state_changed(now);
    Ok(to)
}

pub(crate) fn apply_phase_failure_typed(
    task: &mut Task,
    phase_mode: &'static str,
    reason: &str,
    now: &str,
) -> Result<()> {
    let from = task_workflow_state_typed(task)?;
    let to = transition_workflow_state(from, WorkflowEvent::PhaseFailed(phase_mode))?;
    task.set_workflow_state(to);
    let runtime = task.ensure_runtime();
    runtime.set_status(RuntimeStatus::Failed);
    runtime.current_phase = Some(phase_mode.to_string());
    runtime.last_error = Some(reason.to_string());
    runtime.pid = None;
    task.touch_state_changed(now);
    Ok(())
}

pub(crate) fn apply_merge_success_typed(task: &mut Task, now: &str) -> Result<()> {
    let from = task_workflow_state_typed(task)?;
    let to = transition_workflow_state(from, WorkflowEvent::MergeSucceeded)?;
    task.set_workflow_state(to);
    let runtime = task.ensure_runtime();
    runtime.set_status(RuntimeStatus::Idle);
    runtime.pid = None;
    task.touch_state_changed(now);
    Ok(())
}

pub(crate) fn apply_merge_failure_typed(task: &mut Task, reason: &str, now: &str) -> Result<()> {
    let from = task_workflow_state_typed(task)?;
    let to = transition_workflow_state(from, WorkflowEvent::MergeFailed)?;
    task.set_workflow_state(to);
    let runtime = task.ensure_runtime();
    runtime.set_status(RuntimeStatus::Paused);
    runtime.current_phase = Some("integrate".to_string());
    runtime.last_error = Some(reason.to_string());
    runtime.pid = None;
    task.touch_state_changed(now);
    Ok(())
}

fn apply_job_completion_typed(
    task: &mut Task,
    input: &JobCompletionInput,
    now: &str,
) -> JobCompletionResult {
    let error_code = input
        .error_code
        .clone()
        .unwrap_or_else(|| "E101".to_string());
    let error_origin = input
        .error_origin
        .clone()
        .unwrap_or_else(|| "runner".to_string());
    let error_message = input
        .error_message
        .clone()
        .unwrap_or_else(|| input.status_text.clone());
    if input.attempt == 0 || input.max_attempts == 0 {
        task.set_workflow_state(WorkflowState::Blocked);
        let runtime = task.ensure_runtime();
        runtime.set_status(RuntimeStatus::Failed);
        runtime.pid = None;
        let detail = "performer completion received with invalid attempt counters".to_string();
        runtime.set_last_error_details("E901", "coordinator", detail.clone());
        runtime.last_error = Some(detail.clone());
        task.touch_state_changed(now);
        return JobCompletionResult {
            should_retry: false,
            status_label: "failed",
            detail,
        };
    }
    if input.status_text.is_empty() {
        task.set_workflow_state(WorkflowState::Blocked);
        let runtime = task.ensure_runtime();
        runtime.set_status(RuntimeStatus::Failed);
        runtime.pid = None;
        let detail = "performer completion received without status detail".to_string();
        runtime.set_last_error_details("E901", "coordinator", detail.clone());
        runtime.last_error = Some(detail.clone());
        task.touch_state_changed(now);
        return JobCompletionResult {
            should_retry: false,
            status_label: "failed",
            detail,
        };
    }
    if input.success {
        task.set_workflow_state(WorkflowState::InProgress);
        let runtime = task.ensure_runtime();
        runtime.set_status(RuntimeStatus::PhaseDone);
        runtime.current_phase = Some("dev".to_string());
        runtime.pid = None;
        task.touch_state_changed(now);
        return JobCompletionResult {
            should_retry: false,
            status_label: "phase_done",
            detail: input.status_text.clone(),
        };
    }
    if input.attempt < input.max_attempts {
        task.set_workflow_state(WorkflowState::Claimed);
        let runtime = task.ensure_runtime();
        runtime.set_status(RuntimeStatus::Running);
        runtime.current_phase = Some("dev".to_string());
        runtime.pid = None;
        let reason = if input.timed_out {
            format!(
                "performer timed out after {}s on attempt {} (elapsed={}s)",
                input.phase_timeout_seconds, input.attempt, input.elapsed_seconds
            )
        } else {
            format!(
                "performer failed on attempt {}: {}",
                input.attempt, input.status_text
            )
        };
        runtime.set_last_error_details(
            error_code.clone(),
            error_origin.clone(),
            error_message.clone(),
        );
        runtime.last_error = Some(reason.clone());
        task.touch_state_changed(now);
        return JobCompletionResult {
            should_retry: true,
            status_label: "retry",
            detail: reason,
        };
    }
    let reason = if input.timed_out {
        format!(
            "performer timed out after {}s (max attempts reached: {}, elapsed={}s)",
            input.phase_timeout_seconds, input.max_attempts, input.elapsed_seconds
        )
    } else {
        format!(
            "performer failed after {} attempts: {}",
            input.attempt, input.status_text
        )
    };
    let retries_total = task.task_runtime.retries_count();
    if should_auto_retry_error_code(
        &error_code,
        &input.auto_retry_error_codes,
        input.auto_retry_max,
        retries_total,
    ) {
        task.set_workflow_state(WorkflowState::Todo);
        let runtime = task.ensure_runtime();
        runtime.increment_retries();
        runtime.set_status(RuntimeStatus::Idle);
        runtime.pid = None;
        runtime.current_phase = Some("dev".to_string());
        runtime.set_last_error_details(
            error_code.clone(),
            error_origin.clone(),
            error_message.clone(),
        );
        runtime.last_error = Some(reason.clone());
        task.touch_state_changed(now);
        return JobCompletionResult {
            should_retry: false,
            status_label: "auto_retry",
            detail: format!("auto-retry scheduled for error code {}", error_code),
        };
    }
    task.set_workflow_state(WorkflowState::Blocked);
    let runtime = task.ensure_runtime();
    runtime.set_status(RuntimeStatus::Failed);
    runtime.pid = None;
    runtime.set_last_error_details(error_code, error_origin, error_message);
    runtime.last_error = Some(reason.clone());
    task.touch_state_changed(now);
    JobCompletionResult {
        should_retry: false,
        status_label: "failed",
        detail: reason,
    }
}

fn should_auto_retry_error_code(
    code: &str,
    list: &[String],
    max_retries: usize,
    current_retries: usize,
) -> bool {
    if code.is_empty() || max_retries == 0 {
        return false;
    }
    if current_retries >= max_retries {
        return false;
    }
    list.iter().any(|entry| entry.trim() == code)
}

pub fn cleanup_dead_runtime_tasks_in_registry_with<F>(
    registry: &mut Value,
    now: &str,
    heartbeat_grace_seconds: i64,
    mut is_pid_running: F,
) -> Result<Vec<DeadRuntimeCleanupEntry>>
where
    F: FnMut(i64) -> bool,
{
    let now_ts = chrono::DateTime::parse_from_rfc3339(now)
        .ok()
        .map(|dt| dt.timestamp())
        .unwrap_or_default();
    let mut cleaned = Vec::new();
    let mut typed = TaskRegistry::from_value(registry)?;
    for task in typed.tasks.iter_mut() {
        let Some(pid) = task.runtime_pid() else {
            continue;
        };
        let runtime_status = task.runtime_status();
        if runtime_status != RuntimeStatus::Running || is_pid_running(pid) {
            continue;
        }
        if heartbeat_grace_seconds > 0 {
            let within_grace = task
                .task_runtime
                .last_heartbeat
                .as_deref()
                .and_then(|ts| chrono::DateTime::parse_from_rfc3339(ts).ok())
                .map(|dt| now_ts.saturating_sub(dt.timestamp()) <= heartbeat_grace_seconds)
                .unwrap_or(false);
            if within_grace {
                continue;
            }
        }

        let task_id = task.id.clone();
        let phase = task.current_phase().to_string();
        let old_state = task.state.clone();

        let runtime = task.ensure_runtime();
        runtime.pid = None;
        runtime.set_status(RuntimeStatus::Stale);
        runtime.last_error = Some(format!("runtime pid {} is not running; auto-reset", pid));
        let new_state = if old_state == WorkflowState::Claimed.as_str() && phase == "dev" {
            task.set_workflow_state(WorkflowState::Todo);
            task.assignee = None;
            WorkflowState::Todo.as_str().to_string()
        } else {
            task.set_workflow_state(WorkflowState::Blocked);
            WorkflowState::Blocked.as_str().to_string()
        };
        task.touch_state_changed(now);

        cleaned.push(DeadRuntimeCleanupEntry {
            task_id,
            old_state,
            phase,
            pid,
            new_state,
        });
    }
    *registry = typed.to_value()?;
    Ok(cleaned)
}

impl CoordinatorRunController {
    pub fn new(cfg: ControlPlaneLoopConfig) -> Self {
        Self {
            cfg,
            started: Instant::now(),
            no_progress_cycles: 0,
            previous_counts: None,
        }
    }

    pub fn on_cycle_counts(&mut self, counts: CoordinatorCounts) -> Result<ControlPlaneDecision> {
        if counts.todo == 0 && counts.active == 0 {
            if counts.blocked > 0 {
                return Err(MaccError::Validation(format!(
                    "Coordinator run finished with blocked tasks: {}. Run `macc coordinator status`, then `macc coordinator unlock --all`, and inspect logs with `macc logs tail --component coordinator`.",
                    counts.blocked
                )));
            }
            return Ok(ControlPlaneDecision::Complete);
        }

        if counts.active > 0 {
            self.no_progress_cycles = 0;
        } else if self.previous_counts == Some(counts) {
            self.no_progress_cycles += 1;
        } else {
            self.no_progress_cycles = 0;
        }
        self.previous_counts = Some(counts);

        if self.no_progress_cycles >= self.cfg.max_no_progress_cycles {
            return Err(MaccError::Validation(format!(
                "Coordinator made no progress for {} cycles (todo={}, active={}, blocked={}). Run `macc coordinator status`, then `macc coordinator unlock --all`, and inspect logs with `macc logs tail --component coordinator`.",
                self.no_progress_cycles, counts.todo, counts.active, counts.blocked
            )));
        }

        if let Some(timeout) = self.cfg.timeout {
            if self.started.elapsed() >= timeout {
                return Err(MaccError::Validation(format!(
                    "Coordinator run timed out after {} seconds. Run `macc coordinator status` and `macc logs tail --component coordinator`.",
                    timeout.as_secs()
                )));
            }
        }

        Ok(ControlPlaneDecision::Continue)
    }
}

pub async fn run_control_plane<B: ControlPlaneBackend + ?Sized>(
    backend: &mut B,
    cfg: ControlPlaneLoopConfig,
) -> Result<()> {
    let mut controller = CoordinatorRunController::new(cfg);
    let mut cycle: usize = 0;
    loop {
        cycle += 1;
        backend.on_cycle_start(cycle).await?;

        backend.monitor_active_jobs().await?;
        if let Some((task_id, reason)) = backend.monitor_merge_jobs().await? {
            backend.on_blocked_merge(&task_id, &reason).await?;
        }

        let advance = backend.advance_tasks().await?;

        backend.monitor_active_jobs().await?;
        if let Some((task_id, reason)) = backend
            .monitor_merge_jobs()
            .await?
            .or_else(|| advance.blocked_merge.clone())
        {
            backend.on_blocked_merge(&task_id, &reason).await?;
        }

        let dispatched = backend.dispatch_ready_tasks().await?;

        if let Some((task_id, reason)) = backend.monitor_merge_jobs().await? {
            backend.on_blocked_merge(&task_id, &reason).await?;
        }

        let counts = backend.on_cycle_end(cycle, &advance, dispatched).await?;
        if backend.should_terminate_run(&counts) {
            return Ok(());
        }
        match controller.on_cycle_counts(counts) {
            Ok(ControlPlaneDecision::Continue) => {}
            Ok(ControlPlaneDecision::Complete) => return Ok(()),
            Err(err) => return Err(err),
        }

        backend.sleep_between_cycles().await?;
    }
}

struct NativeControlPlaneBackend<'a> {
    repo_root: &'a Path,
    canonical: &'a CanonicalConfig,
    coordinator: Option<&'a CoordinatorConfig>,
    env_cfg: &'a super::types::CoordinatorEnvConfig,
    logger: Option<&'a dyn CoordinatorLog>,
    prd_file: PathBuf,
    run_state: CoordinatorRunState,
    phase_runner_max_attempts: usize,
    coordinator_tool_override: Option<String>,
    phase_timeout_seconds: usize,
    last_logged_counts: Option<CoordinatorCounts>,
    last_cycle_progressed: bool,
}

#[async_trait]
impl ControlPlaneBackend for NativeControlPlaneBackend<'_> {
    async fn on_cycle_start(&mut self, _cycle: usize) -> Result<()> {
        crate::coordinator::control_plane::sync_registry_from_prd_native(
            self.repo_root,
            &self.prd_file,
            self.logger,
        )?;
        let logger = self.logger;
        let _ = process_branch_cleanup_queue(
            self.repo_root,
            |event_type, task_id, phase, status, message, severity| {
                let _ = crate::coordinator::helpers::append_coordinator_event_with_severity(
                    self.repo_root,
                    event_type,
                    task_id,
                    phase,
                    status,
                    message,
                    severity,
                );
            },
            Some(move |msg| {
                if let Some(log) = logger {
                    let _ = log.note(msg);
                }
            }),
        )?;
        let cleaned = if let Some(log) = self.logger {
            let note = |line: String| {
                let _ = log.note(line);
            };
            cleanup_dead_runtime_tasks(self.repo_root, "run-cycle", Some(&note))?
        } else {
            cleanup_dead_runtime_tasks(self.repo_root, "run-cycle", None)?
        };
        if cleaned > 0 {
            if let Some(log) = self.logger {
                let _ = log.note(format!("- Runtime cleanup fixed {} ghost task(s)", cleaned));
            }
        }
        Ok(())
    }

    async fn monitor_active_jobs(&mut self) -> Result<()> {
        crate::coordinator::control_plane::monitor_active_jobs_native(
            self.repo_root,
            self.env_cfg,
            &mut self.run_state,
            self.phase_runner_max_attempts,
            self.phase_timeout_seconds,
            self.logger,
        )
        .await
    }

    async fn monitor_merge_jobs(&mut self) -> Result<Option<(String, String)>> {
        crate::coordinator::control_plane::monitor_merge_jobs_native(
            self.repo_root,
            &mut self.run_state,
            self.logger,
        )
        .await
    }

    async fn on_blocked_merge(&mut self, task_id: &str, reason: &str) -> Result<()> {
        for (tid, pid) in terminate_active_jobs(&self.run_state) {
            if let Some(log) = self.logger {
                let _ = log.note(format!("- Sent TERM to active task={} pid={}", tid, pid));
            }
        }
        self.run_state.merge_join_set.abort_all();
        self.run_state.active_merge_jobs.clear();
        set_task_paused_for_integrate(self.repo_root, task_id, reason)?;
        write_coordinator_pause_file(self.repo_root, task_id, "integrate", reason)?;
        if let Some(log) = self.logger {
            let _ = log.note(format!(
                "- Run paused task={} phase=integrate reason={}",
                task_id, reason
            ));
        }
        loop {
            if !coordinator_pause_file_path(self.repo_root).exists() {
                if let Some(log) = self.logger {
                    let _ = log.note("- Resume signal received; continuing run loop".to_string());
                }
                break;
            }
            tokio::time::sleep(Duration::from_secs(1)).await;
        }
        resume_paused_task_integrate(self.repo_root, task_id)?;
        let _ = clear_coordinator_pause_file(self.repo_root)?;
        Ok(())
    }

    async fn advance_tasks(&mut self) -> Result<AdvanceResult> {
        crate::coordinator::control_plane::advance_tasks_native(
            self.repo_root,
            self.coordinator_tool_override.as_deref(),
            self.phase_runner_max_attempts,
            &mut self.run_state,
            self.logger,
        )
        .await
    }

    async fn dispatch_ready_tasks(&mut self) -> Result<usize> {
        crate::coordinator::control_plane::dispatch_ready_tasks_native(
            self.repo_root,
            self.canonical,
            self.coordinator,
            self.env_cfg,
            &self.prd_file,
            &mut self.run_state,
            self.logger,
        )
        .await
    }

    async fn on_cycle_end(
        &mut self,
        _cycle: usize,
        advance: &AdvanceResult,
        dispatched: usize,
    ) -> Result<CoordinatorCounts> {
        self.last_cycle_progressed = advance.progressed || dispatched > 0;
        let snapshot = crate::coordinator::state::coordinator_state_snapshot(
            self.repo_root,
            &std::collections::BTreeMap::new(),
        )?;
        let (total, todo, active, blocked, merged) = snapshot.registry.counts();
        let counts = CoordinatorCounts {
            total,
            todo,
            active,
            blocked,
            merged,
        };
        self.last_logged_counts = Some(counts);
        Ok(counts)
    }

    async fn sleep_between_cycles(&mut self) -> Result<()> {
        let ms = if self.last_cycle_progressed {
            500
        } else {
            2000
        };
        tokio::time::sleep(Duration::from_millis(ms)).await;
        Ok(())
    }

    fn should_terminate_run(&self, counts: &CoordinatorCounts) -> bool {
        let max_dispatch_total = self
            .env_cfg
            .max_dispatch
            .or_else(|| self.coordinator.and_then(|c| c.max_dispatch))
            .unwrap_or(10);
        if max_dispatch_total == 0 {
            return false;
        }
        self.run_state.dispatched_total_run >= max_dispatch_total
            && counts.active == 0
            && self.run_state.active_jobs.is_empty()
            && self.run_state.active_merge_jobs.is_empty()
    }
}

pub fn resolve_storage_mode(
    env_cfg: &super::types::CoordinatorEnvConfig,
    coordinator: Option<&CoordinatorConfig>,
) -> Result<CoordinatorStorageMode> {
    let raw = env_cfg
        .storage_mode
        .clone()
        .or_else(|| coordinator.and_then(|c| c.storage_mode.clone()))
        .unwrap_or_else(|| "sqlite".to_string());
    raw.parse::<CoordinatorStorageMode>()
        .map_err(MaccError::Validation)
}

pub fn sync_storage_with_startup_reconcile(
    project_paths: &crate::ProjectPaths,
    storage_mode: CoordinatorStorageMode,
    logger: Option<&dyn CoordinatorLog>,
) -> Result<()> {
    if storage_mode == CoordinatorStorageMode::Json {
        return Ok(());
    }
    let imported = coordinator_storage_bootstrap_sqlite_from_json(project_paths)?;
    if imported {
        if let Some(log) = logger {
            let _ = log.note("- Storage bootstrap: imported JSON snapshot into SQLite".to_string());
        }
    }
    if std::env::var("COORDINATOR_JSON_COMPAT")
        .ok()
        .map(|raw| {
            !matches!(
                raw.trim().to_ascii_lowercase().as_str(),
                "0" | "false" | "no" | "off"
            )
        })
        .unwrap_or(false)
    {
        coordinator_storage_export_sqlite_to_json(project_paths)?;
    }
    Ok(())
}

pub async fn run_native_control_plane(
    repo_root: &Path,
    canonical: &CanonicalConfig,
    coordinator: Option<&CoordinatorConfig>,
    env_cfg: &super::types::CoordinatorEnvConfig,
    logger: Option<&dyn CoordinatorLog>,
) -> Result<()> {
    let run_id = if let Ok(existing) = std::env::var("COORDINATOR_RUN_ID") {
        let trimmed = existing.trim();
        if trimmed.is_empty() {
            let generated = format!(
                "run-{}-{}",
                chrono::Utc::now().timestamp_nanos_opt().unwrap_or_default(),
                std::process::id()
            );
            std::env::set_var("COORDINATOR_RUN_ID", &generated);
            generated
        } else {
            trimmed.to_string()
        }
    } else {
        let generated = format!(
            "run-{}-{}",
            chrono::Utc::now().timestamp_nanos_opt().unwrap_or_default(),
            std::process::id()
        );
        std::env::set_var("COORDINATOR_RUN_ID", &generated);
        generated
    };

    let _ = crate::coordinator::helpers::append_coordinator_event_with_severity(
        repo_root,
        "command_start",
        "-",
        "run",
        "started",
        &format!("Coordinator run started (run_id={})", run_id),
        "info",
    );

    let prd_file = env_cfg
        .prd
        .as_ref()
        .map(PathBuf::from)
        .or_else(|| {
            coordinator
                .and_then(|c| c.prd_file.clone())
                .map(PathBuf::from)
        })
        .unwrap_or_else(|| repo_root.join("prd.json"));
    if !prd_file.exists() {
        return Err(MaccError::Validation(format!(
            "Coordinator PRD file not found: {}. Configure `automation.coordinator.prd_file` or pass `--prd`.",
            prd_file.display()
        )));
    }

    let phase_runner_max_attempts = env_cfg
        .phase_runner_max_attempts
        .or_else(|| coordinator.and_then(|c| c.phase_runner_max_attempts))
        .unwrap_or(1)
        .max(1);
    let coordinator_tool_override = env_cfg
        .coordinator_tool
        .clone()
        .or_else(|| coordinator.and_then(|c| c.coordinator_tool.clone()));
    let phase_timeout_seconds = env_cfg
        .stale_in_progress_seconds
        .or_else(|| coordinator.and_then(|c| c.stale_in_progress_seconds))
        .unwrap_or(0);

    let storage_mode = resolve_storage_mode(env_cfg, coordinator)?;
    let storage_paths = crate::ProjectPaths::from_root(repo_root);
    sync_storage_with_startup_reconcile(&storage_paths, storage_mode, logger)?;
    let startup_cleaned = if let Some(log) = logger {
        let note = |line: String| {
            let _ = log.note(line);
        };
        cleanup_dead_runtime_tasks(repo_root, "run-startup", Some(&note))?
    } else {
        cleanup_dead_runtime_tasks(repo_root, "run-startup", None)?
    };
    if startup_cleaned > 0 {
        if let Some(log) = logger {
            let _ = log.note(format!(
                "- Startup runtime cleanup fixed {} ghost task(s)",
                startup_cleaned
            ));
        }
    }

    let mut backend = NativeControlPlaneBackend {
        repo_root,
        canonical,
        coordinator,
        env_cfg,
        logger,
        prd_file,
        run_state: CoordinatorRunState::new(),
        phase_runner_max_attempts,
        coordinator_tool_override,
        phase_timeout_seconds,
        last_logged_counts: None,
        last_cycle_progressed: false,
    };

    let timeout_seconds = env_cfg
        .timeout_seconds
        .or_else(|| coordinator.and_then(|c| c.timeout_seconds))
        .unwrap_or(0);
    let loop_cfg = ControlPlaneLoopConfig {
        timeout: if timeout_seconds > 0 {
            Some(Duration::from_secs(timeout_seconds as u64))
        } else {
            None
        },
        max_no_progress_cycles: 2,
    };

    // Set up graceful shutdown signal handling
    let (shutdown_tx, mut shutdown_rx) = tokio::sync::watch::channel(false);

    #[cfg(unix)]
    {
        let mut sigterm = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
            .map_err(|e| MaccError::Io {
                path: "signal".into(),
                action: "setup sigterm handler".into(),
                source: e,
            })?;
        let mut sigint = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::interrupt())
            .map_err(|e| MaccError::Io {
                path: "signal".into(),
                action: "setup sigint handler".into(),
                source: e,
            })?;

        tokio::spawn(async move {
            tokio::select! {
                _ = sigterm.recv() => {
                    let _ = shutdown_tx.send(true);
                }
                _ = sigint.recv() => {
                    let _ = shutdown_tx.send(true);
                }
            }
        });
    }

    let run_result = tokio::select! {
        res = run_control_plane(&mut backend, loop_cfg) => res,
        _ = shutdown_rx.changed() => {
            if let Some(log) = logger {
                let _ = log.note("- Graceful shutdown signal received".to_string());
            }
            Ok(())
        }
    };

    let result_label = if run_result.is_err() {
        "failed"
    } else {
        let is_shutdown = *shutdown_rx.borrow();
        if is_shutdown {
            "stopped"
        } else if crate::coordinator::state_runtime::read_coordinator_pause_file(repo_root)
            .ok()
            .flatten()
            .is_some()
        {
            "paused"
        } else {
            "success"
        }
    };

    if let Some(log) = logger {
        let _ = log.note(format!(
            "\n# Run footer\n- ended_at: {}\n- result: {}\n",
            crate::coordinator::helpers::now_iso_coordinator(),
            result_label
        ));
    }

    if run_result.is_err() {
        if let Err(err) = &run_result {
            let _ = crate::coordinator::helpers::append_coordinator_event_with_severity(
                repo_root,
                "command_end",
                "-",
                "run",
                "failed",
                &format!("Coordinator run failed: {}", err),
                "blocking",
            );
        }
        for (_tid, _pid) in terminate_active_jobs(&backend.run_state) {}
        backend.run_state.active_jobs.clear();
        backend.run_state.join_set.abort_all();
        backend.run_state.active_merge_jobs.clear();
        backend.run_state.merge_join_set.abort_all();
        return run_result;
    }

    let _ = crate::coordinator::helpers::append_coordinator_event_with_severity(
        repo_root,
        "command_end",
        "-",
        "run",
        "done",
        "Coordinator run complete",
        "info",
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plan_advance_maps_states() {
        assert!(matches!(
            plan_advance(WorkflowState::InProgress),
            AdvancePlan::RunPhase(PhaseTransition { mode: "review", .. })
        ));
        assert!(matches!(
            plan_advance(WorkflowState::PrOpen),
            AdvancePlan::RunPhase(PhaseTransition {
                mode: "integrate",
                ..
            })
        ));
        assert!(matches!(
            plan_advance(WorkflowState::ChangesRequested),
            AdvancePlan::RunPhase(PhaseTransition { mode: "fix", .. })
        ));
        assert!(matches!(
            plan_advance(WorkflowState::Queued),
            AdvancePlan::Merge
        ));
        assert!(matches!(
            plan_advance(WorkflowState::Todo),
            AdvancePlan::Noop
        ));
    }

    #[test]
    fn apply_phase_failure_sets_blocked_failed() {
        let mut task = json!({ "id": "T1", "state": "in_progress" });
        apply_phase_failure(&mut task, "review", "boom", "2026-02-20T00:00:00Z").unwrap();
        assert_eq!(task["state"], "blocked");
        assert_eq!(task["task_runtime"]["status"], "failed");
        assert_eq!(task["task_runtime"]["current_phase"], "review");
    }

    #[test]
    fn apply_dispatch_claim_sets_runtime_and_worktree() {
        let mut task = json!({ "id": "T2", "state": "todo" });
        let update = DispatchClaimUpdate {
            task_id: "T2".to_string(),
            tool: "codex".to_string(),
            worktree_path: "/tmp/wt".to_string(),
            branch: "ai/codex/x".to_string(),
            base_branch: "main".to_string(),
            last_commit: "abc".to_string(),
            session_id: "s-1".to_string(),
            pid: Some(123),
            phase: "dev".to_string(),
            now: "2026-02-20T00:00:00Z".to_string(),
        };
        apply_dispatch_claim(&mut task, &update);
        assert_eq!(task["state"], "claimed");
        assert_eq!(task["task_runtime"]["status"], "running");
        assert_eq!(task["task_runtime"]["pid"], 123);
    }

    #[test]
    fn apply_job_completion_success_sets_in_progress() {
        let mut task =
            json!({"id":"T3","state":"claimed","task_runtime":{"status":"running","pid":123}});
        let out = apply_job_completion(
            &mut task,
            &JobCompletionInput {
                success: true,
                attempt: 1,
                max_attempts: 1,
                timed_out: false,
                phase_timeout_seconds: 0,
                elapsed_seconds: 2,
                status_text: "exit status: 0".to_string(),
                error_code: None,
                error_origin: None,
                error_message: None,
                auto_retry_error_codes: Vec::new(),
                auto_retry_max: 0,
            },
            "2026-02-21T00:00:00Z",
        );
        assert!(!out.should_retry);
        assert_eq!(task["state"], "in_progress");
        assert_eq!(task["task_runtime"]["status"], "phase_done");
        assert!(task["task_runtime"]["pid"].is_null());
    }

    #[test]
    fn cleanup_dead_runtime_tasks_resets_claimed_dev_to_todo() {
        let mut registry = json!({
            "tasks": [{
                "id":"T4",
                "state":"claimed",
                "assignee":"agentA",
                "task_runtime":{
                    "status":"running",
                    "current_phase":"dev",
                    "pid":999
                }
            }]
        });
        let cleaned = cleanup_dead_runtime_tasks_in_registry_with(
            &mut registry,
            "2026-02-21T00:00:00Z",
            0,
            |_| false,
        )
        .unwrap();
        assert_eq!(cleaned.len(), 1);
        assert_eq!(registry["tasks"][0]["state"], "todo");
        assert!(registry["tasks"][0]["assignee"].is_null());
        assert_eq!(registry["tasks"][0]["task_runtime"]["status"], "stale");
        assert!(registry["tasks"][0]["task_runtime"]["pid"].is_null());
    }

    #[test]
    fn cleanup_dead_runtime_tasks_respects_recent_heartbeat_grace() {
        let mut registry = json!({
            "tasks": [{
                "id":"T4b",
                "state":"claimed",
                "assignee":"agentA",
                "task_runtime":{
                    "status":"running",
                    "current_phase":"dev",
                    "pid":999,
                    "last_heartbeat":"2026-02-21T00:00:30Z"
                }
            }]
        });
        let cleaned = cleanup_dead_runtime_tasks_in_registry_with(
            &mut registry,
            "2026-02-21T00:01:00Z",
            60,
            |_| false,
        )
        .unwrap();
        assert_eq!(cleaned.len(), 0);
        assert_eq!(registry["tasks"][0]["state"], "claimed");
        assert_eq!(registry["tasks"][0]["task_runtime"]["status"], "running");
        assert_eq!(registry["tasks"][0]["task_runtime"]["pid"], 999);
    }

    #[test]
    fn fsm_rejects_skipping_review_phase() {
        let mut task = json!({"id":"T5","state":"in_progress","task_runtime":{"status":"running"}});
        let err = apply_phase_success(
            &mut task,
            PhaseTransition {
                mode: "integrate",
                next_state: WorkflowState::Queued,
                runtime_phase: "integrate",
            },
            "2026-02-21T00:00:00Z",
        )
        .unwrap_err();
        assert!(
            err.to_string()
                .contains("Invalid coordinator FSM transition"),
            "unexpected error: {}",
            err
        );
    }
}
