use super::errors::ApiError;
use super::WebState;
use axum::extract::{Path, State};
use axum::Json;
use macc_core::coordinator::task_selector::SelectedTask;
use macc_core::coordinator::types::CoordinatorEnvConfig;
use macc_core::service::coordinator_workflow::{
    CoordinatorCommand, CoordinatorCommandRequest, CoordinatorCommandResult, CoordinatorStatus,
    ThrottledToolStatus, PsProcessEntry, RecoveryReportEntry,
};
use macc_core::service::diagnostic::{FailureKind, FailureReport};
use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize)]
pub(super) struct ApiCoordinatorStatus {
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
    pub failure_report: Option<ApiFailureReport>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub throttled_tools: Vec<ApiThrottledToolStatus>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub effective_max_parallel: Option<usize>,
}

#[derive(Debug, Serialize)]
pub(super) struct ApiThrottledToolStatus {
    pub tool_id: String,
    pub throttled_until: String,
    pub consecutive_count: usize,
}

impl From<ThrottledToolStatus> for ApiThrottledToolStatus {
    fn from(s: ThrottledToolStatus) -> Self {
        Self {
            tool_id: s.tool_id,
            throttled_until: s.throttled_until,
            consecutive_count: s.consecutive_count,
        }
    }
}

impl From<CoordinatorStatus> for ApiCoordinatorStatus {
    fn from(status: CoordinatorStatus) -> Self {
        Self {
            total: status.total,
            todo: status.todo,
            active: status.active,
            blocked: status.blocked,
            merged: status.merged,
            paused: status.paused,
            pause_reason: status.pause_reason,
            pause_task_id: status.pause_task_id,
            pause_phase: status.pause_phase,
            latest_error: status.latest_error,
            failure_report: status.failure_report.map(ApiFailureReport::from),
            throttled_tools: status
                .throttled_tools
                .into_iter()
                .map(ApiThrottledToolStatus::from)
                .collect(),
            effective_max_parallel: status.effective_max_parallel,
        }
    }
}

#[derive(Debug, Serialize)]
pub(super) struct ApiCoordinatorCommandResult {
    pub status: Option<ApiCoordinatorStatus>,
    pub resumed: Option<bool>,
    pub aggregated_performer_logs: Option<usize>,
    pub runtime_status: Option<String>,
    pub exported_events_path: Option<String>,
    pub removed_worktrees: Option<usize>,
    pub selected_task: Option<ApiSelectedTask>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_cooldowns: Option<Vec<ApiToolCooldownEntry>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub processes: Option<Vec<ApiPsProcessEntry>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub recovery_report: Option<Vec<ApiRecoveryReportEntry>>,
}

impl From<CoordinatorCommandResult> for ApiCoordinatorCommandResult {
    fn from(result: CoordinatorCommandResult) -> Self {
        Self {
            status: result.status.map(ApiCoordinatorStatus::from),
            resumed: result.resumed,
            aggregated_performer_logs: result.aggregated_performer_logs,
            runtime_status: result.runtime_status,
            exported_events_path: result
                .exported_events_path
                .map(|path| path.to_string_lossy().into_owned()),
            removed_worktrees: result.removed_worktrees,
            selected_task: result.selected_task.map(ApiSelectedTask::from),
            tool_cooldowns: result.tool_cooldowns.map(|entries| {
                entries
                    .into_iter()
                    .map(ApiToolCooldownEntry::from)
                    .collect()
            }),
            processes: result.processes.map(|list| {
                list.into_iter().map(ApiPsProcessEntry::from).collect()
            }),
            recovery_report: result.recovery_report.map(|list| {
                list.into_iter().map(ApiRecoveryReportEntry::from).collect()
            }),
        }
    }
}

#[derive(Debug, Serialize)]
pub(super) struct ApiToolCooldownEntry {
    pub tool_id: String,
    pub throttled_until: u64,
    pub remaining_seconds: i64,
    pub backoff_seconds: u64,
}

#[derive(Debug, Deserialize)]
pub(super) struct ApiToolCooldownSetRequest {
    pub tool: String,
    pub duration_seconds: u64,
}

impl From<macc_core::service::coordinator_workflow::ToolCooldownEntry> for ApiToolCooldownEntry {
    fn from(e: macc_core::service::coordinator_workflow::ToolCooldownEntry) -> Self {
        Self {
            tool_id: e.tool_id,
            throttled_until: e.throttled_until,
            remaining_seconds: e.remaining_seconds,
            backoff_seconds: e.backoff_seconds,
        }
    }
}

