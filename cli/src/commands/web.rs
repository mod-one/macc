use crate::commands::AppContext;
use crate::commands::Command;
use crate::services::engine_provider::SharedEngine;
use axum::extract::State;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use axum::Json;
use axum::Router;
use macc_core::service::coordinator_workflow::CoordinatorStatus;
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
            let listener = tokio::net::TcpListener::bind(addr).await.map_err(|e| {
                MaccError::Io {
                    path: addr.to_string(),
                    action: "bind web server".into(),
                    source: e,
                }
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
}
