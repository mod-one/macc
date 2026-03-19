use crate::commands::AppContext;
use crate::commands::Command;
use crate::services::engine_provider::SharedEngine;
use async_stream::stream;
use axum::body::Body;
use axum::extract::State;
use axum::http::header::{self, HeaderValue};
use axum::http::{HeaderMap, Method, StatusCode, Uri};
use axum::response::sse::{Event, Sse};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::Json;
use axum::Router;
use macc_core::config::WebAssetsMode;
use macc_core::coordinator::task_selector::SelectedTask;
use macc_core::coordinator::types::CoordinatorEnvConfig;
use macc_core::coordinator::COORDINATOR_EVENT_SCHEMA_VERSION;
use macc_core::engine::CoordinatorEvent;
use macc_core::service::coordinator_workflow::{
    CoordinatorCommand, CoordinatorCommandRequest, CoordinatorCommandResult, CoordinatorStatus,
    ThrottledToolStatus,
};
use macc_core::service::diagnostic::{FailureKind, FailureReport};
use macc_core::{load_canonical_config, MaccError, ProjectPaths, Result};
#[cfg(not(any(test, clippy)))]
use rust_embed::RustEmbed;
use serde::Serialize;
use std::collections::VecDeque;
use std::convert::Infallible;
use std::ffi::OsStr;
use std::net::{IpAddr, SocketAddr};
use std::path::Path;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

pub struct WebCommand {
    app: AppContext,
    host: String,
    port: Option<u16>,
    assets_mode: Option<WebAssetsMode>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct WebServerConfig {
    host: IpAddr,
    port: u16,
    assets_mode: WebAssetsMode,
}

impl WebCommand {
    pub fn new(
        app: AppContext,
        host: String,
        port: Option<u16>,
        assets_mode: Option<WebAssetsMode>,
    ) -> Self {
        Self {
            app,
            host,
            port,
            assets_mode,
        }
    }
}

impl Command for WebCommand {
    fn run(&self) -> Result<()> {
        let config = self.server_config()?;
        let state = WebState {
            engine: self.app.engine.clone(),
            paths: self.app.project_paths()?,
            assets_mode: config.assets_mode,
        };
        let app = build_web_router(state);

        println!("Web server starting on http://{}...", config.bind_addr());

        let runtime = tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .build()
            .map_err(|e| MaccError::Validation(format!("build web runtime: {}", e)))?;

        runtime.block_on(async move {
            let addr = config.bind_addr();
            let listener =
                tokio::net::TcpListener::bind(addr)
                    .await
                    .map_err(|e| MaccError::Io {
                        path: addr.to_string(),
                        action: "bind web server".into(),
                        source: e,
                    })?;
            axum::serve(listener, app)
                .await
                .map_err(|e| MaccError::Validation(format!("web server failed: {}", e)))
        })?;

        Ok(())
    }
}

impl WebCommand {
    fn server_config(&self) -> Result<WebServerConfig> {
        let canonical = self.app.canonical_config()?;
        let host = self.host.parse::<IpAddr>().map_err(|e| {
            MaccError::Validation(format!("invalid web host '{}': {}", self.host, e))
        })?;
        Ok(WebServerConfig {
            host,
            port: self
                .port
                .unwrap_or(canonical.settings.web_port.unwrap_or(3450)),
            assets_mode: self.assets_mode.unwrap_or_else(|| {
                canonical
                    .settings
                    .web_assets
                    .unwrap_or_else(default_web_assets_mode)
            }),
        })
    }
}

impl WebServerConfig {
    fn bind_addr(self) -> SocketAddr {
        SocketAddr::new(self.host, self.port)
    }
}

#[derive(Clone)]
struct WebState {
    engine: SharedEngine,
    paths: ProjectPaths,
    assets_mode: WebAssetsMode,
}

const SSE_POLL_INTERVAL: Duration = Duration::from_millis(250);
const SSE_HEARTBEAT_INTERVAL: Duration = Duration::from_secs(5);

#[cfg(debug_assertions)]
fn default_web_assets_mode() -> WebAssetsMode {
    WebAssetsMode::Dist
}

#[cfg(not(debug_assertions))]
fn default_web_assets_mode() -> WebAssetsMode {
    WebAssetsMode::Embedded
}

#[cfg(not(any(test, clippy)))]
#[derive(RustEmbed)]
#[folder = "../web/dist"]
struct EmbeddedWebAssets;

#[cfg(not(any(test, clippy)))]
fn web_asset(path: &str) -> Option<Vec<u8>> {
    EmbeddedWebAssets::get(path).map(|asset| asset.data.into_owned())
}

#[cfg(test)]
fn web_asset_paths() -> impl Iterator<Item = &'static str> {
    TEST_WEB_ASSETS.iter().map(|(path, _)| *path)
}

#[cfg(any(test, clippy))]
fn web_asset(path: &str) -> Option<Vec<u8>> {
    TEST_WEB_ASSETS
        .iter()
        .find_map(|(candidate, body)| (*candidate == path).then(|| body.as_bytes().to_vec()))
}

#[cfg(any(test, clippy))]
const TEST_WEB_ASSETS: &[(&str, &str)] = &[
    (
        "index.html",
        "<!doctype html><html><body>macc web</body></html>",
    ),
    ("assets/app.js", "console.log('macc');"),
];

fn build_web_router(state: WebState) -> Router {
    Router::new()
        .route("/api/v1/health", get(health_handler))
        .route("/api/v1/status", get(status_handler))
        .route("/api/v1/events", get(events_handler))
        .route("/api/v1/coordinator/run", post(coordinator_run_handler))
        .route(
            "/api/v1/coordinator/dispatch",
            post(coordinator_dispatch_handler),
        )
        .route(
            "/api/v1/coordinator/advance",
            post(coordinator_advance_handler),
        )
        .route(
            "/api/v1/coordinator/reconcile",
            post(coordinator_reconcile_handler),
        )
        .route(
            "/api/v1/coordinator/cleanup",
            post(coordinator_cleanup_handler),
        )
        .route("/api/v1/coordinator/stop", post(coordinator_stop_handler))
        .route(
            "/api/v1/coordinator/resume",
            post(coordinator_resume_handler),
        )
        .fallback(get(spa_handler))
        .with_state(state)
}

async fn spa_handler(State(state): State<WebState>, uri: Uri) -> Response {
    let asset_path = uri.path().trim_start_matches('/');
    let asset_path = if asset_path.is_empty() {
        "index.html"
    } else {
        asset_path
    };

    asset_response(&state, asset_path)
        .or_else(|| asset_response(&state, "index.html"))
        .unwrap_or_else(|| StatusCode::NOT_FOUND.into_response())
}

fn asset_response(state: &WebState, path: &str) -> Option<Response> {
    let asset = match state.assets_mode {
        WebAssetsMode::Dist => dist_asset(path, &state.paths.root),
        WebAssetsMode::Embedded => web_asset(path),
    }?;
    let mime = mime_guess::from_path(path).first_or_octet_stream();
    let mut response = Response::new(Body::from(asset));
    response.headers_mut().insert(
        header::CONTENT_TYPE,
        HeaderValue::from_str(mime.as_ref()).ok()?,
    );
    response.headers_mut().insert(
        header::CACHE_CONTROL,
        HeaderValue::from_static(cache_control_header(path)),
    );
    Some(response)
}

fn dist_asset(path: &str, root: &Path) -> Option<Vec<u8>> {
    std::fs::read(root.join("web").join("dist").join(path)).ok()
}

fn cache_control_header(path: &str) -> &'static str {
    match Path::new(path).extension().and_then(OsStr::to_str) {
        Some("html") => "no-cache",
        _ => "public, max-age=31536000, immutable",
    }
}

async fn health_handler() -> Json<serde_json::Value> {
    Json(serde_json::json!({ "status": "ok" }))
}

async fn status_handler(
    State(state): State<WebState>,
) -> std::result::Result<Json<ApiCoordinatorStatus>, ApiError> {
    let status = state
        .engine
        .get_coordinator_status(&state.paths)
        .map_err(ApiError::from)?;
    Ok(Json(ApiCoordinatorStatus::from(status)))
}