#[derive(Debug, Serialize)]
pub(super) struct ApiSelectedTask {
    pub id: String,
    pub title: String,
    pub tool: String,
    pub base_branch: String,
}

impl From<SelectedTask> for ApiSelectedTask {
    fn from(task: SelectedTask) -> Self {
        Self {
            id: task.id,
            title: task.title,
            tool: task.tool,
            base_branch: task.base_branch,
        }
    }
}

#[derive(Debug, Serialize)]
pub(super) struct ApiFailureReport {
    pub message: String,
    pub task_id: Option<String>,
    pub phase: Option<String>,
    pub source: String,
    pub blocking: bool,
    pub event_type: Option<String>,
    pub kind: String,
    pub suggested_fixes: Vec<String>,
}

impl From<FailureReport> for ApiFailureReport {
    fn from(report: FailureReport) -> Self {
        Self {
            message: report.message,
            task_id: report.task_id,
            phase: report.phase,
            source: report.source,
            blocking: report.blocking,
            event_type: report.event_type,
            kind: map_failure_kind(&report.kind).to_string(),
            suggested_fixes: report.suggested_fixes,
        }
    }
}

fn map_failure_kind(kind: &FailureKind) -> &'static str {
    match kind {
        FailureKind::ProcessError => "ProcessError",
        FailureKind::ConfigurationError => "ConfigurationError",
        FailureKind::InternalError => "InternalError",
    }
}

pub(super) async fn status_handler(
    State(state): State<WebState>,
) -> std::result::Result<Json<ApiCoordinatorStatus>, ApiError> {
    let paths = state.paths.clone();
    let engine = state.engine.clone();
    let status = tokio::task::spawn_blocking(move || engine.get_coordinator_status(&paths))
        .await
        .map_err(|e| ApiError::validation(e.to_string()))??;
    Ok(Json(ApiCoordinatorStatus::from(status)))
}

pub(super) async fn coordinator_run_handler(
    State(state): State<WebState>,
    headers: axum::http::HeaderMap,
) -> std::result::Result<Json<ApiCoordinatorCommandResult>, ApiError> {
    crate::commands::web::mutation_gate::require_project_owner(&state, &headers)?;
    let _ = state.engine.project_ensure_coordinator_run_id();
    let paths = state.paths.clone();
    let engine = state.engine.clone();
    // Start the coordinator subprocess and return immediately.
    // The coordinator is a long-running process; callers monitor progress via SSE.
    tokio::task::spawn_blocking(move || {
        engine.coordinator_start_managed_command_process(&paths, &CoordinatorCommand::Run, None)
    })
    .await
    .map_err(|e| ApiError::validation(e.to_string()))??;
    Ok(Json(ApiCoordinatorCommandResult::from(
        CoordinatorCommandResult::default(),
    )))
}

pub(super) async fn coordinator_stop_handler(
    State(state): State<WebState>,
    headers: axum::http::HeaderMap,
    req: Option<Json<ApiStopRequest>>,
) -> std::result::Result<Json<ApiCoordinatorCommandResult>, ApiError> {
    crate::commands::web::mutation_gate::require_project_owner(&state, &headers)?;
    let paths = state.paths.clone();
    let engine = state.engine.clone();

    let req = req.map(|Json(r)| r).unwrap_or_default();
    let mode = req.mode.as_deref().unwrap_or("graceful");
    let drain = mode == "drain";
    let graceful = mode == "graceful" || mode == "drain";
    let force = mode == "force" || mode == "force_cleanup";
    let remove_worktrees = req.cleanup_worktrees.unwrap_or(false) || mode == "force_cleanup";
    let reason = req.reason.clone().unwrap_or_else(|| "web api stop".to_string());

    if force {
        let expected_confirm = if remove_worktrees { "FORCE CLEANUP" } else { "FORCE" };
        if req.confirm.as_deref() != Some(expected_confirm) {
            let msg = if remove_worktrees {
                "Confirmation required. Type FORCE CLEANUP to terminate active tools and clean MACC-managed worktrees."
            } else {
                "Confirmation required. This will terminate all active MACC-launched tool processes. Worktrees will be preserved for inspection."
            };
            return Err(ApiError::confirmation_required(msg, None));
        }
    }

    let start_time = std::time::Instant::now();
    let result = tokio::task::spawn_blocking(move || {
        let env_cfg = CoordinatorEnvConfig::default();
        engine.coordinator_execute_command(
            &paths,
            CoordinatorCommand::Stop {
                drain,
                graceful,
                force,
                remove_worktrees,
                remove_branches: remove_worktrees,
                reason,
            },
            CoordinatorCommandRequest {
                canonical: None,
                coordinator_cfg: None,
                env_cfg: &env_cfg,
                logger: None,
            },
        )
    })
    .await;

    let duration_ms = start_time.elapsed().as_millis() as u64;
    let status_str = match &result {
        Ok(Ok(_)) => "ok",
        _ => "error",
    };

    let record = crate::commands::web::audit::StopAuditRecord {
        timestamp: chrono::Utc::now().to_rfc3339(),
        client: "web",
        action: "coordinator.stop",
        mode: mode.to_string(),
        cleanup_worktrees: remove_worktrees,
        status: status_str.to_string(),
        duration_ms,
    };
    let _ = crate::commands::web::audit::append_stop_record(&state, &record).await;

    let execute_res = result.map_err(|e| ApiError::validation(e.to_string()))??;
    Ok(Json(ApiCoordinatorCommandResult::from(execute_res)))
}

