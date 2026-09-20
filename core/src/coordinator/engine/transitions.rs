use super::fsm::{
    capture_last_assignment_before_clear, preserve_active_session_chain,
    store_classified_error_in_extra, BlockOutcome, JobCompletionResult, RetryOutcome,
};
use super::retry::RetryStrategy;
use crate::coordinator::error_normalizer::{
    E104_PERFORMER_PARTIAL_CHANGES, E105_PERFORMER_EXIT_NON_ZERO,
};
use crate::coordinator::model::Task;
use crate::coordinator::rate_limit::{
    update_throttle_state, RateLimitInfo, ToolThrottleState, E601_RATE_LIMITED,
    E602_QUOTA_EXHAUSTED,
};
use crate::coordinator::{PerformerCompletionKind, RuntimeStatus, WorkflowState};

/// Error code for a failure the tool reported about itself.
fn tool_error_code(completion_kind: &PerformerCompletionKind) -> &'static str {
    match completion_kind {
        // The tool stopped after writing partial work.
        PerformerCompletionKind::ErrorWithChanges => E104_PERFORMER_PARTIAL_CHANGES,
        // The tool stopped without producing anything.
        _ => E105_PERFORMER_EXIT_NON_ZERO,
    }
}

/// Build the recorded failure text for a tool-reported error.
///
/// `reason` is the generic status line ("Tool execution failed with repository
/// changes."), which says *what* happened but never *why*. The tool's own
/// `MACC_TASK_RESULT_EXP` explanation is the only account of the cause, so it is
/// appended here — and when the tool omitted it, that omission is stated
/// explicitly rather than leaving a failure with no recorded reason.
fn describe_tool_error(task: &Task, reason: &str) -> String {
    match task
        .task_runtime
        .result_explanation
        .as_deref()
        .map(str::trim)
        .filter(|exp| !exp.is_empty())
    {
        Some(explanation) => format!("{} — {}", reason, explanation),
        None => format!(
            "{} (no explanation provided: the tool omitted the required MACC_TASK_RESULT_EXP line)",
            reason
        ),
    }
}

pub(super) fn apply_state_transitions(
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
                let detail = describe_tool_error(task, reason);
                task.set_workflow_state(WorkflowState::Todo);
                preserve_active_session_chain(task);
                capture_last_assignment_before_clear(task);
                if !same_worktree {
                    task.worktree = None;
                }
                let runtime = task.ensure_runtime();
                // Tool-reported errors are re-dispatched as new runner
                // invocations. Count every re-dispatch so failures without
                // changes cannot bypass the retry budget indefinitely.
                runtime.increment_retries();
                runtime.completion_kind = Some(completion_kind.as_str().to_string());
                runtime.set_status(RuntimeStatus::Failed);
                runtime.current_phase = None;
                runtime.pid = None;
                // Record the reason in the *typed* error fields, not only in
                // `last_error`. `result_explanation` lives in the runtime's
                // serialized payload, which later writes can overwrite, so the
                // tool's stated reason for stopping was routinely lost -- a task
                // could fail with nothing but a generic "Tool execution failed"
                // on record. `last_error_message` is a first-class storage
                // column and survives.
                runtime.set_last_error_details(
                    tool_error_code(completion_kind),
                    "tool",
                    detail.clone(),
                );
                runtime.last_error = Some(detail.clone());
                task.tool = None;
                task.assignee = None;
                task.touch_state_changed(now);
                JobCompletionResult {
                    should_retry: false,
                    status_label: completion_kind.as_str(),
                    detail,
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
                store_classified_error_in_extra(runtime, tool_error.as_ref(), *now_ts);
                task.touch_state_changed(now);
                JobCompletionResult {
                    should_retry: false,
                    status_label: "failed",
                    detail: reason.clone(),
                    completion_kind: None,
                    tool_error: tool_error.as_ref().clone(),
                }
            }
            BlockOutcome::RetryBudgetExhausted {
                completion_kind,
                tool_error,
                attempts,
            } => {
                // Preserve committed work for error-with-changes so the operator
                // can recover it. A failure without changes has no worktree state
                // worth retaining.
                // Carry the tool's own explanation into the blocked record too:
                // it is the operator's only account of why the task kept failing.
                let described = describe_tool_error(task, reason);
                task.set_workflow_state(WorkflowState::Blocked);
                preserve_active_session_chain(task);
                let branch = task.branch().unwrap_or_default().to_string();
                if *completion_kind == PerformerCompletionKind::ErrorWithoutChanges {
                    task.worktree = None;
                }
                let runtime = task.ensure_runtime();
                runtime.completion_kind = Some(completion_kind.as_str().to_string());
                runtime.set_status(RuntimeStatus::Failed);
                runtime.current_phase = None;
                runtime.pid = None;
                let detail = if *completion_kind == PerformerCompletionKind::ErrorWithoutChanges {
                    format!(
                        "{} (retry budget exhausted after {} attempt(s))",
                        described, attempts
                    )
                } else if branch.is_empty() {
                    format!(
                        "{} (retry budget exhausted after {} attempt(s) in the same worktree)",
                        described, attempts
                    )
                } else {
                    format!(
                        "{} (retry budget exhausted after {} attempt(s); committed work is unmerged on branch {})",
                        described, attempts, branch
                    )
                };
                runtime.set_last_error_details("E902", "coordinator", detail.clone());
                runtime.last_error = Some(detail.clone());
                task.tool = None;
                task.assignee = None;
                task.touch_state_changed(now);
                JobCompletionResult {
                    should_retry: false,
                    status_label: "retry_budget_exhausted",
                    detail,
                    completion_kind: Some(*completion_kind),
                    tool_error: tool_error.as_ref().clone(),
                }
            }
            BlockOutcome::ToolReportedTerminal {
                completion_kind,
                error_code,
                attempts,
            } => {
                let described = describe_tool_error(task, reason);
                task.set_workflow_state(WorkflowState::Blocked);
                preserve_active_session_chain(task);
                capture_last_assignment_before_clear(task);
                if *completion_kind != PerformerCompletionKind::ErrorWithChanges {
                    task.worktree = None;
                }
                let runtime = task.ensure_runtime();
                runtime.completion_kind = Some(completion_kind.as_str().to_string());
                runtime.set_status(RuntimeStatus::Failed);
                runtime.current_phase = None;
                runtime.pid = None;
                let suffix = match completion_kind {
                    PerformerCompletionKind::PreconditionUnmet => {
                        "correct terminal stop; precondition is not met".to_string()
                    }
                    _ if *error_code == "E907" => format!(
                        "identical explanation repeated on {} consecutive attempts",
                        attempts
                    ),
                    _ => "explained failure without repository changes; not retried".to_string(),
                };
                let detail = format!("{} ({})", described, suffix);
                runtime.set_last_error_details(*error_code, "coordinator", detail.clone());
                runtime.last_error = Some(detail.clone());
                task.tool = None;
                task.assignee = None;
                task.touch_state_changed(now);
                JobCompletionResult {
                    should_retry: false,
                    status_label: completion_kind.as_str(),
                    detail,
                    completion_kind: Some(*completion_kind),
                    tool_error: None,
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
