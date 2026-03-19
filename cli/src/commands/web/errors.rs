use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::Json;
use macc_core::MaccError;
use serde::Serialize;

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

pub(super) const WEB_ERR_VALIDATION: &str = "MACC-WEB-0100";
const WEB_ERR_TOOLSPEC: &str = "MACC-WEB-0101";
const WEB_ERR_SECRET_DETECTED: &str = "MACC-WEB-0102";
const WEB_ERR_CONFIG: &str = "MACC-WEB-0103";
const WEB_ERR_CATALOG: &str = "MACC-WEB-0104";
const WEB_ERR_AUTH_SCOPE: &str = "MACC-WEB-0200";
const WEB_ERR_PROJECT_ROOT_NOT_FOUND: &str = "MACC-WEB-0300";
const WEB_ERR_HOME_NOT_FOUND: &str = "MACC-WEB-0301";
const WEB_ERR_IO: &str = "MACC-WEB-0400";
const WEB_ERR_FETCH: &str = "MACC-WEB-0401";
pub(super) const WEB_ERR_COORDINATOR: &str = "MACC-WEB-0500";
pub(super) const WEB_ERR_STORAGE: &str = "MACC-WEB-0501";
const WEB_ERR_GIT: &str = "MACC-WEB-0502";

pub(super) struct ApiError {
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
