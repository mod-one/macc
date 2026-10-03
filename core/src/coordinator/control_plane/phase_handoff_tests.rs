use super::*;
use crate::coordinator::control_plane::phase_handoff::handle_phase_tool_unavailability;
use crate::coordinator::control_plane::phase_runner::phase_tool_for_task;
use crate::coordinator::engine::{apply_phase_outcome_in_registry, PhaseTransition, ReviewVerdict};
use crate::coordinator::WorkflowState;

#[test]
fn interrupted_review_and_fix_preserve_work_and_resume_with_the_actual_phase_tool() {
    for mode in ["review", "fix"] {
        for committed in [false, true] {
            for fallback_available in [false, true] {
                let fixture = Fixture::new(committed);
                let before = fixture.snapshot();
                let mut registry = fixture.registry("tool-b");
                registry["tasks"][0]["state"] = json!(if mode == "fix" {
                    "changes_requested"
                } else {
                    "in_progress"
                });
                registry["tasks"][0]["task_runtime"]["status"] = json!("phase_done");
                let snapshot = TaskRegistry::from_value(&registry)
                    .unwrap()
                    .find_task("HANDOFF")
                    .unwrap()
                    .clone();
                let mut state = CoordinatorRunState::new();
                let now = "2026-10-03T20:00:00Z";
                if !fallback_available {
                    state.throttle_registry.insert(
                        "tool-b".into(),
                        ToolThrottleState {
                            tool_id: "tool-b".into(),
                            throttled_until: 1791059400,
                            ..Default::default()
                        },
                    );
                }
                // Coordinator tool differs from the original task performer.
                handle_phase_tool_unavailability(
                    fixture.root.path(),
                    &mut registry,
                    &mut state,
                    &snapshot,
                    "HANDOFF",
                    mode,
                    "E602 quota exhausted",
                    Some("tool-a"),
                    now,
                    &["tool-a".into(), "tool-b".into()],
                    None,
                )
                .unwrap();
                assert_eq!(fixture.snapshot(), before);
                assert!(state.throttle_registry.contains_key("tool-a"));
                assert_eq!(registry["tasks"][0]["worktree"], json!(snapshot.worktree));
                let task = TaskRegistry::from_value(&registry)
                    .unwrap()
                    .find_task("HANDOFF")
                    .unwrap()
                    .clone();
                assert_eq!(phase_tool_for_task(&task, Some("tool-a")), Some("tool-b"));
                if fallback_available {
                    assert!(task.task_runtime.delayed_until.is_none());
                } else {
                    assert_eq!(
                        task.task_runtime.delayed_until.as_deref(),
                        Some("2026-10-03T20:30:00Z")
                    );
                }
                // A restart must not treat the retained phase as a dead claim.
                crate::coordinator::state::coordinator_state_registry_save(
                    fixture.root.path(),
                    &BTreeMap::new(),
                    &registry,
                )
                .unwrap();
                let recovery = crate::coordinator::state_runtime::execute_startup_recovery_sweep(
                    fixture.root.path(),
                    "main",
                    false,
                    None,
                )
                .unwrap();
                assert!(recovery.is_empty(), "{recovery:?}");
                let saved = crate::coordinator::state::coordinator_state_registry_load(
                    fixture.root.path(),
                    &BTreeMap::new(),
                )
                .unwrap();
                assert_eq!(
                    saved["tasks"][0]["worktree"],
                    registry["tasks"][0]["worktree"]
                );
                assert_eq!(fixture.snapshot(), before);
                let prompt = crate::coordinator::runtime::build_phase_prompt(
                    mode, "HANDOFF", "tool-b", &task,
                )
                .unwrap();
                if mode == "fix" {
                    assert!(prompt.contains("Retained work may come from an interrupted performer"));
                }
                // Success closes the interrupted-phase marker; the normal FSM resumes.
                apply_phase_outcome_in_registry(
                    &mut registry,
                    "HANDOFF",
                    mode,
                    PhaseTransition {
                        mode,
                        next_state: WorkflowState::Testing,
                        runtime_phase: "test",
                    },
                    if mode == "review" {
                        Some(ReviewVerdict::Ok)
                    } else {
                        None
                    },
                    None,
                    now,
                )
                .unwrap();
                assert!(registry["tasks"][0]["task_runtime"]["phase_tool_unavailable"].is_null());
                assert_eq!(fixture.snapshot(), before);
            }
        }
    }
}
