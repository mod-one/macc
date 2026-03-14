use crate::config::CoordinatorConfig;
use crate::coordinator::args::{
    parse_coordinator_extra_kv_args, RuntimeStatusFromEventArgs, RuntimeTransitionArgs,
    WorkflowTransitionArgs,
};
use crate::coordinator::control_plane::CoordinatorLog;
use crate::coordinator::engine as coordinator_engine;
use crate::coordinator::runtime as coordinator_runtime;
use crate::coordinator::runtime_status_from_event;
use crate::coordinator::state_runtime;
use crate::coordinator::task_selector::{
    select_next_ready_task_typed, SelectedTask, TaskSelectorConfig,
};
use crate::coordinator::types::CoordinatorEnvConfig;
use crate::coordinator::{
    is_valid_runtime_transition, is_valid_workflow_transition, RuntimeStatus, WorkflowState,
};
use crate::coordinator_storage::{
    CoordinatorSnapshot, CoordinatorStorage, CoordinatorStorageMode, CoordinatorStoragePaths,
    JsonStorage, SqliteStorage,
};
use crate::service::coordinator::{
    coordinator_poll_managed_command_process, coordinator_start_managed_command_process,
    coordinator_stop_managed_command_process, CoordinatorManagedCommandPoll,
};
use crate::{MaccError, ProjectPaths, Result};
use std::collections::BTreeMap;
use std::path::PathBuf;
use std::time::Duration;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CoordinatorCommand {
    Run,
    RunControlPlane,
    RunCycle,
    DispatchReadyTasks,
    AdvanceTasks,
    SyncRegistry,
    GetStatus,
    ReconcileRuntime,
    CleanupMaintenance,
    ResumePausedRun,
    AggregatePerformerLogs,
    EvaluateCutoverGate,
    Unlock {
        task_id: Option<String>,
        resource: Option<String>,
        clear_all: bool,
        unlock_state: String,
    },
    RetryTaskPhase {
        task_id: String,
        phase: String,
        skip: bool,
    },
    ImportStorageJsonToSqlite,
    ExportStorageSqliteToJson,
    VerifyStorageParity,
    SelectReadyTask {
        registry_path: Option<PathBuf>,
        config: TaskSelectorConfig,
    },
    ValidateWorkflowTransition {
        from: WorkflowState,
        to: WorkflowState,
    },
    ValidateRuntimeTransition {
        from: RuntimeStatus,
        to: RuntimeStatus,
    },
    RuntimeStatusFromEvent {
        event_type: String,
        status: String,
    },
    StateApplyTransition {
        task_id: String,
        state: String,
        pr_url: Option<String>,
        reviewer: Option<String>,
        reason: Option<String>,
    },
    StateSetRuntime {
        task_id: String,
        runtime_status: String,
        phase: Option<String>,
        pid: Option<i64>,
        last_error: Option<String>,
        heartbeat_ts: Option<String>,
        attempt: Option<i64>,
    },
    StateTaskField {
        task_id: String,
        field: String,
    },
    StateTaskExists {
        task_id: String,
    },
    StateCounts,
    StateLocks {
        format: Option<String>,
    },
    StateSetMergePending {
        task_id: String,
        result_file: String,
        pid: Option<i64>,
        now: Option<String>,
    },
    StateSetMergeProcessed {
        task_id: String,
        result_file: Option<String>,
        status: Option<String>,
        rc: Option<i64>,
        now: Option<String>,
    },
    StateIncrementRetries {
        task_id: String,
        now: Option<String>,
    },
    StateUpsertSloWarning {
        task_id: String,
        metric: String,
        threshold: i64,
        value: i64,
        suggestion: Option<String>,
        now: Option<String>,
    },
    StateSloMetric {
        task_id: String,
        metric: String,
    },
    Stop {
        graceful: bool,
        remove_worktrees: bool,
        remove_branches: bool,
        reason: String,
    },
}

#[derive(Debug, Clone)]
pub struct CoordinatorRunOptions {
    pub extra_args: Vec<String>,
    pub env_cfg: CoordinatorEnvConfig,
}

pub struct CoordinatorCommandRequest<'a> {
    pub canonical: Option<&'a crate::config::CanonicalConfig>,
    pub coordinator_cfg: Option<&'a CoordinatorConfig>,
    pub env_cfg: &'a CoordinatorEnvConfig,
    pub logger: Option<&'a dyn CoordinatorLog>,
}

#[derive(Debug, Clone, Default)]
pub struct CoordinatorCommandResult {
    pub status: Option<CoordinatorStatus>,
    pub resumed: Option<bool>,
    pub aggregated_performer_logs: Option<usize>,
    pub runtime_status: Option<String>,
    pub exported_events_path: Option<PathBuf>,
    pub removed_worktrees: Option<usize>,
    pub selected_task: Option<SelectedTask>,
}

#[derive(Debug, Clone, Default)]
pub struct CoordinatorStatus {
    pub total: usize,
    pub todo: usize,
    pub active: usize,
    pub blocked: usize,
    pub merged: usize,
    pub paused: bool,
    pub pause_reason: Option<String>,
    pub pause_task_id: Option<String>,
    pub pause_phase: Option<String>,
    pub latest_error: Option<String>,
    pub failure_report: Option<crate::service::diagnostic::FailureReport>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CoordinatorCommandInvocation {
    pub action: &'static str,
    pub args: Vec<String>,
}

pub fn coordinator_command_display_name(command: &CoordinatorCommand) -> &'static str {
    match command {
        CoordinatorCommand::Run => "run",
        CoordinatorCommand::RunControlPlane => "control-plane-run",
        CoordinatorCommand::RunCycle => "run-cycle",
        CoordinatorCommand::DispatchReadyTasks => "dispatch",
        CoordinatorCommand::AdvanceTasks => "advance",
        CoordinatorCommand::SyncRegistry => "sync",
        CoordinatorCommand::GetStatus => "status",
        CoordinatorCommand::ReconcileRuntime => "reconcile",
        CoordinatorCommand::CleanupMaintenance => "cleanup",
        CoordinatorCommand::ResumePausedRun => "resume",
        CoordinatorCommand::AggregatePerformerLogs => "aggregate-performer-logs",
        CoordinatorCommand::EvaluateCutoverGate => "cutover-gate",
        CoordinatorCommand::Unlock { .. } => "unlock",
        CoordinatorCommand::RetryTaskPhase { .. } => "retry-phase",
        CoordinatorCommand::ImportStorageJsonToSqlite => "storage-import",
        CoordinatorCommand::ExportStorageSqliteToJson => "storage-export",
        CoordinatorCommand::VerifyStorageParity => "storage-verify",
        CoordinatorCommand::SelectReadyTask { .. } => "select-ready-task",
        CoordinatorCommand::ValidateWorkflowTransition { .. } => "validate-transition",
        CoordinatorCommand::ValidateRuntimeTransition { .. } => "validate-runtime-transition",
        CoordinatorCommand::RuntimeStatusFromEvent { .. } => "runtime-status-from-event",
        CoordinatorCommand::StateApplyTransition { .. } => "state-apply-transition",
        CoordinatorCommand::StateSetRuntime { .. } => "state-set-runtime",
        CoordinatorCommand::StateTaskField { .. } => "state-task-field",
        CoordinatorCommand::StateTaskExists { .. } => "state-task-exists",
        CoordinatorCommand::StateCounts => "state-counts",
        CoordinatorCommand::StateLocks { .. } => "state-locks",
        CoordinatorCommand::StateSetMergePending { .. } => "state-set-merge-pending",
        CoordinatorCommand::StateSetMergeProcessed { .. } => "state-set-merge-processed",
        CoordinatorCommand::StateIncrementRetries { .. } => "state-increment-retries",
        CoordinatorCommand::StateUpsertSloWarning { .. } => "state-upsert-slo-warning",
        CoordinatorCommand::StateSloMetric { .. } => "state-slo-metric",
        CoordinatorCommand::Stop { .. } => "stop",
    }
}

