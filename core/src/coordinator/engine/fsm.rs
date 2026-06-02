use crate::config::{
    CanonicalConfig, CoordinatorConfig,
    CoordinatorConfigResolved as CanonicalCoordinatorConfigResolved,
};
use crate::coordinator::control_plane::CoordinatorLog;
use crate::coordinator::error_normalizer::{
    error_code_to_canonical_class, CanonicalClass, NormalizerRegistry, ToolError,
};
use crate::coordinator::model::{Task, TaskRegistry};
use crate::coordinator::rate_limit::RateLimitInfo;
use crate::coordinator::runtime::{
    merge_task_with_policy_native, process_branch_cleanup_queue, terminate_active_jobs,
    CoordinatorRunState,
};
use crate::coordinator::state_runtime::{
    cleanup_dead_runtime_tasks, clear_coordinator_pause_file, coordinator_pause_file_path,
    execute_startup_recovery_sweep, resume_paused_task_merge, set_task_paused_for_merge,
    write_coordinator_pause_file,
};
use crate::coordinator::{
    CompletionAuthority, PerformerCompletionKind, RuntimeStatus, WorkflowState,
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

use super::completion::{classify_completion_error, ErrorClassification};
use super::retry::resolve_retry_strategy;
use super::transitions::apply_state_transitions;

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
    pub run_id: String,
    pub coordinator_epoch: i64,
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
    /// Task is in a merge-ready state but has no branch recorded (e.g. the
    /// worktree was cleared by ghost cleanup before the job-exit was applied).
    /// Block it so the coordinator can make progress instead of spinning.
    BlockNoBranch { task_id: String },
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
    phases: &crate::config::PhasesConfig,
) -> AdvancePlan {
    match state {
        WorkflowState::InProgress => {
            if phases.testing.enabled {
                AdvancePlan::RunPhase(PhaseTransition {
                    mode: "test",
                    next_state: WorkflowState::Testing,
                    runtime_phase: "test",
                })
            } else if phases.review.enabled && max_review_cycles != Some(0) {
                AdvancePlan::RunPhase(PhaseTransition {
                    mode: "review",
                    next_state: WorkflowState::Reviewing,
                    runtime_phase: "review",
                })
            } else {
                AdvancePlan::Merge
            }
        }
        WorkflowState::Testing => {
            if phases.review.enabled && max_review_cycles != Some(0) {
                AdvancePlan::RunPhase(PhaseTransition {
                    mode: "review",
                    next_state: WorkflowState::Reviewing,
                    runtime_phase: "review",
                })
            } else {
                AdvancePlan::Merge
            }
        }
        WorkflowState::Reviewing => {
            AdvancePlan::Merge
        }
        WorkflowState::PrOpen => AdvancePlan::Merge,
        WorkflowState::ChangesRequested => {
            // If we've exhausted the review cycle budget, skip fix and go to merge.
            if let Some(max) = max_review_cycles {
                if review_cycles >= max {
                    return AdvancePlan::Merge;
                }
            }
            AdvancePlan::RunPhase(PhaseTransition {
                mode: "fix",
                next_state: if phases.testing.enabled {
                    WorkflowState::Testing
                } else if phases.review.enabled {
                    WorkflowState::Reviewing
                } else {
                    WorkflowState::PrOpen // fallback
                },
                runtime_phase: "fix",
            })
        }
        WorkflowState::Queued => AdvancePlan::Merge,
        _ => AdvancePlan::Noop,
    }
}

