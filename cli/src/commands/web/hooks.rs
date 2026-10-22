//! Hook application operations; frontend never reimplements domain decisions.
use super::{errors::ApiError, WebState};
use axum::{body::Bytes, extract::State, http::HeaderMap, Json};
use macc_core::hooks::{execute, Action};
pub(super) async fn status(
    State(state): State<WebState>,
) -> Result<Json<serde_json::Value>, ApiError> {
    execute(&state.paths.root, Action::Status)
        .and_then(|value| {
            serde_json::to_value(value).map_err(|e| macc_core::MaccError::Validation(e.to_string()))
        })
        .map(Json)
        .map_err(api_error)
}
pub(super) async fn action(
    State(state): State<WebState>,
    headers: HeaderMap,
    body: Bytes,
) -> Result<Json<serde_json::Value>, ApiError> {
    let action: Action = serde_json::from_slice(&body).map_err(|e| {
        ApiError::from(macc_core::MaccError::Validation(format!(
            "Invalid Hook action: {e}"
        )))
    })?;
    if action.is_mutation() {
        super::mutation_gate::require_project_owner(&state, &headers)?;
    }
    tokio::task::spawn_blocking(move || execute(&state.paths.root, action))
        .await
        .map_err(|e| {
            ApiError::from(macc_core::MaccError::Validation(format!(
                "Hook operation worker failed: {e}"
            )))
        })?
        .and_then(|value| {
            serde_json::to_value(value).map_err(|e| macc_core::MaccError::Validation(e.to_string()))
        })
        .map(Json)
        .map_err(api_error)
}

pub(super) async fn inspect(body: Bytes) -> Result<Json<serde_json::Value>, ApiError> {
    let definition = serde_json::from_slice(&body).map_err(|e| {
        ApiError::from(macc_core::MaccError::Validation(format!(
            "Invalid hook manifest: {e}"
        )))
    })?;
    macc_core::hooks::review(definition)
        .and_then(|review| {
            serde_json::to_value(review)
                .map_err(|e| macc_core::MaccError::Validation(e.to_string()))
        })
        .map(Json)
        .map_err(api_error)
}

fn api_error(error: macc_core::MaccError) -> ApiError {
    if matches!(&error, macc_core::MaccError::Validation(message)
        if message == "Configuration revision conflict; reload and review your draft")
    {
        ApiError::new(
            axum::http::StatusCode::CONFLICT,
            super::errors::WEB_ERR_REGISTRY_CONFLICT,
            "Conflict",
            error.to_string(),
            false,
            Some("Reload hooks and review the requested change again".into()),
            None,
            None,
        )
    } else {
        ApiError::from(error)
    }
}