pub fn coordinator_command_invocation(
    command: &CoordinatorCommand,
) -> Result<CoordinatorCommandInvocation> {
    let invocation = match command {
        CoordinatorCommand::Run => CoordinatorCommandInvocation {
            action: "run",
            args: Vec::new(),
        },
        CoordinatorCommand::RunControlPlane => CoordinatorCommandInvocation {
            action: "control-plane-run",
            args: Vec::new(),
        },
        CoordinatorCommand::DispatchReadyTasks => CoordinatorCommandInvocation {
            action: "dispatch",
            args: Vec::new(),
        },
        CoordinatorCommand::AdvanceTasks => CoordinatorCommandInvocation {
            action: "advance",
            args: Vec::new(),
        },
        CoordinatorCommand::SyncRegistry => CoordinatorCommandInvocation {
            action: "sync",
            args: Vec::new(),
        },
        CoordinatorCommand::ReconcileRuntime => CoordinatorCommandInvocation {
            action: "reconcile",
            args: Vec::new(),
        },
        CoordinatorCommand::CleanupMaintenance => CoordinatorCommandInvocation {
            action: "cleanup",
            args: Vec::new(),
        },
        CoordinatorCommand::AggregatePerformerLogs => CoordinatorCommandInvocation {
            action: "aggregate-performer-logs",
            args: Vec::new(),
        },
        CoordinatorCommand::EvaluateCutoverGate => CoordinatorCommandInvocation {
            action: "cutover-gate",
            args: Vec::new(),
        },
        CoordinatorCommand::Unlock {
            task_id,
            resource,
            clear_all,
            unlock_state,
        } => {
            let mut args = Vec::new();
            if let Some(task_id) = task_id {
                args.push("--task".to_string());
                args.push(task_id.clone());
            }
            if let Some(resource) = resource {
                args.push("--resource".to_string());
                args.push(resource.clone());
            }
            if *clear_all {
                args.push("--all".to_string());
            }
            if unlock_state != "blocked" {
                args.push("--unlock-state".to_string());
                args.push(unlock_state.clone());
            }
            CoordinatorCommandInvocation {
                action: "unlock",
                args,
            }
        }
        CoordinatorCommand::RetryTaskPhase {
            task_id,
            phase,
            skip,
        } => {
            let mut args = vec![
                "--retry-task".to_string(),
                task_id.clone(),
                "--retry-phase".to_string(),
                phase.clone(),
            ];
            if *skip {
                args.push("--skip".to_string());
            }
            CoordinatorCommandInvocation {
                action: "retry-phase",
                args,
            }
        }
        CoordinatorCommand::ImportStorageJsonToSqlite => CoordinatorCommandInvocation {
            action: "storage-import",
            args: Vec::new(),
        },
        CoordinatorCommand::ExportStorageSqliteToJson => CoordinatorCommandInvocation {
            action: "storage-export",
            args: Vec::new(),
        },
        CoordinatorCommand::VerifyStorageParity => CoordinatorCommandInvocation {
            action: "storage-verify",
            args: Vec::new(),
        },
        CoordinatorCommand::SelectReadyTask {
            registry_path,
            config,
        } => {
            let mut args = Vec::new();
            if let Some(path) = registry_path {
                args.push("--registry".to_string());
                args.push(path.display().to_string());
            }
            if !config.enabled_tools.is_empty() {
                args.push("--enabled-tools-json".to_string());
                args.push(serde_json::to_string(&config.enabled_tools).map_err(|e| {
                    MaccError::Validation(format!("failed to serialize enabled-tools-json: {}", e))
                })?);
            }
            if !config.tool_priority.is_empty() {
                args.push("--tool-priority-json".to_string());
                args.push(serde_json::to_string(&config.tool_priority).map_err(|e| {
                    MaccError::Validation(format!("failed to serialize tool-priority-json: {}", e))
                })?);
            }
            if !config.max_parallel_per_tool.is_empty() {
                args.push("--max-parallel-per-tool-json".to_string());
                args.push(
                    serde_json::to_string(&config.max_parallel_per_tool).map_err(|e| {
                        MaccError::Validation(format!(
                            "failed to serialize max-parallel-per-tool-json: {}",
                            e
                        ))
                    })?,
                );
            }
            if !config.tool_specializations.is_empty() {
                args.push("--tool-specializations-json".to_string());
                args.push(
                    serde_json::to_string(&config.tool_specializations).map_err(|e| {
                        MaccError::Validation(format!(
                            "failed to serialize tool-specializations-json: {}",
                            e
                        ))
                    })?,
                );
            }
            args.push("--max-parallel".to_string());
            args.push(config.max_parallel.to_string());
            if !config.default_tool.is_empty() {
                args.push("--default-tool".to_string());
                args.push(config.default_tool.clone());
            }
            if !config.default_base_branch.is_empty() {
                args.push("--default-base-branch".to_string());
                args.push(config.default_base_branch.clone());
            }
            CoordinatorCommandInvocation {
                action: "select-ready-task",
                args,
            }
        }
        CoordinatorCommand::StateApplyTransition { .. }
        | CoordinatorCommand::ValidateWorkflowTransition { .. }
        | CoordinatorCommand::ValidateRuntimeTransition { .. }
        | CoordinatorCommand::RuntimeStatusFromEvent { .. }
        | CoordinatorCommand::StateSetRuntime { .. }
        | CoordinatorCommand::StateTaskField { .. }
        | CoordinatorCommand::StateTaskExists { .. }
        | CoordinatorCommand::StateCounts
        | CoordinatorCommand::StateLocks { .. }
        | CoordinatorCommand::StateSetMergePending { .. }
        | CoordinatorCommand::StateSetMergeProcessed { .. }
        | CoordinatorCommand::StateIncrementRetries { .. }
        | CoordinatorCommand::StateUpsertSloWarning { .. }
        | CoordinatorCommand::StateSloMetric { .. }
        | CoordinatorCommand::RunCycle
        | CoordinatorCommand::GetStatus
        | CoordinatorCommand::ResumePausedRun
        | CoordinatorCommand::Stop { .. } => {
            return Err(MaccError::Validation(format!(
                "Coordinator command '{}' is not available as a managed process invocation",
                coordinator_command_display_name(command)
            )));
        }
    };
    Ok(invocation)
}

pub fn coordinator_command_from_name(
    action: &str,
    extra_args: &[String],
    graceful: bool,
    remove_worktrees: bool,
    remove_branches: bool,
) -> Result<CoordinatorCommand> {
    match action {
        "run" => Ok(CoordinatorCommand::Run),
        "control-plane-run" => Ok(CoordinatorCommand::RunControlPlane),
        "dispatch" => Ok(CoordinatorCommand::DispatchReadyTasks),
        "advance" => Ok(CoordinatorCommand::AdvanceTasks),
        "resume" => Ok(CoordinatorCommand::ResumePausedRun),
        "sync" => Ok(CoordinatorCommand::SyncRegistry),
        "status" => Ok(CoordinatorCommand::GetStatus),
        "reconcile" => Ok(CoordinatorCommand::ReconcileRuntime),
        "cleanup" => Ok(CoordinatorCommand::CleanupMaintenance),
        "aggregate-performer-logs" => Ok(CoordinatorCommand::AggregatePerformerLogs),
        "cutover-gate" => Ok(CoordinatorCommand::EvaluateCutoverGate),
        "unlock" => {
            let (task_id, resource, clear_all, unlock_state) = parse_unlock_args(extra_args)?;
            Ok(CoordinatorCommand::Unlock {
                task_id,
                resource,
                clear_all,
                unlock_state,
            })
        }
        "retry-phase" => {
            let (task_id, phase, skip) = parse_retry_phase_args(extra_args)?;
            Ok(CoordinatorCommand::RetryTaskPhase {
                task_id,
                phase,
                skip,
            })
        }
        "storage-import" => Ok(CoordinatorCommand::ImportStorageJsonToSqlite),
        "storage-export" | "events-export" => Ok(CoordinatorCommand::ExportStorageSqliteToJson),
        "storage-verify" => Ok(CoordinatorCommand::VerifyStorageParity),
        "select-ready-task" => Ok(parse_select_ready_task_command(extra_args)?),
        "validate-transition" => Ok(parse_validate_transition_command(extra_args)?),
        "validate-runtime-transition" => Ok(parse_validate_runtime_transition_command(extra_args)?),
        "runtime-status-from-event" => Ok(parse_runtime_status_from_event_command(extra_args)?),
        "storage-sync" => {
            let direction =
                crate::coordinator::args::StorageSyncArgs::try_from(extra_args)?.direction;
            Ok(match direction {
                crate::coordinator_storage::CoordinatorStorageTransfer::ImportJsonToSqlite => {
                    CoordinatorCommand::ImportStorageJsonToSqlite
                }
                crate::coordinator_storage::CoordinatorStorageTransfer::ExportSqliteToJson => {
                    CoordinatorCommand::ExportStorageSqliteToJson
                }
                crate::coordinator_storage::CoordinatorStorageTransfer::VerifyParity => {
                    CoordinatorCommand::VerifyStorageParity
                }
            })
        }
        "state-apply-transition" => Ok(parse_state_apply_transition_command(extra_args)?),
        "state-set-runtime" => Ok(parse_state_set_runtime_command(extra_args)?),
        "state-task-field" => Ok(parse_state_task_field_command(extra_args)?),
        "state-task-exists" => Ok(parse_state_task_exists_command(extra_args)?),
        "state-counts" => Ok(CoordinatorCommand::StateCounts),
        "state-locks" => Ok(parse_state_locks_command(extra_args)?),
        "state-set-merge-pending" => Ok(parse_state_set_merge_pending_command(extra_args)?),
        "state-set-merge-processed" => Ok(parse_state_set_merge_processed_command(extra_args)?),
        "state-increment-retries" => Ok(parse_state_increment_retries_command(extra_args)?),
        "state-upsert-slo-warning" => Ok(parse_state_upsert_slo_warning_command(extra_args)?),
        "state-slo-metric" => Ok(parse_state_slo_metric_command(extra_args)?),
        "stop" => Ok(CoordinatorCommand::Stop {
            graceful,
            remove_worktrees,
            remove_branches,
            reason: "manual stop".to_string(),
        }),
        other => Err(MaccError::Validation(format!(
            "Unknown coordinator action '{}'",
            other
        ))),
    }
}

