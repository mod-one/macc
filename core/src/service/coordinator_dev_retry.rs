//! Single-shot recovery of a retained development assignment.
use crate::config::CoordinatorConfig;
use crate::coordinator::{
    control_plane::CoordinatorLog, types::CoordinatorEnvConfig, WorkflowState,
};
use crate::coordinator::{model::Task, runtime as coordinator_runtime, RuntimeStatus};
use crate::{MaccError, ProjectPaths, Result};
use std::collections::BTreeMap;
use std::path::PathBuf;

fn retained_worktree(paths: &ProjectPaths, task: &Task) -> Result<PathBuf> {
    if !matches!(
        task.workflow_state(),
        Some(WorkflowState::Blocked | WorkflowState::InProgress)
    ) || task
        .task_runtime
        .current_phase
        .as_deref()
        .is_some_and(|phase| phase != "dev")
        || task
            .runtime_pid()
            .is_some_and(crate::coordinator::helpers::is_pid_running)
    {
        return Err(MaccError::Validation(format!(
            "Task {} has no inactive development assignment to retry",
            task.id
        )));
    }
    let path = paths.root.join(task.worktree_path().ok_or_else(|| {
        MaccError::Validation("Development retry requires a retained worktree".into())
    })?);
    let path = path.canonicalize().map_err(|source| MaccError::Io {
        path: path.display().to_string(),
        action: "inspect retained worktree".into(),
        source,
    })?;
    let meta = crate::read_worktree_metadata(&path)?.ok_or_else(|| {
        MaccError::Validation(format!("Missing worktree metadata for {}", task.id))
    })?;
    if task.branch().is_some_and(|branch| branch != meta.branch)
        || task.task_tool().is_some_and(|tool| tool != meta.tool)
        || path
            == paths
                .root
                .canonicalize()
                .map_err(|err| MaccError::Validation(err.to_string()))?
        || !crate::git::worktrees_for_branch(&paths.root, &meta.branch)?
            .iter()
            .any(|registered| {
                registered
                    .canonicalize()
                    .is_ok_and(|registered| registered == path)
            })
        || !path.join("worktree.prd.json").is_file()
        || crate::resolve_worktree_task_context(&paths.root, &path, &meta.id)?.0 != task.id
    {
        return Err(MaccError::Validation(format!(
            "Retained worktree {} does not match the assignment for task {}",
            path.display(),
            task.id
        )));
    }
    Ok(path)
}

pub(super) fn retry_dev_phase<E: crate::engine::Engine + ?Sized>(
    engine: &E,
    paths: &ProjectPaths,
    coordinator_cfg: Option<&CoordinatorConfig>,
    env_cfg: &CoordinatorEnvConfig,
    task: &Task,
    state_args: &BTreeMap<String, String>,
    logger: Option<&dyn CoordinatorLog>,
) -> Result<()> {
    let worktree = retained_worktree(paths, task)?;
    let task_id = &task.id;
    let current_exe = std::env::current_exe()
        .map_err(|e| MaccError::Validation(format!("Failed to resolve current executable: {e}")))?;
    let timeout = env_cfg.stale_in_progress_seconds.unwrap_or(0);
    tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .enable_all()
        .build()
        .map_err(|e| MaccError::Validation(format!("Failed to initialize retry runtime: {e}")))?
        .block_on(async {
            let mut state = coordinator_runtime::CoordinatorRunState::new();
            crate::coordinator::ipc::ensure_performer_ipc_listener(&paths.root, &mut state, logger)
                .await?;
            // Record a launch attempt only after worktree and IPC preflight succeed.
            let mut args = state_args.clone();
            args.insert("task-id".into(), task.id.clone());
            engine.coordinator_state_increment_retries(&paths.root, &args)?;
            let claim_id = task.task_runtime.claim_id.clone().unwrap_or_default();
            let epoch = task.task_runtime.coordinator_epoch.unwrap_or(0);
            let group = coordinator_runtime::spawn_performer_job(
                &current_exe,
                &paths.root,
                task_id,
                &task.base_branch("master"),
                &worktree,
                &state.event_tx,
                &mut state.join_set,
                timeout,
                state.performer_ipc_addr.as_deref(),
                &claim_id,
                epoch,
                &[],
                state.activity_wake.clone(),
            )?;
            crate::coordinator::state::record_retry_group(
                &paths.root,
                state_args,
                task_id,
                &claim_id,
                epoch,
                group.as_ref(),
            )?;
            state.active_jobs.insert(
                task_id.to_string(),
                coordinator_runtime::CoordinatorJob {
                    tool: task.task_tool().unwrap_or_default().to_string(),
                    base_branch: task.base_branch("master"),
                    worktree_path: worktree.to_owned(),
                    attempt: 1,
                    started_at: std::time::Instant::now(),
                    group,
                    failure_signaled_at: None,
                },
            );
            while !state.active_jobs.is_empty() {
                engine
                    .coordinator_monitor_active_jobs_native(
                        &paths.root,
                        env_cfg,
                        coordinator_cfg,
                        &mut state,
                        1,
                        timeout,
                        logger,
                    )
                    .await?;
                if !state.active_jobs.is_empty() {
                    state
                        .wait_for_activity(std::time::Duration::from_millis(120))
                        .await;
                }
            }
            let snapshot = engine.coordinator_state_snapshot(&paths.root, state_args)?;
            let result = snapshot.registry.find_task(task_id).ok_or_else(|| {
                MaccError::Validation(format!("Task {task_id} missing after development retry"))
            })?;
            if result.workflow_state() != Some(WorkflowState::Merged)
                && result.runtime_status() != RuntimeStatus::PhaseDone
            {
                return Err(MaccError::Coordinator {
                    code: "dev_retry_failed",
                    message: format!(
                        "Task {task_id}: {}",
                        result
                            .task_runtime
                            .last_error
                            .as_deref()
                            .unwrap_or("development retry failed; inspect the performer log")
                    ),
                });
            }
            Ok(())
        })
}