pub(super) async fn coordinator_cleanup_handler(
    State(state): State<WebState>,
    headers: axum::http::HeaderMap,
) -> std::result::Result<Json<ApiCoordinatorCommandResult>, ApiError> {
    crate::commands::web::mutation_gate::require_project_owner(&state, &headers)?;
    let paths = state.paths.clone();
    let engine = state.engine.clone();
    tokio::task::spawn_blocking(move || engine.coordinator_cleanup(&paths))
        .await
        .map_err(|e| ApiError::validation(e.to_string()))??;
    Ok(Json(ApiCoordinatorCommandResult::from(
        CoordinatorCommandResult::default(),
    )))
}

pub(super) async fn coordinator_dispatch_handler(
    State(state): State<WebState>,
    headers: axum::http::HeaderMap,
) -> std::result::Result<Json<ApiCoordinatorCommandResult>, ApiError> {
    crate::commands::web::mutation_gate::require_project_owner(&state, &headers)?;
    let paths = state.paths.clone();
    let engine = state.engine.clone();
    let result = tokio::task::spawn_blocking(move || {
        let canonical = engine.load_canonical_config(&paths)?;
        let env_cfg = CoordinatorEnvConfig::default();
        engine.coordinator_execute_command(
            &paths,
            CoordinatorCommand::DispatchReadyTasks,
            CoordinatorCommandRequest {
                canonical: Some(&canonical),
                coordinator_cfg: None,
                env_cfg: &env_cfg,
                logger: None,
            },
        )
    })
    .await
    .map_err(|e| ApiError::validation(e.to_string()))??;
    Ok(Json(ApiCoordinatorCommandResult::from(result)))
}

pub(super) async fn coordinator_advance_handler(
    State(state): State<WebState>,
    headers: axum::http::HeaderMap,
) -> std::result::Result<Json<ApiCoordinatorCommandResult>, ApiError> {
    crate::commands::web::mutation_gate::require_project_owner(&state, &headers)?;
    let paths = state.paths.clone();
    let engine = state.engine.clone();
    let result = tokio::task::spawn_blocking(move || {
        let env_cfg = CoordinatorEnvConfig::default();
        engine.coordinator_execute_command(
            &paths,
            CoordinatorCommand::AdvanceTasks,
            CoordinatorCommandRequest {
                canonical: None,
                coordinator_cfg: None,
                env_cfg: &env_cfg,
                logger: None,
            },
        )
    })
    .await
    .map_err(|e| ApiError::validation(e.to_string()))??;
    Ok(Json(ApiCoordinatorCommandResult::from(result)))
}

pub(super) async fn coordinator_reconcile_handler(
    State(state): State<WebState>,
    headers: axum::http::HeaderMap,
) -> std::result::Result<Json<ApiCoordinatorCommandResult>, ApiError> {
    crate::commands::web::mutation_gate::require_project_owner(&state, &headers)?;
    let paths = state.paths.clone();
    let engine = state.engine.clone();
    let result = tokio::task::spawn_blocking(move || {
        let env_cfg = CoordinatorEnvConfig::default();
        engine.coordinator_execute_command(
            &paths,
            CoordinatorCommand::ReconcileRuntime,
            CoordinatorCommandRequest {
                canonical: None,
                coordinator_cfg: None,
                env_cfg: &env_cfg,
                logger: None,
            },
        )
    })
    .await
    .map_err(|e| ApiError::validation(e.to_string()))??;
    Ok(Json(ApiCoordinatorCommandResult::from(result)))
}