pub fn coordinator_command_emits_runtime_events(command: &CoordinatorCommand) -> bool {
    matches!(
        command,
        CoordinatorCommand::Run
            | CoordinatorCommand::RunControlPlane
            | CoordinatorCommand::DispatchReadyTasks
            | CoordinatorCommand::AdvanceTasks
            | CoordinatorCommand::SyncRegistry
            | CoordinatorCommand::ReconcileRuntime
            | CoordinatorCommand::CleanupMaintenance
            | CoordinatorCommand::RetryTaskPhase { .. }
    )
}

pub fn coordinator_run_cycle<E: crate::engine::Engine + ?Sized>(
    engine: &E,
    paths: &ProjectPaths,
    canonical: &crate::config::CanonicalConfig,
    coordinator_cfg: Option<&CoordinatorConfig>,
    env_cfg: &CoordinatorEnvConfig,
    logger: Option<&dyn CoordinatorLog>,
) -> Result<()> {
    coordinator_sync(engine, paths, coordinator_cfg, env_cfg, logger)?;
    coordinator_dispatch(engine, paths, canonical, coordinator_cfg, env_cfg, logger)?;
    coordinator_advance(engine, paths, coordinator_cfg, env_cfg, logger)?;
    Ok(())
}

pub fn coordinator_execute_command<E: crate::engine::Engine + ?Sized>(
    engine: &E,
    paths: &ProjectPaths,
    command: CoordinatorCommand,
    request: CoordinatorCommandRequest<'_>,
) -> Result<CoordinatorCommandResult> {
    let mut result = CoordinatorCommandResult::default();
    match command {
        CoordinatorCommand::Run => {
            coordinator_run(
                paths,
                request.coordinator_cfg,
                &CoordinatorRunOptions {
                    extra_args: Vec::new(),
                    env_cfg: request.env_cfg.clone(),
                },
            )?;
        }
        CoordinatorCommand::RunControlPlane => {
            let canonical = request.canonical.ok_or_else(|| {
                MaccError::Validation("run-control-plane requires canonical config".into())
            })?;
            let runtime = tokio::runtime::Builder::new_current_thread()
                .enable_time()
                .enable_io()
                .build()
                .map_err(|e| {
                    MaccError::Validation(format!("Failed to initialize tokio runtime: {}", e))
                })?;
            runtime.block_on(coordinator_engine::run_native_control_plane(
                &paths.root,
                canonical,
                request.coordinator_cfg,
                request.env_cfg,
                request.logger,
            ))?;
        }
        CoordinatorCommand::RunCycle => {
            let canonical = request.canonical.ok_or_else(|| {
                MaccError::Validation("run-cycle requires canonical config".into())
            })?;
            coordinator_run_cycle(
                engine,
                paths,
                canonical,
                request.coordinator_cfg,
                request.env_cfg,
                request.logger,
            )?;
        }
        CoordinatorCommand::DispatchReadyTasks => {
            let canonical = request.canonical.ok_or_else(|| {
                MaccError::Validation("dispatch requires canonical config".into())
            })?;
            coordinator_dispatch(
                engine,
                paths,
                canonical,
                request.coordinator_cfg,
                request.env_cfg,
                request.logger,
            )?;
        }
        CoordinatorCommand::AdvanceTasks => {
            coordinator_advance(
                engine,
                paths,
                request.coordinator_cfg,
                request.env_cfg,
                request.logger,
            )?;
        }
        CoordinatorCommand::SyncRegistry => {
            coordinator_sync(
                engine,
                paths,
                request.coordinator_cfg,
                request.env_cfg,
                request.logger,
            )?;
        }
        CoordinatorCommand::GetStatus => {
            result.status = Some(get_coordinator_status(paths)?);
        }
        CoordinatorCommand::ReconcileRuntime => {
            coordinator_reconcile(paths, request.logger)?;
        }
        CoordinatorCommand::CleanupMaintenance => {
            coordinator_cleanup(paths, request.logger)?;
        }
        CoordinatorCommand::ResumePausedRun => {
            let was_paused = state_runtime::read_coordinator_pause_file(&paths.root)?.is_some();
            engine.coordinator_resume(&paths.root)?;
            result.resumed = Some(was_paused);
        }
        CoordinatorCommand::AggregatePerformerLogs => {
            result.aggregated_performer_logs =
                Some(engine.coordinator_aggregate_performer_logs(&paths.root)?);
        }
        CoordinatorCommand::EvaluateCutoverGate => {
            coordinator_cutover_gate(paths)?;
        }
        CoordinatorCommand::Unlock {
            task_id,
            resource,
            clear_all,
            unlock_state,
        } => {
            let mut args = Vec::new();
            if let Some(task_id) = task_id {
                args.push("--task".to_string());
                args.push(task_id);
            }
            if let Some(resource) = resource {
                args.push("--resource".to_string());
                args.push(resource);
            }
            if clear_all {
                args.push("--all".to_string());
            }
            if unlock_state != "blocked" {
                args.push("--unlock-state".to_string());
                args.push(unlock_state);
            }
            coordinator_unlock(
                engine,
                paths,
                request.coordinator_cfg,
                request.env_cfg,
                &args,
            )?;
        }
        CoordinatorCommand::RetryTaskPhase {
            task_id,
            phase,
            skip,
        } => {
            let canonical = request.canonical.ok_or_else(|| {
                MaccError::Validation("retry-phase requires canonical config".into())
            })?;
            let mut args = vec![
                "--retry-task".to_string(),
                task_id,
                "--retry-phase".to_string(),
                phase,
            ];
            if skip {
                args.push("--skip".to_string());
            }
            coordinator_retry_phase(
                engine,
                paths,
                canonical,
                request.coordinator_cfg,
                request.env_cfg,
                &args,
                request.logger,
            )?;
        }
        CoordinatorCommand::ImportStorageJsonToSqlite => {
            engine.coordinator_storage_import_json_to_sqlite(paths)?;
        }
        CoordinatorCommand::ExportStorageSqliteToJson => {
            engine.coordinator_storage_export_sqlite_to_json(paths)?;
            result.exported_events_path = Some(
                paths
                    .root
                    .join(".macc")
                    .join("log")
                    .join("coordinator")
                    .join("events.jsonl"),
            );
        }
        CoordinatorCommand::VerifyStorageParity => {
            engine.coordinator_storage_verify_parity(paths)?;
        }
        CoordinatorCommand::SelectReadyTask {
            registry_path,
            config,
        } => {
            let registry = if let Some(path) = registry_path {
                let absolute = if path.is_absolute() {
                    path
                } else {
                    paths.root.join(path)
                };
                let raw = std::fs::read_to_string(&absolute).map_err(|e| MaccError::Io {
                    path: absolute.to_string_lossy().into(),
                    action: "read task registry for select-ready-task".into(),
                    source: e,
                })?;
                crate::coordinator::model::TaskRegistry::from_value(
                    &serde_json::from_str::<serde_json::Value>(&raw).map_err(|e| {
                        MaccError::Validation(format!(
                            "Failed to parse task registry JSON '{}': {}",
                            absolute.display(),
                            e
                        ))
                    })?,
                )?
            } else {
                engine
                    .coordinator_state_snapshot(&paths.root, &BTreeMap::new())?
                    .registry
            };
            result.selected_task = select_next_ready_task_typed(&registry, &config);
        }
        CoordinatorCommand::ValidateWorkflowTransition { from, to } => {
            if !is_valid_workflow_transition(from, to) {
                return Err(MaccError::Validation(format!(
                    "invalid transition {} -> {}",
                    from.as_str(),
                    to.as_str()
                )));
            }
        }
        CoordinatorCommand::ValidateRuntimeTransition { from, to } => {
            if !is_valid_runtime_transition(from, to) {
                return Err(MaccError::Validation(format!(
                    "invalid runtime transition {} -> {}",
                    from.as_str(),
                    to.as_str()
                )));
            }
        }
        CoordinatorCommand::RuntimeStatusFromEvent { event_type, status } => {
            result.runtime_status = Some(
                runtime_status_from_event(&event_type, &status)
                    .as_str()
                    .to_string(),
            );
        }
        CoordinatorCommand::StateApplyTransition {
            task_id,
            state,
            pr_url,
            reviewer,
            reason,
        } => {
            let mut args = BTreeMap::new();
            args.insert("task-id".to_string(), task_id);
            args.insert("state".to_string(), state);
            if let Some(pr_url) = pr_url {
                args.insert("pr-url".to_string(), pr_url);
            }
            if let Some(reviewer) = reviewer {
                args.insert("reviewer".to_string(), reviewer);
            }
            if let Some(reason) = reason {
                args.insert("reason".to_string(), reason);
            }
            engine.coordinator_state_apply_transition(&paths.root, &args)?;
        }
        CoordinatorCommand::StateSetRuntime {
            task_id,
            runtime_status,
            phase,
            pid,
            last_error,
            heartbeat_ts,
            attempt,
        } => {
            let mut args = BTreeMap::new();
            args.insert("task-id".to_string(), task_id);
            args.insert("runtime-status".to_string(), runtime_status);
            if let Some(phase) = phase {
                args.insert("phase".to_string(), phase);
            }
            if let Some(pid) = pid {
                args.insert("pid".to_string(), pid.to_string());
            }
            if let Some(last_error) = last_error {
                args.insert("last-error".to_string(), last_error);
            }
            if let Some(heartbeat_ts) = heartbeat_ts {
                args.insert("heartbeat-ts".to_string(), heartbeat_ts);
            }
            if let Some(attempt) = attempt {
                args.insert("attempt".to_string(), attempt.to_string());
            }
            engine.coordinator_state_set_runtime(&paths.root, &args)?;
        }
        CoordinatorCommand::StateTaskField { task_id, field } => {
            let mut args = BTreeMap::new();
            args.insert("task-id".to_string(), task_id);
            args.insert("field".to_string(), field);
            engine.coordinator_state_task_field(&paths.root, &args)?;
        }
        CoordinatorCommand::StateTaskExists { task_id } => {
            let mut args = BTreeMap::new();
            args.insert("task-id".to_string(), task_id);
            engine.coordinator_state_task_exists(&paths.root, &args)?;
        }
        CoordinatorCommand::StateCounts => {
            engine.coordinator_state_counts(&paths.root, &BTreeMap::new())?;
        }
        CoordinatorCommand::StateLocks { format } => {
            let mut args = BTreeMap::new();
            if let Some(format) = format {
                args.insert("format".to_string(), format);
            }
            engine.coordinator_state_locks(&paths.root, &args)?;
        }
        CoordinatorCommand::StateSetMergePending {
            task_id,
            result_file,
            pid,
            now,
        } => {
            let mut args = BTreeMap::new();
            args.insert("task-id".to_string(), task_id);
            args.insert("result-file".to_string(), result_file);
            if let Some(pid) = pid {
                args.insert("pid".to_string(), pid.to_string());
            }
            if let Some(now) = now {
                args.insert("now".to_string(), now);
            }
            engine.coordinator_state_set_merge_pending(&paths.root, &args)?;
        }
        CoordinatorCommand::StateSetMergeProcessed {
            task_id,
            result_file,
            status,
            rc,
            now,
        } => {
            let mut args = BTreeMap::new();
            args.insert("task-id".to_string(), task_id);
            if let Some(result_file) = result_file {
                args.insert("result-file".to_string(), result_file);
            }
            if let Some(status) = status {
                args.insert("status".to_string(), status);
            }
            if let Some(rc) = rc {
                args.insert("rc".to_string(), rc.to_string());
            }
            if let Some(now) = now {
                args.insert("now".to_string(), now);
            }
            engine.coordinator_state_set_merge_processed(&paths.root, &args)?;
        }
        CoordinatorCommand::StateIncrementRetries { task_id, now } => {
            let mut args = BTreeMap::new();
            args.insert("task-id".to_string(), task_id);
            if let Some(now) = now {
                args.insert("now".to_string(), now);
            }
            engine.coordinator_state_increment_retries(&paths.root, &args)?;
        }
        CoordinatorCommand::StateUpsertSloWarning {
            task_id,
            metric,
            threshold,
            value,
            suggestion,
            now,
        } => {
            let mut args = BTreeMap::new();
            args.insert("task-id".to_string(), task_id);
            args.insert("metric".to_string(), metric);
            args.insert("threshold".to_string(), threshold.to_string());
            args.insert("value".to_string(), value.to_string());
            if let Some(suggestion) = suggestion {
                args.insert("suggestion".to_string(), suggestion);
            }
            if let Some(now) = now {
                args.insert("now".to_string(), now);
            }
            engine.coordinator_state_upsert_slo_warning(&paths.root, &args)?;
        }
        CoordinatorCommand::StateSloMetric { task_id, metric } => {
            let mut args = BTreeMap::new();
            args.insert("task-id".to_string(), task_id);
            args.insert("metric".to_string(), metric);
            engine.coordinator_state_slo_metric(&paths.root, &args)?;
        }
        CoordinatorCommand::Stop {
            graceful,
            remove_worktrees,
            remove_branches,
            reason,
        } => {
            coordinator_stop(paths, &reason)?;
            coordinator_reconcile(paths, request.logger)?;
            coordinator_cleanup(paths, request.logger)?;
            coordinator_unlock(
                engine,
                paths,
                request.coordinator_cfg,
                request.env_cfg,
                &[
                    "--all".to_string(),
                    "--unlock-state".to_string(),
                    "blocked".to_string(),
                ],
            )?;
            result.removed_worktrees = Some(handle_stop_cleanup(
                paths,
                graceful,
                remove_worktrees,
                remove_branches,
            )?);
        }
    }
    Ok(result)
}

