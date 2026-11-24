//! Retry a retained phase using the same review budget as the control plane.
use super::{apply_storage_mode_args, parse_retry_phase_args};
use crate::config::CoordinatorConfig;
use crate::coordinator::{
    control_plane::CoordinatorLog, engine as coordinator_engine, types::CoordinatorEnvConfig,
    WorkflowState,
};
use crate::{MaccError, ProjectPaths, Result};
use std::collections::BTreeMap;

pub fn coordinator_retry_phase<E: crate::engine::Engine + ?Sized>(
    engine: &E,
    paths: &ProjectPaths,
    _canonical: &crate::config::CanonicalConfig,
    coordinator_cfg: Option<&CoordinatorConfig>,
    env_cfg: &CoordinatorEnvConfig,
    args: &[String],
    logger: Option<&dyn CoordinatorLog>,
) -> Result<()> {
    let (task_id, phase, skip) = parse_retry_phase_args(args)?;
    let _phase_guard = if matches!(phase.as_str(), "review" | "fix" | "dev") {
        Some(crate::coordinator::control_plane::acquire_phase_driver(
            &paths.root,
        )?)
    } else {
        None
    };
    let mut state_args = BTreeMap::new();
    apply_storage_mode_args(&mut state_args, env_cfg, coordinator_cfg);
    let mut snapshot = engine.coordinator_state_snapshot(&paths.root, &state_args)?;
    let Some(task) = snapshot.registry.find_task_mut(&task_id) else {
        return Err(MaccError::Validation(format!(
            "Task not found in registry: {}",
            task_id
        )));
    };

    if skip {
        return crate::coordinator::state::skip_retained_phase(paths, &state_args, task, &phase);
    }

    if phase == "dev" {
        return retry_dev_phase(
            engine,
            paths,
            coordinator_cfg,
            env_cfg,
            task,
            &state_args,
            logger,
        );
    }
    let retry_state = match phase.as_str() {
        "review" => WorkflowState::Reviewing,
        "fix" => WorkflowState::ChangesRequested,
        other => {
            return Err(MaccError::Validation(format!(
                "unsupported retry phase '{other}'"
            )))
        }
    };
    let eligible = match task.workflow_state() {
        Some(WorkflowState::Blocked) => {
            task.task_runtime.current_phase.as_deref() == Some(phase.as_str())
        }
        Some(WorkflowState::InProgress | WorkflowState::Testing | WorkflowState::Reviewing) => {
            phase == "review"
        }
        Some(WorkflowState::ChangesRequested) => phase == "fix",
        _ => false,
    };
    if !eligible {
        return Err(MaccError::Validation(format!(
            "Task {task_id} cannot retry {phase} from {}",
            task.state
        )));
    }
    let attempts = env_cfg
        .phase_runner_max_attempts
        .or_else(|| coordinator_cfg.and_then(|c| c.phase_runner_max_attempts))
        .unwrap_or(1)
        .max(1);
    let expected = task.clone();
    if phase == "review" {
        let limit = env_cfg
            .max_review_cycles
            .or_else(|| coordinator_cfg.and_then(|c| c.max_review_cycles));
        crate::coordinator::review_budget::reserve(task, limit, attempts)?;
    }
    task.task_runtime.retries = Some(
        task.task_runtime
            .retries
            .unwrap_or_default()
            .saturating_add(1),
    );
    task.set_workflow_state(retry_state);
    let assignment = task.clone();
    crate::coordinator::state::replace_phase_task(paths, &state_args, &expected, &assignment)?;
    let tool = env_cfg
        .coordinator_tool
        .as_deref()
        .or_else(|| coordinator_cfg.and_then(|cfg| cfg.coordinator_tool.as_deref()));
    let mut review_attempts = None;
    let reviewed_commit = if phase == "review" && assignment.task_runtime.retained_review.is_some()
    {
        assignment
            .worktree_path()
            .and_then(|path| crate::git::head_commit(&paths.root.join(path)).ok())
    } else {
        None
    };
    let result = if phase == "review" {
        engine
            .coordinator_run_review_phase_for_task_native(&paths.root, &assignment, tool, logger)
            .map(|execution| {
                review_attempts = Some(execution.attempts);
                execution.outcome.map(Some)
            })
    } else {
        engine
            .coordinator_run_phase_for_task_native(
                &paths.root,
                &assignment,
                "fix",
                tool,
                attempts,
                logger,
            )
            .map(|result| result.map(|_| None))
    };
    // The runner can update native decision context and event history.
    let mut snapshot = engine.coordinator_state_snapshot(&paths.root, &state_args)?;
    let task = snapshot
        .registry
        .find_task_mut(&task_id)
        .ok_or_else(|| MaccError::Validation(format!("Task {task_id} missing after retry")))?;
    if task.workflow_state() != Some(retry_state) || !assignment.same_phase_assignment(task) {
        return Err(MaccError::Validation(format!(
            "Task {task_id} changed state during {phase} retry; outcome discarded"
        )));
    }
    let expected = task.clone();
    if let Some(count) = review_attempts {
        task.task_runtime.settle_review_attempts(count);
    }
    let now = crate::coordinator::helpers::now_iso_coordinator();
    let outcome = match result.unwrap_or_else(|error| Err(error.to_string())) {
        Ok(Some(verdict)) => {
            if let Some(review) = &mut task.task_runtime.retained_review {
                review.approved_commit = if verdict == coordinator_engine::ReviewVerdict::Ok {
                    reviewed_commit
                } else {
                    None
                };
            }
            coordinator_engine::apply_review_phase_success_typed(task, verdict, &now).map(|_| ())
        }
        Ok(None) => coordinator_engine::apply_phase_success_typed(
            task,
            coordinator_engine::PhaseTransition {
                mode: "fix",
                next_state: WorkflowState::InProgress,
                runtime_phase: "fix",
            },
            &now,
        ),
        Err(reason) => {
            coordinator_engine::apply_phase_failure_typed(
                task,
                if phase == "review" { "review" } else { "fix" },
                &reason,
                &now,
            )?;
            Err(MaccError::Validation(reason))
        }
    };
    crate::coordinator::state::replace_phase_task(paths, &state_args, &expected, task)?;
    outcome
}