fn transition_workflow_state(
    from: WorkflowState,
    event: WorkflowEvent,
    next_state: Option<WorkflowState>,
) -> Result<WorkflowState> {
    let to = match (from, event) {
        // --- Success transitions ---
        (WorkflowState::InProgress, WorkflowEvent::PhaseSucceeded("test")) => {
            WorkflowState::Testing
        }
        (WorkflowState::InProgress, WorkflowEvent::PhaseSucceeded("review")) => {
            next_state.unwrap_or(WorkflowState::PrOpen)
        }
        (WorkflowState::Testing, WorkflowEvent::PhaseSucceeded("test")) => {
            next_state.unwrap_or(WorkflowState::Reviewing)
        }
        (WorkflowState::Testing, WorkflowEvent::PhaseSucceeded("review")) => {
            WorkflowState::Reviewing
        }
        (WorkflowState::Reviewing, WorkflowEvent::PhaseSucceeded("review")) => {
            WorkflowState::Merged
        }
        (WorkflowState::ChangesRequested, WorkflowEvent::PhaseSucceeded("fix")) => {
            next_state.unwrap_or(WorkflowState::PrOpen)
        }

        // --- Review verdict transitions ---
        (WorkflowState::InProgress, WorkflowEvent::ReviewChangesRequested) => {
            WorkflowState::ChangesRequested
        }
        (WorkflowState::Reviewing, WorkflowEvent::ReviewChangesRequested) => {
            WorkflowState::ChangesRequested
        }

        // --- Failure transitions ---
        (WorkflowState::InProgress, WorkflowEvent::PhaseFailed("dev")) => {
            WorkflowState::Blocked
        }
        (WorkflowState::InProgress, WorkflowEvent::PhaseFailed("review")) => {
            WorkflowState::Blocked
        }
        (WorkflowState::Testing, WorkflowEvent::PhaseFailed("test")) => {
            WorkflowState::InProgress
        }
        (WorkflowState::Reviewing, WorkflowEvent::PhaseFailed("review")) => {
            WorkflowState::Blocked
        }
        (WorkflowState::ChangesRequested, WorkflowEvent::PhaseFailed("fix")) => {
            WorkflowState::Blocked
        }

        // --- Merge transitions ---
        (WorkflowState::InProgress, WorkflowEvent::MergeSucceeded)
        | (WorkflowState::Testing, WorkflowEvent::MergeSucceeded)
        | (WorkflowState::Reviewing, WorkflowEvent::MergeSucceeded)
        | (WorkflowState::PrOpen, WorkflowEvent::MergeSucceeded)
        | (WorkflowState::ChangesRequested, WorkflowEvent::MergeSucceeded)
        | (WorkflowState::Queued, WorkflowEvent::MergeSucceeded) => {
            WorkflowState::Merged
        }

        (WorkflowState::InProgress, WorkflowEvent::MergeFailed)
        | (WorkflowState::Testing, WorkflowEvent::MergeFailed)
        | (WorkflowState::Reviewing, WorkflowEvent::MergeFailed)
        | (WorkflowState::PrOpen, WorkflowEvent::MergeFailed)
        | (WorkflowState::ChangesRequested, WorkflowEvent::MergeFailed)
        | (WorkflowState::Queued, WorkflowEvent::MergeFailed) => {
            WorkflowState::Blocked
        }
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

    if !crate::coordinator::is_valid_workflow_transition(from, to) {
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
    phases: &crate::config::PhasesConfig,
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
            .map(|s| plan_advance(s, max_review_cycles, review_cycles, phases))
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
                    // Worktree/branch was cleared (e.g. by ghost cleanup) while
                    // the job-exit completion raced ahead and set the task to
                    // in_progress.  Block it so active > 0 does not spin the
                    // coordinator forever.
                    actions.push(AdvanceTaskAction::BlockNoBranch { task_id });
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
    runtime.set_status(RuntimeStatus::Dispatched);
    runtime.current_phase = Some(update.phase.clone());
    runtime.started_at = Some(update.now.clone());
    runtime.run_id = Some(update.run_id.clone());
    runtime.coordinator_epoch = Some(update.coordinator_epoch);
    runtime.claim_id = Some(update.session_id.clone());
    if let Some(active_session_id) = update.active_session_id.as_ref() {
        runtime.active_session_id = Some(active_session_id.clone());
    }
    let log_prefix = format!(".macc/log/performer/{}/{}", update.task_id, update.run_id);
    runtime.events_log = Some(format!("{}.events.jsonl", log_prefix));
    runtime.stdout_log = Some(format!("{}.stdout.log", log_prefix));
    runtime.stderr_log = Some(format!("{}.stderr.log", log_prefix));
    runtime.pid = update.pid;
    task.touch_state_changed(&update.now);
}

fn apply_dispatch_pid_typed(task: &mut Task, pid: Option<i64>) {
    let runtime = task.ensure_runtime();
    runtime.pid = pid;
    runtime.set_status(RuntimeStatus::Running);
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
    let to = transition_workflow_state(
        from,
        WorkflowEvent::PhaseSucceeded(transition.mode),
        Some(transition.next_state),
    )?;
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
            transition_workflow_state(from, WorkflowEvent::PhaseSucceeded("review"), Some(WorkflowState::Merged))?
        }
        ReviewVerdict::ChangesRequested => {
            transition_workflow_state(from, WorkflowEvent::ReviewChangesRequested, Some(WorkflowState::ChangesRequested))?
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
    let to = transition_workflow_state(from, WorkflowEvent::PhaseFailed(phase_mode), None)?;
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
    let to = transition_workflow_state(from, WorkflowEvent::MergeSucceeded, Some(WorkflowState::Merged))?;
    task.set_workflow_state(to);
    let runtime = task.ensure_runtime();
    runtime.set_status(RuntimeStatus::Idle);
    runtime.pid = None;
    task.touch_state_changed(now);
    Ok(())
}

pub(crate) fn apply_merge_failure_typed(task: &mut Task, reason: &str, now: &str) -> Result<()> {
    let from = task_workflow_state_typed(task)?;
    let to = transition_workflow_state(from, WorkflowEvent::MergeFailed, Some(WorkflowState::Blocked))?;
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
pub(super) fn store_classified_error_in_extra(
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

pub(super) fn capture_last_assignment_before_clear(task: &mut Task) {
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

pub(super) fn preserve_active_session_chain(task: &mut Task) {
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
pub(super) struct CompletionErrorDetails {
    pub(super) code: String,
    pub(super) origin: String,
    pub(super) message: String,
}

impl CompletionErrorDetails {
    pub(super) fn from_classification(classification: &ErrorClassification) -> Self {
        Self {
            code: classification.error_code.clone(),
            origin: classification.error_origin.clone(),
            message: classification.error_message.clone(),
        }
    }
}

#[derive(Debug, Clone)]
pub(super) struct CoordinatorConfigResolved {
    pub(super) auto_retry_error_codes: Vec<String>,
    pub(super) auto_retry_max: usize,
    pub(super) backoff_base_seconds: u64,
    pub(super) backoff_max_seconds: u64,
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
pub(super) enum RetryOutcome {
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
pub(super) enum BlockOutcome {
    InvalidInput,
    TerminalFailure {
        error: CompletionErrorDetails,
        tool_error: Box<Option<ToolError>>,
        now_ts: u64,
    },
}

#[allow(dead_code)]
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

pub(super) fn should_auto_retry_error_code(
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
        runtime.last_error_code = Some(crate::coordinator::error_normalizer::E414_PERFORMER_PROCESS_DEAD.to_string());
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
                backend.sleep_between_cycles().await?;
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

/// Returns true when a merge failure reason represents an actual conflict that
/// requires operator intervention and justifies killing other running performers.
///
/// Transient failures — `precheck_clean` (dirty working tree), `verify_branch`,
/// `verify_base`, and `timeout` — should be retried automatically without
/// disturbing other running tasks.  Only `step=merge` (a genuine git conflict)
/// needs the full operator-wait flow.
fn is_operator_blocked_merge(reason: &str) -> bool {
    reason.starts_with("failure:local_merge step=merge")
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
    if let Some(_reason) = backend.should_terminate_run(&counts) {
        // This is a planned stop (e.g. dispatch limit reached), not a failure.
        // The dispatch_limit_reached event was already emitted by the dispatcher.
        // Returning Complete causes an exit code 0 so the TUI shows a green
        // "finished" message instead of a red error overlay.
        return Ok(ControlPlaneDecision::Complete);
    }
    let last_fail = backend.last_dispatch_failure();
    controller.on_cycle_counts(counts, last_fail.as_deref())
}

struct NativeControlPlaneBackend<'a> {
    repo_root: &'a Path,
    canonical: &'a CanonicalConfig,
    coordinator: Option<&'a CoordinatorConfig>,
    env_cfg: &'a crate::coordinator::types::CoordinatorEnvConfig,
    logger: Option<&'a dyn CoordinatorLog>,
    prd_file: PathBuf,
    run_state: CoordinatorRunState,
    phase_runner_max_attempts: usize,
    coordinator_tool_override: Option<String>,
    phase_timeout_seconds: usize,
    ghost_heartbeat_grace_seconds: i64,
    last_logged_counts: Option<CoordinatorCounts>,
    last_cycle_progressed: bool,
    /// Timestamp of the last coordinator-alive heartbeat event emitted to SQLite.
    /// Used to throttle periodic heartbeats to once every 30 seconds so that
    /// viewer TUIs always see recent activity even while performers are running.
    last_sqlite_heartbeat_at: Option<std::time::Instant>,
}

#[async_trait]
impl ControlPlaneBackend for NativeControlPlaneBackend<'_> {
    async fn on_cycle_start(&mut self, _cycle: usize) -> Result<()> {
        let storage_paths = crate::coordinator_storage::CoordinatorStoragePaths::from_project_paths(
            &crate::ProjectPaths::from_root(self.repo_root),
        );
        let sqlite = crate::coordinator_storage::SqliteStorage::new(storage_paths);
        
        let _ = sqlite.get_active_coordinator_run().map(|run_opt| {
            if let Some(mut r) = run_opt {
                r.last_tick_at = Some(chrono::Utc::now().to_rfc3339());
                let _ = sqlite.upsert_coordinator_run(&r);
            }
        });

        if let Ok(Some(ctrl)) = sqlite.get_coordinator_control() {
            if ctrl.mode == "force_stopping" {
                if let Some(log) = self.logger {
                    let _ = log.note("- Force stop requested. Terminating all performers...".to_string());
                }
                
                let mut pids_to_kill = std::collections::HashSet::new();
                if let Ok(db_pids) = sqlite.get_running_task_pids() {
                    for (task_id, pid) in db_pids {
                        pids_to_kill.insert(pid);
                        let _ = crate::coordinator::helpers::append_coordinator_event_with_severity(
                            self.repo_root,
                            "task_blocked",
                            &task_id,
                            "dev",
                            "aborted_by_operator",
                            "Task aborted by operator force-stop",
                            "warning",
                        );
                    }
                }
                for (_, job) in &self.run_state.active_jobs {
                    if let Some(pid) = job.pid {
                        pids_to_kill.insert(pid);
                    }
                }
                
                let grace_secs = ctrl.force_grace_seconds.unwrap_or(10);
                for pid in &pids_to_kill {
                    if let Some(log) = self.logger {
                        let _ = log.note(format!("- Sending TERM to performer process group {}", pid));
                    }
                    #[cfg(unix)]
                    if *pid > 0 {
                        unsafe {
                            let _ = libc::killpg(*pid as libc::pid_t, libc::SIGTERM);
                        }
                    }
                }
                if !pids_to_kill.is_empty() {
                    tokio::time::sleep(std::time::Duration::from_secs(grace_secs)).await;
                    for pid in &pids_to_kill {
                        if let Some(log) = self.logger {
                            let _ = log.note(format!("- Sending KILL to performer process group {}", pid));
                        }
                        #[cfg(unix)]
                        if *pid > 0 {
                            unsafe {
                                let _ = libc::killpg(*pid as libc::pid_t, libc::SIGKILL);
                            }
                        }
                    }
                    tokio::time::sleep(std::time::Duration::from_millis(100)).await;
                    for pid in &pids_to_kill {
                        if crate::coordinator::helpers::is_pid_running(*pid) {
                            if let Some(log) = self.logger {
                                let _ = log.note(format!("- Warning: performer process group {} survived SIGKILL [E416]", pid));
                            }
                        }
                    }
                }

                if ctrl.cleanup_after_force.unwrap_or(false) {
                    if let Some(log) = self.logger {
                        let _ = log.note("- Cleaning worktrees as requested by force cleanup...".to_string());
                    }
                    let _ = crate::service::worktree::remove_all_worktrees(self.repo_root, true);
                    let _ = crate::prune_worktrees(self.repo_root);
                }

                let mut stopped_ctrl = ctrl.clone();
                stopped_ctrl.mode = "stopped".to_string();
                let _ = sqlite.set_coordinator_control(&stopped_ctrl);

                return Err(MaccError::Validation("force stopped by operator".to_string()));
            } else if ctrl.mode == "draining" {
                let mut active_draining_tasks_left = false;
                if let Some(snapshot_raw) = &ctrl.drain_snapshot_json {
                    if let Ok(task_ids) = serde_json::from_str::<Vec<String>>(snapshot_raw) {
                        let registry = crate::coordinator::state::coordinator_state_registry_load(self.repo_root, &std::collections::BTreeMap::new())?;
                        if let Some(tasks_arr) = registry.get("tasks").and_then(Value::as_array) {
                            for task_val in tasks_arr {
                                if let Some(tid) = task_val.get("id").and_then(Value::as_str) {
                                    if task_ids.contains(&tid.to_string()) {
                                        if let Some(state_str) = task_val.get("state").and_then(Value::as_str) {
                                            if state_str == WorkflowState::Claimed.as_str()
                                                || state_str == WorkflowState::InProgress.as_str()
                                                || state_str == WorkflowState::ChangesRequested.as_str()
                                                || state_str == WorkflowState::Queued.as_str()
                                            {
                                                active_draining_tasks_left = true;
                                                break;
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
                if !active_draining_tasks_left {
                    if let Some(log) = self.logger {
                        let _ = log.note("- All active drain tasks completed. Draining complete.".to_string());
                    }
                    let mut stopped_ctrl = ctrl.clone();
                    stopped_ctrl.mode = "stopped".to_string();
                    let _ = sqlite.set_coordinator_control(&stopped_ctrl);

                    return Err(MaccError::Validation("draining complete".to_string()));
                }
            } else if ctrl.mode == "graceful_stopping" {
                if self.run_state.active_jobs.is_empty() && self.run_state.active_merge_jobs.is_empty() {
                    if let Some(log) = self.logger {
                        let _ = log.note("- Graceful stopping completed. Exiting.".to_string());
                    }
                    let mut stopped_ctrl = ctrl.clone();
                    stopped_ctrl.mode = "stopped".to_string();
                    let _ = sqlite.set_coordinator_control(&stopped_ctrl);

                    return Err(MaccError::Validation("graceful stop complete".to_string()));
                }
            }
        }

        // Emit a periodic coordinator-alive heartbeat so viewer TUIs always
        // see recent activity even while performers are long-running.
        // Throttled to once every 30 seconds.
        const HEARTBEAT_INTERVAL_SECS: u64 = 30;
        let should_emit_heartbeat = self
            .last_sqlite_heartbeat_at
            .map(|last| last.elapsed().as_secs() >= HEARTBEAT_INTERVAL_SECS)
            .unwrap_or(true);
        if should_emit_heartbeat {
            let _ = crate::coordinator::helpers::append_coordinator_event_with_severity(
                self.repo_root,
                "coordinator_heartbeat",
                "",
                "run",
                "ok",
                "coordinator is running",
                "info",
            );
            self.last_sqlite_heartbeat_at = Some(std::time::Instant::now());
        }

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
        if !is_operator_blocked_merge(reason) {
            // Transient failure (e.g. precheck_clean, timeout, verify_branch).
            // Do not kill other running performers or enter the operator-wait
            // loop — just resume the task immediately so the merge is retried
            // in the next advance cycle.  A short sleep lets any transient git
            // or filesystem state settle before the retry.
            if let Some(log) = self.logger {
                let _ = log.note(format!(
                    "- Merge retry pending task={} reason={} (transient; retrying next cycle)",
                    task_id, reason
                ));
            }
            tokio::time::sleep(Duration::from_secs(3)).await;
            resume_paused_task_merge(self.repo_root, task_id)?;
            return Ok(());
        }

        // Actual merge conflict — pause all other performers and wait for
        // an operator to resolve the conflict before continuing.
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
    env_cfg: &crate::coordinator::types::CoordinatorEnvConfig,
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
    env_cfg: &crate::coordinator::types::CoordinatorEnvConfig,
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

    let storage_paths = crate::coordinator_storage::CoordinatorStoragePaths::from_project_paths(
        &crate::ProjectPaths::from_root(repo_root),
    );
    let sqlite = crate::coordinator_storage::SqliteStorage::new(storage_paths.clone());
    
    // Register coordinator run and seed control mode
    let last_run = sqlite.get_active_coordinator_run().unwrap_or(None);
    let max_epoch = {
        if let Ok(conn) = rusqlite::Connection::open(&sqlite.paths.sqlite_path) {
            let _ = sqlite.init_schema(&conn);
            conn.query_row("SELECT COALESCE(MAX(epoch), 0) FROM coordinator_runs", [], |row| row.get::<_, i64>(0)).unwrap_or(0)
        } else {
            0
        }
    };
    let next_epoch = max_epoch + 1;
    std::env::set_var("COORDINATOR_EPOCH", next_epoch.to_string());
    if let Some(mut prev) = last_run {
        if (prev.status == "running" || prev.status == "draining")
            && crate::coordinator::helpers::is_pid_running(prev.pid)
            && prev.pid != std::process::id() as i64
        {
            return Err(MaccError::Coordinator {
                code: crate::coordinator::error_normalizer::E410_COORDINATOR_LEASE_CONFLICT,
                message: format!(
                    "Lease conflict: coordinator is already running on host {} under PID {}",
                    prev.hostname, prev.pid
                ),
            });
        }
        prev.status = "crashed".to_string();
        prev.stopped_at = Some(chrono::Utc::now().to_rfc3339());
        prev.stop_reason = Some("coordinator process crashed or exited uncleanly".to_string());
        let _ = sqlite.upsert_coordinator_run(&prev);
    }
    let hostname = std::env::var("HOSTNAME").unwrap_or_else(|_| {
        std::env::var("COMPUTERNAME").unwrap_or_else(|_| "localhost".to_string())
    });
    let current_run = crate::coordinator_storage::CoordinatorRun {
        run_id: run_id.clone(),
        pid: std::process::id() as i64,
        hostname,
        started_at: chrono::Utc::now().to_rfc3339(),
        last_tick_at: Some(chrono::Utc::now().to_rfc3339()),
        stopped_at: None,
        status: "running".to_string(),
        epoch: next_epoch,
        version: env!("CARGO_PKG_VERSION").to_string(),
        stop_reason: None,
    };
    let _ = sqlite.upsert_coordinator_run(&current_run);

    let _ = sqlite.set_coordinator_control(&crate::coordinator_storage::CoordinatorControl {
        mode: "running".to_string(),
        requested_at: Some(chrono::Utc::now().to_rfc3339()),
        requested_by: Some("system".to_string()),
        drain_snapshot_json: None,
        force_grace_seconds: Some(10),
        cleanup_after_force: Some(false),
        reason: None,
    });

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
    let max_review_cycles = env_cfg.max_review_cycles.or(cfg.max_review_cycles);
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

    let json_compat = env_cfg.json_compat.unwrap_or(cfg.json_compat);

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

    // Reset sessions that are stuck "active" because a performer exited without
    // its EXIT trap (SIGKILL, OOM, host crash). 1800 s matches the shell-side
    // SESSION_LEASE_TTL_SECONDS default in performer_lib.sh.
    match crate::coordinator::session_manager::reset_stale_active_sessions(repo_root, 1800) {
        Ok(n) if n > 0 => {
            if let Some(log) = logger {
                let _ = log.note(format!(
                    "- Startup session recovery: reset {} stale active session(s) to available",
                    n
                ));
            }
        }
        Ok(_) => {}
        Err(e) => {
            if let Some(log) = logger {
                let _ = log.note(format!("- Warning: startup session recovery failed: {}", e));
            }
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

    // ── Part E: replay unread events from SQLite on startup ─────────────
    if let Err(e) = crate::coordinator::control_plane::consume_runtime_events(repo_root, &mut run_state, logger) {
        if let Some(log) = logger {
            let _ = log.note(format!("- Warning: startup event replay failed: {}", e));
        }
    } else {
        let _ = crate::coordinator::control_plane::apply_runtime_event_bus_updates(
            repo_root,
            env_cfg,
            coordinator,
            &mut run_state,
            logger,
        );
    }

    // ── Part F: §11.1 Steps 7-10 — startup recovery sweep ───────────────
    // Inspects worktree state (step 7), Git branch/merge state (step 8),
    // runs deterministic PRD reconciliation (step 9), and persists
    // recovery classifications to SQLite (step 10).
    // Must complete before any new dispatch (step 11).
    {
        let reference_branch: &str = env_cfg
            .reference_branch
            .as_deref()
            .unwrap_or(&cfg.reference_branch);
        // We cannot cheaply coerce &dyn CoordinatorLog into &dyn Fn(String)
        // across lifetimes, so we collect the entries and log the summary
        // ourselves using the coordinator logger.
        match execute_startup_recovery_sweep(repo_root, reference_branch, false, None) {
            Ok(entries) => {
                let classified = entries.len();
                let mutated = entries.iter().filter(|e| e.mutated).count();
                if let Some(log) = logger {
                    for entry in &entries {
                        let _ = log.note(format!(
                            "- Recovery sweep task={} classification={} action=\"{}\"",
                            entry.task_id, entry.classification, entry.action
                        ));
                    }
                    if classified > 0 {
                        let _ = log.note(format!(
                            "- Startup recovery sweep: {} task(s) classified, {} state(s) corrected",
                            classified, mutated
                        ));
                    }
                }
            }
            Err(e) => {
                if let Some(log) = logger {
                    let _ = log.note(format!(
                        "- Warning: startup recovery sweep failed: {}",
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
        last_sqlite_heartbeat_at: None,
    };

    let timeout_seconds = env_cfg.timeout_seconds.unwrap_or(cfg.timeout_seconds);
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

    let mut is_clean_exit = false;
    let mut final_status = "success".to_string();
    if let Err(ref err) = run_result {
        if let MaccError::Validation(ref msg) = err {
            if msg == "draining complete" || msg == "graceful stop complete" || msg == "force stopped by operator" {
                is_clean_exit = true;
                final_status = if msg == "draining complete" || msg == "graceful stop complete" { "stopped".to_string() } else { "force_stopping".to_string() };
            }
        }
    }

    let run_result = if is_clean_exit {
        Ok(())
    } else {
        run_result
    };

    let result_label = if run_result.is_err() {
        "failed"
    } else {
        let is_shutdown = *shutdown_rx.borrow();
        if is_shutdown {
            "stopped"
        } else if is_clean_exit {
            &final_status
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

    let _ = sqlite.get_active_coordinator_run().map(|run_opt| {
        if let Some(mut r) = run_opt {
            r.status = if is_clean_exit { final_status.clone() } else if run_result.is_err() { "crashed".to_string() } else { "stopped".to_string() };
            r.stopped_at = Some(chrono::Utc::now().to_rfc3339());
            if !is_clean_exit && run_result.is_err() {
                r.stop_reason = Some(format!("{:?}", run_result));
            } else if is_clean_exit {
                r.stop_reason = Some(final_status.clone());
            }
            let _ = sqlite.upsert_coordinator_run(&r);
        }
    });

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
    use crate::coordinator::engine::retry::RetryStrategy;
    use crate::coordinator::error_normalizer::ErrorNormalizer;
    use crate::coordinator::rate_limit::{E601_RATE_LIMITED, E602_QUOTA_EXHAUSTED};
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
    fn plan_advance_testing_phase() {
        let phases_enabled = crate::config::PhasesConfig {
            testing: crate::config::PhaseConfig { enabled: true, ..Default::default() },
            review: crate::config::PhaseConfig { enabled: true, ..Default::default() },
        };
        let phases_disabled = crate::config::PhasesConfig {
            testing: crate::config::PhaseConfig { enabled: false, ..Default::default() },
            review: crate::config::PhaseConfig { enabled: true, ..Default::default() },
        };

        // When testing is enabled:
        // InProgress should transition to RunPhase("test") / WorkflowState::Testing
        assert!(matches!(
            plan_advance(WorkflowState::InProgress, None, 0, &phases_enabled),
            AdvancePlan::RunPhase(PhaseTransition { mode: "test", next_state: WorkflowState::Testing, .. })
        ));

        // Testing should transition to RunPhase("review") / WorkflowState::Reviewing if review enabled
        assert!(matches!(
            plan_advance(WorkflowState::Testing, None, 0, &phases_enabled),
            AdvancePlan::RunPhase(PhaseTransition { mode: "review", next_state: WorkflowState::Reviewing, .. })
        ));

        // ChangesRequested should transition to RunPhase("fix") with next_state: WorkflowState::Testing
        assert!(matches!(
            plan_advance(WorkflowState::ChangesRequested, None, 0, &phases_enabled),
            AdvancePlan::RunPhase(PhaseTransition { mode: "fix", next_state: WorkflowState::Testing, .. })
        ));

        // When testing is disabled:
        // InProgress should transition to RunPhase("review") / WorkflowState::Reviewing if review enabled
        assert!(matches!(
            plan_advance(WorkflowState::InProgress, None, 0, &phases_disabled),
            AdvancePlan::RunPhase(PhaseTransition { mode: "review", next_state: WorkflowState::Reviewing, .. })
        ));

        // ChangesRequested should transition to RunPhase("fix") with next_state: WorkflowState::Reviewing
        assert!(matches!(
            plan_advance(WorkflowState::ChangesRequested, None, 0, &phases_disabled),
            AdvancePlan::RunPhase(PhaseTransition { mode: "fix", next_state: WorkflowState::Reviewing, .. })
        ));
    }

    #[test]
    fn plan_advance_maps_states() {
        let phases = crate::config::PhasesConfig {
            testing: crate::config::PhaseConfig { enabled: false, ..Default::default() },
            review: crate::config::PhaseConfig { enabled: true, ..Default::default() },
        };
        // Default (unlimited review cycles)
        assert!(matches!(
            plan_advance(WorkflowState::InProgress, None, 0, &phases),
            AdvancePlan::RunPhase(PhaseTransition { mode: "review", .. })
        ));
        assert!(matches!(
            plan_advance(WorkflowState::PrOpen, None, 0, &phases),
            AdvancePlan::Merge
        ));
        assert!(matches!(
            plan_advance(WorkflowState::ChangesRequested, None, 0, &phases),
            AdvancePlan::RunPhase(PhaseTransition { mode: "fix", .. })
        ));
        assert!(matches!(
            plan_advance(WorkflowState::Queued, None, 0, &phases),
            AdvancePlan::Merge
        ));
        assert!(matches!(
            plan_advance(WorkflowState::Todo, None, 0, &phases),
            AdvancePlan::Noop
        ));
    }

    #[test]
    fn plan_advance_max_review_cycles_zero_skips_review() {
        let phases = crate::config::PhasesConfig {
            testing: crate::config::PhaseConfig { enabled: false, ..Default::default() },
            review: crate::config::PhaseConfig { enabled: true, ..Default::default() },
        };
        // max_review_cycles=0 → skip review entirely
        assert!(matches!(
            plan_advance(WorkflowState::InProgress, Some(0), 0, &phases),
            AdvancePlan::Merge
        ));
        // ChangesRequested with 0 cycles used but max=0 → merge directly
        assert!(matches!(
            plan_advance(WorkflowState::ChangesRequested, Some(0), 0, &phases),
            AdvancePlan::Merge
        ));
    }

    #[test]
    fn plan_advance_max_review_cycles_one_allows_single_fix() {
        let phases = crate::config::PhasesConfig {
            testing: crate::config::PhaseConfig { enabled: false, ..Default::default() },
            review: crate::config::PhaseConfig { enabled: true, ..Default::default() },
        };
        // max=1, 0 cycles done → allow fix
        assert!(matches!(
            plan_advance(WorkflowState::ChangesRequested, Some(1), 0, &phases),
            AdvancePlan::RunPhase(PhaseTransition { mode: "fix", .. })
        ));
        // max=1, 1 cycle done → exhausted, merge directly
        assert!(matches!(
            plan_advance(WorkflowState::ChangesRequested, Some(1), 1, &phases),
            AdvancePlan::Merge
        ));
    }

    #[test]
    fn plan_advance_max_review_cycles_n_caps_loops() {
        let phases = crate::config::PhasesConfig {
            testing: crate::config::PhaseConfig { enabled: false, ..Default::default() },
            review: crate::config::PhaseConfig { enabled: true, ..Default::default() },
        };
        // max=3, 2 done → allow another
        assert!(matches!(
            plan_advance(WorkflowState::ChangesRequested, Some(3), 2, &phases),
            AdvancePlan::RunPhase(PhaseTransition { mode: "fix", .. })
        ));
        // max=3, 3 done → exhausted, merge
        assert!(matches!(
            plan_advance(WorkflowState::ChangesRequested, Some(3), 3, &phases),
            AdvancePlan::Merge
        ));
        // max=3, 5 done → way past, merge
        assert!(matches!(
            plan_advance(WorkflowState::ChangesRequested, Some(3), 5, &phases),
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
            pid: None,
            phase: "dev".to_string(),
            now: "2026-02-20T00:00:00Z".to_string(),
            run_id: "run-1".to_string(),
            coordinator_epoch: 1,
        };
        apply_dispatch_claim(&mut task, &update);
        assert_eq!(task["state"], "claimed");
        assert_eq!(task["task_runtime"]["status"], "dispatched");
        assert_eq!(task["task_runtime"]["active_session_id"], "tool-session-1");
        assert_eq!(task["task_runtime"]["pid"], serde_json::Value::Null);

        apply_dispatch_pid(&mut task, Some(123));
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
            build_advance_actions(&registry, &HashSet::new(), "", None, &crate::config::PhasesConfig::default()).expect("advance actions");
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
            build_advance_actions(&registry, &HashSet::new(), "", None, &crate::config::PhasesConfig::default()).expect("advance actions");
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
