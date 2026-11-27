//! Plan validation, review/correction cycles and delivery without skipping a requested fix.
use super::fsm::{AdvancePlan, PhaseTransition};
use crate::coordinator::WorkflowState;

/// Resolve every source of a review request before testing the shared limit.
pub(crate) fn plan_task(
    task: &crate::coordinator::model::Task,
    limit: Option<usize>,
    phases: &crate::config::PhasesConfig,
    decisions_enabled: bool,
) -> AdvancePlan {
    let Some(state) = task.workflow_state() else {
        return AdvancePlan::Noop;
    };
    let runtime = &task.task_runtime;
    let used = runtime.reviews_used();
    let ordinary = plan_advance(state, limit, used, phases);
    let needs_review = runtime
        .retained_review
        .as_ref()
        .is_some_and(|review| review.approved_commit.is_none())
        || runtime.review_needs_verdict()
        || runtime.extra.contains_key("review_refresh_required")
        || runtime.extra.contains_key("compliance_review_required")
        || (decisions_enabled
            && ["decision_has_bindings", "decision_merge_repair"]
                .iter()
                .any(|k| runtime.extra.get(*k).and_then(serde_json::Value::as_bool) == Some(true))
            && runtime
                .extra
                .get("decision_context")
                .and_then(|v| v["phase"].as_str())
                != Some("review"));
    if matches!(ordinary, AdvancePlan::Merge) && needs_review {
        if limit.is_some_and(|max| used >= max) {
            return AdvancePlan::MergeAfterReviewLimit;
        }
        return AdvancePlan::RunPhase(PhaseTransition {
            mode: "review",
            next_state: WorkflowState::Reviewing,
            runtime_phase: "review",
        });
    }
    ordinary
}

