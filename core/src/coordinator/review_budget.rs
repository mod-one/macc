//! Per-task accounting shared by automatic phases and operator retries.
use super::model::{Task, TaskRuntime};
use super::{RuntimeStatus, WorkflowState};
use crate::{MaccError, Result};
use serde::{Deserialize, Serialize};

/// Actual reviewer dispatches and the result of the read-only phase checks.
#[derive(Debug, PartialEq, Eq)]
pub struct ReviewExecution {
    pub attempts: usize,
    pub outcome: std::result::Result<super::engine::ReviewVerdict, String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
enum ReviewState {
    AwaitingVerdict,
    Approved,
    ChangesRequested,
    Corrected,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
struct Corrections {
    requested: usize,
    completed: usize,
}

impl std::fmt::Display for Corrections {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        if self.requested == 0 {
            f.write_str("no corrections requested")
        } else {
            write!(
                f,
                "{}/{} corrections completed",
                self.completed, self.requested
            )
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReviewUsage {
    started: usize,
    reserved: usize,
    limit: Option<usize>,
    /// None when an older task already had reviews but no correction history.
    corrections: Option<Corrections>,
    state: ReviewState,
}

impl ReviewUsage {
    pub fn summary(&self) -> String {
        let total = self.limit.map(|n| format!("/{n}")).unwrap_or_default();
        let mut message = if self.reserved == 0 {
            format!("{}{total} reviews used", self.started)
        } else {
            format!(
                "{}{total} reviews used · reserved slots: {}",
                self.started, self.reserved
            )
        };
        if let Some(corrections) = &self.corrections {
            message.push_str(&format!(" · {corrections}"));
        }
        message.push_str(match self.state {
            ReviewState::AwaitingVerdict => " · no verdict recorded for this attempt",
            ReviewState::Approved => " · review approved",
            ReviewState::ChangesRequested => " · changes requested",
            ReviewState::Corrected => " · no review verdict for the corrected revision",
        });
        message
    }
}

impl TaskRuntime {
    pub(crate) fn reviews_used(&self) -> usize {
        self.review_cycles.unwrap_or_default().max(
            self.review_usage
                .as_ref()
                .map_or(0, |usage| usage.started.saturating_add(usage.reserved)),
        )
    }

    fn review_usage_mut(&mut self) -> &mut ReviewUsage {
        let completed = self.review_cycles.unwrap_or_default();
        self.review_usage.get_or_insert_with(|| ReviewUsage {
            started: completed,
            reserved: 0,
            limit: None,
            corrections: (completed == 0).then(Corrections::default),
            state: ReviewState::AwaitingVerdict,
        })
    }

    pub(crate) fn reserved_review_attempts(&self) -> usize {
        self.review_usage.as_ref().map_or(0, |u| u.reserved)
    }

    pub(crate) fn settle_review_attempts(&mut self, attempts: usize) {
        if let Some(usage) = self.review_usage.as_mut() {
            usage.started = usage.started.saturating_add(attempts);
            usage.reserved = 0;
        }
    }

    pub(crate) fn review_needs_verdict(&self) -> bool {
        self.review_usage
            .as_ref()
            .is_some_and(|u| u.state == ReviewState::AwaitingVerdict)
    }

    pub(crate) fn record_review_verdict(&mut self, approved: bool) {
        let completed = self.review_cycles.unwrap_or_default().saturating_add(1);
        let usage = self.review_usage_mut();
        usage.started = usage.started.max(completed);
        if !approved {
            if let Some(corrections) = &mut usage.corrections {
                corrections.requested = corrections.requested.saturating_add(1);
            }
        }
        usage.state = if approved {
            ReviewState::Approved
        } else {
            ReviewState::ChangesRequested
        };
        self.review_cycles = Some(completed);
    }

    pub(crate) fn record_review_correction(&mut self) {
        if self.review_usage.is_none() && self.review_cycles.unwrap_or_default() == 0 {
            return; // A merge repair can enter the fix phase without any reviewer.
        }
        let usage = self.review_usage_mut();
        if usage.state == ReviewState::ChangesRequested {
            if let Some(corrections) = &mut usage.corrections {
                corrections.completed = corrections.completed.saturating_add(1);
            }
        }
        usage.state = ReviewState::Corrected;
    }
}

/// Check a request without consuming a slot. Dispatch checks again at reservation.
pub fn ensure_review_available(task: &Task, limit: Option<usize>) -> Result<()> {
    if let Some(max) = limit.filter(|max| task.task_runtime.reviews_used() >= *max) {
        return Err(MaccError::Validation(exhausted_reason(task, max)));
    }
    Ok(())
}

fn exhausted_reason(task: &Task, limit: usize) -> String {
    let used = task.task_runtime.reviews_used();
    let corrections = task
        .task_runtime
        .review_usage
        .as_ref()
        .and_then(|u| u.corrections.as_ref())
        .map(|corrections| format!("; {corrections}"))
        .unwrap_or_default();
    let reserved = task.task_runtime.reserved_review_attempts();
    let uncertainty = if reserved == 0 {
        String::new()
    } else {
        format!("; {reserved} reserved before interruption")
    };
    let reason = if task.workflow_state() == Some(WorkflowState::ChangesRequested) {
        "the requested correction is outstanding"
    } else if task
        .task_runtime
        .review_usage
        .as_ref()
        .is_some_and(|u| u.state == ReviewState::Corrected)
    {
        "the corrected revision has no review verdict and publication requires one"
    } else {
        "a current independent review is required for publication"
    };
    format!("Review limit reached ({used}/{limit} review slots used{uncertainty}{corrections}): {reason}. Work retained. Increase max_review_cycles to resume.")
}

/// The caller persists this reservation before starting the reviewer. An interrupted
/// attempt still occupies its slot and cannot stand in for an accepted verdict.
pub(crate) fn reserve(task: &mut Task, limit: Option<usize>, runner_attempts: usize) -> Result<()> {
    ensure_review_available(task, limit)?;
    let used = task.task_runtime.reviews_used();
    let usage = task.task_runtime.review_usage_mut();
    // A previous driver may have exited before reporting which slots it used.
    usage.started = used;
    usage.reserved = runner_attempts
        .max(1)
        .min(limit.unwrap_or(usize::MAX).saturating_sub(used));
    usage.limit = limit;
    usage.state = ReviewState::AwaitingVerdict;
    if matches!(
        task.workflow_state(),
        Some(WorkflowState::PrOpen | WorkflowState::Queued)
    ) {
        task.set_workflow_state(WorkflowState::Reviewing);
    }
    Ok(())
}

pub(crate) fn block(task: &mut Task, limit: usize, now: &str) -> String {
    if let Some(usage) = task.task_runtime.review_usage.as_mut() {
        usage.limit = Some(limit);
    }
    let phase = if task.workflow_state() == Some(WorkflowState::ChangesRequested) {
        "fix"
    } else {
        "review"
    };
    let reason = exhausted_reason(task, limit);
    task.set_workflow_state(WorkflowState::Blocked);
    task.task_runtime.current_phase = Some(phase.into());
    task.task_runtime.set_status(RuntimeStatus::Idle);
    task.task_runtime.last_error = Some(reason.clone());
    task.task_runtime
        .set_last_error_details("E901", "review_budget", &reason);
    task.touch_state_changed(now);
    reason
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::coordinator::engine::{build_advance_actions, AdvanceTaskAction};
    use serde_json::json;

    #[test]
    fn restart_or_governed_publication_cannot_replace_a_missing_verdict() {
        let mut phases = crate::config::PhasesConfig::default();
        phases.review.enabled = false;
        for state in ["testing", "pr_open", "queued"] {
            let mut task: Task = serde_json::from_value(json!({
                "id":"A","state":state,"worktree":{"branch":"task"},
                "task_runtime":{"review_cycles":1,"decision_has_bindings":true,"decision_context":{"phase":"fix"}}
            })).unwrap();
            let plan = |task: &Task, limit| {
                build_advance_actions(
                    &json!({"tasks":[task]}),
                    &Default::default(),
                    "",
                    Some(limit),
                    &phases,
                    true,
                )
                .unwrap()
            };
            assert!(matches!(
                plan(&task, 1).as_slice(),
                [AdvanceTaskAction::BlockReviewBudget { .. }]
            ));
            assert!(matches!(
                plan(&task, 2).as_slice(),
                [AdvanceTaskAction::RunPhase { mode: "review", .. }]
            ));
            reserve(&mut task, Some(2), 1).unwrap();
            let mut restored: Task =
                serde_json::from_value(serde_json::to_value(task).unwrap()).unwrap();
            assert_eq!(restored.task_runtime.review_cycles, Some(1));
            assert!(reserve(&mut restored, Some(2), 1).is_err());
            assert!(matches!(
                plan(&restored, 2).as_slice(),
                [AdvanceTaskAction::BlockReviewBudget { .. }]
            ));
            let msg = restored.task_runtime.review_usage.unwrap().summary();
            assert!(
                msg.contains("1/2 reviews used · reserved slots: 1")
                    && msg.contains("no verdict recorded")
            );
            assert!(
                !msg.contains("corrections"),
                "older data has no correction count"
            );
        }
    }

    #[test]
    fn only_requested_corrections_contribute_to_the_displayed_total() {
        let mut task = Task::default();
        task.task_runtime.record_review_correction();
        assert!(task.task_runtime.review_usage.is_none());
        reserve(&mut task, Some(2), 1).unwrap();
        task.task_runtime.settle_review_attempts(1);
        task.task_runtime.record_review_verdict(false);
        task.task_runtime.record_review_correction();
        task.task_runtime.record_review_correction();
        reserve(&mut task, Some(2), 1).unwrap();
        task.task_runtime.settle_review_attempts(1);
        task.task_runtime.record_review_verdict(true);
        assert_eq!(
            task.task_runtime.review_usage.unwrap().summary(),
            "2/2 reviews used · 1/1 corrections completed · review approved"
        );
    }
}
