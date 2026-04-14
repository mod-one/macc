use super::{
    resolve_completion_authority, CompletionAuthority, PerformerCompletionKind, RuntimeStatus,
    WorkflowState,
};
use crate::config::{
    CanonicalConfig, CoordinatorConfig, CoordinatorConfigResolved as CanonicalCoordinatorConfigResolved,
};
use crate::coordinator::control_plane::CoordinatorLog;
use crate::coordinator::error_normalizer::{
    error_code_to_canonical_class, CanonicalClass, NormalizerRegistry, ToolError,
};
use crate::coordinator::model::{Task, TaskRegistry};
use crate::coordinator::rate_limit::{
    compute_backoff_delay, update_throttle_state, RateLimitInfo, ToolThrottleState,
    E601_RATE_LIMITED, E602_QUOTA_EXHAUSTED,
};
use crate::coordinator::runtime::{
    merge_task_with_policy_native, process_branch_cleanup_queue, terminate_active_jobs,
    CoordinatorRunState,
};
use crate::coordinator::state_runtime::{
    cleanup_dead_runtime_tasks, clear_coordinator_pause_file, coordinator_pause_file_path,
    resume_paused_task_merge, set_task_paused_for_merge, write_coordinator_pause_file,
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

pub mod completion;
pub use completion::{classify_completion_error, ErrorClassification};

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
    pub active_session_id: Option<String>,
    pub pid: Option<i64>,
    pub phase: String,
    pub now: String,
}

/// Raw output captured from a performer process for error normalization.
/// Passed as part of [`JobCompletionInput`] when the caller has access to
/// the process's exit code and stdio streams.
#[derive(Debug, Clone, Default)]
pub struct NormalizerInput {
    /// Process exit code (non-zero indicates failure).
    pub exit_code: i32,
    /// Full stderr text from the performer process.
    pub stderr: String,
    /// Full stdout text from the performer process.
    pub stdout: String,
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
    pub completion_kind: Option<PerformerCompletionKind>,
    pub error_code: Option<String>,
    pub error_origin: Option<String>,
    pub error_message: Option<String>,
    pub auto_retry_error_codes: Vec<String>,
    pub auto_retry_max: usize,
    /// Base backoff delay in seconds for E601 rate-limit retries.
    /// Resolved from config; defaults to 30 when not set.
    pub backoff_base_seconds: u64,
    /// Maximum backoff delay in seconds for E601 rate-limit retries.
    /// Resolved from config; defaults to 300 when not set.
    pub backoff_max_seconds: u64,
    /// Raw performer output for per-adapter error normalization.
    /// `None` when the caller does not have access to the raw process output
    /// (e.g. in legacy paths or unit tests that pre-classify errors).
    pub normalizer_input: Option<NormalizerInput>,
}

#[derive(Debug, Clone)]
pub struct JobCompletionResult {
    pub should_retry: bool,
    pub status_label: &'static str,
    pub detail: String,
    pub completion_kind: Option<PerformerCompletionKind>,
    /// Canonical error classification produced by the per-adapter normalizer.
    /// `None` when the job succeeded or the normalizer was not invoked.
    pub tool_error: Option<ToolError>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SalvageResult {
    Merged,
    ConflictProceedRetry,
    NoCommitsProceedRetry,
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
        merge_context: MergeTaskContext,
    },
}

/// Task metadata passed to the merge-fix hook so it can resolve conflicts
/// without needing to read the task registry from disk.
#[derive(Debug, Clone, Default)]
pub struct MergeTaskContext {
    pub tool: String,
    pub worktree_path: String,
    pub title: String,
    pub description: String,
    pub objective: String,
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
    fn should_terminate_run(&self, _counts: &CoordinatorCounts) -> Option<String> {
        None
    }
    fn last_dispatch_failure(&self) -> Option<String> {
        None
    }
}

/// Plan the next advance action for a task.
///
/// `max_review_cycles` controls the review→fix loop budget:
///   - `None`  → unlimited loops (default)
///   - `Some(0)` → skip review entirely; go straight to merge
///   - `Some(1)` → one review + one fix if changes requested, no loopback
///   - `Some(n)` → up to n review→fix→review loops
///
/// `review_cycles` is the number of review cycles already completed for
/// this task (stored in `task_runtime.review_cycles`).
pub fn plan_advance(
    state: WorkflowState,
    max_review_cycles: Option<usize>,
    review_cycles: usize,
) -> AdvancePlan {
    match state {
        WorkflowState::InProgress => {
            if max_review_cycles == Some(0) {
                // No review at all — go straight to merge.
                return AdvancePlan::Merge;
            }
            AdvancePlan::RunPhase(PhaseTransition {
                mode: "review",
                next_state: WorkflowState::PrOpen,
                runtime_phase: "review",
            })
        }
        // Review approved → merge.
        WorkflowState::PrOpen => AdvancePlan::Merge,
        WorkflowState::ChangesRequested => {
            // If we've exhausted the review cycle budget, skip the fix→review
            // loop and go straight to merge with whatever we have.
            if let Some(max) = max_review_cycles {
                if review_cycles >= max {
                    return AdvancePlan::Merge;
                }
            }
            AdvancePlan::RunPhase(PhaseTransition {
                mode: "fix",
                next_state: WorkflowState::PrOpen,
                runtime_phase: "fix",
            })
        }
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
        (WorkflowState::ChangesRequested, WorkflowEvent::PhaseSucceeded("fix")) => {
            WorkflowState::PrOpen
        }
        (WorkflowState::InProgress, WorkflowEvent::PhaseFailed("review"))
        | (WorkflowState::ChangesRequested, WorkflowEvent::PhaseFailed("fix"))
        | (WorkflowState::InProgress, WorkflowEvent::MergeFailed)
        | (WorkflowState::PrOpen, WorkflowEvent::MergeFailed)
        | (WorkflowState::ChangesRequested, WorkflowEvent::MergeFailed)
        | (WorkflowState::Queued, WorkflowEvent::MergeFailed) => WorkflowState::Blocked,
        (WorkflowState::InProgress, WorkflowEvent::MergeSucceeded)
        | (WorkflowState::PrOpen, WorkflowEvent::MergeSucceeded)
        | (WorkflowState::ChangesRequested, WorkflowEvent::MergeSucceeded)
        | (WorkflowState::Queued, WorkflowEvent::MergeSucceeded) => WorkflowState::Merged,
        _ => {
            return Err(MaccError::Coordinator {
                code: "invalid_transition",
                message: format!(
                    "Invalid coordinator FSM transition: from={} event={:?}",
                    from.as_str(),
                    event
                ),
            });
        }
    };

    if !super::is_valid_workflow_transition(from, to) {
        return Err(MaccError::Coordinator {
            code: "invalid_transition",
            message: format!(
                "Coordinator FSM produced invalid workflow transition {} -> {}",
                from.as_str(),
                to.as_str()
            ),
        });
    }

    Ok(to)
}