pub(super) async fn coordinator_resume_handler(
    State(state): State<WebState>,
    headers: axum::http::HeaderMap,
) -> std::result::Result<Json<ApiCoordinatorCommandResult>, ApiError> {
    crate::commands::web::mutation_gate::require_project_owner(&state, &headers)?;
    let paths = state.paths.clone();
    let engine = state.engine.clone();
    let result = tokio::task::spawn_blocking(move || {
        let was_paused = engine.get_coordinator_status(&paths)?.paused;
        engine.coordinator_resume(&paths.root)?;
        Ok::<_, macc_core::MaccError>(CoordinatorCommandResult {
            resumed: Some(was_paused),
            ..CoordinatorCommandResult::default()
        })
    })
    .await
    .map_err(|e| ApiError::validation(e.to_string()))??;
    Ok(Json(ApiCoordinatorCommandResult::from(result)))
}

pub(super) async fn coordinator_sync_handler(
    State(state): State<WebState>,
    headers: axum::http::HeaderMap,
) -> std::result::Result<Json<ApiCoordinatorCommandResult>, ApiError> {
    crate::commands::web::mutation_gate::require_project_owner(&state, &headers)?;
    let paths = state.paths.clone();
    let engine = state.engine.clone();
    let result = tokio::task::spawn_blocking(move || {
        let env_cfg = CoordinatorEnvConfig::default();
        engine.coordinator_execute_command(
            &paths,
            CoordinatorCommand::SyncRegistry,
            CoordinatorCommandRequest {
                canonical: None,
                coordinator_cfg: None,
                env_cfg: &env_cfg,
                logger: None,
            },
        )
    })
    .await
    .map_err(|e| ApiError::validation(e.to_string()))??;
    Ok(Json(ApiCoordinatorCommandResult::from(result)))
}

pub(super) async fn coordinator_audit_prd_handler(
    State(state): State<WebState>,
    headers: axum::http::HeaderMap,
) -> std::result::Result<Json<ApiCoordinatorCommandResult>, ApiError> {
    crate::commands::web::mutation_gate::require_project_owner(&state, &headers)?;
    let paths = state.paths.clone();
    let engine = state.engine.clone();
    let result = tokio::task::spawn_blocking(move || {
        let env_cfg = CoordinatorEnvConfig::default();
        engine.coordinator_execute_command(
            &paths,
            CoordinatorCommand::AuditPrd {
                tool: None,
                dry_run: false,
            },
            CoordinatorCommandRequest {
                canonical: None,
                coordinator_cfg: None,
                env_cfg: &env_cfg,
                logger: None,
            },
        )
    })
    .await
    .map_err(|e| ApiError::validation(e.to_string()))??;
    Ok(Json(ApiCoordinatorCommandResult::from(result)))
}

pub(super) async fn get_tool_cooldowns_handler(
    State(state): State<WebState>,
) -> std::result::Result<Json<ApiCoordinatorCommandResult>, ApiError> {
    let paths = state.paths.clone();
    let engine = state.engine.clone();
    let result = tokio::task::spawn_blocking(move || {
        let env_cfg = CoordinatorEnvConfig::default();
        engine.coordinator_execute_command(
            &paths,
            CoordinatorCommand::ToolCooldownList,
            CoordinatorCommandRequest {
                canonical: None,
                coordinator_cfg: None,
                env_cfg: &env_cfg,
                logger: None,
            },
        )
    })
    .await
    .map_err(|e| ApiError::validation(e.to_string()))??;
    Ok(Json(ApiCoordinatorCommandResult::from(result)))
}

pub(super) async fn set_tool_cooldown_handler(
    State(state): State<WebState>,
    headers: axum::http::HeaderMap,
    Json(request): Json<ApiToolCooldownSetRequest>,
) -> std::result::Result<Json<ApiCoordinatorCommandResult>, ApiError> {
    crate::commands::web::mutation_gate::require_project_owner(&state, &headers)?;
    let env_cfg = CoordinatorEnvConfig::default();
    let result = state
        .engine
        .coordinator_execute_command(
            &state.paths,
            CoordinatorCommand::ToolCooldownSet {
                tool: request.tool,
                duration: request.duration_seconds,
            },
            CoordinatorCommandRequest {
                canonical: None,
                coordinator_cfg: None,
                env_cfg: &env_cfg,
                logger: None,
            },
        )
        .map_err(ApiError::from)?;
    Ok(Json(ApiCoordinatorCommandResult::from(result)))
}