pub fn coordinator_run(
    paths: &ProjectPaths,
    cfg: Option<&CoordinatorConfig>,
    options: &CoordinatorRunOptions,
) -> Result<()> {
    let _ = options.env_cfg;
    coordinator_start_managed_command_process(paths, "run", &options.extra_args, cfg)?;

    loop {
        match coordinator_poll_managed_command_process(paths)? {
            CoordinatorManagedCommandPoll::Idle => return Ok(()),
            CoordinatorManagedCommandPoll::Running { .. } => {
                std::thread::sleep(Duration::from_millis(250))
            }
            CoordinatorManagedCommandPoll::Exited {
                success,
                command,
                code,
                ..
            } => {
                if success {
                    return Ok(());
                }
                return Err(MaccError::Validation(format!(
                    "coordinator '{}' failed (status {})",
                    command,
                    code.map(|c| c.to_string())
                        .unwrap_or_else(|| "unknown".to_string())
                )));
            }
        }
    }
}

fn handle_stop_cleanup(
    paths: &ProjectPaths,
    _graceful: bool,
    remove_worktrees: bool,
    remove_branches: bool,
) -> Result<usize> {
    if !remove_worktrees {
        return Ok(0);
    }
    let removed = crate::service::worktree::remove_all_worktrees(&paths.root, remove_branches)?;
    crate::prune_worktrees(&paths.root)?;
    Ok(removed)
}

pub fn coordinator_stop(paths: &ProjectPaths, reason: &str) -> Result<()> {
    state_runtime::write_coordinator_pause_file(
        &paths.root,
        "global",
        "dev",
        &format!("stopped: {}", reason),
    )?;
    let _ = coordinator_stop_managed_command_process(paths, false)?;
    Ok(())
}

pub fn get_coordinator_status(paths: &ProjectPaths) -> Result<CoordinatorStatus> {
    let storage_paths = CoordinatorStoragePaths::from_project_paths(paths);
    let sqlite = SqliteStorage::new(storage_paths.clone());
    let snapshot: CoordinatorSnapshot = if sqlite.has_snapshot_data()? {
        sqlite.load_snapshot()?
    } else {
        JsonStorage::new(storage_paths).load_snapshot()?
    };

    let mut status = CoordinatorStatus::default();
    let (total, todo, active, blocked, merged) = snapshot.registry.counts();
    status.total = total;
    status.todo = todo;
    status.active = active;
    status.blocked = blocked;
    status.merged = merged;

    if let Some(pause) = state_runtime::read_coordinator_pause_file(&paths.root)? {
        status.paused = true;
        status.pause_reason = Some(pause.reason);
        status.pause_task_id = Some(pause.task_id);
        status.pause_phase = Some(pause.phase);
    }

    status.failure_report = crate::service::diagnostic::analyze_last_failure(paths)?;
    if let Some(report) = &status.failure_report {
        status.latest_error = Some(report.message.clone());
    } else {
        let logs = crate::service::logs::read_log_content(paths, "coordinator", None, None)
            .unwrap_or_default();
        status.latest_error = logs
            .lines()
            .rev()
            .find(|line| line.contains("ERROR") || line.contains("failed"))
            .map(|line| line.trim().to_string());
    }

    Ok(status)
}