#[path = "coordinator_dev_retry.rs"]
mod dev;
use dev::retry_dev_phase;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::coordinator::state::{
        coordinator_state_registry_save, coordinator_state_snapshot, skip_retained_phase,
    };
    use serde_json::json;

    #[test]
    fn skip_preserves_delivered_tasks_and_rejects_obsolete_assignments() {
        for mode in ["sqlite", "json", "dual-write"] {
            let tmp = tempfile::tempdir().unwrap();
            let paths = ProjectPaths::from_root(tmp.path());
            let args = BTreeMap::from([("storage-mode".into(), mode.into())]);
            let env = CoordinatorEnvConfig {
                storage_mode: Some(mode.into()),
                ..Default::default()
            };
            let engine = crate::engine::MaccEngine::new(crate::tool::ToolRegistry::new());
            let skip = |id, phase| {
                coordinator_retry_phase(
                    &engine,
                    &paths,
                    &Default::default(),
                    None,
                    &env,
                    &["--retry-task", id, "--retry-phase", phase, "--skip"].map(String::from),
                    None,
                )
            };
            let snapshot = || {
                coordinator_state_snapshot(tmp.path(), &args)
                    .unwrap()
                    .registry
            };
            let mut fixture = json!({"tasks":[{"id":"A","state":"blocked","worktree":{"worktree_path":"worker","branch":"task"},
                "task_runtime":{"current_phase":"review","status":"failed","claim_id":"first"}},
                {"id":"B","state":"merged","worktree":{"worktree_path":"delivered"},"task_runtime":{"current_phase":"review"}},
                {"id":"C","state":"blocked","worktree":{"worktree_path":"another"},"task_runtime":{"current_phase":"dev","status":"failed"}}]});
            coordinator_state_registry_save(tmp.path(), &args, &fixture).unwrap();
            let original = snapshot();
            assert!(skip("B", "review").is_err());
            assert!(skip("A", "fix").is_err());
            assert_eq!(snapshot().tasks, original.tasks);

            let expected = original.find_task("A").unwrap();
            fixture["tasks"][0]["task_runtime"]["claim_id"] = json!("second");
            coordinator_state_registry_save(tmp.path(), &args, &fixture).unwrap();
            assert!(skip_retained_phase(&paths, &args, expected, "review").is_err());

            skip("C", "dev").unwrap();
            assert_eq!(snapshot().find_task("C").unwrap().state, "todo");
            let current = snapshot();
            let expected = current.find_task("A").unwrap();
            let barrier = std::sync::Barrier::new(2);
            let client = || {
                barrier.wait();
                skip_retained_phase(&paths, &args, expected, "review").is_ok()
            };
            std::thread::scope(|scope| {
                let one = scope.spawn(client);
                assert_ne!(client(), one.join().unwrap());
            });
            let final_state = snapshot();
            let retained = final_state.find_task("A").unwrap();
            assert_eq!(retained.state, "todo");
            assert_eq!(retained.worktree, expected.worktree);
            assert_eq!(retained.task_runtime.current_phase, None);
            assert_eq!(final_state.find_task("B"), original.find_task("B"));
            assert!(skip("A", "review").is_err());
        }
    }
}

#[cfg(test)]
#[path = "coordinator_phase_retry_concurrency_tests.rs"]
mod concurrency_tests;
