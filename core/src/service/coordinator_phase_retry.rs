//! Retry a retained phase using the same review budget as the control plane.
use super::{apply_storage_mode_args, parse_retry_phase_args, retry_dev_phase};
use crate::config::CoordinatorConfig;
use crate::coordinator::{
    control_plane::CoordinatorLog, engine as coordinator_engine, types::CoordinatorEnvConfig,
    RuntimeStatus, WorkflowState,
};
use crate::{MaccError, ProjectPaths, Result};
use std::collections::BTreeMap;

pub fn coordinator_retry_phase<E: crate::engine::Engine + ?Sized>(
    engine: &E,
    paths: &ProjectPaths,
    canonical: &crate::config::CanonicalConfig,
    coordinator_cfg: Option<&CoordinatorConfig>,
    env_cfg: &CoordinatorEnvConfig,
    args: &[String],
    logger: Option<&dyn CoordinatorLog>,
) -> Result<()> {
    let (task_id, phase, skip) = parse_retry_phase_args(args)?;
    let _phase_guard = if matches!(phase.as_str(), "review" | "fix") {
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
        task.state = "todo".to_string();
        task.task_runtime.status = Some(RuntimeStatus::Idle.as_str().to_string());
        task.task_runtime.pid = None;
        task.task_runtime.started_at = None;
        task.task_runtime.current_phase = None;
        task.task_runtime.merge_result_pending = Some(false);
        task.task_runtime.merge_result_file = None;
        snapshot.registry.updated_at = Some(crate::coordinator::helpers::now_iso_coordinator());
        engine.coordinator_state_save_snapshot(&paths.root, &state_args, &snapshot)?;
        return Ok(());
    }

    if phase == "dev" {
        state_args.insert("task-id".to_string(), task_id.clone());
        engine.coordinator_state_increment_retries(&paths.root, &state_args)?;
        return retry_dev_phase(
            engine,
            paths,
            canonical,
            coordinator_cfg,
            env_cfg,
            &task_id,
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
    engine.coordinator_state_save_snapshot(&paths.root, &state_args, &snapshot)?;
    let tool = env_cfg
        .coordinator_tool
        .as_deref()
        .or_else(|| coordinator_cfg.and_then(|cfg| cfg.coordinator_tool.as_deref()));
    let mut review_attempts = None;
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
    if let Some(count) = review_attempts {
        task.task_runtime.settle_review_attempts(count);
    }
    let now = crate::coordinator::helpers::now_iso_coordinator();
    let outcome = match result.unwrap_or_else(|error| Err(error.to_string())) {
        Ok(Some(verdict)) => {
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
    engine.coordinator_state_save_snapshot(&paths.root, &state_args, &snapshot)?;
    outcome
}