pub fn coordinator_dispatch<E: crate::engine::Engine + ?Sized>(
    engine: &E,
    paths: &ProjectPaths,
    canonical: &crate::config::CanonicalConfig,
    coordinator_cfg: Option<&CoordinatorConfig>,
    env_cfg: &CoordinatorEnvConfig,
    logger: Option<&dyn CoordinatorLog>,
) -> Result<()> {
    let prd_file = env_cfg
        .prd
        .as_ref()
        .map(std::path::PathBuf::from)
        .or_else(|| {
            coordinator_cfg
                .and_then(|c| c.prd_file.clone())
                .map(std::path::PathBuf::from)
        })
        .unwrap_or_else(|| paths.root.join("prd.json"));
    engine.coordinator_sync_registry_from_prd(&paths.root, &prd_file)?;
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_time()
        .enable_io()
        .build()
        .map_err(|e| MaccError::Validation(format!("Failed to initialize tokio runtime: {}", e)))?;
    runtime.block_on(async {
        let mut state = coordinator_runtime::CoordinatorRunState::new();
        let _ = engine
            .coordinator_dispatch_ready_tasks_native(
                &paths.root,
                canonical,
                coordinator_cfg,
                env_cfg,
                &prd_file,
                &mut state,
                logger,
            )
            .await?;
        let max_attempts = env_cfg
            .phase_runner_max_attempts
            .or_else(|| coordinator_cfg.and_then(|c| c.phase_runner_max_attempts))
            .unwrap_or(1)
            .max(1);
        let phase_timeout = env_cfg
            .stale_in_progress_seconds
            .or_else(|| coordinator_cfg.and_then(|c| c.stale_in_progress_seconds))
            .unwrap_or(0);
        while !state.active_jobs.is_empty() {
            engine
                .coordinator_monitor_active_jobs_native(
                    &paths.root,
                    env_cfg,
                    &mut state,
                    max_attempts,
                    phase_timeout,
                    logger,
                )
                .await?;
            tokio::time::sleep(std::time::Duration::from_millis(120)).await;
        }
        Result::<()>::Ok(())
    })?;
    Ok(())
}

pub fn coordinator_advance<E: crate::engine::Engine + ?Sized>(
    engine: &E,
    paths: &ProjectPaths,
    coordinator_cfg: Option<&CoordinatorConfig>,
    env_cfg: &CoordinatorEnvConfig,
    logger: Option<&dyn CoordinatorLog>,
) -> Result<()> {
    let coordinator_tool_override = env_cfg
        .coordinator_tool
        .clone()
        .or_else(|| coordinator_cfg.and_then(|c| c.coordinator_tool.clone()));
    let phase_runner_max_attempts = env_cfg
        .phase_runner_max_attempts
        .or_else(|| coordinator_cfg.and_then(|c| c.phase_runner_max_attempts))
        .unwrap_or(1)
        .max(1);
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_time()
        .enable_io()
        .build()
        .map_err(|e| MaccError::Validation(format!("Failed to initialize tokio runtime: {}", e)))?;
    let advance = runtime.block_on(async {
        let mut state = coordinator_runtime::CoordinatorRunState::new();
        engine
            .coordinator_advance_tasks_native(
                &paths.root,
                coordinator_tool_override.as_deref(),
                phase_runner_max_attempts,
                &mut state,
                logger,
            )
            .await
    })?;
    if let Some((task_id, reason)) = advance.blocked_merge {
        state_runtime::set_task_paused_for_integrate(&paths.root, &task_id, &reason)?;
        state_runtime::write_coordinator_pause_file(&paths.root, &task_id, "integrate", &reason)?;
        return Err(MaccError::Validation(format!(
            "Coordinator paused on task {} (integrate). Resolve the merge issue, then run `macc coordinator resume`. Reason: {}",
            task_id, reason
        )));
    }
    Ok(())
}

pub fn coordinator_reconcile(
    paths: &ProjectPaths,
    logger: Option<&dyn CoordinatorLog>,
) -> Result<()> {
    if let Some(logger) = logger {
        let _ = logger.note("- Reconcile start".to_string());
    }
    state_runtime::reconcile_registry_native(&paths.root)?;
    if let Some(logger) = logger {
        let _ = logger.note("- Reconcile complete".to_string());
    }
    Ok(())
}

pub fn coordinator_cleanup(
    paths: &ProjectPaths,
    logger: Option<&dyn CoordinatorLog>,
) -> Result<()> {
    if let Some(logger) = logger {
        let _ = logger.note("- Cleanup start".to_string());
    }
    state_runtime::cleanup_registry_native(&paths.root)?;
    if let Some(logger) = logger {
        let _ = logger.note("- Cleanup complete".to_string());
    }
    Ok(())
}

pub fn coordinator_sync<E: crate::engine::Engine + ?Sized>(
    engine: &E,
    paths: &ProjectPaths,
    coordinator_cfg: Option<&CoordinatorConfig>,
    env_cfg: &CoordinatorEnvConfig,
    logger: Option<&dyn CoordinatorLog>,
) -> Result<()> {
    let prd_file = env_cfg
        .prd
        .as_ref()
        .map(std::path::PathBuf::from)
        .or_else(|| {
            coordinator_cfg
                .and_then(|c| c.prd_file.clone())
                .map(std::path::PathBuf::from)
        })
        .unwrap_or_else(|| paths.root.join("prd.json"));
    let storage_mode = coordinator_engine::resolve_storage_mode(env_cfg, coordinator_cfg)?;
    if storage_mode != CoordinatorStorageMode::Json {
        engine.coordinator_storage_import_json_to_sqlite(paths)?;
    }
    engine.coordinator_sync_registry_from_prd_with_logger(&paths.root, &prd_file, logger)?;
    if storage_mode != CoordinatorStorageMode::Json {
        if std::env::var("COORDINATOR_JSON_COMPAT")
            .ok()
            .map(|raw| {
                !matches!(
                    raw.trim().to_ascii_lowercase().as_str(),
                    "0" | "false" | "no" | "off"
                )
            })
            .unwrap_or(false)
        {
            engine.coordinator_storage_export_sqlite_to_json(paths)?;
        }
    }
    Ok(())
}

pub fn coordinator_unlock<E: crate::engine::Engine + ?Sized>(
    engine: &E,
    paths: &ProjectPaths,
    coordinator_cfg: Option<&CoordinatorConfig>,
    env_cfg: &CoordinatorEnvConfig,
    args: &[String],
) -> Result<()> {
    let (task_id, resource, clear_all, unlock_state) = parse_unlock_args(args)?;
    let mut state_args = BTreeMap::new();
    apply_storage_mode_args(&mut state_args, env_cfg, coordinator_cfg);
    if let Some(task_id) = task_id {
        state_args.insert("task-id".to_string(), task_id);
        state_args.insert("state".to_string(), unlock_state);
        state_args.insert("reason".to_string(), "manual_unlock".to_string());
        engine.coordinator_state_apply_transition(&paths.root, &state_args)?;
        return Ok(());
    }
    if clear_all {
        let _ = engine.coordinator_state_unlock_resource(&paths.root, &state_args, None, true)?;
        return Ok(());
    }
    if let Some(resource) = resource {
        let _ = engine.coordinator_state_unlock_resource(
            &paths.root,
            &state_args,
            Some(&resource),
            false,
        )?;
        return Ok(());
    }
    Err(MaccError::Validation(format!(
        "unlock requires --task, --resource, or --all (unlock-state={})",
        unlock_state
    )))
}

