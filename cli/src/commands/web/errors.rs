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
    retryable: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    recommended_action: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    context: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    cause: Option<String>,
}

pub(super) const WEB_ERR_VALIDATION: &str = "MACC-WEB-1000";
const WEB_ERR_TOOLSPEC: &str = "MACC-WEB-1001";
const WEB_ERR_SECRET_DETECTED: &str = "MACC-WEB-1002";
const WEB_ERR_CONFIG: &str = "MACC-WEB-1003";
const WEB_ERR_CATALOG: &str = "MACC-WEB-1004";
const WEB_ERR_AUTH_SCOPE: &str = "MACC-WEB-3000";
const WEB_ERR_PROJECT_ROOT_NOT_FOUND: &str = "MACC-WEB-2000";
const WEB_ERR_HOME_NOT_FOUND: &str = "MACC-WEB-2001";
const WEB_ERR_IO: &str = "MACC-WEB-4000";
const WEB_ERR_FETCH: &str = "MACC-WEB-4001";
pub(super) const WEB_ERR_COORDINATOR: &str = "MACC-WEB-5000";
pub(super) const WEB_ERR_STORAGE: &str = "MACC-WEB-5001";
const WEB_ERR_GIT: &str = "MACC-WEB-5002";

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
        retryable: bool,
        recommended_action: Option<String>,
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
                    retryable,
                    recommended_action,
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
                false,
                Some("Check input data for correctness".to_string()),
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
                false,
                Some("Fix the tool specification format".to_string()),
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
                false,
                Some("Ensure correct authentication scope".to_string()),
                None,
                None,
            ),
            MaccError::ProjectRootNotFound { start_dir } => ApiError::new(
                StatusCode::NOT_FOUND,
                WEB_ERR_PROJECT_ROOT_NOT_FOUND,
                "NotFound",
                "Project root not found.".to_string(),
                false,
                Some("Run the command from within a valid project directory".to_string()),
                Some(serde_json::json!({ "start_dir": start_dir })),
                None,
            ),
            MaccError::HomeDirNotFound => ApiError::new(
                StatusCode::NOT_FOUND,
                WEB_ERR_HOME_NOT_FOUND,
                "NotFound",
                "User home directory not found.".to_string(),
                false,
                Some("Ensure HOME environment variable is set".to_string()),
                None,
                None,
            ),
            MaccError::SecretDetected { path, details } => ApiError::new(
                StatusCode::BAD_REQUEST,
                WEB_ERR_SECRET_DETECTED,
                "Validation",
                "Secret detected in output.".to_string(),
                false,
                Some("Remove the detected secret from the file or output".to_string()),
                Some(serde_json::json!({ "path": path, "details": details })),
                None,
            ),
            MaccError::Config { path, source } => ApiError::new(
                StatusCode::BAD_REQUEST,
                WEB_ERR_CONFIG,
                "Validation",
                format!("Configuration error in {}: {}", path, source),
                false,
                Some("Fix the syntax or content of the configuration file".to_string()),
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
                true,
                Some("Check file permissions and disk space, then retry".to_string()),
                Some(serde_json::json!({ "path": path, "action": action })),
                Some(source.to_string()),
            ),
            MaccError::Coordinator { code, message } => ApiError::new(
                StatusCode::INTERNAL_SERVER_ERROR,
                WEB_ERR_COORDINATOR,
                "Internal",
                message,
                true,
                Some("Retry the operation. If it persists, check coordinator logs".to_string()),
                Some(serde_json::json!({ "code": code })),
                None,
            ),
            MaccError::Storage { backend, message } => ApiError::new(
                StatusCode::INTERNAL_SERVER_ERROR,
                WEB_ERR_STORAGE,
                "Internal",
                message,
                true,
                Some("Check storage backend availability and retry".to_string()),
                Some(serde_json::json!({ "backend": backend })),
                None,
            ),
            MaccError::Git { operation, message } => ApiError::new(
                StatusCode::INTERNAL_SERVER_ERROR,
                WEB_ERR_GIT,
                "Internal",
                message,
                false,
                Some("Check git repository state manually".to_string()),
                Some(serde_json::json!({ "operation": operation })),
                None,
            ),
            MaccError::Fetch { url, message } => ApiError::new(
                StatusCode::BAD_GATEWAY,
                WEB_ERR_FETCH,
                "Dependency",
                message,
                true,
                Some("Check network connection and retry".to_string()),
                Some(serde_json::json!({ "url": url })),
                None,
            ),
            MaccError::Catalog { operation, message } => ApiError::new(
                StatusCode::BAD_REQUEST,
                WEB_ERR_CATALOG,
                "Validation",
                message,
                false,
                Some("Check catalog operation payload".to_string()),
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
    use macc_core::MaccError;

    #[test]
    fn api_error_mapping_validation() {
        let err: ApiError = MaccError::Validation("bad input".to_string()).into();
        assert_eq!(err.body.error.code, "MACC-WEB-1000");
        assert_eq!(err.body.error.category, "Validation");
        assert_eq!(err.body.error.retryable, false);
        assert!(err.body.error.recommended_action.is_some());
    }

    #[test]
    fn api_error_mapping_not_found() {
        let err: ApiError = MaccError::ProjectRootNotFound { start_dir: "/tmp".to_string() }.into();
        assert_eq!(err.body.error.code, "MACC-WEB-2000");
        assert_eq!(err.body.error.category, "NotFound");
        assert_eq!(err.body.error.retryable, false);
        assert!(err.body.error.recommended_action.is_some());
    }

    #[test]
    fn api_error_mapping_conflict_auth() {
        let err: ApiError = MaccError::UserScopeNotAllowed("no".to_string()).into();
        assert_eq!(err.body.error.code, "MACC-WEB-3000");
        assert_eq!(err.body.error.category, "Auth");
        assert_eq!(err.body.error.retryable, false);
        assert!(err.body.error.recommended_action.is_some());
    }

    #[test]
    fn api_error_mapping_dependency() {
        let err: ApiError = MaccError::Io { path: "f".to_string(), action: "read".to_string(), source: std::io::Error::from(std::io::ErrorKind::NotFound).into() }.into();
        assert_eq!(err.body.error.code, "MACC-WEB-4000");
        assert_eq!(err.body.error.category, "Dependency");
        assert_eq!(err.body.error.retryable, true);
        assert!(err.body.error.recommended_action.is_some());
    }

    #[test]
    fn api_error_mapping_internal() {
        let err: ApiError = MaccError::Coordinator { code: "C", message: "M".to_string() }.into();
        assert_eq!(err.body.error.code, "MACC-WEB-5000");
        assert_eq!(err.body.error.category, "Internal");
        assert_eq!(err.body.error.retryable, true);
        assert!(err.body.error.recommended_action.is_some());
    }
}
