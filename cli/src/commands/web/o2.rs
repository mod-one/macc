//! O2 application operations; frontend never reimplements domain decisions.
use super::{errors::ApiError, WebState};
use axum::{body::Bytes, extract::State, http::HeaderMap, Json};
use macc_core::o2::actions::{execute, Action};
pub(super) async fn status(
    State(state): State<WebState>,
) -> Result<Json<serde_json::Value>, ApiError> {
    execute(&state.paths.root, Action::Status)
        .map(Json)
        .map_err(ApiError::from)
}
pub(super) async fn action(
    State(state): State<WebState>,
    headers: HeaderMap,
    body: Bytes,
) -> Result<Json<serde_json::Value>, ApiError> {
    let action: Action = serde_json::from_slice(&body).map_err(|e| {
        ApiError::from(macc_core::MaccError::Validation(format!(
            "Invalid O2 action: {e}"
        )))
    })?;
    if action.is_mutation() {
        super::mutation_gate::require_project_owner(&state, &headers)?;
    }
    execute(&state.paths.root, action)
        .map(Json)
        .map_err(ApiError::from)
}