pub(super) async fn clear_tool_cooldown_handler(
    State(state): State<WebState>,
    Path(tool): Path<String>,
    headers: axum::http::HeaderMap,
) -> std::result::Result<Json<ApiCoordinatorCommandResult>, ApiError> {
    crate::commands::web::mutation_gate::require_project_owner(&state, &headers)?;
    let env_cfg = CoordinatorEnvConfig::default();
    let result = state
        .engine
        .coordinator_execute_command(
            &state.paths,
            CoordinatorCommand::ToolCooldownClear { tool },
            CoordinatorCommandRequest {
                canonical: None,
                coordinator_cfg: None,
                env_cfg: &env_cfg,
                logger: None,
            },
        )
        .map_err(ApiError::from)?;
    Ok(Json(ApiCoordinatorCommandResult::from(result)))
}

#[derive(Debug, Deserialize, Default)]
pub(super) struct ApiStopRequest {
    pub mode: Option<String>,
    pub cleanup_worktrees: Option<bool>,
    pub force_grace_seconds: Option<u64>,
    pub reason: Option<String>,
    pub confirm: Option<String>,
}

#[derive(Debug, Deserialize, Default)]
pub(super) struct ApiRecoverRequest {
    pub dry_run: Option<bool>,
}

#[derive(Debug, Serialize)]
pub(super) struct ApiPsProcessEntry {
    pub task_id: String,
    pub claim_id: String,
    pub tool: String,
    pub pid: i64,
    pub pgid: i64,
    pub status: String,
    pub heartbeat: String,
    pub worktree: String,
}

impl From<PsProcessEntry> for ApiPsProcessEntry {
    fn from(p: PsProcessEntry) -> Self {
        Self {
            task_id: p.task_id,
            claim_id: p.claim_id,
            tool: p.tool,
            pid: p.pid,
            pgid: p.pgid,
            status: p.status,
            heartbeat: p.heartbeat,
            worktree: p.worktree,
        }
    }
}

#[derive(Debug, Serialize)]
pub(super) struct ApiRecoveryReportEntry {
    pub task_id: String,
    pub situation: String,
    pub classification: String,
    pub action: String,
    pub mutated: bool,
}

impl From<RecoveryReportEntry> for ApiRecoveryReportEntry {
    fn from(r: RecoveryReportEntry) -> Self {
        Self {
            task_id: r.task_id,
            situation: r.situation,
            classification: r.classification,
            action: r.action,
            mutated: r.mutated,
        }
    }
}

pub(super) async fn coordinator_processes_handler(
    State(state): State<WebState>,
) -> std::result::Result<Json<ApiCoordinatorCommandResult>, ApiError> {
    let paths = state.paths.clone();
    let engine = state.engine.clone();

    let result = tokio::task::spawn_blocking(move || {
        let env_cfg = CoordinatorEnvConfig::default();
        engine.coordinator_execute_command(
            &paths,
            CoordinatorCommand::Ps,
            CoordinatorCommandRequest {
                canonical: None,
                coordinator_cfg: None,
                env_cfg: &env_cfg,
                logger: None,
            },
        )
    })
    .await
    .map_err(|e| ApiError::validation(e.to_string()))??;

    Ok(Json(ApiCoordinatorCommandResult::from(result)))
}

pub(super) async fn coordinator_recover_handler(
    State(state): State<WebState>,
    headers: axum::http::HeaderMap,
    req: Option<Json<ApiRecoverRequest>>,
) -> std::result::Result<Json<ApiCoordinatorCommandResult>, ApiError> {
    crate::commands::web::mutation_gate::require_project_owner(&state, &headers)?;
    let paths = state.paths.clone();
    let engine = state.engine.clone();
    let req = req.map(|Json(r)| r).unwrap_or_default();
    let dry_run = req.dry_run.unwrap_or(false);

    let result = tokio::task::spawn_blocking(move || {
        let env_cfg = CoordinatorEnvConfig::default();
        engine.coordinator_execute_command(
            &paths,
            CoordinatorCommand::Recover { dry_run },
            CoordinatorCommandRequest {
                canonical: None,
                coordinator_cfg: None,
                env_cfg: &env_cfg,
                logger: None,
            },
        )
    })
    .await
    .map_err(|e| ApiError::validation(e.to_string()))??;

    Ok(Json(ApiCoordinatorCommandResult::from(result)))
}