pub fn coordinator_cutover_gate(paths: &ProjectPaths) -> Result<()> {
    let window_events: usize = std::env::var("CUTOVER_GATE_WINDOW_EVENTS")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(2000);
    let max_blocked_ratio: f64 = std::env::var("CUTOVER_GATE_MAX_BLOCKED_RATIO")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(0.25);
    let max_stale_ratio: f64 = std::env::var("CUTOVER_GATE_MAX_STALE_RATIO")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(0.25);
    let storage_paths = CoordinatorStoragePaths::from_project_paths(paths);
    let sqlite = SqliteStorage::new(storage_paths.clone());
    let snapshot: CoordinatorSnapshot = if sqlite.has_snapshot_data()? {
        sqlite.load_snapshot()?
    } else {
        JsonStorage::new(storage_paths).load_snapshot()?
    };
    let mut task_events = 0usize;
    let mut blocked_events = 0usize;
    let mut stale_events = 0usize;
    for event in snapshot.events.iter().rev().take(window_events) {
        let event_type = event.event_type.as_str();
        if event.task_id.is_some() {
            task_events += 1;
        }
        if event_type == "task_blocked" || event_type == "local_merge_failed" {
            blocked_events += 1;
        }
        if event_type == "stale_runtime_total" || event_type == "task_runtime_stale" {
            stale_events += 1;
        }
    }
    let blocked_ratio = if task_events == 0 {
        0.0
    } else {
        blocked_events as f64 / task_events as f64
    };
    let stale_ratio = if task_events == 0 {
        0.0
    } else {
        stale_events as f64 / task_events as f64
    };
    if blocked_ratio > max_blocked_ratio {
        return Err(MaccError::Validation(format!(
            "cutover gate failed: blocked ratio {} exceeds {}",
            blocked_ratio, max_blocked_ratio
        )));
    }
    if stale_ratio > max_stale_ratio {
        return Err(MaccError::Validation(format!(
            "cutover gate failed: stale ratio {} exceeds {}",
            stale_ratio, max_stale_ratio
        )));
    }
    Ok(())
}

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

    let mut retry_args = BTreeMap::new();
    apply_storage_mode_args(&mut retry_args, env_cfg, coordinator_cfg);
    retry_args.insert("task-id".to_string(), task_id.clone());
    engine.coordinator_state_increment_retries(&paths.root, &retry_args)?;

    match phase.as_str() {
        "dev" => retry_dev_phase(engine, paths, canonical, env_cfg, task_id.as_str(), logger)?,
        "review" | "fix" | "integrate" => retry_tool_phase(
            engine,
            paths,
            canonical,
            coordinator_cfg,
            env_cfg,
            &mut snapshot,
            task_id.as_str(),
            &phase,
            logger,
        )?,
        other => {
            return Err(MaccError::Validation(format!(
                "unsupported retry phase '{}'",
                other
            )));
        }
    }

    engine.coordinator_state_save_snapshot(&paths.root, &state_args, &snapshot)?;
    Ok(())
}

fn parse_retry_phase_args(args: &[String]) -> Result<(String, String, bool)> {
    let mut task_id = None;
    let mut phase = None;
    let mut skip = false;
    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--retry-task" => {
                if i + 1 >= args.len() {
                    return Err(MaccError::Validation(
                        "retry-phase --retry-task requires a value".into(),
                    ));
                }
                task_id = Some(args[i + 1].clone());
                i += 2;
            }
            "--retry-phase" => {
                if i + 1 >= args.len() {
                    return Err(MaccError::Validation(
                        "retry-phase --retry-phase requires a value".into(),
                    ));
                }
                phase = Some(args[i + 1].clone());
                i += 2;
            }
            "--skip" => {
                skip = true;
                i += 1;
            }
            other => {
                return Err(MaccError::Validation(format!(
                    "Unknown retry-phase arg: {}",
                    other
                )));
            }
        }
    }
    let task_id =
        task_id.ok_or_else(|| MaccError::Validation("retry-phase requires --retry-task".into()))?;
    let phase =
        phase.ok_or_else(|| MaccError::Validation("retry-phase requires --retry-phase".into()))?;
    Ok((task_id, phase, skip))
}

fn retry_dev_phase<E: crate::engine::Engine + ?Sized>(
    engine: &E,
    paths: &ProjectPaths,
    canonical: &crate::config::CanonicalConfig,
    env_cfg: &CoordinatorEnvConfig,
    task_id: &str,
    logger: Option<&dyn CoordinatorLog>,
) -> Result<()> {
    let registry_path = paths
        .root
        .join(".macc")
        .join("automation")
        .join("task")
        .join("task_registry.json");
    let raw = std::fs::read_to_string(&registry_path).map_err(|e| MaccError::Io {
        path: registry_path.to_string_lossy().into(),
        action: "read coordinator registry".into(),
        source: e,
    })?;
    let registry: crate::coordinator::model::TaskRegistry =
        serde_json::from_str(&raw).map_err(|e| {
            MaccError::Validation(format!(
                "Failed to parse coordinator registry {}: {}",
                registry_path.display(),
                e
            ))
        })?;
    let task = registry
        .find_task(task_id)
        .ok_or_else(|| MaccError::Validation("Task missing for retry".into()))?;
    let worktree_path = task
        .worktree_path()
        .ok_or_else(|| MaccError::Validation("retry-phase requires worktree".into()))?;
    let worktree = std::path::PathBuf::from(worktree_path);
    let mut state = coordinator_runtime::CoordinatorRunState::new();
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_time()
        .enable_io()
        .build()
        .map_err(|e| MaccError::Validation(format!("Failed to init tokio: {}", e)))?;
    runtime.block_on(crate::coordinator::ipc::ensure_performer_ipc_listener(
        &paths.root,
        &mut state,
        logger,
    ))?;
    let current_exe = std::env::current_exe().map_err(|e| {
        MaccError::Validation(format!("Failed to resolve current executable path: {}", e))
    })?;
    let pid = coordinator_runtime::spawn_performer_job(
        &current_exe,
        &paths.root,
        task_id,
        &worktree,
        &state.event_tx,
        &mut state.join_set,
        env_cfg.stale_in_progress_seconds.unwrap_or(0),
        state.performer_ipc_addr.as_deref(),
    )?;
    state.active_jobs.insert(
        task_id.to_string(),
        coordinator_runtime::CoordinatorJob {
            tool: task.task_tool().unwrap_or("codex").to_string(),
            worktree_path: worktree.clone(),
            attempt: 1,
            started_at: std::time::Instant::now(),
            pid,
        },
    );
    runtime.block_on(async {
        while !state.active_jobs.is_empty() {
            engine
                .coordinator_monitor_active_jobs_native(
                    &paths.root,
                    env_cfg,
                    &mut state,
                    1,
                    env_cfg.stale_in_progress_seconds.unwrap_or(0),
                    logger,
                )
                .await?;
            tokio::time::sleep(std::time::Duration::from_millis(120)).await;
        }
        Result::<()>::Ok(())
    })?;
    let _ = canonical;
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn retry_tool_phase<E: crate::engine::Engine + ?Sized>(
    engine: &E,
    paths: &ProjectPaths,
    canonical: &crate::config::CanonicalConfig,
    coordinator_cfg: Option<&CoordinatorConfig>,
    env_cfg: &CoordinatorEnvConfig,
    snapshot: &mut crate::coordinator_storage::CoordinatorSnapshot,
    task_id: &str,
    phase: &str,
    logger: Option<&dyn CoordinatorLog>,
) -> Result<()> {
    let task = snapshot
        .registry
        .find_task(task_id)
        .cloned()
        .ok_or_else(|| MaccError::Validation("Task missing for retry".into()))?;
    let coordinator_tool_override = env_cfg
        .coordinator_tool
        .clone()
        .or_else(|| coordinator_cfg.and_then(|c| c.coordinator_tool.clone()));
    let attempts = env_cfg
        .phase_runner_max_attempts
        .or_else(|| coordinator_cfg.and_then(|c| c.phase_runner_max_attempts))
        .unwrap_or(1)
        .max(1);
    if phase == "review" {
        let verdict = engine.coordinator_run_review_phase_for_task_native(
            &paths.root,
            &task,
            coordinator_tool_override.as_deref(),
            attempts,
            logger,
        )?;
        match verdict {
            Ok(v) => {
                coordinator_engine::apply_review_phase_success_typed(
                    find_task_mut(&mut snapshot.registry, task_id)?,
                    v,
                    &crate::coordinator::helpers::now_iso_coordinator(),
                )?;
            }
            Err(reason) => {
                coordinator_engine::apply_phase_failure_typed(
                    find_task_mut(&mut snapshot.registry, task_id)?,
                    "review",
                    &reason,
                    &crate::coordinator::helpers::now_iso_coordinator(),
                )?;
                return Err(MaccError::Validation(reason));
            }
        }
        return Ok(());
    }
    let result = engine.coordinator_run_phase_for_task_native(
        &paths.root,
        &task,
        phase,
        coordinator_tool_override.as_deref(),
        attempts,
        logger,
    )?;
    match result {
        Ok(_) => {
            let transition = match phase {
                "fix" => coordinator_engine::PhaseTransition {
                    mode: "fix",
                    next_state: WorkflowState::PrOpen,
                    runtime_phase: "fix",
                },
                "integrate" => coordinator_engine::PhaseTransition {
                    mode: "integrate",
                    next_state: WorkflowState::Queued,
                    runtime_phase: "integrate",
                },
                _ => return Ok(()),
            };
            coordinator_engine::apply_phase_success_typed(
                find_task_mut(&mut snapshot.registry, task_id)?,
                transition,
                &crate::coordinator::helpers::now_iso_coordinator(),
            )?;
        }
        Err(reason) => {
            let phase_static = match phase {
                "review" => "review",
                "fix" => "fix",
                "integrate" => "integrate",
                _ => {
                    return Err(MaccError::Validation(format!(
                        "unsupported retry phase '{}'",
                        phase
                    )));
                }
            };
            coordinator_engine::apply_phase_failure_typed(
                find_task_mut(&mut snapshot.registry, task_id)?,
                phase_static,
                &reason,
                &crate::coordinator::helpers::now_iso_coordinator(),
            )?;
            return Err(MaccError::Validation(reason));
        }
    }
    let _ = canonical;
    Ok(())
}

fn find_task_mut<'a>(
    registry: &'a mut crate::coordinator::model::TaskRegistry,
    task_id: &str,
) -> Result<&'a mut crate::coordinator::model::Task> {
    registry
        .find_task_mut(task_id)
        .ok_or_else(|| MaccError::Validation(format!("Task not found in registry: {}", task_id)))
}

