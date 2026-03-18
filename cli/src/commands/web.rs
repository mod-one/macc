use crate::commands::AppContext;
use crate::commands::Command;
use crate::services::engine_provider::SharedEngine;
use axum::extract::State;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::Json;
use axum::Router;
use macc_core::coordinator::task_selector::SelectedTask;
use macc_core::coordinator::types::CoordinatorEnvConfig;
use macc_core::service::coordinator_workflow::{
    CoordinatorCommand, CoordinatorCommandRequest, CoordinatorCommandResult, CoordinatorStatus,
    ThrottledToolStatus,
};
use macc_core::service::diagnostic::{FailureKind, FailureReport};
use macc_core::{MaccError, ProjectPaths, Result};
use serde::Serialize;
use std::net::{IpAddr, Ipv4Addr, SocketAddr};

pub struct WebCommand {
    app: AppContext,
}

impl WebCommand {
    pub fn new(app: AppContext) -> Self {
        Self { app }
    }
}

impl Command for WebCommand {
    fn run(&self) -> Result<()> {
        let port = self.web_port()?;
        let addr = SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), port);
        let state = WebState {
            engine: self.app.engine.clone(),
            paths: self.app.project_paths()?,
        };
        let app = build_web_router(state);

        println!("Web server starting on http://{}...", addr);

        let runtime = tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .build()
            .map_err(|e| MaccError::Validation(format!("build web runtime: {}", e)))?;

        runtime.block_on(async move {
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
    fn web_port(&self) -> Result<u16> {
        let canonical = self.app.canonical_config()?;
        Ok(canonical.settings.web_port.unwrap_or(3450))
    }
}

#[derive(Clone)]
struct WebState {
    engine: SharedEngine,
    paths: ProjectPaths,
}

fn build_web_router(state: WebState) -> Router {
    Router::new()
        .route("/api/v1/status", get(status_handler))
        .route("/api/v1/coordinator/run", post(coordinator_run_handler))
        .route("/api/v1/coordinator/stop", post(coordinator_stop_handler))
        .with_state(state)
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

struct ApiError {
    status: StatusCode,
    body: ApiErrorEnvelope,
}

impl ApiError {
    fn new(
        status: StatusCode,
        category: &str,
        message: String,
        context: Option<serde_json::Value>,
        cause: Option<String>,
    ) -> Self {
        Self {
            status,
            body: ApiErrorEnvelope {
                error: ApiErrorBody {
                    code: "MACC-WEB-0000".to_string(),
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
            MaccError::Validation(message) => {
                ApiError::new(StatusCode::BAD_REQUEST, "Validation", message, None, None)
            }
            MaccError::ToolSpec {
                path,
                line,
                column,
                message,
            } => ApiError::new(
                StatusCode::BAD_REQUEST,
                "Validation",
                message,
                Some(serde_json::json!({
                    "path": path,
                    "line": line,
                    "column": column,
                })),
                None,
            ),
            MaccError::UserScopeNotAllowed(message) => {
                ApiError::new(StatusCode::FORBIDDEN, "Auth", message, None, None)
            }
            MaccError::ProjectRootNotFound { start_dir } => ApiError::new(
                StatusCode::NOT_FOUND,
                "NotFound",
                "Project root not found.".to_string(),
                Some(serde_json::json!({ "start_dir": start_dir })),
                None,
            ),
            MaccError::HomeDirNotFound => ApiError::new(
                StatusCode::NOT_FOUND,
                "NotFound",
                "User home directory not found.".to_string(),
                None,
                None,
            ),
            MaccError::SecretDetected { path, details } => ApiError::new(
                StatusCode::BAD_REQUEST,
                "Validation",
                "Secret detected in output.".to_string(),
                Some(serde_json::json!({ "path": path, "details": details })),
                None,
            ),
            MaccError::Config { path, source } => ApiError::new(
                StatusCode::BAD_REQUEST,
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
                "Dependency",
                format!("I/O error during {}.", action),
                Some(serde_json::json!({ "path": path, "action": action })),
                Some(source.to_string()),
            ),
            MaccError::Coordinator { code, message } => ApiError::new(
                StatusCode::INTERNAL_SERVER_ERROR,
                "Coordinator",
                message,
                Some(serde_json::json!({ "code": code })),
                None,
            ),
            MaccError::Storage { backend, message } => ApiError::new(
                StatusCode::INTERNAL_SERVER_ERROR,
                "Storage",
                message,
                Some(serde_json::json!({ "backend": backend })),
                None,
            ),
            MaccError::Git { operation, message } => ApiError::new(
                StatusCode::INTERNAL_SERVER_ERROR,
                "Git",
                message,
                Some(serde_json::json!({ "operation": operation })),
                None,
            ),
            MaccError::Fetch { url, message } => ApiError::new(
                StatusCode::BAD_GATEWAY,
                "Fetch",
                message,
                Some(serde_json::json!({ "url": url })),
                None,
            ),
            MaccError::Catalog { operation, message } => ApiError::new(
                StatusCode::BAD_REQUEST,
                "Catalog",
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
    use axum::body::Body;
    use axum::http::Request;
    use http_body_util::BodyExt;
    use macc_core::TestEngine;
    use std::fs;
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

    struct WebTestEngine {
        inner: TestEngine,
        run_result:
            std::sync::Mutex<Option<std::result::Result<CoordinatorCommandResult, MaccError>>>,
        stop_result: std::sync::Mutex<Option<std::result::Result<(), MaccError>>>,
    }

    impl WebTestEngine {
        fn new(result: std::result::Result<CoordinatorCommandResult, MaccError>) -> Self {
            Self {
                inner: TestEngine::with_fixtures(),
                run_result: std::sync::Mutex::new(Some(result)),
                stop_result: std::sync::Mutex::new(Some(Ok(()))),
            }
        }

        fn with_stop_result(result: std::result::Result<(), MaccError>) -> Self {
            Self {
                inner: TestEngine::with_fixtures(),
                run_result: std::sync::Mutex::new(Some(Ok(CoordinatorCommandResult::default()))),
                stop_result: std::sync::Mutex::new(Some(result)),
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
    }

    #[tokio::test]
    async fn status_endpoint_returns_status_payload() {
        let root = temp_root("ok");
        fs::create_dir_all(&root).expect("create root");
        let state = WebState {
            engine: Arc::new(TestEngine::with_fixtures()),
            paths: ProjectPaths::from_root(&root),
        };
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
        let state = WebState {
            engine: Arc::new(TestEngine::with_fixtures()),
            paths: ProjectPaths::from_root(&root),
        };
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
        assert_eq!(payload["error"]["code"], "MACC-WEB-0000");
        assert_eq!(payload["error"]["category"], "Validation");
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
        let state = WebState {
            engine: Arc::new(WebTestEngine::new(Ok(result))),
            paths: ProjectPaths::from_root(&root),
        };
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
        let state = WebState {
            engine: Arc::new(WebTestEngine::new(Err(MaccError::Validation(
                "coordinator failed".to_string(),
            )))),
            paths: ProjectPaths::from_root(&root),
        };
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
        assert_eq!(payload["error"]["code"], "MACC-WEB-0000");
        assert_eq!(payload["error"]["category"], "Validation");
    }

    #[tokio::test]
    async fn coordinator_stop_endpoint_returns_envelope() {
        let root = temp_root("stop-ok");
        fs::create_dir_all(&root).expect("create root");
        let state = WebState {
            engine: Arc::new(WebTestEngine::with_stop_result(Ok(()))),
            paths: ProjectPaths::from_root(&root),
        };
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
        let state = WebState {
            engine: Arc::new(WebTestEngine::with_stop_result(Err(MaccError::Validation(
                "stop failed".to_string(),
            )))),
            paths: ProjectPaths::from_root(&root),
        };
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
        assert_eq!(payload["error"]["code"], "MACC-WEB-0000");
        assert_eq!(payload["error"]["category"], "Validation");
    }
}
