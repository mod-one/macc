//! Retry a retained phase using the same review budget as the control plane.
use super::{apply_storage_mode_args, parse_retry_phase_args};
use crate::config::CoordinatorConfig;
use crate::coordinator::{
    control_plane::CoordinatorLog, engine as coordinator_engine, types::CoordinatorEnvConfig,
    WorkflowState,
};
use crate::{MaccError, ProjectPaths, Result};
use std::collections::BTreeMap;

fn review_worktree(
    paths: &ProjectPaths,
    task: &crate::coordinator::model::Task,
) -> Result<std::path::PathBuf> {
    if task
        .runtime_pid()
        .is_some_and(crate::coordinator::helpers::is_pid_running)
    {
        return Err(MaccError::Validation(format!(
            "task {} still has an active worker",
            task.id
        )));
    }
    let path = task
        .worktree_path()
        .ok_or_else(|| MaccError::Validation("missing retained worktree path".into()))?;
    let path = paths.root.join(path);
    let path = path.canonicalize().map_err(|source| MaccError::Io {
        path: path.display().to_string(),
        action: "inspect retained review worktree".into(),
        source,
    })?;
    let branch = crate::git::current_branch_name(&path)?;
    if task.branch().is_some_and(|expected| expected != branch)
        || !crate::git::worktrees_for_branch(&paths.root, &branch)?
            .iter()
            .any(|p| p.canonicalize().is_ok_and(|p| p == path))
    {
        return Err(MaccError::Validation(format!(
            "{} does not match the retained Git assignment for {}",
            path.display(),
            task.id
        )));
    }
    crate::coordinator::runtime::review_candidate_head(task, &path)?;
    Ok(path)
}

fn review_not_started(
    paths: &ProjectPaths,
    task_id: &str,
    reason: impl std::fmt::Display,
) -> MaccError {
    let message = format!("Review retry for {task_id} did not start: {reason}. Original diagnostic and retry budget preserved.");
    if let Err(error) = crate::coordinator::helpers::append_coordinator_event_with_severity(
        &paths.root,
        "retry_preflight_failed",
        task_id,
        "review",
        "not_started",
        &message,
        "warning",
    ) {
        return MaccError::Validation(format!(
            "{message} Could not record preflight event: {error}"
        ));
    }
    MaccError::Validation(message)
}

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
    let before_retry = task.clone();
    let review_path = if phase == "review" {
        let path =
            review_worktree(paths, task).map_err(|err| review_not_started(paths, &task_id, err))?;
        let limit = env_cfg
            .max_review_cycles
            .or_else(|| coordinator_cfg.and_then(|c| c.max_review_cycles));
        crate::coordinator::review_budget::reserve(task, limit, attempts)?;
        Some(path)
    } else {
        None
    };
    task.task_runtime.retries = Some(
        task.task_runtime
            .retries
            .unwrap_or_default()
            .saturating_add(1),
    );
    task.set_workflow_state(retry_state);
    let assignment = task.clone();
    crate::coordinator::state::replace_phase_task(paths, &state_args, &before_retry, &assignment)?;
    let tool = env_cfg
        .coordinator_tool
        .as_deref()
        .or_else(|| coordinator_cfg.and_then(|cfg| cfg.coordinator_tool.as_deref()));
    let mut review_attempts = None;
    let mut reviewed_commit = None;
    let result = if let Some(path) = review_path {
        let mut reviewer = assignment.clone();
        reviewer.ensure_worktree().worktree_path = Some(path.to_string_lossy().into_owned());
        if reviewer.task_runtime.retained_review.is_some() {
            reviewed_commit = crate::git::head_commit(&path).ok();
        }
        engine
            .coordinator_run_review_phase_for_task_native(&paths.root, &reviewer, tool, logger)
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
    if let (Some(0), Ok(Err(reason))) = (review_attempts, &result) {
        task.state = before_retry.state;
        task.task_runtime.retries = before_retry.task_runtime.retries;
        task.task_runtime.review_usage = before_retry.task_runtime.review_usage;
        crate::coordinator::state::replace_phase_task(paths, &state_args, &expected, task)?;
        return Err(review_not_started(paths, &task_id, reason));
    }
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