fn apply_storage_mode_args(
    args: &mut BTreeMap<String, String>,
    env_cfg: &CoordinatorEnvConfig,
    coordinator_cfg: Option<&CoordinatorConfig>,
) {
    if let Some(value) = env_cfg
        .storage_mode
        .clone()
        .or_else(|| coordinator_cfg.and_then(|c| c.storage_mode.clone()))
    {
        args.insert("storage-mode".to_string(), value);
    }
    if let Some(value) = env_cfg
        .mirror_json_debounce_ms
        .or_else(|| coordinator_cfg.and_then(|c| c.mirror_json_debounce_ms))
    {
        args.insert("mirror-json-debounce-ms".to_string(), value.to_string());
    }
}

fn parse_unlock_args(args: &[String]) -> Result<(Option<String>, Option<String>, bool, String)> {
    let mut task_id = None;
    let mut resource = None;
    let mut clear_all = false;
    let mut unlock_state = "blocked".to_string();
    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--task" => {
                if i + 1 >= args.len() {
                    return Err(MaccError::Validation(
                        "unlock --task requires a value".into(),
                    ));
                }
                task_id = Some(args[i + 1].clone());
                i += 2;
            }
            "--resource" => {
                if i + 1 >= args.len() {
                    return Err(MaccError::Validation(
                        "unlock --resource requires a value".into(),
                    ));
                }
                resource = Some(args[i + 1].clone());
                i += 2;
            }
            "--all" => {
                clear_all = true;
                i += 1;
            }
            "--unlock-state" => {
                if i + 1 >= args.len() {
                    return Err(MaccError::Validation(
                        "unlock --unlock-state requires a value".into(),
                    ));
                }
                unlock_state = args[i + 1].clone();
                i += 2;
            }
            other => {
                return Err(MaccError::Validation(format!(
                    "Unknown unlock arg: {}",
                    other
                )))
            }
        }
    }
    Ok((task_id, resource, clear_all, unlock_state))
}

fn parse_state_args(args: &[String]) -> Result<BTreeMap<String, String>> {
    parse_coordinator_extra_kv_args(args)
}

fn state_required_arg(args: &BTreeMap<String, String>, key: &str) -> Result<String> {
    let value = args
        .get(key)
        .cloned()
        .unwrap_or_default()
        .trim()
        .to_string();
    if value.is_empty() {
        return Err(MaccError::Validation(format!(
            "state command missing required --{}",
            key
        )));
    }
    Ok(value)
}

fn state_optional_arg(args: &BTreeMap<String, String>, key: &str) -> Option<String> {
    args.get(key)
        .cloned()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
}

fn parse_state_apply_transition_command(args: &[String]) -> Result<CoordinatorCommand> {
    let args = parse_state_args(args)?;
    Ok(CoordinatorCommand::StateApplyTransition {
        task_id: state_required_arg(&args, "task-id")?,
        state: state_required_arg(&args, "state")?,
        pr_url: state_optional_arg(&args, "pr-url"),
        reviewer: state_optional_arg(&args, "reviewer"),
        reason: state_optional_arg(&args, "reason"),
    })
}

fn parse_state_set_runtime_command(args: &[String]) -> Result<CoordinatorCommand> {
    let args = parse_state_args(args)?;
    Ok(CoordinatorCommand::StateSetRuntime {
        task_id: state_required_arg(&args, "task-id")?,
        runtime_status: state_required_arg(&args, "runtime-status")?,
        phase: state_optional_arg(&args, "phase"),
        pid: state_optional_arg(&args, "pid").and_then(|v| v.parse::<i64>().ok()),
        last_error: state_optional_arg(&args, "last-error"),
        heartbeat_ts: state_optional_arg(&args, "heartbeat-ts"),
        attempt: state_optional_arg(&args, "attempt").and_then(|v| v.parse::<i64>().ok()),
    })
}

fn parse_state_task_field_command(args: &[String]) -> Result<CoordinatorCommand> {
    let args = parse_state_args(args)?;
    Ok(CoordinatorCommand::StateTaskField {
        task_id: state_required_arg(&args, "task-id")?,
        field: state_required_arg(&args, "field")?,
    })
}

fn parse_state_task_exists_command(args: &[String]) -> Result<CoordinatorCommand> {
    let args = parse_state_args(args)?;
    Ok(CoordinatorCommand::StateTaskExists {
        task_id: state_required_arg(&args, "task-id")?,
    })
}

fn parse_state_locks_command(args: &[String]) -> Result<CoordinatorCommand> {
    let args = parse_state_args(args)?;
    Ok(CoordinatorCommand::StateLocks {
        format: state_optional_arg(&args, "format"),
    })
}

fn parse_state_set_merge_pending_command(args: &[String]) -> Result<CoordinatorCommand> {
    let args = parse_state_args(args)?;
    Ok(CoordinatorCommand::StateSetMergePending {
        task_id: state_required_arg(&args, "task-id")?,
        result_file: state_required_arg(&args, "result-file")?,
        pid: state_optional_arg(&args, "pid").and_then(|v| v.parse::<i64>().ok()),
        now: state_optional_arg(&args, "now"),
    })
}

fn parse_state_set_merge_processed_command(args: &[String]) -> Result<CoordinatorCommand> {
    let args = parse_state_args(args)?;
    Ok(CoordinatorCommand::StateSetMergeProcessed {
        task_id: state_required_arg(&args, "task-id")?,
        result_file: state_optional_arg(&args, "result-file"),
        status: state_optional_arg(&args, "status"),
        rc: state_optional_arg(&args, "rc").and_then(|v| v.parse::<i64>().ok()),
        now: state_optional_arg(&args, "now"),
    })
}

fn parse_state_increment_retries_command(args: &[String]) -> Result<CoordinatorCommand> {
    let args = parse_state_args(args)?;
    Ok(CoordinatorCommand::StateIncrementRetries {
        task_id: state_required_arg(&args, "task-id")?,
        now: state_optional_arg(&args, "now"),
    })
}

fn parse_state_upsert_slo_warning_command(args: &[String]) -> Result<CoordinatorCommand> {
    let args = parse_state_args(args)?;
    Ok(CoordinatorCommand::StateUpsertSloWarning {
        task_id: state_required_arg(&args, "task-id")?,
        metric: state_required_arg(&args, "metric")?,
        threshold: state_required_arg(&args, "threshold")?
            .parse::<i64>()
            .map_err(|e| MaccError::Validation(format!("Invalid --threshold: {}", e)))?,
        value: state_required_arg(&args, "value")?
            .parse::<i64>()
            .map_err(|e| MaccError::Validation(format!("Invalid --value: {}", e)))?,
        suggestion: state_optional_arg(&args, "suggestion"),
        now: state_optional_arg(&args, "now"),
    })
}

fn parse_state_slo_metric_command(args: &[String]) -> Result<CoordinatorCommand> {
    let args = parse_state_args(args)?;
    Ok(CoordinatorCommand::StateSloMetric {
        task_id: state_required_arg(&args, "task-id")?,
        metric: state_required_arg(&args, "metric")?,
    })
}

fn parse_validate_transition_command(args: &[String]) -> Result<CoordinatorCommand> {
    let parsed = WorkflowTransitionArgs::try_from(args)?;
    Ok(CoordinatorCommand::ValidateWorkflowTransition {
        from: parsed.from,
        to: parsed.to,
    })
}

fn parse_validate_runtime_transition_command(args: &[String]) -> Result<CoordinatorCommand> {
    let parsed = RuntimeTransitionArgs::try_from(args)?;
    Ok(CoordinatorCommand::ValidateRuntimeTransition {
        from: parsed.from,
        to: parsed.to,
    })
}

fn parse_runtime_status_from_event_command(args: &[String]) -> Result<CoordinatorCommand> {
    let parsed = RuntimeStatusFromEventArgs::try_from(args)?;
    Ok(CoordinatorCommand::RuntimeStatusFromEvent {
        event_type: parsed.event_type,
        status: parsed.status,
    })
}

