use super::errors::ApiError;
use super::WebState;
use axum::extract::{Path, State};
use axum::Json;
use macc_core::coordinator::task_selector::SelectedTask;
use macc_core::coordinator::types::CoordinatorEnvConfig;
use macc_core::load_canonical_config;
use macc_core::service::coordinator_workflow::{
    CoordinatorCommand, CoordinatorCommandRequest, CoordinatorCommandResult, CoordinatorStatus,
    ThrottledToolStatus,
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
    let status = state
        .engine
        .get_coordinator_status(&state.paths)
        .map_err(ApiError::from)?;
    Ok(Json(ApiCoordinatorStatus::from(status)))
}

pub(super) async fn coordinator_run_handler(
    State(state): State<WebState>,
) -> std::result::Result<Json<ApiCoordinatorCommandResult>, ApiError> {
    let env_cfg = CoordinatorEnvConfig::default();
    let _ = state.engine.project_ensure_coordinator_run_id();
    let result = state
        .engine
        .coordinator_execute_command(
            &state.paths,
            CoordinatorCommand::Run,
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

pub(super) async fn coordinator_stop_handler(
    State(state): State<WebState>,
) -> std::result::Result<Json<ApiCoordinatorCommandResult>, ApiError> {
    state
        .engine
        .coordinator_stop(&state.paths.root, "web api stop")
        .map_err(ApiError::from)?;
    Ok(Json(ApiCoordinatorCommandResult::from(
        CoordinatorCommandResult::default(),
    )))
}

pub(super) async fn coordinator_cleanup_handler(
    State(state): State<WebState>,
) -> std::result::Result<Json<ApiCoordinatorCommandResult>, ApiError> {
    state
        .engine
        .coordinator_cleanup(&state.paths)
        .map_err(ApiError::from)?;
    Ok(Json(ApiCoordinatorCommandResult::from(
        CoordinatorCommandResult::default(),
    )))
}

pub(super) async fn coordinator_dispatch_handler(
    State(state): State<WebState>,
) -> std::result::Result<Json<ApiCoordinatorCommandResult>, ApiError> {
    let canonical = load_canonical_config(&state.paths.config_path).map_err(ApiError::from)?;
    let env_cfg = CoordinatorEnvConfig::default();
    let result = state
        .engine
        .coordinator_execute_command(
            &state.paths,
            CoordinatorCommand::DispatchReadyTasks,
            CoordinatorCommandRequest {
                canonical: Some(&canonical),
                coordinator_cfg: None,
                env_cfg: &env_cfg,
                logger: None,
            },
        )
        .map_err(ApiError::from)?;
    Ok(Json(ApiCoordinatorCommandResult::from(result)))
}

pub(super) async fn coordinator_advance_handler(
    State(state): State<WebState>,
) -> std::result::Result<Json<ApiCoordinatorCommandResult>, ApiError> {
    let env_cfg = CoordinatorEnvConfig::default();
    let result = state
        .engine
        .coordinator_execute_command(
            &state.paths,
            CoordinatorCommand::AdvanceTasks,
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

pub(super) async fn coordinator_reconcile_handler(
    State(state): State<WebState>,
) -> std::result::Result<Json<ApiCoordinatorCommandResult>, ApiError> {
    let env_cfg = CoordinatorEnvConfig::default();
    let result = state
        .engine
        .coordinator_execute_command(
            &state.paths,
            CoordinatorCommand::ReconcileRuntime,
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

pub(super) async fn coordinator_resume_handler(
    State(state): State<WebState>,
) -> std::result::Result<Json<ApiCoordinatorCommandResult>, ApiError> {
    let was_paused =
        macc_core::coordinator::state_runtime::read_coordinator_pause_file(&state.paths.root)
            .map_err(ApiError::from)?
            .is_some();
    state
        .engine
        .coordinator_resume(&state.paths.root)
        .map_err(ApiError::from)?;
    Ok(Json(ApiCoordinatorCommandResult::from(
        CoordinatorCommandResult {
            resumed: Some(was_paused),
            ..CoordinatorCommandResult::default()
        },
    )))
}

pub(super) async fn coordinator_sync_handler(
    State(state): State<WebState>,
) -> std::result::Result<Json<ApiCoordinatorCommandResult>, ApiError> {
    let env_cfg = CoordinatorEnvConfig::default();
    let result = state
        .engine
        .coordinator_execute_command(
            &state.paths,
            CoordinatorCommand::SyncRegistry,
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

pub(super) async fn coordinator_audit_prd_handler(
    State(state): State<WebState>,
) -> std::result::Result<Json<ApiCoordinatorCommandResult>, ApiError> {
    let env_cfg = CoordinatorEnvConfig::default();
    let result = state
        .engine
        .coordinator_execute_command(
            &state.paths,
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
        .map_err(ApiError::from)?;
    Ok(Json(ApiCoordinatorCommandResult::from(result)))
}

pub(super) async fn get_tool_cooldowns_handler(
    State(state): State<WebState>,
) -> std::result::Result<Json<ApiCoordinatorCommandResult>, ApiError> {
    let env_cfg = CoordinatorEnvConfig::default();
    let result = state
        .engine
        .coordinator_execute_command(
            &state.paths,
            CoordinatorCommand::ToolCooldownList,
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

pub(super) async fn set_tool_cooldown_handler(
    State(state): State<WebState>,
    Json(request): Json<ApiToolCooldownSetRequest>,
) -> std::result::Result<Json<ApiCoordinatorCommandResult>, ApiError> {
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
) -> std::result::Result<Json<ApiCoordinatorCommandResult>, ApiError> {
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
