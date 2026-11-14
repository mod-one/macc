use crate::coordinator::engine::{
    apply_phase_outcome_in_registry, build_advance_actions, AdvanceTaskAction, ReviewVerdict,
};
use crate::coordinator::task_selector::reconcile_task_blocks;
use serde_json::json;
#[test]
fn interrupted_fix_uses_native_phase_pin_then_requires_fresh_tests_and_review() {
    let root = tempfile::tempdir().unwrap();
    let mut publishing:super::super::model::Task=serde_json::from_value(json!({"id":"PUBLISH","state":"blocked","task_runtime":{"current_phase":"merge","decision_wait":"device","last_error_origin":"decisions","decision_context":{"phase":"fix"}}})).unwrap();
    super::resume_state(root.path(), &mut publishing);
    assert_eq!(
        publishing.task_runtime.current_phase.as_deref(),
        Some("merge"),
        "an old AI pin cannot replace a later publication phase"
    );
    for testing in [false, true] {
        let mut task:super::super::model::Task=serde_json::from_value(json!({"id":"A","state":"blocked","task_runtime":{"current_phase":"review","decision_wait":"device","last_error_origin":"decisions","decision_context":{"phase":"fix"}}})).unwrap();
        let root = tempfile::tempdir().unwrap();
        assert_eq!(
            super::resume_state(root.path(), &mut task),
            super::WorkflowState::ChangesRequested
        );
        assert_eq!(task.task_runtime.current_phase.as_deref(), Some("fix"));
        task.set_workflow_state(super::WorkflowState::ChangesRequested);
        let mut registry = json!({"tasks":[task]});
        let mut phases = crate::config::PhasesConfig::default();
        phases.testing.enabled = testing;
        phases.review.enabled = false;
        for expected in if testing {
            vec!["fix", "test", "review"]
        } else {
            vec!["fix", "review"]
        } {
            let actions = build_advance_actions(
                &registry,
                &Default::default(),
                "2026-11-14T21:10:00Z",
                Some(0),
                &phases,
            )
            .unwrap();
            let [AdvanceTaskAction::RunPhase {
                mode, transition, ..
            }] = actions.as_slice()
            else {
                panic!("{actions:?}")
            };
            assert_eq!(*mode, expected);
            apply_phase_outcome_in_registry(
                &mut registry,
                "A",
                expected,
                *transition,
                if expected == "review" {
                    Some(ReviewVerdict::Ok)
                } else {
                    None
                },
                None,
                "2026-11-14T21:10:00Z",
            )
            .unwrap();
        }
        assert_ne!(registry["tasks"][0]["state"], "merged");
    }
}
#[test]
fn refreshed_review_is_mandatory_and_does_not_consume_an_extra_correction_cycle() {
    for limit in [0, 1] {
        let mut registry = json!({"tasks":[{"id":"A","state":"testing","worktree":{"branch":"ai/future/A","worktree_path":"/tmp/retained"},"task_runtime":{"current_phase":"merge","review_cycles":1,"review_refresh_required":"DM_STALE: reviewed source changed"}}]});
        registry["tasks"][0]["task_runtime"]["review_cycles"] = json!(limit);
        let mut phases = crate::config::PhasesConfig::default();
        phases.review.enabled = false;
        let actions = build_advance_actions(
            &registry,
            &Default::default(),
            "2026-11-09T12:00:00Z",
            Some(limit),
            &phases,
        )
        .unwrap();
        let [AdvanceTaskAction::RunPhase {
            mode, transition, ..
        }] = actions.as_slice()
        else {
            panic!("{actions:?}")
        };
        assert_eq!(*mode, "review");
        apply_phase_outcome_in_registry(
            &mut registry,
            "A",
            "review",
            *transition,
            Some(ReviewVerdict::ChangesRequested),
            None,
            "2026-11-09T12:01:00Z",
        )
        .unwrap();
        assert_eq!(registry["tasks"][0]["state"], "changes_requested");
        assert_eq!(registry["tasks"][0]["task_runtime"]["review_cycles"], limit);
        assert!(registry["tasks"][0]["task_runtime"]
            .get("review_refresh_required")
            .is_none());
        let actions = build_advance_actions(
            &registry,
            &Default::default(),
            "2026-11-09T12:02:00Z",
            Some(limit),
            &phases,
        )
        .unwrap();
        assert!(matches!(
            actions.as_slice(),
            [AdvanceTaskAction::RunPhase { mode: "fix", .. }]
        ));
    }
}
#[test]
fn dependency_diagnostics_refresh_the_native_root_code_without_repeated_state_changes() {
    let mut reg=super::super::model::TaskRegistry::from_value(&json!({"tasks":[{"id":"ROOT","state":"blocked","task_runtime":{"last_error":"Old native context failure"}},{"id":"CHILD","state":"todo","dependencies":["ROOT"]}]})).unwrap();
    reconcile_task_blocks(&mut reg, "2026-11-09T12:00:00Z");
    reg.find_task_mut("ROOT")
        .unwrap()
        .task_runtime
        .set_last_error_details(
            "DM_CONTEXT",
            "decisions",
            "DM_STALE: exact reviewed commit changed",
        );
    assert!(reconcile_task_blocks(&mut reg, "2026-11-09T12:01:00Z").contains(&"CHILD".into()));
    let message = reg
        .find_task("CHILD")
        .unwrap()
        .task_runtime
        .last_error_message
        .as_deref()
        .unwrap();
    assert!(message.contains("DM_CONTEXT"));
    assert!(!message.contains("unknown error"));
    assert!(reconcile_task_blocks(&mut reg, "2026-11-09T12:02:00Z").is_empty());
}