fn parse_select_ready_task_command(args: &[String]) -> Result<CoordinatorCommand> {
    let map = parse_coordinator_extra_kv_args(args)?;
    let registry_path = map.get("registry").map(PathBuf::from);
    let max_parallel_raw = map
        .get("max-parallel")
        .cloned()
        .or_else(|| std::env::var("MAX_PARALLEL").ok())
        .unwrap_or_else(|| "0".to_string());
    let default_tool = map
        .get("default-tool")
        .cloned()
        .or_else(|| std::env::var("DEFAULT_TOOL").ok())
        .unwrap_or_else(|| "codex".to_string());
    let default_base_branch = map
        .get("default-base-branch")
        .cloned()
        .or_else(|| std::env::var("DEFAULT_BASE_BRANCH").ok())
        .unwrap_or_else(|| "master".to_string());

    Ok(CoordinatorCommand::SelectReadyTask {
        registry_path,
        config: TaskSelectorConfig {
            enabled_tools: parse_json_string_vec(
                map.get("enabled-tools-json")
                    .map(String::as_str)
                    .unwrap_or("[]"),
                "enabled-tools-json",
            )?,
            tool_priority: parse_json_string_vec(
                map.get("tool-priority-json")
                    .map(String::as_str)
                    .unwrap_or("[]"),
                "tool-priority-json",
            )?,
            max_parallel_per_tool: parse_json_string_usize_map(
                map.get("max-parallel-per-tool-json")
                    .map(String::as_str)
                    .unwrap_or("{}"),
                "max-parallel-per-tool-json",
            )?,
            tool_specializations: parse_json_string_vec_map(
                map.get("tool-specializations-json")
                    .map(String::as_str)
                    .unwrap_or("{}"),
                "tool-specializations-json",
            )?,
            max_parallel: max_parallel_raw
                .parse::<usize>()
                .map_err(|e| MaccError::Validation(format!("Invalid max-parallel value: {}", e)))?,
            default_tool,
            default_base_branch,
        },
    })
}

fn parse_json_string_vec(raw: &str, field_name: &str) -> Result<Vec<String>> {
    let value: serde_json::Value = serde_json::from_str(raw)
        .map_err(|e| MaccError::Validation(format!("Invalid JSON for {}: {}", field_name, e)))?;
    let arr = value
        .as_array()
        .ok_or_else(|| MaccError::Validation(format!("{} must be a JSON array", field_name)))?;
    let mut out = Vec::new();
    for item in arr {
        let value = item.as_str().ok_or_else(|| {
            MaccError::Validation(format!("{} must contain string values only", field_name))
        })?;
        if !value.is_empty() {
            out.push(value.to_string());
        }
    }
    Ok(out)
}

fn parse_json_string_usize_map(
    raw: &str,
    field_name: &str,
) -> Result<std::collections::HashMap<String, usize>> {
    let value: serde_json::Value = serde_json::from_str(raw)
        .map_err(|e| MaccError::Validation(format!("Invalid JSON for {}: {}", field_name, e)))?;
    let obj = value
        .as_object()
        .ok_or_else(|| MaccError::Validation(format!("{} must be a JSON object", field_name)))?;
    let mut out = std::collections::HashMap::new();
    for (k, v) in obj {
        let cap = if let Some(n) = v.as_u64() {
            n as usize
        } else if let Some(s) = v.as_str() {
            s.parse::<usize>().map_err(|e| {
                MaccError::Validation(format!(
                    "Invalid numeric value '{}' for key '{}' in {}: {}",
                    s, k, field_name, e
                ))
            })?
        } else {
            return Err(MaccError::Validation(format!(
                "Invalid value type for key '{}' in {}; expected number/string",
                k, field_name
            )));
        };
        out.insert(k.clone(), cap);
    }
    Ok(out)
}

fn parse_json_string_vec_map(
    raw: &str,
    field_name: &str,
) -> Result<std::collections::HashMap<String, Vec<String>>> {
    let value: serde_json::Value = serde_json::from_str(raw)
        .map_err(|e| MaccError::Validation(format!("Invalid JSON for {}: {}", field_name, e)))?;
    let obj = value
        .as_object()
        .ok_or_else(|| MaccError::Validation(format!("{} must be a JSON object", field_name)))?;
    let mut out = std::collections::HashMap::new();
    for (k, v) in obj {
        let arr = v.as_array().ok_or_else(|| {
            MaccError::Validation(format!(
                "Invalid value type for key '{}' in {}; expected array",
                k, field_name
            ))
        })?;
        let values = arr
            .iter()
            .map(|item| {
                item.as_str().map(|s| s.to_string()).ok_or_else(|| {
                    MaccError::Validation(format!(
                        "Invalid array item for key '{}' in {}; expected string",
                        k, field_name
                    ))
                })
            })
            .collect::<Result<Vec<_>>>()?;
        out.insert(k.clone(), values);
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::{
        coordinator_command_from_name, coordinator_command_invocation, CoordinatorCommand,
    };

    #[test]
    fn retry_phase_action_maps_to_typed_command() {
        let command = coordinator_command_from_name(
            "retry-phase",
            &[
                "--retry-task".to_string(),
                "TASK-123".to_string(),
                "--retry-phase".to_string(),
                "integrate".to_string(),
                "--skip".to_string(),
            ],
            false,
            false,
            false,
        )
        .expect("retry-phase should parse");
        assert_eq!(
            command,
            CoordinatorCommand::RetryTaskPhase {
                task_id: "TASK-123".to_string(),
                phase: "integrate".to_string(),
                skip: true,
            }
        );
    }

    #[test]
    fn storage_sync_verify_maps_to_typed_command() {
        let command = coordinator_command_from_name(
            "storage-sync",
            &["--direction".to_string(), "verify".to_string()],
            false,
            false,
            false,
        )
        .expect("storage-sync should parse");
        assert_eq!(command, CoordinatorCommand::VerifyStorageParity);
    }

    #[test]
    fn unlock_command_invocation_serializes_args() {
        let invocation = coordinator_command_invocation(&CoordinatorCommand::Unlock {
            task_id: Some("TASK-1".to_string()),
            resource: None,
            clear_all: false,
            unlock_state: "queued".to_string(),
        })
        .expect("unlock command should serialize");
        assert_eq!(invocation.action, "unlock");
        assert_eq!(
            invocation.args,
            vec![
                "--task".to_string(),
                "TASK-1".to_string(),
                "--unlock-state".to_string(),
                "queued".to_string()
            ]
        );
    }

    #[test]
    fn state_set_runtime_action_maps_to_typed_command() {
        let command = coordinator_command_from_name(
            "state-set-runtime",
            &[
                "--task-id".to_string(),
                "TASK-9".to_string(),
                "--runtime-status".to_string(),
                "running".to_string(),
                "--phase".to_string(),
                "review".to_string(),
                "--pid".to_string(),
                "1234".to_string(),
            ],
            false,
            false,
            false,
        )
        .expect("state-set-runtime should parse");
        assert_eq!(
            command,
            CoordinatorCommand::StateSetRuntime {
                task_id: "TASK-9".to_string(),
                runtime_status: "running".to_string(),
                phase: Some("review".to_string()),
                pid: Some(1234),
                last_error: None,
                heartbeat_ts: None,
                attempt: None,
            }
        );
    }

    #[test]
    fn state_counts_action_maps_to_typed_command() {
        let command = coordinator_command_from_name("state-counts", &[], false, false, false)
            .expect("state-counts should parse");
        assert_eq!(command, CoordinatorCommand::StateCounts);
    }

    #[test]
    fn validate_transition_action_maps_to_typed_command() {
        let command = coordinator_command_from_name(
            "validate-transition",
            &[
                "--from".to_string(),
                "todo".to_string(),
                "--to".to_string(),
                "claimed".to_string(),
            ],
            false,
            false,
            false,
        )
        .expect("validate-transition should parse");
        assert_eq!(
            command,
            CoordinatorCommand::ValidateWorkflowTransition {
                from: crate::coordinator::WorkflowState::Todo,
                to: crate::coordinator::WorkflowState::Claimed,
            }
        );
    }

    #[test]
    fn runtime_status_from_event_action_maps_to_typed_command() {
        let command = coordinator_command_from_name(
            "runtime-status-from-event",
            &[
                "--type".to_string(),
                "phase_result".to_string(),
                "--status".to_string(),
                "done".to_string(),
            ],
            false,
            false,
            false,
        )
        .expect("runtime-status-from-event should parse");
        assert_eq!(
            command,
            CoordinatorCommand::RuntimeStatusFromEvent {
                event_type: "phase_result".to_string(),
                status: "done".to_string(),
            }
        );
    }

    #[test]
    fn select_ready_task_action_maps_to_typed_command() {
        let command = coordinator_command_from_name(
            "select-ready-task",
            &[
                "--max-parallel".to_string(),
                "3".to_string(),
                "--default-tool".to_string(),
                "codex".to_string(),
            ],
            false,
            false,
            false,
        )
        .expect("select-ready-task should parse");
        match command {
            CoordinatorCommand::SelectReadyTask { config, .. } => {
                assert_eq!(config.max_parallel, 3);
                assert_eq!(config.default_tool, "codex");
            }
            other => panic!("unexpected command: {:?}", other),
        }
    }
}