/// Plan the next advance action for a task.
///
/// `max_review_cycles` controls the review→fix loop budget:
///   - `None`  → unlimited loops (default)
///   - `Some(0)` → skip optional review; required checks are handled by `plan_task`
///   - `Some(1)` → one review + one fix if changes requested, no loopback
///   - `Some(n)` → up to n reviews, each including its requested correction
///
/// `reviews_used` counts review slots used, including interrupted attempts.
pub fn plan_advance(
    state: WorkflowState,
    max_review_cycles: Option<usize>,
    reviews_used: usize,
    phases: &crate::config::PhasesConfig,
) -> AdvancePlan {
    let review_available =
        phases.review.enabled && max_review_cycles.is_none_or(|max| reviews_used < max);
    match state {
        WorkflowState::InProgress => {
            if phases.testing.enabled {
                AdvancePlan::RunPhase(PhaseTransition {
                    mode: "test",
                    next_state: WorkflowState::Testing,
                    runtime_phase: "test",
                })
            } else if review_available {
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
            if review_available {
                AdvancePlan::RunPhase(PhaseTransition {
                    mode: "review",
                    next_state: WorkflowState::Reviewing,
                    runtime_phase: "review",
                })
            } else {
                AdvancePlan::Merge
            }
        }
        WorkflowState::Reviewing => AdvancePlan::Merge,
        WorkflowState::PrOpen => AdvancePlan::Merge,
        WorkflowState::ChangesRequested => {
            // A completed review owns its requested correction, including at the limit.
            // Inconsistent/existing rejected states must never bypass correction by merging.
            if max_review_cycles.is_some_and(|max| max == 0 || reviews_used > max) {
                return AdvancePlan::ReviewBudgetExhausted;
            }
            AdvancePlan::RunPhase(PhaseTransition {
                mode: "fix",
                // Re-enter validation so enabled tests run on the corrected sources.
                // The next review is scheduled only while another cycle is available.
                next_state: WorkflowState::InProgress,
                runtime_phase: "fix",
            })
        }
        WorkflowState::Queued => AdvancePlan::Merge,
        _ => AdvancePlan::Noop,
    }
}

#[cfg(test)]
mod tests {
    use super::super::fsm::{
        apply_phase_outcome_in_registry, build_advance_actions, AdvanceTaskAction, ReviewVerdict,
    };
    use serde_json::json;
    use std::collections::HashSet;
    const NOW: &str = "2026-10-15T16:00:00Z";
    fn run_phase(
        registry: &mut serde_json::Value,
        phases: &crate::config::PhasesConfig,
        limit: Option<usize>,
        mode: &str,
        verdict: Option<ReviewVerdict>,
    ) {
        let actions =
            build_advance_actions(registry, &HashSet::new(), NOW, limit, phases, false).unwrap();
        let [AdvanceTaskAction::RunPhase {
            transition,
            mode: actual,
            ..
        }] = actions.as_slice()
        else {
            panic!("expected {mode}, got {actions:?}")
        };
        assert_eq!(*actual, mode);
        apply_phase_outcome_in_registry(
            registry,
            "TASK-001",
            transition.mode,
            *transition,
            verdict,
            None,
            NOW,
        )
        .unwrap();
        assert_ne!(
            registry["tasks"][0]["state"], "blocked",
            "after {mode}: {registry}"
        );
    }
    #[test]
    fn one_review_includes_its_correction_then_enabled_tests_and_merge() {
        for testing in [false, true] {
            let mut phases = crate::config::PhasesConfig::default();
            phases.review.enabled = true;
            phases.testing.enabled = testing;
            let mut registry = json!({"tasks":[{"id":"TASK-001","state":"in_progress","worktree":{"branch":"task-branch","worktree_path":"/tmp/retained"}}]});
            if testing {
                run_phase(&mut registry, &phases, Some(1), "test", None);
            }
            run_phase(
                &mut registry,
                &phases,
                Some(1),
                "review",
                Some(ReviewVerdict::ChangesRequested),
            );
            assert_eq!(registry["tasks"][0]["state"], "changes_requested");
            assert_eq!(registry["tasks"][0]["task_runtime"]["review_cycles"], 1);
            run_phase(&mut registry, &phases, Some(1), "fix", None);
            if testing {
                run_phase(&mut registry, &phases, Some(1), "test", None);
            }
            let actions =
                build_advance_actions(&registry, &HashSet::new(), NOW, Some(1), &phases, false)
                    .unwrap();
            assert!(
                matches!(actions.as_slice(), [AdvanceTaskAction::QueueMerge { .. }]),
                "{actions:?}"
            );
            assert_ne!(registry["tasks"][0]["state"], "merged");
            assert_eq!(registry["tasks"][0]["worktree"]["branch"], "task-branch");
        }
    }
    #[test]
    fn failed_last_correction_is_blocked_with_its_worktree_preserved() {
        let mut phases = crate::config::PhasesConfig::default();
        phases.review.enabled = true;
        phases.testing.enabled = false;
        let mut registry = json!({"tasks":[{"id":"TASK-001","state":"changes_requested","worktree":{"branch":"retained"},"task_runtime":{"review_cycles":1}}]});
        let actions =
            build_advance_actions(&registry, &HashSet::new(), NOW, Some(1), &phases, false)
                .unwrap();
        let [AdvanceTaskAction::RunPhase { transition, .. }] = actions.as_slice() else {
            panic!("{actions:?}")
        };
        apply_phase_outcome_in_registry(
            &mut registry,
            "TASK-001",
            "fix",
            *transition,
            None,
            Some("correction failed"),
            NOW,
        )
        .unwrap();
        assert_eq!(registry["tasks"][0]["state"], "blocked");
        assert_eq!(registry["tasks"][0]["worktree"]["branch"], "retained");
        assert!(
            build_advance_actions(&registry, &HashSet::new(), NOW, Some(1), &phases, false)
                .unwrap()
                .is_empty()
        );
    }
    #[test]
    fn two_cycles_allow_two_corrections_without_merging_the_rejection() {
        let mut phases = crate::config::PhasesConfig::default();
        phases.review.enabled = true;
        phases.testing.enabled = false;
        let mut registry = json!({"tasks":[{"id":"TASK-001","state":"in_progress","worktree":{"branch":"task-branch"}}]});
        for cycle in 1..=2 {
            run_phase(
                &mut registry,
                &phases,
                Some(2),
                "review",
                Some(ReviewVerdict::ChangesRequested),
            );
            assert_eq!(registry["tasks"][0]["task_runtime"]["review_cycles"], cycle);
            run_phase(&mut registry, &phases, Some(2), "fix", None);
        }
        assert!(matches!(
            build_advance_actions(&registry, &HashSet::new(), NOW, Some(2), &phases, false)
                .unwrap()
                .as_slice(),
            [AdvanceTaskAction::QueueMerge { .. }]
        ));
    }
    #[test]
    fn approved_review_is_only_ready_for_merge_and_budget_overruns_block() {
        let mut phases = crate::config::PhasesConfig::default();
        phases.review.enabled = true;
        phases.testing.enabled = false;
        let mut registry = json!({"tasks":[{"id":"TASK-001","state":"in_progress","worktree":{"branch":"task-branch"}}]});
        run_phase(
            &mut registry,
            &phases,
            Some(1),
            "review",
            Some(ReviewVerdict::Ok),
        );
        assert_eq!(registry["tasks"][0]["state"], "pr_open");
        assert!(matches!(
            build_advance_actions(&registry, &HashSet::new(), NOW, Some(1), &phases, false)
                .unwrap()
                .as_slice(),
            [AdvanceTaskAction::QueueMerge { .. }]
        ));
        registry["tasks"][0]["state"] = json!("changes_requested");
        registry["tasks"][0]["task_runtime"]["review_cycles"] = json!(2);
        assert!(matches!(
            build_advance_actions(&registry, &HashSet::new(), NOW, Some(1), &phases, false)
                .unwrap()
                .as_slice(),
            [AdvanceTaskAction::BlockReviewBudget { .. }]
        ));
    }
}