async fn events_handler(
    State(state): State<WebState>,
    headers: HeaderMap,
) -> std::result::Result<
    Sse<impl tokio_stream::Stream<Item = std::result::Result<Event, Infallible>>>,
    ApiError,
> {
    let initial_events = state
        .engine
        .get_coordinator_events(&state.paths)
        .map_err(ApiError::from)?;
    let last_event_id = headers
        .get("last-event-id")
        .and_then(|value| value.to_str().ok())
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned);

    Ok(Sse::new(coordinator_event_stream(
        state,
        initial_events,
        last_event_id,
        SSE_POLL_INTERVAL,
        SSE_HEARTBEAT_INTERVAL,
    )))
}

async fn coordinator_run_handler(
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

async fn coordinator_stop_handler(
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

async fn coordinator_cleanup_handler(
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

async fn coordinator_dispatch_handler(
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

async fn coordinator_advance_handler(
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

async fn coordinator_reconcile_handler(
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

async fn coordinator_resume_handler(
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

fn coordinator_event_stream(
    state: WebState,
    initial_events: Vec<CoordinatorEvent>,
    last_event_id: Option<String>,
    poll_interval: Duration,
    heartbeat_interval: Duration,
) -> impl tokio_stream::Stream<Item = std::result::Result<Event, Infallible>> {
    stream! {
        let mut source_seq_cursor = resolve_source_seq_cursor(&initial_events, last_event_id.as_deref());
        let mut pending_events = pending_events_after(&initial_events, source_seq_cursor);
        let mut poll_tick = tokio::time::interval(poll_interval);
        let mut heartbeat_tick = tokio::time::interval(heartbeat_interval);
        poll_tick.tick().await;
        heartbeat_tick.tick().await;

        loop {
            while let Some(event) = pending_events.pop_front() {
                source_seq_cursor = source_seq_cursor.max(event.seq);
                yield Ok(build_coordinator_sse_event(&event));
            }

            tokio::select! {
                _ = poll_tick.tick() => {
                    match state.engine.get_coordinator_events(&state.paths) {
                        Ok(events) => {
                            pending_events = pending_events_after(&events, source_seq_cursor);
                        }
                        Err(err) => {
                            tracing::warn!("failed to refresh coordinator SSE events: {}", err);
                        }
                    }
                }
                _ = heartbeat_tick.tick() => {
                    yield Ok(build_heartbeat_sse_event(source_seq_cursor));
                }
            }
        }
    }
}

fn pending_events_after(
    events: &[CoordinatorEvent],
    source_seq_cursor: i64,
) -> VecDeque<CoordinatorEvent> {
    events
        .iter()
        .filter(|event| event.seq > source_seq_cursor)
        .cloned()
        .collect()
}

fn resolve_source_seq_cursor(events: &[CoordinatorEvent], last_event_id: Option<&str>) -> i64 {
    last_event_id
        .and_then(|id| {
            events
                .iter()
                .find(|event| coordinator_event_sse_id(event) == id)
                .map(|event| event.seq)
                .or_else(|| parse_source_seq_from_sse_id(id))
        })
        .unwrap_or_default()
}

fn coordinator_event_sse_id(event: &CoordinatorEvent) -> String {
    event
        .event_id
        .clone()
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| format!("cursor-{}", event.seq))
}

fn parse_source_seq_from_sse_id(event_id: &str) -> Option<i64> {
    if let Some(seq) = event_id.strip_prefix("cursor-") {
        return seq.parse::<i64>().ok();
    }

    if let Some(heartbeat) = event_id.strip_prefix("hb-") {
        let seq = heartbeat.split('-').next()?;
        return seq.parse::<i64>().ok();
    }

    None
}

fn build_coordinator_sse_event(event: &CoordinatorEvent) -> Event {
    Event::default()
        .id(coordinator_event_sse_id(event))
        .event("coordinator_event")
        .json_data(event.raw.clone())
        .expect("serialize coordinator event payload")
}

fn build_heartbeat_sse_event(source_seq_cursor: i64) -> Event {
    let ts = chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Secs, true);
    let heartbeat_id = format!("hb-{}-{}", source_seq_cursor, unix_timestamp_millis());
    let payload = serde_json::json!({
        "schema_version": COORDINATOR_EVENT_SCHEMA_VERSION,
        "event_id": heartbeat_id,
        "seq": source_seq_cursor,
        "ts": ts,
        "source": "coordinator",
        "type": "heartbeat",
        "status": "ok"
    });

    Event::default()
        .id(payload["event_id"]
            .as_str()
            .expect("heartbeat id")
            .to_string())
        .event("heartbeat")
        .json_data(payload)
        .expect("serialize heartbeat payload")
}

fn unix_timestamp_millis() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
}

#[derive(Debug, Serialize)]
struct ApiCoordinatorStatus {
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
    /// RL-WEB-008: tools currently throttled due to rate-limiting (empty when none).
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub throttled_tools: Vec<ApiThrottledToolStatus>,
    /// RL-WEB-008: effective max_parallel after rate-limit concurrency reductions.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub effective_max_parallel: Option<usize>,
}

/// RL-WEB-008: per-tool throttle status for API responses.
#[derive(Debug, Serialize)]
struct ApiThrottledToolStatus {
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
struct ApiCoordinatorCommandResult {
    pub status: Option<ApiCoordinatorStatus>,
    pub resumed: Option<bool>,
    pub aggregated_performer_logs: Option<usize>,
    pub runtime_status: Option<String>,
    pub exported_events_path: Option<String>,
    pub removed_worktrees: Option<usize>,
    pub selected_task: Option<ApiSelectedTask>,
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
        }
    }
}

#[derive(Debug, Serialize)]
struct ApiSelectedTask {
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
struct ApiFailureReport {
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

#[derive(Debug, Serialize)]
struct ApiErrorEnvelope {
    error: ApiErrorBody,
}

#[derive(Debug, Serialize)]
struct ApiErrorBody {
    code: String,
    category: String,
    message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    context: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    cause: Option<String>,
}

const WEB_ERR_VALIDATION: &str = "MACC-WEB-0100";
const WEB_ERR_TOOLSPEC: &str = "MACC-WEB-0101";
const WEB_ERR_SECRET_DETECTED: &str = "MACC-WEB-0102";
const WEB_ERR_CONFIG: &str = "MACC-WEB-0103";
const WEB_ERR_CATALOG: &str = "MACC-WEB-0104";
const WEB_ERR_AUTH_SCOPE: &str = "MACC-WEB-0200";
const WEB_ERR_PROJECT_ROOT_NOT_FOUND: &str = "MACC-WEB-0300";
const WEB_ERR_HOME_NOT_FOUND: &str = "MACC-WEB-0301";
const WEB_ERR_IO: &str = "MACC-WEB-0400";
const WEB_ERR_FETCH: &str = "MACC-WEB-0401";
const WEB_ERR_COORDINATOR: &str = "MACC-WEB-0500";
const WEB_ERR_STORAGE: &str = "MACC-WEB-0501";
const WEB_ERR_GIT: &str = "MACC-WEB-0502";

struct ApiError {
    status: StatusCode,
    body: ApiErrorEnvelope,
}

impl ApiError {
    fn new(
        status: StatusCode,
        code: &str,
        category: &str,
        message: String,
        context: Option<serde_json::Value>,
        cause: Option<String>,
    ) -> Self {
        Self {
            status,
            body: ApiErrorEnvelope {
                error: ApiErrorBody {
                    code: code.to_string(),
                    category: category.to_string(),
                    message,
                    context,
                    cause,
                },
            },
        }
    }
}

impl From<MaccError> for ApiError {
    fn from(err: MaccError) -> Self {
        match err {
            MaccError::Validation(message) => ApiError::new(
                StatusCode::BAD_REQUEST,
                WEB_ERR_VALIDATION,
                "Validation",
                message,
                None,
                None,
            ),
            MaccError::ToolSpec {
                path,
                line,
                column,
                message,
            } => ApiError::new(
                StatusCode::BAD_REQUEST,
                WEB_ERR_TOOLSPEC,
                "Validation",
                message,
                Some(serde_json::json!({
                    "path": path,
                    "line": line,
                    "column": column,
                })),
                None,
            ),
            MaccError::UserScopeNotAllowed(message) => ApiError::new(
                StatusCode::FORBIDDEN,
                WEB_ERR_AUTH_SCOPE,
                "Auth",
                message,
                None,
                None,
            ),
            MaccError::ProjectRootNotFound { start_dir } => ApiError::new(
                StatusCode::NOT_FOUND,
                WEB_ERR_PROJECT_ROOT_NOT_FOUND,
                "NotFound",
                "Project root not found.".to_string(),
                Some(serde_json::json!({ "start_dir": start_dir })),
                None,
            ),
            MaccError::HomeDirNotFound => ApiError::new(
                StatusCode::NOT_FOUND,
                WEB_ERR_HOME_NOT_FOUND,
                "NotFound",
                "User home directory not found.".to_string(),
                None,
                None,
            ),
            MaccError::SecretDetected { path, details } => ApiError::new(
                StatusCode::BAD_REQUEST,
                WEB_ERR_SECRET_DETECTED,
                "Validation",
                "Secret detected in output.".to_string(),
                Some(serde_json::json!({ "path": path, "details": details })),
                None,
            ),
            MaccError::Config { path, source } => ApiError::new(
                StatusCode::BAD_REQUEST,
                WEB_ERR_CONFIG,
                "Validation",
                format!("Configuration error in {}: {}", path, source),
                Some(serde_json::json!({ "path": path })),
                Some(source.to_string()),
            ),
            MaccError::Io {
                path,
                action,
                source,
            } => ApiError::new(
                StatusCode::BAD_GATEWAY,
                WEB_ERR_IO,
                "Dependency",
                format!("I/O error during {}.", action),
                Some(serde_json::json!({ "path": path, "action": action })),
                Some(source.to_string()),
            ),
            MaccError::Coordinator { code, message } => ApiError::new(
                StatusCode::INTERNAL_SERVER_ERROR,
                WEB_ERR_COORDINATOR,
                "Internal",
                message,
                Some(serde_json::json!({ "code": code })),
                None,
            ),
            MaccError::Storage { backend, message } => ApiError::new(
                StatusCode::INTERNAL_SERVER_ERROR,
                WEB_ERR_STORAGE,
                "Internal",
                message,
                Some(serde_json::json!({ "backend": backend })),
                None,
            ),
            MaccError::Git { operation, message } => ApiError::new(
                StatusCode::INTERNAL_SERVER_ERROR,
                WEB_ERR_GIT,
                "Internal",
                message,
                Some(serde_json::json!({ "operation": operation })),
                None,
            ),
            MaccError::Fetch { url, message } => ApiError::new(
                StatusCode::BAD_GATEWAY,
                WEB_ERR_FETCH,
                "Dependency",
                message,
                Some(serde_json::json!({ "url": url })),
                None,
            ),
            MaccError::Catalog { operation, message } => ApiError::new(
                StatusCode::BAD_REQUEST,
                WEB_ERR_CATALOG,
                "Validation",
                message,
                Some(serde_json::json!({ "operation": operation })),
                None,
            ),
        }
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        (self.status, Json(self.body)).into_response()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::commands::AppContext;
    use axum::body::Body;
    use axum::http::Request;
    use http_body_util::BodyExt;
    use macc_core::config::CanonicalConfig;
    use macc_core::engine::CoordinatorEvent;
    use macc_core::resolve::CliOverrides;
    use macc_core::TestEngine;
    use std::fs;
    use std::net::Ipv4Addr;
    use std::sync::Arc;
    use std::time::{SystemTime, UNIX_EPOCH};
    use tower::util::ServiceExt;

    fn temp_root(label: &str) -> std::path::PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("time")
            .as_nanos();
        std::env::temp_dir().join(format!("macc-web-{}-{}", label, nanos))
    }

    fn write_test_config(root: &std::path::Path) {
        let paths = ProjectPaths::from_root(root);
        fs::create_dir_all(paths.config_path.parent().expect("config dir")).expect("mkdir config");
        let yaml = CanonicalConfig::default()
            .to_yaml()
            .expect("serialize config");
        fs::write(&paths.config_path, yaml).expect("write config");
    }

    fn write_test_config_with_port(root: &std::path::Path, port: u16) {
        let paths = ProjectPaths::from_root(root);
        fs::create_dir_all(paths.config_path.parent().expect("config dir")).expect("mkdir config");
        let mut canonical = CanonicalConfig::default();
        canonical.settings.web_port = Some(port);
        let yaml = canonical.to_yaml().expect("serialize config");
        fs::write(&paths.config_path, yaml).expect("write config");
    }

    fn write_test_config_with_assets_mode(root: &std::path::Path, assets_mode: WebAssetsMode) {
        let paths = ProjectPaths::from_root(root);
        fs::create_dir_all(paths.config_path.parent().expect("config dir")).expect("mkdir config");
        let mut canonical = CanonicalConfig::default();
        canonical.settings.web_assets = Some(assets_mode);
        let yaml = canonical.to_yaml().expect("serialize config");
        fs::write(&paths.config_path, yaml).expect("write config");
    }

    fn write_test_dist_assets(root: &std::path::Path) {
        let dist_dir = root.join("web").join("dist").join("assets");
        fs::create_dir_all(&dist_dir).expect("mkdir dist assets");
        fs::write(
            root.join("web").join("dist").join("index.html"),
            "<!doctype html><html><body>dist web</body></html>",
        )
        .expect("write index");
        fs::write(dist_dir.join("app.js"), "console.log('dist');").expect("write asset");
    }

    fn test_web_state(
        root: &std::path::Path,
        engine: SharedEngine,
        assets_mode: WebAssetsMode,
    ) -> WebState {
        WebState {
            engine,
            paths: ProjectPaths::from_root(root),
            assets_mode,
        }
    }

    struct WebTestEngine {
        inner: TestEngine,
        run_result:
            std::sync::Mutex<Option<std::result::Result<CoordinatorCommandResult, MaccError>>>,
        cleanup_result: std::sync::Mutex<Option<std::result::Result<(), MaccError>>>,
        stop_result: std::sync::Mutex<Option<std::result::Result<(), MaccError>>>,
        resume_result: std::sync::Mutex<Option<std::result::Result<(), MaccError>>>,
        coordinator_events: std::sync::Mutex<Vec<Vec<CoordinatorEvent>>>,
    }

    impl WebTestEngine {
        fn new(result: std::result::Result<CoordinatorCommandResult, MaccError>) -> Self {
            Self {
                inner: TestEngine::with_fixtures(),
                run_result: std::sync::Mutex::new(Some(result)),
                cleanup_result: std::sync::Mutex::new(Some(Ok(()))),
                stop_result: std::sync::Mutex::new(Some(Ok(()))),
                resume_result: std::sync::Mutex::new(Some(Ok(()))),
                coordinator_events: std::sync::Mutex::new(vec![Vec::new()]),
            }
        }

        fn with_cleanup_result(result: std::result::Result<(), MaccError>) -> Self {
            Self {
                inner: TestEngine::with_fixtures(),
                run_result: std::sync::Mutex::new(Some(Ok(CoordinatorCommandResult::default()))),
                cleanup_result: std::sync::Mutex::new(Some(result)),
                stop_result: std::sync::Mutex::new(Some(Ok(()))),
                resume_result: std::sync::Mutex::new(Some(Ok(()))),
                coordinator_events: std::sync::Mutex::new(vec![Vec::new()]),
            }
        }

        fn with_stop_result(result: std::result::Result<(), MaccError>) -> Self {
            Self {
                inner: TestEngine::with_fixtures(),
                run_result: std::sync::Mutex::new(Some(Ok(CoordinatorCommandResult::default()))),
                cleanup_result: std::sync::Mutex::new(Some(Ok(()))),
                stop_result: std::sync::Mutex::new(Some(result)),
                resume_result: std::sync::Mutex::new(Some(Ok(()))),
                coordinator_events: std::sync::Mutex::new(vec![Vec::new()]),
            }
        }

        fn with_resume_result(result: std::result::Result<(), MaccError>) -> Self {
            Self {
                inner: TestEngine::with_fixtures(),
                run_result: std::sync::Mutex::new(Some(Ok(CoordinatorCommandResult::default()))),
                cleanup_result: std::sync::Mutex::new(Some(Ok(()))),
                stop_result: std::sync::Mutex::new(Some(Ok(()))),
                resume_result: std::sync::Mutex::new(Some(result)),
                coordinator_events: std::sync::Mutex::new(vec![Vec::new()]),
            }
        }

        fn with_event_snapshots(event_snapshots: Vec<Vec<CoordinatorEvent>>) -> Self {
            Self {
                inner: TestEngine::with_fixtures(),
                run_result: std::sync::Mutex::new(Some(Ok(CoordinatorCommandResult::default()))),
                cleanup_result: std::sync::Mutex::new(Some(Ok(()))),
                stop_result: std::sync::Mutex::new(Some(Ok(()))),
                resume_result: std::sync::Mutex::new(Some(Ok(()))),
                coordinator_events: std::sync::Mutex::new(event_snapshots),
            }
        }
    }

    impl macc_core::engine::Engine for WebTestEngine {
        fn list_tools(
            &self,
            paths: &ProjectPaths,
        ) -> (
            Vec<macc_core::ToolDescriptor>,
            Vec<macc_core::tool::ToolDiagnostic>,
        ) {
            self.inner.list_tools(paths)
        }

        fn doctor(&self, paths: &ProjectPaths) -> Vec<macc_core::doctor::ToolCheck> {
            self.inner.doctor(paths)
        }

        fn plan(
            &self,
            paths: &ProjectPaths,
            config: &macc_core::config::CanonicalConfig,
            materialized_units: &[macc_core::resolve::MaterializedFetchUnit],
            overrides: &macc_core::resolve::CliOverrides,
        ) -> Result<macc_core::plan::ActionPlan> {
            self.inner
                .plan(paths, config, materialized_units, overrides)
        }

        fn plan_operations(
            &self,
            paths: &ProjectPaths,
            plan: &macc_core::plan::ActionPlan,
        ) -> Vec<macc_core::plan::PlannedOp> {
            self.inner.plan_operations(paths, plan)
        }

        fn apply(
            &self,
            paths: &ProjectPaths,
            plan: &mut macc_core::plan::ActionPlan,
            allow_user_scope: bool,
        ) -> Result<macc_core::ApplyReport> {
            self.inner.apply(paths, plan, allow_user_scope)
        }

        fn builtin_skills(&self) -> Vec<macc_core::catalog::Skill> {
            self.inner.builtin_skills()
        }

        fn builtin_agents(&self) -> Vec<macc_core::catalog::Agent> {
            self.inner.builtin_agents()
        }

        fn coordinator_execute_command(
            &self,
            _paths: &ProjectPaths,
            _command: CoordinatorCommand,
            _request: CoordinatorCommandRequest<'_>,
        ) -> Result<CoordinatorCommandResult> {
            self.run_result
                .lock()
                .expect("lock")
                .take()
                .unwrap_or_else(|| Ok(CoordinatorCommandResult::default()))
        }

        fn coordinator_stop(&self, _repo_root: &std::path::Path, _reason: &str) -> Result<()> {
            self.stop_result
                .lock()
                .expect("lock")
                .take()
                .unwrap_or_else(|| Ok(()))
        }

        fn coordinator_cleanup(&self, _paths: &ProjectPaths) -> Result<()> {
            self.cleanup_result
                .lock()
                .expect("lock")
                .take()
                .unwrap_or_else(|| Ok(()))
        }

        fn coordinator_resume(&self, _repo_root: &std::path::Path) -> Result<()> {
            self.resume_result
                .lock()
                .expect("lock")
                .take()
                .unwrap_or_else(|| Ok(()))
        }

        fn get_coordinator_events(&self, _paths: &ProjectPaths) -> Result<Vec<CoordinatorEvent>> {
            let mut snapshots = self.coordinator_events.lock().expect("lock");
            let snapshot = if snapshots.len() > 1 {
                snapshots.remove(0)
            } else {
                snapshots.first().cloned().unwrap_or_default()
            };
            Ok(snapshot)
        }
    }

    fn coordinator_event(seq: i64, event_id: &str, event_type: &str) -> CoordinatorEvent {
        CoordinatorEvent {
            event_id: Some(event_id.to_string()),
            run_id: Some("run-1".to_string()),
            seq,
            event_type: event_type.to_string(),
            task_id: Some("WEB-BACKEND-008".to_string()),
            phase: Some("implement".to_string()),
            status: Some("ok".to_string()),
            ts: Some("2026-03-19T12:00:00Z".to_string()),
            message: None,
            raw: serde_json::json!({
                "schema_version": COORDINATOR_EVENT_SCHEMA_VERSION,
                "event_id": event_id,
                "run_id": "run-1",
                "seq": seq,
                "ts": "2026-03-19T12:00:00Z",
                "source": "coordinator",
                "task_id": "WEB-BACKEND-008",
                "type": event_type,
                "phase": "implement",
                "status": "ok",
            }),
        }
    }

    #[tokio::test]
    async fn root_serves_spa_index() {
        let root = temp_root("root");
        let state = test_web_state(
            &root,
            Arc::new(TestEngine::with_fixtures()),
            WebAssetsMode::Embedded,
        );
        let app = build_web_router(state);
        let response = app
            .oneshot(
                Request::builder()
                    .uri("/")
                    .method("GET")
                    .body(Body::empty())
                    .expect("request"),
            )
            .await
            .expect("response");

        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(
            response
                .headers()
                .get(axum::http::header::CONTENT_TYPE)
                .expect("content type"),
            "text/html"
        );
        let bytes = response
            .into_body()
            .collect()
            .await
            .expect("collect")
            .to_bytes();
        let payload = String::from_utf8(bytes.to_vec()).expect("utf8");
        assert!(payload.contains("<!doctype html") || payload.contains("<!DOCTYPE html"));
    }

    #[tokio::test]
    async fn client_side_route_serves_spa_index() {
        let root = temp_root("spa-route");
        let state = test_web_state(
            &root,
            Arc::new(TestEngine::with_fixtures()),
            WebAssetsMode::Embedded,
        );
        let app = build_web_router(state);
        let response = app
            .oneshot(
                Request::builder()
                    .uri("/runs/active")
                    .method("GET")
                    .body(Body::empty())
                    .expect("request"),
            )
            .await
            .expect("response");

        assert_eq!(response.status(), StatusCode::OK);
        let bytes = response
            .into_body()
            .collect()
            .await
            .expect("collect")
            .to_bytes();
        let payload = String::from_utf8(bytes.to_vec()).expect("utf8");
        assert!(payload.contains("<!doctype html") || payload.contains("<!DOCTYPE html"));
    }

    #[tokio::test]
    async fn static_asset_request_serves_embedded_asset() {
        let root = temp_root("asset");
        let asset_path = web_asset_paths()
            .find(|path| path.starts_with("assets/"))
            .expect("embedded asset path");
        let state = test_web_state(
            &root,
            Arc::new(TestEngine::with_fixtures()),
            WebAssetsMode::Embedded,
        );
        let app = build_web_router(state);
        let response = app
            .oneshot(
                Request::builder()
                    .uri(format!("/{}", asset_path))
                    .method(Method::GET.as_str())
                    .body(Body::empty())
                    .expect("request"),
            )
            .await
            .expect("response");

        assert_eq!(response.status(), StatusCode::OK);
        assert!(response
            .headers()
            .get(axum::http::header::CONTENT_TYPE)
            .is_some());
        let bytes = response
            .into_body()
            .collect()
            .await
            .expect("collect")
            .to_bytes();
        assert!(!bytes.is_empty());
    }

    #[tokio::test]
    async fn dist_asset_mode_serves_files_from_disk() {
        let root = temp_root("dist-asset");
        write_test_dist_assets(&root);
        let state = test_web_state(
            &root,
            Arc::new(TestEngine::with_fixtures()),
            WebAssetsMode::Dist,
        );
        let app = build_web_router(state);
        let response = app
            .oneshot(
                Request::builder()
                    .uri("/assets/app.js")
                    .method(Method::GET.as_str())
                    .body(Body::empty())
                    .expect("request"),
            )
            .await
            .expect("response");

        assert_eq!(response.status(), StatusCode::OK);
        assert!(response
            .headers()
            .get(axum::http::header::CONTENT_TYPE)
            .is_some());
        let bytes = response
            .into_body()
            .collect()
            .await
            .expect("collect")
            .to_bytes();
        assert_eq!(
            String::from_utf8(bytes.to_vec()).expect("utf8"),
            "console.log('dist');"
        );
    }

    #[tokio::test]
    async fn dist_asset_mode_falls_back_to_dist_index_for_client_routes() {
        let root = temp_root("dist-spa");
        write_test_dist_assets(&root);
        let state = test_web_state(
            &root,
            Arc::new(TestEngine::with_fixtures()),
            WebAssetsMode::Dist,
        );
        let app = build_web_router(state);
        let response = app
            .oneshot(
                Request::builder()
                    .uri("/runs/active")
                    .method("GET")
                    .body(Body::empty())
                    .expect("request"),
            )
            .await
            .expect("response");

        assert_eq!(response.status(), StatusCode::OK);
        let bytes = response
            .into_body()
            .collect()
            .await
            .expect("collect")
            .to_bytes();
        let payload = String::from_utf8(bytes.to_vec()).expect("utf8");
        assert!(payload.contains("dist web"));
    }

    #[tokio::test]
    async fn health_endpoint_returns_ok_status() {
        let root = temp_root("health");
        fs::create_dir_all(&root).expect("create root");
        let state = test_web_state(
            &root,
            Arc::new(TestEngine::with_fixtures()),
            WebAssetsMode::Embedded,
        );
        let app = build_web_router(state);
        let response = app
            .oneshot(
                Request::builder()
                    .uri("/api/v1/health")
                    .method("GET")
                    .body(Body::empty())
                    .expect("request"),
            )
            .await
            .expect("response");

        assert_eq!(response.status(), StatusCode::OK);
        let bytes = response
            .into_body()
            .collect()
            .await
            .expect("collect")
            .to_bytes();
        let payload: serde_json::Value = serde_json::from_slice(&bytes).expect("json");
        assert_eq!(payload["status"], "ok");
    }

    #[tokio::test]
    async fn status_endpoint_returns_status_payload() {
        let root = temp_root("ok");
        fs::create_dir_all(&root).expect("create root");
        let state = test_web_state(
            &root,
            Arc::new(TestEngine::with_fixtures()),
            WebAssetsMode::Embedded,
        );
        let app = build_web_router(state);
        let response = app
            .oneshot(
                Request::builder()
                    .uri("/api/v1/status")
                    .method("GET")
                    .body(Body::empty())
                    .expect("request"),
            )
            .await
            .expect("response");

        assert_eq!(response.status(), StatusCode::OK);
        let bytes = response
            .into_body()
            .collect()
            .await
            .expect("collect")
            .to_bytes();
        let payload: serde_json::Value = serde_json::from_slice(&bytes).expect("json");
        for key in [
            "total",
            "todo",
            "active",
            "blocked",
            "merged",
            "paused",
            "pause_reason",
            "pause_task_id",
            "pause_phase",
            "latest_error",
            "failure_report",
        ] {
            assert!(payload.get(key).is_some(), "missing {}", key);
        }
    }

    #[tokio::test]
    async fn status_endpoint_maps_engine_errors() {
        let root = temp_root("error");
        let registry_path = root
            .join(".macc")
            .join("automation")
            .join("task")
            .join("task_registry.json");
        fs::create_dir_all(registry_path.parent().expect("parent")).expect("mkdir");
        fs::write(&registry_path, "{not-json").expect("write");
        let state = test_web_state(
            &root,
            Arc::new(TestEngine::with_fixtures()),
            WebAssetsMode::Embedded,
        );
        let app = build_web_router(state);
        let response = app
            .oneshot(
                Request::builder()
                    .uri("/api/v1/status")
                    .method("GET")
                    .body(Body::empty())
                    .expect("request"),
            )
            .await
            .expect("response");

        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
        let bytes = response
            .into_body()
            .collect()
            .await
            .expect("collect")
            .to_bytes();
        let payload: serde_json::Value = serde_json::from_slice(&bytes).expect("json");
        assert_eq!(payload["error"]["code"], WEB_ERR_VALIDATION);
        assert_eq!(payload["error"]["category"], "Validation");
        assert!(payload["error"]["message"].is_string());
        assert!(payload["error"].get("context").is_none());
    }

    #[tokio::test]
    async fn events_endpoint_streams_coordinator_events() {
        let root = temp_root("events-stream");
        fs::create_dir_all(&root).expect("create root");
        let state = test_web_state(
            &root,
            Arc::new(WebTestEngine::with_event_snapshots(vec![vec![
                coordinator_event(42, "evt-42", "task_transition"),
            ]])),
            WebAssetsMode::Embedded,
        );
        let app = build_web_router(state);
        let response = app
            .oneshot(
                Request::builder()
                    .uri("/api/v1/events")
                    .method("GET")
                    .body(Body::empty())
                    .expect("request"),
            )
            .await
            .expect("response");

        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(
            response
                .headers()
                .get(axum::http::header::CONTENT_TYPE)
                .expect("content type"),
            "text/event-stream"
        );

        let mut body = response.into_body();
        let first_frame = tokio::time::timeout(Duration::from_secs(1), body.frame())
            .await
            .expect("frame timeout")
            .expect("frame")
            .expect("body frame");
        let chunk = first_frame.into_data().expect("data frame");
        let text = String::from_utf8(chunk.to_vec()).expect("utf8");

        assert!(text.contains("event: coordinator_event"), "{text}");
        assert!(text.contains("id: evt-42"), "{text}");
        assert!(text.contains("\"type\":\"task_transition\""), "{text}");
    }

    #[tokio::test]
    async fn events_endpoint_respects_last_event_id_cursor() {
        let root = temp_root("events-cursor");
        fs::create_dir_all(&root).expect("create root");
        let state = test_web_state(
            &root,
            Arc::new(WebTestEngine::with_event_snapshots(vec![vec![
                coordinator_event(41, "evt-41", "task_transition"),
                coordinator_event(42, "evt-42", "task_transition"),
            ]])),
            WebAssetsMode::Embedded,
        );
        let app = build_web_router(state);
        let response = app
            .oneshot(
                Request::builder()
                    .uri("/api/v1/events")
                    .method("GET")
                    .header("Last-Event-ID", "evt-41")
                    .body(Body::empty())
                    .expect("request"),
            )
            .await
            .expect("response");

        let mut body = response.into_body();
        let first_frame = tokio::time::timeout(Duration::from_secs(1), body.frame())
            .await
            .expect("frame timeout")
            .expect("frame")
            .expect("body frame");
        let chunk = first_frame.into_data().expect("data frame");
        let text = String::from_utf8(chunk.to_vec()).expect("utf8");

        assert!(text.contains("id: evt-42"), "{text}");
        assert!(!text.contains("evt-41"), "{text}");
    }

    #[tokio::test]
    async fn coordinator_event_stream_delivers_new_events_after_connect() {
        let root = temp_root("events-live");
        fs::create_dir_all(&root).expect("create root");
        let state = test_web_state(
            &root,
            Arc::new(WebTestEngine::with_event_snapshots(vec![
                Vec::new(),
                vec![coordinator_event(43, "evt-43", "task_transition")],
            ])),
            WebAssetsMode::Embedded,
        );
        let response = Sse::new(coordinator_event_stream(
            state,
            Vec::new(),
            None,
            Duration::from_millis(10),
            Duration::from_millis(100),
        ))
        .into_response();
        let mut body = response.into_body();

        let frame = tokio::time::timeout(Duration::from_millis(100), body.frame())
            .await
            .expect("live event timeout")
            .expect("frame")
            .expect("body frame");
        let data =
            String::from_utf8(frame.into_data().expect("data frame").to_vec()).expect("utf8");

        assert!(data.contains("event: coordinator_event"), "{data}");
        assert!(data.contains("id: evt-43"), "{data}");
    }

    #[tokio::test]
    async fn coordinator_event_stream_emits_heartbeat_records() {
        let root = temp_root("events-heartbeat");
        fs::create_dir_all(&root).expect("create root");
        let state = test_web_state(
            &root,
            Arc::new(WebTestEngine::with_event_snapshots(vec![Vec::new()])),
            WebAssetsMode::Embedded,
        );
        let response = Sse::new(coordinator_event_stream(
            state,
            Vec::new(),
            None,
            Duration::from_millis(50),
            Duration::from_millis(10),
        ))
        .into_response();
        let mut body = response.into_body();

        let frame = tokio::time::timeout(Duration::from_millis(100), body.frame())
            .await
            .expect("heartbeat timeout")
            .expect("frame")
            .expect("body frame");
        let data =
            String::from_utf8(frame.into_data().expect("data frame").to_vec()).expect("utf8");

        assert!(data.contains("event: heartbeat"), "{data}");
        assert!(data.contains("\"type\":\"heartbeat\""), "{data}");
        assert!(data.contains("\"status\":\"ok\""), "{data}");
    }

    #[test]
    fn server_config_defaults_to_localhost_and_configured_port() {
        let root = temp_root("bind-default");
        fs::create_dir_all(&root).expect("create root");
        write_test_config_with_port(&root, 4567);
        let command = WebCommand::new(
            AppContext::new(
                root.clone(),
                Arc::new(TestEngine::with_fixtures()),
                CliOverrides::default(),
            ),
            Ipv4Addr::LOCALHOST.to_string(),
            None,
            None,
        );

        let config = command.server_config().expect("server config");

        assert_eq!(
            config,
            WebServerConfig {
                host: IpAddr::V4(Ipv4Addr::LOCALHOST),
                port: 4567,
                assets_mode: WebAssetsMode::Dist,
            }
        );
        assert_eq!(
            config.bind_addr(),
            SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), 4567)
        );
    }

    #[test]
    fn server_config_uses_cli_overrides() {
        let root = temp_root("bind-override");
        fs::create_dir_all(&root).expect("create root");
        write_test_config_with_port(&root, 4567);
        write_test_config_with_assets_mode(&root, WebAssetsMode::Dist);
        let command = WebCommand::new(
            AppContext::new(
                root.clone(),
                Arc::new(TestEngine::with_fixtures()),
                CliOverrides::default(),
            ),
            "0.0.0.0".to_string(),
            Some(8080),
            Some(WebAssetsMode::Embedded),
        );

        let config = command.server_config().expect("server config");

        assert_eq!(
            config,
            WebServerConfig {
                host: IpAddr::V4(Ipv4Addr::UNSPECIFIED),
                port: 8080,
                assets_mode: WebAssetsMode::Embedded,
            }
        );
    }

    #[test]
    fn server_config_assets_mode_defaults_to_dist_in_dev() {
        let root = temp_root("assets-default");
        fs::create_dir_all(&root).expect("create root");
        write_test_config(&root);
        let command = WebCommand::new(
            AppContext::new(
                root.clone(),
                Arc::new(TestEngine::with_fixtures()),
                CliOverrides::default(),
            ),
            Ipv4Addr::LOCALHOST.to_string(),
            None,
            None,
        );

        assert_eq!(
            command.server_config().expect("server config").assets_mode,
            WebAssetsMode::Dist
        );
    }

    #[test]
    fn server_config_assets_mode_uses_config_when_cli_flag_is_absent() {
        let root = temp_root("assets-config");
        fs::create_dir_all(&root).expect("create root");
        write_test_config_with_assets_mode(&root, WebAssetsMode::Embedded);
        let command = WebCommand::new(
            AppContext::new(
                root.clone(),
                Arc::new(TestEngine::with_fixtures()),
                CliOverrides::default(),
            ),
            Ipv4Addr::LOCALHOST.to_string(),
            None,
            None,
        );

        assert_eq!(
            command.server_config().expect("server config").assets_mode,
            WebAssetsMode::Embedded
        );
    }

    #[test]
    fn server_config_assets_mode_cli_flag_overrides_config() {
        let root = temp_root("assets-cli");
        fs::create_dir_all(&root).expect("create root");
        write_test_config_with_assets_mode(&root, WebAssetsMode::Dist);
        let command = WebCommand::new(
            AppContext::new(
                root.clone(),
                Arc::new(TestEngine::with_fixtures()),
                CliOverrides::default(),
            ),
            Ipv4Addr::LOCALHOST.to_string(),
            None,
            Some(WebAssetsMode::Embedded),
        );

        assert_eq!(
            command.server_config().expect("server config").assets_mode,
            WebAssetsMode::Embedded
        );
    }

    #[tokio::test]
    async fn coordinator_run_endpoint_returns_envelope() {
        let root = temp_root("run-ok");
        fs::create_dir_all(&root).expect("create root");
        let result = CoordinatorCommandResult {
            status: Some(CoordinatorStatus {
                total: 5,
                todo: 2,
                active: 1,
                blocked: 1,
                merged: 1,
                paused: false,
                pause_reason: None,
                pause_task_id: None,
                pause_phase: None,
                latest_error: None,
                failure_report: None,
                throttled_tools: vec![],
                effective_max_parallel: None,
            }),
            resumed: Some(false),
            aggregated_performer_logs: Some(2),
            runtime_status: Some("running".to_string()),
            exported_events_path: Some(std::path::PathBuf::from(
                ".macc/log/coordinator/events.jsonl",
            )),
            removed_worktrees: Some(0),
            selected_task: Some(SelectedTask {
                id: "TASK-1".to_string(),
                title: "Example".to_string(),
                tool: "mock".to_string(),
                base_branch: "main".to_string(),
                is_fallback: false,
            }),
            audit_prd_report: None,
        };
        let state = test_web_state(
            &root,
            Arc::new(WebTestEngine::new(Ok(result))),
            WebAssetsMode::Embedded,
        );
        let app = build_web_router(state);
        let response = app
            .oneshot(
                Request::builder()
                    .uri("/api/v1/coordinator/run")
                    .method("POST")
                    .body(Body::empty())
                    .expect("request"),
            )
            .await
            .expect("response");

        assert_eq!(response.status(), StatusCode::OK);
        let bytes = response
            .into_body()
            .collect()
            .await
            .expect("collect")
            .to_bytes();
        let payload: serde_json::Value = serde_json::from_slice(&bytes).expect("json");
        for key in [
            "status",
            "resumed",
            "aggregated_performer_logs",
            "runtime_status",
            "exported_events_path",
            "removed_worktrees",
            "selected_task",
        ] {
            assert!(payload.get(key).is_some(), "missing {}", key);
        }
    }

    #[tokio::test]
    async fn coordinator_run_endpoint_maps_errors() {
        let root = temp_root("run-error");
        fs::create_dir_all(&root).expect("create root");
        let state = test_web_state(
            &root,
            Arc::new(WebTestEngine::new(Err(MaccError::Validation(
                "coordinator failed".to_string(),
            )))),
            WebAssetsMode::Embedded,
        );
        let app = build_web_router(state);
        let response = app
            .oneshot(
                Request::builder()
                    .uri("/api/v1/coordinator/run")
                    .method("POST")
                    .body(Body::empty())
                    .expect("request"),
            )
            .await
            .expect("response");

        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
        let bytes = response
            .into_body()
            .collect()
            .await
            .expect("collect")
            .to_bytes();
        let payload: serde_json::Value = serde_json::from_slice(&bytes).expect("json");
        assert_eq!(payload["error"]["code"], WEB_ERR_VALIDATION);
        assert_eq!(payload["error"]["category"], "Validation");
        assert_eq!(payload["error"]["message"], "coordinator failed");
        assert!(payload["error"].get("cause").is_none());
    }

    #[tokio::test]
    async fn coordinator_run_endpoint_maps_internal_coordinator_error_code() {
        let root = temp_root("run-coordinator-error");
        fs::create_dir_all(&root).expect("create root");
        let state = test_web_state(
            &root,
            Arc::new(WebTestEngine::new(Err(MaccError::Coordinator {
                code: "E901",
                message: "coordinator crashed".to_string(),
            }))),
            WebAssetsMode::Embedded,
        );
        let app = build_web_router(state);
        let response = app
            .oneshot(
                Request::builder()
                    .uri("/api/v1/coordinator/run")
                    .method("POST")
                    .body(Body::empty())
                    .expect("request"),
            )
            .await
            .expect("response");

        assert_eq!(response.status(), StatusCode::INTERNAL_SERVER_ERROR);
        let bytes = response
            .into_body()
            .collect()
            .await
            .expect("collect")
            .to_bytes();
        let payload: serde_json::Value = serde_json::from_slice(&bytes).expect("json");
        assert_eq!(payload["error"]["code"], WEB_ERR_COORDINATOR);
        assert_eq!(payload["error"]["category"], "Internal");
        assert_eq!(payload["error"]["context"]["code"], "E901");
    }

    #[tokio::test]
    async fn coordinator_dispatch_endpoint_returns_envelope() {
        let root = temp_root("dispatch-ok");
        fs::create_dir_all(&root).expect("create root");
        write_test_config(&root);
        let result = CoordinatorCommandResult {
            status: Some(CoordinatorStatus {
                total: 3,
                todo: 1,
                active: 1,
                blocked: 0,
                merged: 1,
                paused: false,
                pause_reason: None,
                pause_task_id: None,
                pause_phase: None,
                latest_error: None,
                failure_report: None,
                throttled_tools: vec![],
                effective_max_parallel: None,
            }),
            resumed: Some(false),
            aggregated_performer_logs: Some(0),
            runtime_status: Some("running".to_string()),
            exported_events_path: None,
            removed_worktrees: Some(0),
            selected_task: Some(SelectedTask {
                id: "TASK-2".to_string(),
                title: "Dispatch".to_string(),
                tool: "mock".to_string(),
                base_branch: "main".to_string(),
                is_fallback: false,
            }),
            audit_prd_report: None,
        };
        let state = test_web_state(
            &root,
            Arc::new(WebTestEngine::new(Ok(result))),
            WebAssetsMode::Embedded,
        );
        let app = build_web_router(state);
        let response = app
            .oneshot(
                Request::builder()
                    .uri("/api/v1/coordinator/dispatch")
                    .method("POST")
                    .body(Body::empty())
                    .expect("request"),
            )
            .await
            .expect("response");

        assert_eq!(response.status(), StatusCode::OK);
        let bytes = response
            .into_body()
            .collect()
            .await
            .expect("collect")
            .to_bytes();
        let payload: serde_json::Value = serde_json::from_slice(&bytes).expect("json");
        for key in [
            "status",
            "resumed",
            "aggregated_performer_logs",
            "runtime_status",
            "exported_events_path",
            "removed_worktrees",
            "selected_task",
        ] {
            assert!(payload.get(key).is_some(), "missing {}", key);
        }
    }

    #[tokio::test]
    async fn coordinator_dispatch_endpoint_maps_errors() {
        let root = temp_root("dispatch-error");
        fs::create_dir_all(&root).expect("create root");
        write_test_config(&root);
        let state = test_web_state(
            &root,
            Arc::new(WebTestEngine::new(Err(MaccError::Validation(
                "dispatch failed".to_string(),
            )))),
            WebAssetsMode::Embedded,
        );
        let app = build_web_router(state);
        let response = app
            .oneshot(
                Request::builder()
                    .uri("/api/v1/coordinator/dispatch")
                    .method("POST")
                    .body(Body::empty())
                    .expect("request"),
            )
            .await
            .expect("response");

        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
        let bytes = response
            .into_body()
            .collect()
            .await
            .expect("collect")
            .to_bytes();
        let payload: serde_json::Value = serde_json::from_slice(&bytes).expect("json");
        assert_eq!(payload["error"]["code"], WEB_ERR_VALIDATION);
        assert_eq!(payload["error"]["category"], "Validation");
        assert_eq!(payload["error"]["message"], "dispatch failed");
    }

    #[tokio::test]
    async fn coordinator_stop_endpoint_returns_envelope() {
        let root = temp_root("stop-ok");
        fs::create_dir_all(&root).expect("create root");
        let state = test_web_state(
            &root,
            Arc::new(WebTestEngine::with_stop_result(Ok(()))),
            WebAssetsMode::Embedded,
        );
        let app = build_web_router(state);
        let response = app
            .oneshot(
                Request::builder()
                    .uri("/api/v1/coordinator/stop")
                    .method("POST")
                    .body(Body::empty())
                    .expect("request"),
            )
            .await
            .expect("response");

        assert_eq!(response.status(), StatusCode::OK);
        let bytes = response
            .into_body()
            .collect()
            .await
            .expect("collect")
            .to_bytes();
        let payload: serde_json::Value = serde_json::from_slice(&bytes).expect("json");
        for key in [
            "status",
            "resumed",
            "aggregated_performer_logs",
            "runtime_status",
            "exported_events_path",
            "removed_worktrees",
            "selected_task",
        ] {
            assert!(payload.get(key).is_some(), "missing {}", key);
        }
    }

    #[tokio::test]
    async fn coordinator_stop_endpoint_maps_errors() {
        let root = temp_root("stop-error");
        fs::create_dir_all(&root).expect("create root");
        let state = test_web_state(
            &root,
            Arc::new(WebTestEngine::with_stop_result(Err(MaccError::Validation(
                "stop failed".to_string(),
            )))),
            WebAssetsMode::Embedded,
        );
        let app = build_web_router(state);
        let response = app
            .oneshot(
                Request::builder()
                    .uri("/api/v1/coordinator/stop")
                    .method("POST")
                    .body(Body::empty())
                    .expect("request"),
            )
            .await
            .expect("response");

        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
        let bytes = response
            .into_body()
            .collect()
            .await
            .expect("collect")
            .to_bytes();
        let payload: serde_json::Value = serde_json::from_slice(&bytes).expect("json");
        assert_eq!(payload["error"]["code"], WEB_ERR_VALIDATION);
        assert_eq!(payload["error"]["category"], "Validation");
    }

    #[tokio::test]
    async fn coordinator_cleanup_endpoint_returns_envelope() {
        let root = temp_root("cleanup-ok");
        fs::create_dir_all(&root).expect("create root");
        let state = test_web_state(
            &root,
            Arc::new(WebTestEngine::with_cleanup_result(Ok(()))),
            WebAssetsMode::Embedded,
        );
        let app = build_web_router(state);
        let response = app
            .oneshot(
                Request::builder()
                    .uri("/api/v1/coordinator/cleanup")
                    .method("POST")
                    .body(Body::empty())
                    .expect("request"),
            )
            .await
            .expect("response");

        assert_eq!(response.status(), StatusCode::OK);
        let bytes = response
            .into_body()
            .collect()
            .await
            .expect("collect")
            .to_bytes();
        let payload: serde_json::Value = serde_json::from_slice(&bytes).expect("json");
        for key in [
            "status",
            "resumed",
            "aggregated_performer_logs",
            "runtime_status",
            "exported_events_path",
            "removed_worktrees",
            "selected_task",
        ] {
            assert!(payload.get(key).is_some(), "missing {}", key);
        }
    }

    #[tokio::test]
    async fn coordinator_cleanup_endpoint_maps_errors() {
        let root = temp_root("cleanup-error");
        fs::create_dir_all(&root).expect("create root");
        let state = test_web_state(
            &root,
            Arc::new(WebTestEngine::with_cleanup_result(Err(
                MaccError::Validation("cleanup failed".to_string()),
            ))),
            WebAssetsMode::Embedded,
        );
        let app = build_web_router(state);
        let response = app
            .oneshot(
                Request::builder()
                    .uri("/api/v1/coordinator/cleanup")
                    .method("POST")
                    .body(Body::empty())
                    .expect("request"),
            )
            .await
            .expect("response");

        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
        let bytes = response
            .into_body()
            .collect()
            .await
            .expect("collect")
            .to_bytes();
        let payload: serde_json::Value = serde_json::from_slice(&bytes).expect("json");
        assert_eq!(payload["error"]["code"], WEB_ERR_VALIDATION);
        assert_eq!(payload["error"]["category"], "Validation");
        assert_eq!(payload["error"]["message"], "cleanup failed");
    }

    #[tokio::test]
    async fn coordinator_advance_endpoint_returns_envelope() {
        let root = temp_root("advance-ok");
        fs::create_dir_all(&root).expect("create root");
        let result = CoordinatorCommandResult {
            status: Some(CoordinatorStatus {
                total: 3,
                todo: 1,
                active: 1,
                blocked: 0,
                merged: 1,
                paused: false,
                pause_reason: None,
                pause_task_id: None,
                pause_phase: None,
                latest_error: None,
                failure_report: None,
                throttled_tools: vec![],
                effective_max_parallel: None,
            }),
            aggregated_performer_logs: Some(1),
            runtime_status: Some("running".to_string()),
            removed_worktrees: Some(0),
            ..CoordinatorCommandResult::default()
        };
        let state = test_web_state(
            &root,
            Arc::new(WebTestEngine::new(Ok(result))),
            WebAssetsMode::Embedded,
        );
        let app = build_web_router(state);
        let response = app
            .oneshot(
                Request::builder()
                    .uri("/api/v1/coordinator/advance")
                    .method("POST")
                    .body(Body::empty())
                    .expect("request"),
            )
            .await
            .expect("response");

        assert_eq!(response.status(), StatusCode::OK);
        let bytes = response
            .into_body()
            .collect()
            .await
            .expect("collect")
            .to_bytes();
        let payload: serde_json::Value = serde_json::from_slice(&bytes).expect("json");
        for key in [
            "status",
            "resumed",
            "aggregated_performer_logs",
            "runtime_status",
            "exported_events_path",
            "removed_worktrees",
            "selected_task",
        ] {
            assert!(payload.get(key).is_some(), "missing {}", key);
        }
    }

    #[tokio::test]
    async fn coordinator_advance_endpoint_maps_errors() {
        let root = temp_root("advance-error");
        fs::create_dir_all(&root).expect("create root");
        let state = test_web_state(
            &root,
            Arc::new(WebTestEngine::new(Err(MaccError::Validation(
                "advance failed".to_string(),
            )))),
            WebAssetsMode::Embedded,
        );
        let app = build_web_router(state);
        let response = app
            .oneshot(
                Request::builder()
                    .uri("/api/v1/coordinator/advance")
                    .method("POST")
                    .body(Body::empty())
                    .expect("request"),
            )
            .await
            .expect("response");

        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
        let bytes = response
            .into_body()
            .collect()
            .await
            .expect("collect")
            .to_bytes();
        let payload: serde_json::Value = serde_json::from_slice(&bytes).expect("json");
        assert_eq!(payload["error"]["code"], WEB_ERR_VALIDATION);
        assert_eq!(payload["error"]["category"], "Validation");
        assert_eq!(payload["error"]["message"], "advance failed");
        assert!(payload["error"].get("cause").is_none());
    }

    #[tokio::test]
    async fn coordinator_reconcile_endpoint_returns_envelope() {
        let root = temp_root("reconcile-ok");
        fs::create_dir_all(&root).expect("create root");
        let result = CoordinatorCommandResult {
            runtime_status: Some("running".to_string()),
            removed_worktrees: Some(1),
            ..CoordinatorCommandResult::default()
        };
        let state = test_web_state(
            &root,
            Arc::new(WebTestEngine::new(Ok(result))),
            WebAssetsMode::Embedded,
        );
        let app = build_web_router(state);
        let response = app
            .oneshot(
                Request::builder()
                    .uri("/api/v1/coordinator/reconcile")
                    .method("POST")
                    .body(Body::empty())
                    .expect("request"),
            )
            .await
            .expect("response");

        assert_eq!(response.status(), StatusCode::OK);
        let bytes = response
            .into_body()
            .collect()
            .await
            .expect("collect")
            .to_bytes();
        let payload: serde_json::Value = serde_json::from_slice(&bytes).expect("json");
        for key in [
            "status",
            "resumed",
            "aggregated_performer_logs",
            "runtime_status",
            "exported_events_path",
            "removed_worktrees",
            "selected_task",
        ] {
            assert!(payload.get(key).is_some(), "missing {}", key);
        }
        assert_eq!(payload["runtime_status"], "running");
        assert_eq!(payload["removed_worktrees"], 1);
    }

    #[tokio::test]
    async fn coordinator_reconcile_endpoint_maps_errors() {
        let root = temp_root("reconcile-error");
        fs::create_dir_all(&root).expect("create root");
        let state = test_web_state(
            &root,
            Arc::new(WebTestEngine::new(Err(MaccError::Validation(
                "reconcile failed".to_string(),
            )))),
            WebAssetsMode::Embedded,
        );
        let app = build_web_router(state);
        let response = app
            .oneshot(
                Request::builder()
                    .uri("/api/v1/coordinator/reconcile")
                    .method("POST")
                    .body(Body::empty())
                    .expect("request"),
            )
            .await
            .expect("response");

        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
        let bytes = response
            .into_body()
            .collect()
            .await
            .expect("collect")
            .to_bytes();
        let payload: serde_json::Value = serde_json::from_slice(&bytes).expect("json");
        assert_eq!(payload["error"]["code"], WEB_ERR_VALIDATION);
        assert_eq!(payload["error"]["category"], "Validation");
        assert_eq!(payload["error"]["message"], "reconcile failed");
        assert!(payload["error"].get("cause").is_none());
    }

    #[tokio::test]
    async fn coordinator_resume_endpoint_returns_envelope_with_resumed_true() {
        let root = temp_root("resume-ok");
        fs::create_dir_all(&root).expect("create root");
        macc_core::coordinator::state_runtime::write_coordinator_pause_file(
            &root,
            "global",
            "dev",
            "paused for test",
        )
        .expect("write pause file");
        let state = test_web_state(
            &root,
            Arc::new(WebTestEngine::with_resume_result(Ok(()))),
            WebAssetsMode::Embedded,
        );
        let app = build_web_router(state);
        let response = app
            .oneshot(
                Request::builder()
                    .uri("/api/v1/coordinator/resume")
                    .method("POST")
                    .body(Body::empty())
                    .expect("request"),
            )
            .await
            .expect("response");

        assert_eq!(response.status(), StatusCode::OK);
        let bytes = response
            .into_body()
            .collect()
            .await
            .expect("collect")
            .to_bytes();
        let payload: serde_json::Value = serde_json::from_slice(&bytes).expect("json");
        for key in [
            "status",
            "resumed",
            "aggregated_performer_logs",
            "runtime_status",
            "exported_events_path",
            "removed_worktrees",
            "selected_task",
        ] {
            assert!(payload.get(key).is_some(), "missing {}", key);
        }
        assert_eq!(payload["resumed"], true);
    }

    #[tokio::test]
    async fn coordinator_resume_endpoint_returns_envelope_with_resumed_false_when_not_paused() {
        let root = temp_root("resume-noop");
        fs::create_dir_all(&root).expect("create root");
        let state = test_web_state(
            &root,
            Arc::new(WebTestEngine::with_resume_result(Ok(()))),
            WebAssetsMode::Embedded,
        );
        let app = build_web_router(state);
        let response = app
            .oneshot(
                Request::builder()
                    .uri("/api/v1/coordinator/resume")
                    .method("POST")
                    .body(Body::empty())
                    .expect("request"),
            )
            .await
            .expect("response");

        assert_eq!(response.status(), StatusCode::OK);
        let bytes = response
            .into_body()
            .collect()
            .await
            .expect("collect")
            .to_bytes();
        let payload: serde_json::Value = serde_json::from_slice(&bytes).expect("json");
        assert_eq!(payload["resumed"], false);
    }

    #[tokio::test]
    async fn coordinator_resume_endpoint_maps_errors() {
        let root = temp_root("resume-error");
        fs::create_dir_all(&root).expect("create root");
        let state = test_web_state(
            &root,
            Arc::new(WebTestEngine::with_resume_result(Err(
                MaccError::Validation("resume failed".to_string()),
            ))),
            WebAssetsMode::Embedded,
        );
        let app = build_web_router(state);
        let response = app
            .oneshot(
                Request::builder()
                    .uri("/api/v1/coordinator/resume")
                    .method("POST")
                    .body(Body::empty())
                    .expect("request"),
            )
            .await
            .expect("response");

        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
        let bytes = response
            .into_body()
            .collect()
            .await
            .expect("collect")
            .to_bytes();
        let payload: serde_json::Value = serde_json::from_slice(&bytes).expect("json");
        assert_eq!(payload["error"]["code"], WEB_ERR_VALIDATION);
        assert_eq!(payload["error"]["category"], "Validation");
        assert_eq!(payload["error"]["message"], "resume failed");
    }
}