pub fn apply_dispatch_claim_in_registry(
    registry: &mut Value,
    update: &DispatchClaimUpdate,
) -> Result<()> {
    let mut typed = TaskRegistry::from_value(registry)?;
    let task = typed
        .find_task_mut(&update.task_id)
        .ok_or_else(|| MaccError::Coordinator {
            code: "task_not_found",
            message: format!("Task '{}' not found in registry", update.task_id),
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
    let task = typed
        .find_task_mut(task_id)
        .ok_or_else(|| MaccError::Coordinator {
            code: "task_not_found",
            message: format!("Task '{}' not found in registry", task_id),
        })?;
    apply_dispatch_pid_typed(task, pid);
    *registry = typed.to_value()?;
    Ok(())
}

pub fn build_advance_actions(
    registry: &Value,
    active_merge_jobs: &HashSet<String>,
    now: &str,
    max_review_cycles: Option<usize>,
) -> Result<Vec<AdvanceTaskAction>> {
    let typed = TaskRegistry::from_value(registry)?;
    let mut actions = Vec::new();
    for task in &typed.tasks {
        // Skip tasks that are delayed (e.g. waiting for a throttled tool to
        // recover while preserving committed work on their worktree).
        if crate::coordinator::rate_limit::is_task_delayed(task, now) {
            continue;
        }
        let task_id = task.id.clone();
        let review_cycles = task.task_runtime.review_cycles.unwrap_or(0);
        let workflow_state = task.workflow_state();
        match workflow_state
            .map(|s| plan_advance(s, max_review_cycles, review_cycles))
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
                let merge_context = MergeTaskContext {
                    tool: task.tool.clone().unwrap_or_default(),
                    worktree_path: task.worktree_path().unwrap_or_default().to_string(),
                    title: task.title.clone().unwrap_or_default(),
                    description: task
                        .extra
                        .get("description")
                        .and_then(|v| v.as_str())
                        .unwrap_or_default()
                        .to_string(),
                    objective: task
                        .extra
                        .get("objective")
                        .and_then(|v| v.as_str())
                        .unwrap_or_default()
                        .to_string(),
                };
                actions.push(AdvanceTaskAction::QueueMerge {
                    task_id,
                    branch,
                    base,
                    merge_context,
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
    let task = typed
        .find_task_mut(task_id)
        .ok_or_else(|| MaccError::Coordinator {
            code: "task_not_found",
            message: format!("Task '{}' not found in registry", task_id),
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
    normalizer_registry: &NormalizerRegistry,
    now: &str,
) -> Result<JobCompletionResult> {
    let mut typed = TaskRegistry::from_value(registry)?;
    let task = typed
        .find_task_mut(task_id)
        .ok_or_else(|| MaccError::Coordinator {
            code: "task_not_found",
            message: format!("Task '{}' not found in registry", task_id),
        })?;
    let out = apply_job_completion_typed(task, input, normalizer_registry, now);
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
    let task = typed
        .find_task_mut(task_id)
        .ok_or_else(|| MaccError::Coordinator {
            code: "task_not_found",
            message: format!("Task '{}' not found in registry", task_id),
        })?;
    if success {
        apply_merge_success_typed(task, now)?
    } else {
        apply_merge_failure_typed(task, reason, now)?
    }
    *registry = typed.to_value()?;
    Ok(())
}

pub fn salvage_check_in_registry<FE>(
    registry: &mut Value,
    task_id: &str,
    repo_root: &Path,
    merge_ai_fix: bool,
    merge_hook_timeout: Option<u64>,
    now: &str,
    mut emit_event: FE,
) -> Result<SalvageResult>
where
    FE: FnMut(&str, &str, &str, &str, &str, &str),
{
    let mut typed = TaskRegistry::from_value(registry)?;
    let task = typed
        .find_task_mut(task_id)
        .ok_or_else(|| MaccError::Coordinator {
            code: "task_not_found",
            message: format!("Task '{}' not found in registry", task_id),
        })?;
    let out = salvage_check_typed(
        task,
        repo_root,
        merge_ai_fix,
        merge_hook_timeout,
        now,
        &mut emit_event,
    )?;
    *registry = typed.to_value()?;
    Ok(out)
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
    if let Some(active_session_id) = update.active_session_id.as_ref() {
        runtime.active_session_id = Some(active_session_id.clone());
    }
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
    normalizer_registry: &NormalizerRegistry,
    now: &str,
) -> JobCompletionResult {
    let mut typed = parse_compat_task(task);
    let result = apply_job_completion_typed(&mut typed, input, normalizer_registry, now);
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
    runtime.delayed_until = None;
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
    runtime.delayed_until = None;
    // Track the number of completed review cycles for max_review_cycles enforcement.
    let cycles = runtime.review_cycles.unwrap_or(0) + 1;
    runtime.review_cycles = Some(cycles);
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
    runtime.current_phase = Some("merge".to_string());
    runtime.last_error = Some(reason.to_string());
    runtime.pid = None;
    task.touch_state_changed(now);
    Ok(())
}

/// Store a classified [`ToolError`] (and, when applicable, [`RateLimitInfo`])
/// into `task_runtime.extra` for diagnostics and downstream consumers.
fn store_classified_error_in_extra(
    runtime: &mut crate::coordinator::model::TaskRuntime,
    tool_error: &Option<ToolError>,
    now_ts: u64,
) {
    let Some(te) = tool_error else { return };
    if let Ok(v) = serde_json::to_value(te) {
        runtime.extra.insert("tool_error".to_string(), v);
    }
    if matches!(
        te.canonical_class,
        CanonicalClass::RateLimit | CanonicalClass::QuotaExhausted
    ) {
        let rli = RateLimitInfo {
            tool_id: te.provider.clone(),
            error_code: te.error_code.clone(),
            retry_after_seconds: te.retry_after_seconds,
            detected_at: now_ts,
            source_header: None,
        };
        if let Ok(v) = serde_json::to_value(&rli) {
            runtime.extra.insert("rate_limit_info".to_string(), v);
        }
    }
}

fn capture_last_assignment_before_clear(task: &mut Task) {
    let (last_worktree_path, last_branch) = match task.worktree.as_ref() {
        Some(worktree) => (
            worktree
                .worktree_path
                .as_deref()
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .map(str::to_string),
            worktree
                .branch
                .as_deref()
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .map(str::to_string),
        ),
        None => (None, None),
    };
    let runtime = task.ensure_runtime();
    if let Some(path) = last_worktree_path {
        runtime.last_worktree_path = Some(path);
    }
    if let Some(branch) = last_branch {
        runtime.last_branch = Some(branch);
    }
}

fn preserve_active_session_chain(task: &mut Task) {
    let tool = task.tool.clone();
    let active_session = task
        .task_runtime
        .active_session_id
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
        .or_else(|| {
            task.session_id()
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .map(str::to_string)
        });
    if let Some(session_id) = active_session {
        let runtime = task.ensure_runtime();
        runtime.last_session_id = Some(session_id);
        runtime.last_session_tool = tool;
    }
}

fn git_path_for(worktree_path: &Path, relative_path: &str) -> Option<PathBuf> {
    let output = crate::git::run_git_output_mapped(
        worktree_path,
        &["rev-parse", "--git-path", relative_path],
        "resolve git path",
    )
    .ok()?;
    if !output.status.success() {
        return None;
    }
    let resolved = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if resolved.is_empty() {
        return None;
    }
    let git_path = PathBuf::from(resolved);
    Some(if git_path.is_absolute() {
        git_path
    } else {
        worktree_path.join(git_path)
    })
}

pub fn is_worktree_healthy(worktree_path: &Path) -> bool {
    if !worktree_path.exists() || !worktree_path.is_dir() {
        return false;
    }

    let inside_worktree = crate::git::run_git_output_mapped(
        worktree_path,
        &["rev-parse", "--is-inside-work-tree"],
        "check git worktree",
    )
    .ok()
    .map(|output| {
        output.status.success()
            && String::from_utf8_lossy(&output.stdout)
                .trim()
                .eq_ignore_ascii_case("true")
    })
    .unwrap_or(false);
    if !inside_worktree {
        return false;
    }

    // Any lock file indicates concurrent git mutation; avoid reusing this slot.
    let has_lock = ["index.lock", "HEAD.lock", "packed-refs.lock"]
        .iter()
        .any(|path| git_path_for(worktree_path, path).is_some_and(|p| p.exists()));
    if has_lock {
        return false;
    }

    // Active merge/rebase state is unsafe for a retry-on-same-worktree run.
    let has_rebase_or_merge = ["MERGE_HEAD", "rebase-apply", "rebase-merge"]
        .iter()
        .any(|path| git_path_for(worktree_path, path).is_some_and(|p| p.exists()));
    if has_rebase_or_merge {
        return false;
    }

    true
}

fn salvage_check_typed<FE>(
    task: &mut Task,
    repo_root: &Path,
    merge_ai_fix: bool,
    merge_hook_timeout: Option<u64>,
    now: &str,
    emit_event: &mut FE,
) -> Result<SalvageResult>
where
    FE: FnMut(&str, &str, &str, &str, &str, &str),
{
    let runtime = &task.task_runtime;
    let worktree_path = runtime
        .last_worktree_path
        .as_deref()
        .or_else(|| task.worktree_path())
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string);
    let branch = runtime
        .last_branch
        .as_deref()
        .or_else(|| task.branch())
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string);
    let Some(worktree_path) = worktree_path else {
        return Ok(SalvageResult::NoCommitsProceedRetry);
    };
    let Some(branch) = branch else {
        return Ok(SalvageResult::NoCommitsProceedRetry);
    };
    let base_branch = task.base_branch("master").to_string();
    let commits = match crate::git::commits_containing_pattern(
        Path::new(&worktree_path),
        &base_branch,
        &task.id,
    ) {
        Ok(commits) => commits,
        Err(_) => return Ok(SalvageResult::NoCommitsProceedRetry),
    };
    if commits.is_empty() {
        return Ok(SalvageResult::NoCommitsProceedRetry);
    }

    let description = task
        .extra
        .get("description")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string();
    let objective = task
        .extra
        .get("objective")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string();
    let merge_context = MergeTaskContext {
        tool: task.tool.clone().unwrap_or_default(),
        worktree_path,
        title: task.title.clone().unwrap_or_default(),
        description,
        objective,
    };
    match merge_task_with_policy_native(
        repo_root,
        task.id.as_str(),
        &branch,
        &base_branch,
        merge_ai_fix,
        merge_hook_timeout,
        &merge_context,
        emit_event,
    ) {
        Ok(Ok(())) => {
            task.set_workflow_state(WorkflowState::Merged);
            let runtime = task.ensure_runtime();
            runtime.set_status(RuntimeStatus::Idle);
            runtime.current_phase = None;
            runtime.pid = None;
            task.touch_state_changed(now);
            Ok(SalvageResult::Merged)
        }
        Ok(Err(_)) | Err(_) => Ok(SalvageResult::ConflictProceedRetry),
    }
}

#[derive(Debug, Clone)]
struct CompletionErrorDetails {
    code: String,
    origin: String,
    message: String,
}

impl CompletionErrorDetails {
    fn from_classification(classification: &ErrorClassification) -> Self {
        Self {
            code: classification.error_code.clone(),
            origin: classification.error_origin.clone(),
            message: classification.error_message.clone(),
        }
    }
}

#[derive(Debug, Clone)]
struct CoordinatorConfigResolved {
    auto_retry_error_codes: Vec<String>,
    auto_retry_max: usize,
    backoff_base_seconds: u64,
    backoff_max_seconds: u64,
}

impl CoordinatorConfigResolved {
    fn from_input(input: &JobCompletionInput) -> Self {
        Self {
            auto_retry_error_codes: input.auto_retry_error_codes.clone(),
            auto_retry_max: input.auto_retry_max,
            backoff_base_seconds: input.backoff_base_seconds,
            backoff_max_seconds: input.backoff_max_seconds,
        }
    }
}

#[derive(Debug, Clone)]
enum RetryOutcome {
    ToolReportedError {
        completion_kind: PerformerCompletionKind,
        tool_error: Option<ToolError>,
    },
    AttemptRetry {
        error: CompletionErrorDetails,
        tool_error: Option<ToolError>,
        now_ts: u64,
    },
    RateLimitBackoff {
        backoff: u64,
        delayed_until: String,
        error: CompletionErrorDetails,
        tool_error: Option<ToolError>,
        now_ts: u64,
    },
    QuotaExhaustedRequeue {
        cooldown: u64,
        delayed_until: String,
        error: CompletionErrorDetails,
        tool_error: Option<ToolError>,
        now_ts: u64,
    },
    AutoRetry {
        error: CompletionErrorDetails,
        tool_error: Option<ToolError>,
        now_ts: u64,
    },
}

#[derive(Debug, Clone)]
enum BlockOutcome {
    InvalidInput,
    TerminalFailure {
        error: CompletionErrorDetails,
        tool_error: Option<ToolError>,
        now_ts: u64,
    },
}

#[allow(dead_code)]
#[derive(Debug, Clone)]
enum RetryStrategy {
    Retry {
        same_worktree: bool,
        reason: String,
        outcome: RetryOutcome,
    },
    Block {
        reason: String,
        outcome: BlockOutcome,
    },
    Merge {
        detail: String,
        completion_kind: PerformerCompletionKind,
        tool_error: Option<ToolError>,
    },
    PhaseDone {
        detail: String,
        completion_kind: PerformerCompletionKind,
    },
    NoOp,
}

fn resolve_retry_strategy(
    task: &Task,
    input: &JobCompletionInput,
    classification: &ErrorClassification,
    config: &CoordinatorConfigResolved,
    is_healthy_worktree: bool,
    now: &str,
) -> RetryStrategy {
    if input.attempt == 0 || input.max_attempts == 0 {
        return RetryStrategy::Block {
            reason: "performer completion received with invalid attempt counters".to_string(),
            outcome: BlockOutcome::InvalidInput,
        };
    }
    if input.status_text.is_empty() {
        return RetryStrategy::Block {
            reason: "performer completion received without status detail".to_string(),
            outcome: BlockOutcome::InvalidInput,
        };
    }

    let error_details = CompletionErrorDetails::from_classification(classification);
    let tool_error = classification.tool_error.clone();
    let now_ts = chrono::DateTime::parse_from_rfc3339(now)
        .map(|dt| dt.timestamp() as u64)
        .unwrap_or(0);

    if classification.completion_success {
        let completion_kind = classification
            .completion_kind
            .unwrap_or(PerformerCompletionKind::SuccessWithChanges);
        if completion_kind.is_error() {
            let same_worktree = completion_kind == PerformerCompletionKind::ErrorWithChanges
                && classification.has_commits
                && is_healthy_worktree;
            return RetryStrategy::Retry {
                same_worktree,
                reason: input.status_text.clone(),
                outcome: RetryOutcome::ToolReportedError {
                    completion_kind,
                    tool_error: None,
                },
            };
        }
        if completion_kind == PerformerCompletionKind::AlreadySatisfied
            || completion_kind == PerformerCompletionKind::SuccessWithoutChanges
        {
            return RetryStrategy::Merge {
                detail: input.status_text.clone(),
                completion_kind,
                tool_error: None,
            };
        }
        return RetryStrategy::PhaseDone {
            detail: input.status_text.clone(),
            completion_kind,
        };
    }

    if let Some(ref ni) = input.normalizer_input {
        let stdout_says_done = ni.stdout.contains("MACC_TASK_RESULT: success")
            || ni.stdout.contains("already_satisfied");
        let transient_error = tool_error
            .as_ref()
            .map(|te| {
                matches!(
                    te.canonical_class,
                    CanonicalClass::Overloaded | CanonicalClass::RateLimit
                )
            })
            .unwrap_or(false);
        if stdout_says_done && transient_error {
            return RetryStrategy::Merge {
                detail: "exit-code override: stdout indicates completion despite transient error"
                    .to_string(),
                completion_kind: PerformerCompletionKind::AlreadySatisfied,
                tool_error,
            };
        }

        let stdout_says_error = ni.stdout.contains("MACC_TASK_RESULT: error_with_changes")
            || ni
                .stdout
                .contains("MACC_TASK_RESULT: error_without_changes");
        if stdout_says_error {
            let completion_kind = if ni.stdout.contains("error_with_changes") {
                PerformerCompletionKind::ErrorWithChanges
            } else {
                PerformerCompletionKind::ErrorWithoutChanges
            };
            let same_worktree = completion_kind == PerformerCompletionKind::ErrorWithChanges
                && classification.has_commits
                && is_healthy_worktree;
            return RetryStrategy::Retry {
                same_worktree,
                reason: input.status_text.clone(),
                outcome: RetryOutcome::ToolReportedError {
                    completion_kind,
                    tool_error,
                },
            };
        }
    }

    if error_details.code == E601_RATE_LIMITED {
        let retry_after = tool_error.as_ref().and_then(|te| te.retry_after_seconds);
        let backoff = compute_backoff_delay(
            input.attempt,
            config.backoff_base_seconds,
            config.backoff_max_seconds,
            retry_after,
        );
        let delayed_until = chrono::DateTime::parse_from_rfc3339(now)
            .ok()
            .and_then(|dt| dt.checked_add_signed(chrono::Duration::seconds(backoff as i64)))
            .map(|dt| dt.to_rfc3339_opts(chrono::SecondsFormat::Secs, true))
            .unwrap_or_default();
        return RetryStrategy::Retry {
            same_worktree: false,
            reason: format!(
                "rate-limited; backoff {}s, delayed until {}",
                backoff, delayed_until
            ),
            outcome: RetryOutcome::RateLimitBackoff {
                backoff,
                delayed_until,
                error: error_details,
                tool_error,
                now_ts,
            },
        };
    }

    if error_details.code == E602_QUOTA_EXHAUSTED {
        let retry_after = tool_error.as_ref().and_then(|te| te.retry_after_seconds);
        let cooldown = retry_after.unwrap_or(3600);
        let delayed_until = chrono::DateTime::parse_from_rfc3339(now)
            .ok()
            .and_then(|dt| dt.checked_add_signed(chrono::Duration::seconds(cooldown as i64)))
            .map(|dt| dt.to_rfc3339_opts(chrono::SecondsFormat::Secs, true))
            .unwrap_or_default();
        return RetryStrategy::Retry {
            same_worktree: false,
            reason: format!(
                "quota exhausted; cooldown {}s, re-queued for tool fallback",
                cooldown
            ),
            outcome: RetryOutcome::QuotaExhaustedRequeue {
                cooldown,
                delayed_until,
                error: error_details,
                tool_error,
                now_ts,
            },
        };
    }

    if input.attempt < input.max_attempts {
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
        return RetryStrategy::Retry {
            same_worktree: true,
            reason,
            outcome: RetryOutcome::AttemptRetry {
                error: error_details,
                tool_error,
                now_ts,
            },
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
        &error_details.code,
        &config.auto_retry_error_codes,
        config.auto_retry_max,
        retries_total,
    ) {
        return RetryStrategy::Retry {
            same_worktree: false,
            reason: format!("auto-retry scheduled for error code {}", error_details.code),
            outcome: RetryOutcome::AutoRetry {
                error: error_details,
                tool_error,
                now_ts,
            },
        };
    }

    RetryStrategy::Block {
        reason,
        outcome: BlockOutcome::TerminalFailure {
            error: error_details,
            tool_error,
            now_ts,
        },
    }
}

fn apply_state_transitions(
    task: &mut Task,
    strategy: &RetryStrategy,
    now: &str,
) -> JobCompletionResult {
    match strategy {
        RetryStrategy::Retry {
            same_worktree,
            reason,
            outcome,
        } => match outcome {
            RetryOutcome::ToolReportedError {
                completion_kind,
                tool_error,
            } => {
                task.set_workflow_state(WorkflowState::Todo);
                preserve_active_session_chain(task);
                capture_last_assignment_before_clear(task);
                if !same_worktree {
                    task.worktree = None;
                }
                let runtime = task.ensure_runtime();
                runtime.completion_kind = Some(completion_kind.as_str().to_string());
                runtime.set_status(RuntimeStatus::Failed);
                runtime.current_phase = None;
                runtime.pid = None;
                runtime.last_error = Some(reason.clone());
                task.tool = None;
                task.assignee = None;
                task.touch_state_changed(now);
                JobCompletionResult {
                    should_retry: false,
                    status_label: completion_kind.as_str(),
                    detail: reason.clone(),
                    completion_kind: Some(*completion_kind),
                    tool_error: tool_error.clone(),
                }
            }
            RetryOutcome::AttemptRetry {
                error,
                tool_error,
                now_ts,
            } => {
                task.set_workflow_state(WorkflowState::Claimed);
                let runtime = task.ensure_runtime();
                runtime.set_status(RuntimeStatus::Running);
                runtime.current_phase = Some("dev".to_string());
                runtime.completion_kind = None;
                runtime.pid = None;
                runtime.set_last_error_details(
                    error.code.clone(),
                    error.origin.clone(),
                    error.message.clone(),
                );
                runtime.last_error = Some(reason.clone());
                store_classified_error_in_extra(runtime, tool_error, *now_ts);
                task.touch_state_changed(now);
                JobCompletionResult {
                    should_retry: true,
                    status_label: "retry",
                    detail: reason.clone(),
                    completion_kind: None,
                    tool_error: tool_error.clone(),
                }
            }
            RetryOutcome::RateLimitBackoff {
                backoff,
                delayed_until,
                error,
                tool_error,
                now_ts,
            } => {
                let tool_id_str = task.tool.as_deref().unwrap_or("").to_string();
                let runtime = task.ensure_runtime();
                let mut throttle: ToolThrottleState = runtime
                    .extra
                    .get("throttle_state")
                    .and_then(|v| serde_json::from_value(v.clone()).ok())
                    .unwrap_or_else(|| ToolThrottleState {
                        tool_id: tool_id_str.clone(),
                        ..Default::default()
                    });
                let rli = RateLimitInfo {
                    tool_id: tool_id_str,
                    error_code: E601_RATE_LIMITED.to_string(),
                    retry_after_seconds: tool_error.as_ref().and_then(|te| te.retry_after_seconds),
                    detected_at: *now_ts,
                    source_header: None,
                };
                update_throttle_state(&mut throttle, &rli, *backoff, *now_ts);
                if let Ok(v) = serde_json::to_value(&throttle) {
                    runtime.extra.insert("throttle_state".to_string(), v);
                }
                runtime.delayed_until = Some(delayed_until.clone());
                runtime.set_status(RuntimeStatus::Idle);
                runtime.current_phase = Some("dev".to_string());
                runtime.completion_kind = None;
                runtime.pid = None;
                runtime.set_last_error_details(
                    error.code.clone(),
                    error.origin.clone(),
                    error.message.clone(),
                );
                runtime.last_error = Some(format!("rate-limited; backoff {}s", backoff));
                store_classified_error_in_extra(runtime, tool_error, *now_ts);
                capture_last_assignment_before_clear(task);
                task.worktree = None;
                task.set_workflow_state(WorkflowState::Todo);
                task.touch_state_changed(now);
                JobCompletionResult {
                    should_retry: false,
                    status_label: "rate_limit_backoff",
                    detail: reason.clone(),
                    completion_kind: None,
                    tool_error: tool_error.clone(),
                }
            }
            RetryOutcome::QuotaExhaustedRequeue {
                cooldown,
                delayed_until,
                error,
                tool_error,
                now_ts,
            } => {
                let tool_id_str = task.tool.as_deref().unwrap_or("").to_string();
                let runtime = task.ensure_runtime();
                let mut throttle: ToolThrottleState = runtime
                    .extra
                    .get("throttle_state")
                    .and_then(|v| serde_json::from_value(v.clone()).ok())
                    .unwrap_or_else(|| ToolThrottleState {
                        tool_id: tool_id_str.clone(),
                        ..Default::default()
                    });
                let rli = RateLimitInfo {
                    tool_id: tool_id_str,
                    error_code: E602_QUOTA_EXHAUSTED.to_string(),
                    retry_after_seconds: Some(*cooldown),
                    detected_at: *now_ts,
                    source_header: None,
                };
                update_throttle_state(&mut throttle, &rli, *cooldown, *now_ts);
                if let Ok(v) = serde_json::to_value(&throttle) {
                    runtime.extra.insert("throttle_state".to_string(), v);
                }
                runtime.delayed_until = Some(delayed_until.clone());
                runtime.set_status(RuntimeStatus::Idle);
                runtime.completion_kind = None;
                runtime.pid = None;
                runtime.set_last_error_details(
                    error.code.clone(),
                    error.origin.clone(),
                    error.message.clone(),
                );
                runtime.last_error = Some(format!("quota exhausted; cooldown {}s", cooldown));
                store_classified_error_in_extra(runtime, tool_error, *now_ts);
                capture_last_assignment_before_clear(task);
                task.worktree = None;
                task.set_workflow_state(WorkflowState::Todo);
                task.touch_state_changed(now);
                JobCompletionResult {
                    should_retry: false,
                    status_label: "quota_exhausted_requeue",
                    detail: reason.clone(),
                    completion_kind: None,
                    tool_error: tool_error.clone(),
                }
            }
            RetryOutcome::AutoRetry {
                error,
                tool_error,
                now_ts,
            } => {
                task.set_workflow_state(WorkflowState::Todo);
                capture_last_assignment_before_clear(task);
                task.worktree = None;
                let runtime = task.ensure_runtime();
                runtime.increment_retries();
                runtime.set_status(RuntimeStatus::Idle);
                runtime.pid = None;
                runtime.current_phase = Some("dev".to_string());
                runtime.completion_kind = None;
                runtime.set_last_error_details(
                    error.code.clone(),
                    error.origin.clone(),
                    error.message.clone(),
                );
                runtime.last_error = Some(reason.clone());
                store_classified_error_in_extra(runtime, tool_error, *now_ts);
                task.touch_state_changed(now);
                JobCompletionResult {
                    should_retry: false,
                    status_label: "auto_retry",
                    detail: reason.clone(),
                    completion_kind: None,
                    tool_error: tool_error.clone(),
                }
            }
        },
        RetryStrategy::Block { reason, outcome } => match outcome {
            BlockOutcome::InvalidInput => {
                task.set_workflow_state(WorkflowState::Blocked);
                let runtime = task.ensure_runtime();
                runtime.set_status(RuntimeStatus::Failed);
                runtime.pid = None;
                runtime.set_last_error_details("E901", "coordinator", reason.clone());
                runtime.last_error = Some(reason.clone());
                task.touch_state_changed(now);
                JobCompletionResult {
                    should_retry: false,
                    status_label: "failed",
                    detail: reason.clone(),
                    completion_kind: None,
                    tool_error: None,
                }
            }
            BlockOutcome::TerminalFailure {
                error,
                tool_error,
                now_ts,
            } => {
                task.set_workflow_state(WorkflowState::Blocked);
                let runtime = task.ensure_runtime();
                runtime.set_status(RuntimeStatus::Failed);
                runtime.completion_kind = None;
                runtime.pid = None;
                runtime.set_last_error_details(
                    error.code.clone(),
                    error.origin.clone(),
                    error.message.clone(),
                );
                runtime.last_error = Some(reason.clone());
                store_classified_error_in_extra(runtime, tool_error, *now_ts);
                task.touch_state_changed(now);
                JobCompletionResult {
                    should_retry: false,
                    status_label: "failed",
                    detail: reason.clone(),
                    completion_kind: None,
                    tool_error: tool_error.clone(),
                }
            }
        },
        RetryStrategy::Merge {
            detail,
            completion_kind,
            tool_error,
        } => {
            task.set_workflow_state(WorkflowState::Merged);
            let runtime = task.ensure_runtime();
            runtime.completion_kind = Some(completion_kind.as_str().to_string());
            runtime.set_status(RuntimeStatus::Idle);
            runtime.current_phase = None;
            runtime.pid = None;
            task.touch_state_changed(now);
            JobCompletionResult {
                should_retry: false,
                status_label: match completion_kind {
                    PerformerCompletionKind::AlreadySatisfied => "already_satisfied",
                    PerformerCompletionKind::SuccessWithoutChanges => "success_without_changes",
                    _ => "already_satisfied",
                },
                detail: detail.clone(),
                completion_kind: Some(*completion_kind),
                tool_error: tool_error.clone(),
            }
        }
        RetryStrategy::PhaseDone {
            detail,
            completion_kind,
        } => {
            task.set_workflow_state(WorkflowState::InProgress);
            let runtime = task.ensure_runtime();
            runtime.completion_kind = Some(completion_kind.as_str().to_string());
            runtime.set_status(RuntimeStatus::PhaseDone);
            runtime.current_phase = Some("dev".to_string());
            runtime.pid = None;
            task.touch_state_changed(now);
            JobCompletionResult {
                should_retry: false,
                status_label: "phase_done",
                detail: detail.clone(),
                completion_kind: Some(*completion_kind),
                tool_error: None,
            }
        }
        RetryStrategy::NoOp => JobCompletionResult {
            should_retry: false,
            status_label: "failed",
            detail: "completion strategy resolved to no-op".to_string(),
            completion_kind: None,
            tool_error: None,
        },
    }
}

fn apply_job_completion_typed(
    task: &mut Task,
    input: &JobCompletionInput,
    normalizer_registry: &NormalizerRegistry,
    now: &str,
) -> JobCompletionResult {
    let worktree_path = task.worktree_path().map(str::to_string);
    let has_commits_ahead_of_base = worktree_path
        .as_deref()
        .map(|worktree_path| {
            let base_branch = task.base_branch("master");
            crate::git::has_commits_ahead(Path::new(worktree_path), &base_branch)
        })
        .unwrap_or(false);
    let is_healthy_worktree = worktree_path
        .as_deref()
        .map(|path| is_worktree_healthy(Path::new(path)))
        .unwrap_or(false);
    let normalizer = task
        .tool
        .as_deref()
        .and_then(|tool_id| normalizer_registry.get(tool_id));
    let classification =
        classify_completion_error(input, normalizer.as_deref(), has_commits_ahead_of_base);
    debug_assert!(
        classification.tool_error.is_some()
            || classification.canonical_class
                == error_code_to_canonical_class(&classification.error_code)
    );
    debug_assert!(
        classification.completion_authority != CompletionAuthority::IpcSignal
            || classification.has_commits
    );
    let config = CoordinatorConfigResolved::from_input(input);
    let strategy = resolve_retry_strategy(
        task,
        input,
        &classification,
        &config,
        is_healthy_worktree,
        now,
    );
    apply_state_transitions(task, &strategy, now)
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
            // Clear worktree attachment so the task can be re-dispatched.
            task.worktree = None;
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

    pub fn on_cycle_counts(
        &mut self,
        counts: CoordinatorCounts,
        last_dispatch_failure: Option<&str>,
    ) -> Result<ControlPlaneDecision> {
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
            let hint = match last_dispatch_failure {
                Some(msg) => format!(" Last dispatch failure: {}", msg),
                None => String::new(),
            };
            return Err(MaccError::Validation(format!(
                "Coordinator made no progress for {} cycles (todo={}, active={}, blocked={}).{} Run `macc coordinator status`, then `macc coordinator unlock --all`, and inspect logs with `macc logs tail --component coordinator`.",
                self.no_progress_cycles, counts.todo, counts.active, counts.blocked, hint
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
    let mut consecutive_transient_errors: usize = 0;
    const MAX_TRANSIENT_ERRORS: usize = 5;
    loop {
        cycle += 1;
        match run_control_plane_cycle(backend, &mut controller, cycle).await {
            Ok(ControlPlaneDecision::Continue) => {
                consecutive_transient_errors = 0;
            }
            Ok(ControlPlaneDecision::Complete) => return Ok(()),
            Err(err) if err.is_transient() => {
                consecutive_transient_errors += 1;
                tracing::warn!(
                    cycle,
                    consecutive = consecutive_transient_errors,
                    "control plane cycle failed with transient error, will retry: {}",
                    err
                );
                if consecutive_transient_errors >= MAX_TRANSIENT_ERRORS {
                    return Err(err);
                }
                // Wait before retrying to give the system time to recover.
                tokio::time::sleep(std::time::Duration::from_secs(5)).await;
            }
            Err(err) => return Err(err),
        }
    }
}

async fn run_control_plane_cycle<B: ControlPlaneBackend + ?Sized>(
    backend: &mut B,
    controller: &mut CoordinatorRunController,
    cycle: usize,
) -> std::result::Result<ControlPlaneDecision, MaccError> {
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
    if let Some(reason) = backend.should_terminate_run(&counts) {
        return Err(MaccError::Validation(reason));
    }
    let last_fail = backend.last_dispatch_failure();
    controller.on_cycle_counts(counts, last_fail.as_deref())
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
    ghost_heartbeat_grace_seconds: i64,
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
            cleanup_dead_runtime_tasks(
                self.repo_root,
                "run-cycle",
                self.ghost_heartbeat_grace_seconds,
                Some(&note),
            )?
        } else {
            cleanup_dead_runtime_tasks(
                self.repo_root,
                "run-cycle",
                self.ghost_heartbeat_grace_seconds,
                None,
            )?
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
            self.coordinator,
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
            self.env_cfg,
            self.coordinator,
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
        set_task_paused_for_merge(self.repo_root, task_id, reason)?;
        write_coordinator_pause_file(self.repo_root, task_id, "merge", reason)?;
        if let Some(log) = self.logger {
            let _ = log.note(format!(
                "- Run paused task={} phase=merge reason={}",
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
        resume_paused_task_merge(self.repo_root, task_id)?;
        let _ = clear_coordinator_pause_file(self.repo_root)?;
        Ok(())
    }

    async fn advance_tasks(&mut self) -> Result<AdvanceResult> {
        crate::coordinator::control_plane::advance_tasks_native(
            self.repo_root,
            self.env_cfg,
            self.coordinator,
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

    fn should_terminate_run(&self, counts: &CoordinatorCounts) -> Option<String> {
        let cfg = CanonicalCoordinatorConfigResolved::resolve(self.coordinator);
        let max_dispatch_total = self.env_cfg.max_dispatch.unwrap_or(cfg.max_dispatch);
        if max_dispatch_total > 0
            && self.run_state.dispatched_total_run >= max_dispatch_total
            && counts.active == 0
            && self.run_state.active_jobs.is_empty()
            && self.run_state.active_merge_jobs.is_empty()
        {
            return Some(format!(
                "Coordinator stopped: dispatch limit reached (dispatched={}, max_dispatch={}). \
                 Remaining: todo={}, merged={}. Restart the coordinator to continue.",
                self.run_state.dispatched_total_run, max_dispatch_total, counts.todo, counts.merged,
            ));
        }
        None
    }

    fn last_dispatch_failure(&self) -> Option<String> {
        self.run_state.last_dispatch_failure.clone()
    }
}

pub fn resolve_storage_mode(
    env_cfg: &super::types::CoordinatorEnvConfig,
    coordinator: Option<&CoordinatorConfig>,
) -> Result<CoordinatorStorageMode> {
    let cfg = CanonicalCoordinatorConfigResolved::resolve(coordinator);
    let raw = env_cfg
        .storage_mode
        .clone()
        .unwrap_or_else(|| cfg.storage_mode.clone());
    raw.parse::<CoordinatorStorageMode>()
        .map_err(MaccError::Validation)
}

pub fn sync_storage_with_startup_reconcile(
    project_paths: &crate::ProjectPaths,
    storage_mode: CoordinatorStorageMode,
    json_compat: bool,
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
    if json_compat {
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
    let cfg = CanonicalCoordinatorConfigResolved::resolve(coordinator);
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
        .or_else(|| cfg.prd_file.clone().map(PathBuf::from))
        .unwrap_or_else(|| repo_root.join("prd.json"));
    if !prd_file.exists() {
        return Err(MaccError::Validation(format!(
            "Coordinator PRD file not found: {}. Configure `automation.coordinator.prd_file` or pass `--prd`.",
            prd_file.display()
        )));
    }

    let phase_runner_max_attempts = env_cfg
        .phase_runner_max_attempts
        .unwrap_or(cfg.phase_runner_max_attempts)
        .max(1);
    let coordinator_tool_override = env_cfg
        .coordinator_tool
        .clone()
        .or_else(|| cfg.coordinator_tool.clone())
        .or_else(|| {
            // Auto-resolve: pick the first enabled tool from tool_priority,
            // or fall back to the first enabled tool overall.  This ensures a
            // single coordinator performer tool for all review/fix
            // phases, preventing different tasks from using different tools.
            let enabled = &canonical.tools.enabled;
            // CLI --tool-priority or config tool_priority
            let priority: Vec<String> = env_cfg
                .tool_priority
                .as_ref()
                .map(|csv| {
                    csv.split(',')
                        .map(|v| v.trim().to_string())
                        .filter(|v| !v.is_empty())
                        .collect()
                })
                .or_else(|| {
                    if cfg.tool_priority.is_empty() {
                        None
                    } else {
                        Some(cfg.tool_priority.clone())
                    }
                })
                .unwrap_or_default();
            priority
                .iter()
                .find(|t| enabled.contains(t))
                .cloned()
                .or_else(|| enabled.first().cloned())
        });
    if let Some(log) = logger {
        if let Some(ref tool) = coordinator_tool_override {
            let _ = log.note(format!("- Coordinator tool: {}", tool));
        }
    }
    let max_review_cycles = env_cfg
        .max_review_cycles
        .or(cfg.max_review_cycles);
    if let Some(log) = logger {
        match max_review_cycles {
            Some(n) => {
                let _ = log.note(format!("- Max review cycles: {}", n));
            }
            None => {
                let _ = log.note("- Max review cycles: unlimited".to_string());
            }
        }
    }

    let phase_timeout_seconds = env_cfg
        .stale_in_progress_seconds
        .unwrap_or(cfg.stale_in_progress_seconds);
    let ghost_heartbeat_grace_seconds = env_cfg
        .ghost_heartbeat_grace_seconds
        .unwrap_or(cfg.ghost_heartbeat_grace_seconds);

    let json_compat = env_cfg
        .json_compat
        .unwrap_or(cfg.json_compat);

    let storage_mode = resolve_storage_mode(env_cfg, coordinator)?;
    let storage_paths = crate::ProjectPaths::from_root(repo_root);
    sync_storage_with_startup_reconcile(&storage_paths, storage_mode, json_compat, logger)?;
    let startup_cleaned = if let Some(log) = logger {
        let note = |line: String| {
            let _ = log.note(line);
        };
        cleanup_dead_runtime_tasks(
            repo_root,
            "run-startup",
            ghost_heartbeat_grace_seconds,
            Some(&note),
        )?
    } else {
        cleanup_dead_runtime_tasks(
            repo_root,
            "run-startup",
            ghost_heartbeat_grace_seconds,
            None,
        )?
    };
    if startup_cleaned > 0 {
        if let Some(log) = logger {
            let _ = log.note(format!(
                "- Startup runtime cleanup fixed {} ghost task(s)",
                startup_cleaned
            ));
        }
    }

    let mut run_state = CoordinatorRunState::new();
    // ── Part D: restore persisted throttle state on startup ─────────────
    {
        let storage_paths = crate::coordinator_storage::CoordinatorStoragePaths::from_project_paths(
            &crate::ProjectPaths::from_root(repo_root),
        );
        let sqlite = crate::coordinator_storage::SqliteStorage::new(storage_paths);
        match sqlite.load_throttle_registry() {
            Ok(reg) if !reg.is_empty() => {
                if let Some(log) = logger {
                    let _ = log.note(format!(
                        "- Restored {} persisted tool throttle(s)",
                        reg.len()
                    ));
                }
                run_state.throttle_registry = reg;
            }
            Ok(_) => {} // empty — nothing to restore
            Err(e) => {
                if let Some(log) = logger {
                    let _ = log.note(format!(
                        "- Warning: failed to load persisted throttle state: {}",
                        e
                    ));
                }
            }
        }
    }

    let mut backend = NativeControlPlaneBackend {
        repo_root,
        canonical,
        coordinator,
        env_cfg,
        logger,
        prd_file,
        run_state,
        phase_runner_max_attempts,
        coordinator_tool_override,
        phase_timeout_seconds,
        ghost_heartbeat_grace_seconds,
        last_logged_counts: None,
        last_cycle_progressed: false,
    };

    let timeout_seconds = env_cfg
        .timeout_seconds
        .unwrap_or(cfg.timeout_seconds);
    let loop_cfg = ControlPlaneLoopConfig {
        timeout: if timeout_seconds > 0 {
            Some(Duration::from_secs(timeout_seconds as u64))
        } else {
            None
        },
        max_no_progress_cycles: 5,
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
    use crate::coordinator::error_normalizer::ErrorNormalizer;
    use std::fs;
    use std::path::{Path, PathBuf};
    use std::process::Command;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn run_git(dir: &Path, args: &[&str]) {
        let output = Command::new("git")
            .args(args)
            .current_dir(dir)
            .output()
            .expect("run git command");
        assert!(
            output.status.success(),
            "git {:?} failed: {}",
            args,
            String::from_utf8_lossy(&output.stderr)
        );
    }

    #[derive(Clone)]
    struct StubNormalizer {
        tool_error: ToolError,
    }

    impl ErrorNormalizer for StubNormalizer {
        fn normalize(&self, _exit_code: i32, _stderr: &str, _stdout: &str) -> Option<ToolError> {
            Some(self.tool_error.clone())
        }
    }

    fn make_classification_input(
        success: bool,
        completion_kind: Option<PerformerCompletionKind>,
    ) -> JobCompletionInput {
        JobCompletionInput {
            success,
            attempt: 1,
            max_attempts: 1,
            timed_out: false,
            phase_timeout_seconds: 300,
            elapsed_seconds: 5,
            status_text: "performer failed".to_string(),
            completion_kind,
            error_code: None,
            error_origin: None,
            error_message: None,
            auto_retry_error_codes: Vec::new(),
            auto_retry_max: 0,
            backoff_base_seconds: 30,
            backoff_max_seconds: 300,
            normalizer_input: None,
        }
    }

    fn make_repo_with_commit_ahead(base_branch: &str) -> PathBuf {
        let mut repo = std::env::temp_dir();
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system clock before unix epoch")
            .as_nanos();
        repo.push(format!("macc-engine-ipc-{}", nanos));
        fs::create_dir_all(&repo).expect("create temp repo");

        run_git(&repo, &["init"]);
        run_git(&repo, &["checkout", "-b", base_branch]);
        fs::write(repo.join("fixture.txt"), "base\n").expect("write base fixture");
        run_git(&repo, &["add", "fixture.txt"]);
        run_git(
            &repo,
            &[
                "-c",
                "user.name=MACC Test",
                "-c",
                "user.email=macc-test@example.com",
                "commit",
                "-m",
                "base commit",
            ],
        );

        run_git(&repo, &["checkout", "-b", "task/ipc-override"]);
        fs::write(repo.join("fixture.txt"), "base\nahead\n").expect("write ahead fixture");
        run_git(&repo, &["add", "fixture.txt"]);
        run_git(
            &repo,
            &[
                "-c",
                "user.name=MACC Test",
                "-c",
                "user.email=macc-test@example.com",
                "commit",
                "-m",
                "ahead commit",
            ],
        );

        repo
    }

    fn make_salvage_repo(
        base_branch: &str,
        task_id: &str,
        conflict: bool,
    ) -> (PathBuf, String, PathBuf) {
        let mut repo = std::env::temp_dir();
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system clock before unix epoch")
            .as_nanos();
        repo.push(format!("macc-engine-salvage-{}", nanos));
        fs::create_dir_all(&repo).expect("create temp repo");

        run_git(&repo, &["init"]);
        run_git(&repo, &["checkout", "-b", base_branch]);
        fs::write(repo.join("fixture.txt"), "base\n").expect("write base fixture");
        run_git(&repo, &["add", "fixture.txt"]);
        run_git(
            &repo,
            &[
                "-c",
                "user.name=MACC Test",
                "-c",
                "user.email=macc-test@example.com",
                "commit",
                "-m",
                "base commit",
            ],
        );

        let task_branch = format!("task/{}", task_id.to_ascii_lowercase());
        run_git(&repo, &["checkout", "-b", &task_branch]);
        fs::write(
            repo.join("task.txt"),
            format!("task {}\ncommitted work\n", task_id),
        )
        .expect("write task fixture");
        run_git(&repo, &["add", "task.txt"]);
        run_git(
            &repo,
            &[
                "-c",
                "user.name=MACC Test",
                "-c",
                "user.email=macc-test@example.com",
                "commit",
                "-m",
                &format!("macc: {} - salvage candidate", task_id),
            ],
        );
        if conflict {
            fs::write(repo.join("fixture.txt"), "task-side-change\n").expect("write task change");
            run_git(&repo, &["add", "fixture.txt"]);
            run_git(
                &repo,
                &[
                    "-c",
                    "user.name=MACC Test",
                    "-c",
                    "user.email=macc-test@example.com",
                    "commit",
                    "-m",
                    &format!("macc: {} - conflicting change", task_id),
                ],
            );
        }

        run_git(&repo, &["checkout", base_branch]);
        if conflict {
            fs::write(repo.join("fixture.txt"), "base-side-change\n").expect("write base change");
            run_git(&repo, &["add", "fixture.txt"]);
            run_git(
                &repo,
                &[
                    "-c",
                    "user.name=MACC Test",
                    "-c",
                    "user.email=macc-test@example.com",
                    "commit",
                    "-m",
                    "base conflicting change",
                ],
            );
        }

        let mut worktree = std::env::temp_dir();
        let wt_nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system clock before unix epoch")
            .as_nanos();
        worktree.push(format!("macc-engine-salvage-wt-{}", wt_nanos));
        run_git(
            &repo,
            &[
                "worktree",
                "add",
                worktree.to_string_lossy().as_ref(),
                &task_branch,
            ],
        );

        (repo, task_branch, worktree)
    }

    #[test]
    fn plan_advance_maps_states() {
        // Default (unlimited review cycles)
        assert!(matches!(
            plan_advance(WorkflowState::InProgress, None, 0),
            AdvancePlan::RunPhase(PhaseTransition { mode: "review", .. })
        ));
        assert!(matches!(
            plan_advance(WorkflowState::PrOpen, None, 0),
            AdvancePlan::Merge
        ));
        assert!(matches!(
            plan_advance(WorkflowState::ChangesRequested, None, 0),
            AdvancePlan::RunPhase(PhaseTransition { mode: "fix", .. })
        ));
        assert!(matches!(
            plan_advance(WorkflowState::Queued, None, 0),
            AdvancePlan::Merge
        ));
        assert!(matches!(
            plan_advance(WorkflowState::Todo, None, 0),
            AdvancePlan::Noop
        ));
    }

    #[test]
    fn plan_advance_max_review_cycles_zero_skips_review() {
        // max_review_cycles=0 → skip review entirely
        assert!(matches!(
            plan_advance(WorkflowState::InProgress, Some(0), 0),
            AdvancePlan::Merge
        ));
        // ChangesRequested with 0 cycles used but max=0 → merge directly
        assert!(matches!(
            plan_advance(WorkflowState::ChangesRequested, Some(0), 0),
            AdvancePlan::Merge
        ));
    }

    #[test]
    fn plan_advance_max_review_cycles_one_allows_single_fix() {
        // max=1, 0 cycles done → allow fix
        assert!(matches!(
            plan_advance(WorkflowState::ChangesRequested, Some(1), 0),
            AdvancePlan::RunPhase(PhaseTransition { mode: "fix", .. })
        ));
        // max=1, 1 cycle done → exhausted, merge directly
        assert!(matches!(
            plan_advance(WorkflowState::ChangesRequested, Some(1), 1),
            AdvancePlan::Merge
        ));
    }

    #[test]
    fn plan_advance_max_review_cycles_n_caps_loops() {
        // max=3, 2 done → allow another
        assert!(matches!(
            plan_advance(WorkflowState::ChangesRequested, Some(3), 2),
            AdvancePlan::RunPhase(PhaseTransition { mode: "fix", .. })
        ));
        // max=3, 3 done → exhausted, merge
        assert!(matches!(
            plan_advance(WorkflowState::ChangesRequested, Some(3), 3),
            AdvancePlan::Merge
        ));
        // max=3, 5 done → way past, merge
        assert!(matches!(
            plan_advance(WorkflowState::ChangesRequested, Some(3), 5),
            AdvancePlan::Merge
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
            active_session_id: Some("tool-session-1".to_string()),
            pid: Some(123),
            phase: "dev".to_string(),
            now: "2026-02-20T00:00:00Z".to_string(),
        };
        apply_dispatch_claim(&mut task, &update);
        assert_eq!(task["state"], "claimed");
        assert_eq!(task["task_runtime"]["status"], "running");
        assert_eq!(task["task_runtime"]["active_session_id"], "tool-session-1");
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
                completion_kind: None,
                error_code: None,
                error_origin: None,
                error_message: None,
                auto_retry_error_codes: Vec::new(),
                auto_retry_max: 0,
                backoff_base_seconds: 30,
                backoff_max_seconds: 300,
                normalizer_input: None,
            },
            &NormalizerRegistry::empty(),
            "2026-02-21T00:00:00Z",
        );
        assert!(!out.should_retry);
        assert_eq!(task["state"], "in_progress");
        assert_eq!(task["task_runtime"]["status"], "phase_done");
        assert!(task["task_runtime"]["pid"].is_null());
    }

    #[test]
    fn apply_job_completion_already_satisfied_is_explicit_success() {
        let mut task =
            json!({"id":"T3b","state":"claimed","task_runtime":{"status":"running","pid":123}});
        let out = apply_job_completion(
            &mut task,
            &JobCompletionInput {
                success: true,
                attempt: 1,
                max_attempts: 1,
                timed_out: false,
                phase_timeout_seconds: 0,
                elapsed_seconds: 1,
                status_text: "task already satisfied; verified axum/tokio config".to_string(),
                completion_kind: Some(PerformerCompletionKind::AlreadySatisfied),
                error_code: None,
                error_origin: None,
                error_message: None,
                auto_retry_error_codes: Vec::new(),
                auto_retry_max: 0,
                backoff_base_seconds: 30,
                backoff_max_seconds: 300,
                normalizer_input: None,
            },
            &NormalizerRegistry::empty(),
            "2026-02-21T00:00:00Z",
        );
        assert!(!out.should_retry);
        assert_eq!(out.status_label, "already_satisfied");
        assert_eq!(
            out.completion_kind,
            Some(PerformerCompletionKind::AlreadySatisfied)
        );
        assert_eq!(task["state"], "merged");
        assert_eq!(task["task_runtime"]["status"], "idle");
        assert_eq!(task["task_runtime"]["completion_kind"], "already_satisfied");
        assert!(task["task_runtime"]["current_phase"].is_null());
    }

    #[test]
    fn apply_job_completion_success_without_changes_bypasses_review() {
        let mut task =
            json!({"id":"T3c","state":"claimed","task_runtime":{"status":"running","pid":123}});
        let out = apply_job_completion(
            &mut task,
            &JobCompletionInput {
                success: true,
                attempt: 1,
                max_attempts: 1,
                timed_out: false,
                phase_timeout_seconds: 0,
                elapsed_seconds: 1,
                status_text: "performer finished but no changes made".to_string(),
                completion_kind: Some(PerformerCompletionKind::SuccessWithoutChanges),
                error_code: None,
                error_origin: None,
                error_message: None,
                auto_retry_error_codes: Vec::new(),
                auto_retry_max: 0,
                backoff_base_seconds: 30,
                backoff_max_seconds: 300,
                normalizer_input: None,
            },
            &NormalizerRegistry::empty(),
            "2026-02-21T00:00:00Z",
        );
        assert!(!out.should_retry);
        assert_eq!(out.status_label, "success_without_changes");
        assert_eq!(task["state"], "merged");
        assert_eq!(task["task_runtime"]["status"], "idle");
        assert_eq!(
            task["task_runtime"]["completion_kind"],
            "success_without_changes"
        );
        assert!(task["task_runtime"]["current_phase"].is_null());
    }

    #[test]
    fn apply_job_completion_ipc_phase_done_with_commits_ahead_overrides_failed_exit() {
        let repo = make_repo_with_commit_ahead("main");
        let mut task = json!({
            "id":"T3d",
            "state":"claimed",
            "base_branch":"main",
            "worktree":{"worktree_path": repo.to_string_lossy().to_string()},
            "task_runtime":{"status":"running","pid":123}
        });
        let out = apply_job_completion(
            &mut task,
            &JobCompletionInput {
                success: false,
                attempt: 1,
                max_attempts: 1,
                timed_out: false,
                phase_timeout_seconds: 0,
                elapsed_seconds: 1,
                status_text: "exit status: 1".to_string(),
                completion_kind: Some(PerformerCompletionKind::SuccessWithChanges),
                error_code: Some("E101".to_string()),
                error_origin: Some("runner".to_string()),
                error_message: Some("non-zero exit".to_string()),
                auto_retry_error_codes: Vec::new(),
                auto_retry_max: 0,
                backoff_base_seconds: 30,
                backoff_max_seconds: 300,
                normalizer_input: None,
            },
            &NormalizerRegistry::empty(),
            "2026-02-21T00:00:00Z",
        );

        assert!(!out.should_retry);
        assert_eq!(out.status_label, "phase_done");
        assert_eq!(
            out.completion_kind,
            Some(PerformerCompletionKind::SuccessWithChanges)
        );
        assert_eq!(task["state"], "in_progress");
        assert_eq!(task["task_runtime"]["status"], "phase_done");

        let _ = fs::remove_dir_all(&repo);
    }

    #[test]
    fn apply_job_completion_no_ipc_signal_keeps_failed_exit_classification() {
        let mut task = json!({
            "id":"T3e",
            "state":"claimed",
            "task_runtime":{"status":"running","pid":123}
        });
        let out = apply_job_completion(
            &mut task,
            &JobCompletionInput {
                success: false,
                attempt: 1,
                max_attempts: 1,
                timed_out: false,
                phase_timeout_seconds: 300,
                elapsed_seconds: 5,
                status_text: "performer failed".to_string(),
                completion_kind: None,
                error_code: Some("E201".to_string()),
                error_origin: Some("runner".to_string()),
                error_message: Some("auth error".to_string()),
                auto_retry_error_codes: Vec::new(),
                auto_retry_max: 0,
                backoff_base_seconds: 30,
                backoff_max_seconds: 300,
                normalizer_input: None,
            },
            &NormalizerRegistry::empty(),
            "2026-02-21T00:00:00Z",
        );

        assert_eq!(out.status_label, "failed");
        assert_eq!(task["task_runtime"]["last_error_code"], "E201");
    }

    #[test]
    fn classify_completion_error_ipc_with_commits_uses_ipc_authority() {
        let input =
            make_classification_input(false, Some(PerformerCompletionKind::SuccessWithChanges));
        let classified = classify_completion_error(&input, None, true);

        assert_eq!(
            classified.completion_authority,
            CompletionAuthority::IpcSignal
        );
        assert!(classified.completion_success);
        assert_eq!(
            classified.completion_kind,
            Some(PerformerCompletionKind::SuccessWithChanges)
        );
        assert!(classified.has_commits);
    }

    #[test]
    fn classify_completion_error_exit_error_without_commits_falls_back() {
        let input =
            make_classification_input(false, Some(PerformerCompletionKind::SuccessWithChanges));
        let classified = classify_completion_error(&input, None, false);

        assert_eq!(
            classified.completion_authority,
            CompletionAuthority::Fallback
        );
        assert!(!classified.completion_success);
        assert_eq!(
            classified.completion_kind,
            Some(PerformerCompletionKind::SuccessWithChanges)
        );
        assert!(!classified.has_commits);
    }

    #[test]
    fn classify_completion_error_sets_rate_limit_class_from_normalizer() {
        let mut input = make_classification_input(false, None);
        input.normalizer_input = Some(NormalizerInput {
            exit_code: 1,
            stderr: "429 rate limit".to_string(),
            stdout: String::new(),
        });
        let normalizer = StubNormalizer {
            tool_error: ToolError {
                provider: "stub".to_string(),
                canonical_class: CanonicalClass::RateLimit,
                retryable: true,
                retry_after_seconds: Some(30),
                user_action_required: false,
                raw_message: "rate-limited".to_string(),
                error_code: E601_RATE_LIMITED.to_string(),
                request_id: None,
                attempt: 0,
                operation: String::new(),
            },
        };
        let classified = classify_completion_error(&input, Some(&normalizer), false);

        assert_eq!(classified.canonical_class, CanonicalClass::RateLimit);
        assert_eq!(classified.error_code, E601_RATE_LIMITED);
        assert_eq!(
            classified
                .tool_error
                .as_ref()
                .map(|te| te.error_code.as_str()),
            Some(E601_RATE_LIMITED)
        );
    }

    #[test]
    fn classify_completion_error_sets_quota_class_from_normalizer() {
        let mut input = make_classification_input(false, None);
        input.normalizer_input = Some(NormalizerInput {
            exit_code: 1,
            stderr: "quota exhausted".to_string(),
            stdout: String::new(),
        });
        let normalizer = StubNormalizer {
            tool_error: ToolError {
                provider: "stub".to_string(),
                canonical_class: CanonicalClass::QuotaExhausted,
                retryable: false,
                retry_after_seconds: Some(3600),
                user_action_required: true,
                raw_message: "quota exhausted".to_string(),
                error_code: E602_QUOTA_EXHAUSTED.to_string(),
                request_id: None,
                attempt: 0,
                operation: String::new(),
            },
        };
        let classified = classify_completion_error(&input, Some(&normalizer), false);

        assert_eq!(classified.canonical_class, CanonicalClass::QuotaExhausted);
        assert_eq!(classified.error_code, E602_QUOTA_EXHAUSTED);
        assert_eq!(
            classified
                .tool_error
                .as_ref()
                .map(|te| te.error_code.as_str()),
            Some(E602_QUOTA_EXHAUSTED)
        );
    }

    fn make_strategy_task() -> Task {
        parse_compat_task(&json!({
            "id":"S1",
            "state":"claimed",
            "assignee":"agentA",
            "tool":"claude",
            "worktree":{"worktree_path":"/tmp/wt-s1","branch":"task/S1","base_branch":"main"},
            "task_runtime":{"status":"running","pid":123,"retries":1}
        }))
    }

    #[test]
    fn resolve_retry_strategy_maps_core_variants() {
        let task = make_strategy_task();
        let cfg = CoordinatorConfigResolved {
            auto_retry_error_codes: vec!["E201".to_string()],
            auto_retry_max: 2,
            backoff_base_seconds: 30,
            backoff_max_seconds: 300,
        };

        let mut phase_done_input = make_classification_input(true, None);
        phase_done_input.status_text = "ok".to_string();
        let phase_done_classified = classify_completion_error(&phase_done_input, None, false);
        let phase_done = resolve_retry_strategy(
            &task,
            &phase_done_input,
            &phase_done_classified,
            &cfg,
            true,
            "2026-02-21T00:00:00Z",
        );
        assert!(matches!(phase_done, RetryStrategy::PhaseDone { .. }));

        let merge_input =
            make_classification_input(true, Some(PerformerCompletionKind::AlreadySatisfied));
        let merge_classified = classify_completion_error(&merge_input, None, false);
        let merge = resolve_retry_strategy(
            &task,
            &merge_input,
            &merge_classified,
            &cfg,
            true,
            "2026-02-21T00:00:00Z",
        );
        assert!(matches!(merge, RetryStrategy::Merge { .. }));

        let mut retry_input = make_classification_input(false, None);
        retry_input.attempt = 1;
        retry_input.max_attempts = 2;
        let retry_classified = classify_completion_error(&retry_input, None, false);
        let retry = resolve_retry_strategy(
            &task,
            &retry_input,
            &retry_classified,
            &cfg,
            true,
            "2026-02-21T00:00:00Z",
        );
        assert!(matches!(
            retry,
            RetryStrategy::Retry {
                outcome: RetryOutcome::AttemptRetry { .. },
                ..
            }
        ));

        let mut block_input = make_classification_input(false, None);
        block_input.attempt = 1;
        block_input.max_attempts = 1;
        block_input.error_code = Some("E999".to_string());
        let block_classified = classify_completion_error(&block_input, None, false);
        let block = resolve_retry_strategy(
            &task,
            &block_input,
            &block_classified,
            &cfg,
            true,
            "2026-02-21T00:00:00Z",
        );
        assert!(matches!(
            block,
            RetryStrategy::Block {
                outcome: BlockOutcome::TerminalFailure { .. },
                ..
            }
        ));
    }

    #[test]
    fn apply_state_transitions_updates_state_fields_consistently() {
        let now = "2026-02-21T00:00:00Z";
        let mut retry_task = make_strategy_task();
        let retry_out = apply_state_transitions(
            &mut retry_task,
            &RetryStrategy::Retry {
                same_worktree: false,
                reason: "tool error".to_string(),
                outcome: RetryOutcome::ToolReportedError {
                    completion_kind: PerformerCompletionKind::ErrorWithoutChanges,
                    tool_error: None,
                },
            },
            now,
        );
        assert_eq!(retry_out.status_label, "error_without_changes");
        assert_eq!(retry_task.state, "todo");
        assert!(retry_task.worktree.is_none());
        assert!(retry_task.assignee.is_none());

        let mut auto_retry_task = make_strategy_task();
        let auto_retry_out = apply_state_transitions(
            &mut auto_retry_task,
            &RetryStrategy::Retry {
                same_worktree: false,
                reason: "auto-retry scheduled for error code E201".to_string(),
                outcome: RetryOutcome::AutoRetry {
                    error: CompletionErrorDetails {
                        code: "E201".to_string(),
                        origin: "runner".to_string(),
                        message: "auth".to_string(),
                    },
                    tool_error: None,
                    now_ts: 0,
                },
            },
            now,
        );
        assert_eq!(auto_retry_out.status_label, "auto_retry");
        assert_eq!(auto_retry_task.state, "todo");
        assert_eq!(auto_retry_task.task_runtime.retries_count(), 2);

        let mut block_task = make_strategy_task();
        let block_out = apply_state_transitions(
            &mut block_task,
            &RetryStrategy::Block {
                reason: "terminal failure".to_string(),
                outcome: BlockOutcome::InvalidInput,
            },
            now,
        );
        assert_eq!(block_out.status_label, "failed");
        assert_eq!(block_task.state, "blocked");

        let mut merge_task = make_strategy_task();
        let merge_out = apply_state_transitions(
            &mut merge_task,
            &RetryStrategy::Merge {
                detail: "already satisfied".to_string(),
                completion_kind: PerformerCompletionKind::AlreadySatisfied,
                tool_error: None,
            },
            now,
        );
        assert_eq!(merge_out.status_label, "already_satisfied");
        assert_eq!(merge_task.state, "merged");
        assert!(merge_task.worktree.is_some());

        let mut phase_done_task = make_strategy_task();
        let phase_done_out = apply_state_transitions(
            &mut phase_done_task,
            &RetryStrategy::PhaseDone {
                detail: "phase done".to_string(),
                completion_kind: PerformerCompletionKind::SuccessWithChanges,
            },
            now,
        );
        assert_eq!(phase_done_out.status_label, "phase_done");
        assert_eq!(phase_done_task.state, "in_progress");

        let mut noop_task = make_strategy_task();
        let noop_out = apply_state_transitions(&mut noop_task, &RetryStrategy::NoOp, now);
        assert_eq!(noop_out.status_label, "failed");
        assert_eq!(noop_task.state, "claimed");
    }

    #[test]
    fn already_satisfied_task_does_not_schedule_review_or_merge() {
        let mut registry = json!({
            "tasks": [
                {
                    "id": "DONE",
                    "title": "done",
                    "state": "claimed",
                    "priority": "0",
                    "dependencies": [],
                    "exclusive_resources": [],
                    "task_runtime": {
                        "status": "running",
                        "pid": 123
                    }
                },
                {
                    "id": "NEXT",
                    "title": "next",
                    "state": "todo",
                    "priority": "1",
                    "dependencies": [],
                    "exclusive_resources": []
                }
            ],
            "resource_locks": {}
        });
        let completion = apply_job_completion_in_registry(
            &mut registry,
            "DONE",
            &JobCompletionInput {
                success: true,
                attempt: 1,
                max_attempts: 1,
                timed_out: false,
                phase_timeout_seconds: 0,
                elapsed_seconds: 1,
                status_text: "task already satisfied".to_string(),
                completion_kind: Some(PerformerCompletionKind::AlreadySatisfied),
                error_code: None,
                error_origin: None,
                error_message: None,
                auto_retry_error_codes: Vec::new(),
                auto_retry_max: 0,
                backoff_base_seconds: 30,
                backoff_max_seconds: 300,
                normalizer_input: None,
            },
            &NormalizerRegistry::empty(),
            "2026-02-21T00:00:00Z",
        )
        .expect("apply completion");
        assert_eq!(completion.status_label, "already_satisfied");
        assert_eq!(registry["tasks"][0]["state"], "merged");
        let actions =
            build_advance_actions(&registry, &HashSet::new(), "", None).expect("advance actions");
        assert!(!actions.iter().any(|action| {
            matches!(
                action,
                AdvanceTaskAction::RunPhase { task_id, .. }
                    | AdvanceTaskAction::QueueMerge { task_id, .. }
                    if task_id == "DONE"
            )
        }));
    }

    #[test]
    fn success_without_changes_task_does_not_schedule_review_or_merge() {
        let mut registry = json!({
            "tasks": [
                {
                    "id": "DONE",
                    "title": "done",
                    "state": "claimed",
                    "priority": "0",
                    "dependencies": [],
                    "exclusive_resources": [],
                    "task_runtime": {
                        "status": "running",
                        "pid": 123
                    }
                },
                {
                    "id": "NEXT",
                    "title": "next",
                    "state": "todo",
                    "priority": "1",
                    "dependencies": [],
                    "exclusive_resources": []
                }
            ],
            "resource_locks": {}
        });
        let completion = apply_job_completion_in_registry(
            &mut registry,
            "DONE",
            &JobCompletionInput {
                success: true,
                attempt: 1,
                max_attempts: 1,
                timed_out: false,
                phase_timeout_seconds: 0,
                elapsed_seconds: 1,
                status_text: "task completed without code changes".to_string(),
                completion_kind: Some(PerformerCompletionKind::SuccessWithoutChanges),
                error_code: None,
                error_origin: None,
                error_message: None,
                auto_retry_error_codes: Vec::new(),
                auto_retry_max: 0,
                backoff_base_seconds: 30,
                backoff_max_seconds: 300,
                normalizer_input: None,
            },
            &NormalizerRegistry::empty(),
            "2026-02-21T00:00:00Z",
        )
        .expect("apply completion");
        assert_eq!(completion.status_label, "success_without_changes");
        assert_eq!(registry["tasks"][0]["state"], "merged");
        let actions =
            build_advance_actions(&registry, &HashSet::new(), "", None).expect("advance actions");
        assert!(!actions.iter().any(|action| {
            matches!(
                action,
                AdvanceTaskAction::RunPhase { task_id, .. }
                    | AdvanceTaskAction::QueueMerge { task_id, .. }
                    if task_id == "DONE"
            )
        }));
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
        // Applying a fix transition on an in_progress task should fail
        // because fix is only valid from changes_requested state.
        let mut task = json!({"id":"T5","state":"in_progress","task_runtime":{"status":"running"}});
        let err = apply_phase_success(
            &mut task,
            PhaseTransition {
                mode: "fix",
                next_state: WorkflowState::PrOpen,
                runtime_phase: "fix",
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

    // ── Normalizer boundary tests (registry-independent) ─────────────
    // Tests that require a real normalizer registry live in
    // `registry/tests/engine_normalizer.rs` where all adapter crates
    // are linked and NormalizerRegistry::from_inventory() is populated.

    #[test]
    fn unknown_tool_falls_back_to_e101() {
        // No normalizer registered for "agentic-x" → caller's E101 used.
        let task_json = json!({
            "id": "TN1",
            "state": "claimed",
            "tool": "agentic-x",
            "task_runtime": { "status": "running", "pid": 1 }
        });
        let input = JobCompletionInput {
            success: false,
            attempt: 1,
            max_attempts: 1,
            timed_out: false,
            phase_timeout_seconds: 300,
            elapsed_seconds: 10,
            status_text: "performer exited with error".to_string(),
            completion_kind: None,
            error_code: None,
            error_origin: None,
            error_message: None,
            auto_retry_error_codes: Vec::new(),
            auto_retry_max: 0,
            backoff_base_seconds: 30,
            backoff_max_seconds: 300,
            normalizer_input: Some(NormalizerInput {
                exit_code: 1,
                stderr: "some unexpected error text".to_string(),
                stdout: String::new(),
            }),
        };
        let mut task_val = task_json;
        apply_job_completion(
            &mut task_val,
            &input,
            &NormalizerRegistry::empty(),
            "2026-02-21T00:00:00Z",
        );
        assert_eq!(task_val["task_runtime"]["last_error_code"], "E101");
        assert!(task_val["task_runtime"]["tool_error"].is_null());
    }

    #[test]
    fn normalizer_not_invoked_when_normalizer_input_is_none() {
        // Legacy path: no normalizer_input → caller's error_code wins.
        let mut task_val = json!({
            "id": "TN2",
            "state": "claimed",
            "tool": "claude",
            "task_runtime": { "status": "running" }
        });
        let out = apply_job_completion(
            &mut task_val,
            &JobCompletionInput {
                success: false,
                attempt: 1,
                max_attempts: 1,
                timed_out: false,
                phase_timeout_seconds: 300,
                elapsed_seconds: 5,
                status_text: "performer failed".to_string(),
                completion_kind: None,
                error_code: Some("E201".to_string()),
                error_origin: Some("runner".to_string()),
                error_message: Some("auth error".to_string()),
                auto_retry_error_codes: Vec::new(),
                auto_retry_max: 0,
                backoff_base_seconds: 30,
                backoff_max_seconds: 300,
                normalizer_input: None,
            },
            &NormalizerRegistry::empty(),
            "2026-02-21T00:00:00Z",
        );
        assert_eq!(out.status_label, "failed");
        assert_eq!(task_val["task_runtime"]["last_error_code"], "E201");
        assert!(out.tool_error.is_none());
    }

    // ── L4-SES-001: session preservation on error ──────────────────────

    fn make_error_completion_input(kind: PerformerCompletionKind) -> JobCompletionInput {
        JobCompletionInput {
            success: true,
            attempt: 1,
            max_attempts: 1,
            timed_out: false,
            phase_timeout_seconds: 300,
            elapsed_seconds: 5,
            status_text: "performer reported error".to_string(),
            completion_kind: Some(kind),
            error_code: None,
            error_origin: None,
            error_message: None,
            auto_retry_error_codes: Vec::new(),
            auto_retry_max: 0,
            backoff_base_seconds: 30,
            backoff_max_seconds: 300,
            normalizer_input: None,
        }
    }

    #[test]
    fn error_with_changes_preserves_session_id_in_runtime() {
        // Healthy worktree + committed progress should keep the same worktree
        // attached so the retry can continue from existing commits.
        let repo = make_repo_with_commit_ahead("main");
        let wt = repo.to_string_lossy().to_string();
        let mut task_val = json!({
            "id": "SES1",
            "state": "claimed",
            "tool": "claude",
            "worktree": {
                "worktree_path": wt,
                "base_branch": "main",
                "session_id": "claude-session-abc"
            },
            "task_runtime": { "status": "running", "pid": 42 }
        });
        let out = apply_job_completion(
            &mut task_val,
            &make_error_completion_input(PerformerCompletionKind::ErrorWithChanges),
            &NormalizerRegistry::empty(),
            "2026-04-12T00:00:00Z",
        );
        assert_eq!(out.status_label, "error_with_changes");
        assert_eq!(
            task_val["worktree"]["worktree_path"],
            repo.to_string_lossy().to_string()
        );
        // Task reset to todo for retry.
        assert_eq!(task_val["state"], "todo");
        // Session preserved in runtime.
        assert_eq!(
            task_val["task_runtime"]["last_session_id"],
            "claude-session-abc"
        );
        assert_eq!(task_val["task_runtime"]["last_session_tool"], "claude");
        let _ = fs::remove_dir_all(&repo);
    }

    #[test]
    fn error_with_changes_prefers_active_session_chain_over_claim_session() {
        let mut task_val = json!({
            "id": "SES1B",
            "state": "claimed",
            "tool": "codex",
            "worktree": {
                "worktree_path": "/tmp/wt-ses1b",
                "session_id": "coordinator-L4-SES-002-2026"
            },
            "task_runtime": {
                "status": "running",
                "active_session_id": "codex-session-real"
            }
        });
        apply_job_completion(
            &mut task_val,
            &make_error_completion_input(PerformerCompletionKind::ErrorWithChanges),
            &NormalizerRegistry::empty(),
            "2026-04-12T00:00:00Z",
        );
        assert_eq!(
            task_val["task_runtime"]["last_session_id"],
            "codex-session-real"
        );
        assert_eq!(task_val["task_runtime"]["last_session_tool"], "codex");
    }

    #[test]
    fn error_without_changes_preserves_session_id_in_runtime() {
        // Same guarantee for error_without_changes.
        let mut task_val = json!({
            "id": "SES2",
            "state": "claimed",
            "tool": "codex",
            "worktree": {
                "worktree_path": "/tmp/wt-ses2",
                "session_id": "codex-session-xyz"
            },
            "task_runtime": { "status": "running" }
        });
        apply_job_completion(
            &mut task_val,
            &make_error_completion_input(PerformerCompletionKind::ErrorWithoutChanges),
            &NormalizerRegistry::empty(),
            "2026-04-12T00:00:00Z",
        );
        assert!(task_val["worktree"].is_null());
        assert_eq!(
            task_val["task_runtime"]["last_session_id"],
            "codex-session-xyz"
        );
        assert_eq!(task_val["task_runtime"]["last_session_tool"], "codex");
    }

    #[test]
    fn error_with_changes_no_session_does_not_overwrite_prior_preserved() {
        // When the worktree has no session_id (e.g. was never set), the existing
        // last_session_id in the runtime must not be cleared.
        let mut task_val = json!({
            "id": "SES3",
            "state": "claimed",
            "tool": "claude",
            "worktree": { "worktree_path": "/tmp/wt-ses3" },
            "task_runtime": {
                "status": "running",
                "last_session_id": "claude-session-prev",
                "last_session_tool": "claude"
            }
        });
        apply_job_completion(
            &mut task_val,
            &make_error_completion_input(PerformerCompletionKind::ErrorWithChanges),
            &NormalizerRegistry::empty(),
            "2026-04-12T00:00:00Z",
        );
        // No new session to preserve; prior value must remain unchanged.
        assert_eq!(
            task_val["task_runtime"]["last_session_id"],
            "claude-session-prev"
        );
        assert_eq!(task_val["task_runtime"]["last_session_tool"], "claude");
    }

    #[test]
    fn error_with_changes_unhealthy_worktree_is_cleared() {
        let repo = make_repo_with_commit_ahead("main");
        fs::write(repo.join(".git/index.lock"), "").expect("write index lock");
        let mut task_val = json!({
            "id": "SES4",
            "state": "claimed",
            "tool": "claude",
            "worktree": {
                "worktree_path": repo.to_string_lossy().to_string(),
                "base_branch": "main",
                "session_id": "claude-session-unhealthy"
            },
            "task_runtime": { "status": "running" }
        });

        apply_job_completion(
            &mut task_val,
            &make_error_completion_input(PerformerCompletionKind::ErrorWithChanges),
            &NormalizerRegistry::empty(),
            "2026-04-12T00:00:00Z",
        );
        assert!(task_val["worktree"].is_null());
        let _ = fs::remove_dir_all(&repo);
    }

    #[test]
    fn error_with_changes_stdout_marker_preserves_healthy_worktree() {
        let repo = make_repo_with_commit_ahead("main");
        let mut task_val = json!({
            "id": "SES5",
            "state": "claimed",
            "tool": "claude",
            "worktree": {
                "worktree_path": repo.to_string_lossy().to_string(),
                "base_branch": "main"
            },
            "task_runtime": { "status": "running" }
        });
        let out = apply_job_completion(
            &mut task_val,
            &JobCompletionInput {
                success: false,
                attempt: 1,
                max_attempts: 1,
                timed_out: false,
                phase_timeout_seconds: 300,
                elapsed_seconds: 5,
                status_text: "performer failed".to_string(),
                completion_kind: None,
                error_code: None,
                error_origin: None,
                error_message: None,
                auto_retry_error_codes: Vec::new(),
                auto_retry_max: 0,
                backoff_base_seconds: 30,
                backoff_max_seconds: 300,
                normalizer_input: Some(NormalizerInput {
                    exit_code: 1,
                    stderr: String::new(),
                    stdout: "MACC_TASK_RESULT: error_with_changes".to_string(),
                }),
            },
            &NormalizerRegistry::empty(),
            "2026-04-12T00:00:00Z",
        );
        assert_eq!(out.status_label, "error_with_changes");
        assert_eq!(
            task_val["worktree"]["worktree_path"],
            repo.to_string_lossy().to_string()
        );
        let _ = fs::remove_dir_all(&repo);
    }

    #[test]
    fn salvage_check_merged_marks_task_merged_and_skips_retry() {
        let task_id = "SALVAGE-MERGED-001";
        let (repo, branch, worktree) = make_salvage_repo("main", task_id, false);
        let mut registry = json!({
            "tasks": [{
                "id": task_id,
                "state": "claimed",
                "base_branch": "main",
                "task_runtime": {
                    "status": "running",
                    "last_worktree_path": worktree.to_string_lossy().to_string(),
                    "last_branch": branch
                }
            }],
            "resource_locks": {}
        });

        let result = salvage_check_in_registry(
            &mut registry,
            task_id,
            &repo,
            false,
            None,
            "2026-03-01T00:00:00Z",
            |_, _, _, _, _, _| {},
        )
        .expect("salvage check");

        assert_eq!(result, SalvageResult::Merged);
        assert_eq!(registry["tasks"][0]["state"], "merged");
        assert_eq!(registry["tasks"][0]["task_runtime"]["status"], "idle");
        let _ = fs::remove_dir_all(&worktree);
        let _ = fs::remove_dir_all(&repo);
    }

    #[test]
    fn salvage_check_conflict_proceeds_retry() {
        let task_id = "SALVAGE-CONFLICT-001";
        let (repo, branch, worktree) = make_salvage_repo("main", task_id, true);
        let mut registry = json!({
            "tasks": [{
                "id": task_id,
                "state": "claimed",
                "base_branch": "main",
                "task_runtime": {
                    "status": "running",
                    "last_worktree_path": worktree.to_string_lossy().to_string(),
                    "last_branch": branch
                }
            }],
            "resource_locks": {}
        });

        let result = salvage_check_in_registry(
            &mut registry,
            task_id,
            &repo,
            false,
            None,
            "2026-03-01T00:00:00Z",
            |_, _, _, _, _, _| {},
        )
        .expect("salvage check");

        assert_eq!(result, SalvageResult::ConflictProceedRetry);
        assert_eq!(registry["tasks"][0]["state"], "claimed");
        let _ = fs::remove_dir_all(&worktree);
        let _ = fs::remove_dir_all(&repo);
    }

    #[test]
    fn salvage_check_no_commits_proceeds_retry() {
        let repo = make_repo_with_commit_ahead("main");
        let mut registry = json!({
            "tasks": [{
                "id": "SALVAGE-NOCOMMITS-001",
                "state": "claimed",
                "base_branch": "main",
                "task_runtime": {
                    "status": "running",
                    "last_worktree_path": repo.to_string_lossy().to_string(),
                    "last_branch": "task/ipc-override"
                }
            }],
            "resource_locks": {}
        });

        let result = salvage_check_in_registry(
            &mut registry,
            "SALVAGE-NOCOMMITS-001",
            &repo,
            false,
            None,
            "2026-03-01T00:00:00Z",
            |_, _, _, _, _, _| {},
        )
        .expect("salvage check");

        assert_eq!(result, SalvageResult::NoCommitsProceedRetry);
        assert_eq!(registry["tasks"][0]["state"], "claimed");
        let _ = fs::remove_dir_all(&repo);
    }

    #[test]
    fn apply_job_completion_error_preserves_last_worktree_metadata_before_clear() {
        let mut task = json!({
            "id":"T-LAST-WT",
            "state":"claimed",
            "worktree":{
                "worktree_path":"/tmp/salvage-wt",
                "branch":"task/T-LAST-WT",
                "base_branch":"main"
            },
            "task_runtime":{"status":"running","pid":123}
        });
        let out = apply_job_completion(
            &mut task,
            &JobCompletionInput {
                success: true,
                attempt: 1,
                max_attempts: 1,
                timed_out: false,
                phase_timeout_seconds: 0,
                elapsed_seconds: 1,
                status_text: "performer reported error".to_string(),
                completion_kind: Some(PerformerCompletionKind::ErrorWithoutChanges),
                error_code: None,
                error_origin: None,
                error_message: None,
                auto_retry_error_codes: Vec::new(),
                auto_retry_max: 0,
                backoff_base_seconds: 30,
                backoff_max_seconds: 300,
                normalizer_input: None,
            },
            &NormalizerRegistry::empty(),
            "2026-03-01T00:00:00Z",
        );

        assert_eq!(out.status_label, "error_without_changes");
        assert!(task["worktree"].is_null());
        assert_eq!(
            task["task_runtime"]["last_worktree_path"],
            "/tmp/salvage-wt"
        );
        assert_eq!(task["task_runtime"]["last_branch"], "task/T-LAST-WT");
    }
}
