use super::completion::ErrorClassification;
use super::fsm::{
    should_auto_retry_error_code, BlockOutcome, CompletionErrorDetails, CoordinatorConfigResolved,
    JobCompletionInput, RetryOutcome,
};
use crate::coordinator::error_normalizer::{CanonicalClass, ToolError};
use crate::coordinator::model::Task;
use crate::coordinator::rate_limit::{
    compute_backoff_delay, E601_RATE_LIMITED, E602_QUOTA_EXHAUSTED,
};
use crate::coordinator::PerformerCompletionKind;

#[allow(dead_code)]
#[derive(Debug, Clone)]
pub(super) enum RetryStrategy {
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

pub(super) fn resolve_retry_strategy(
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
