//! Read-only usage telemetry; all normalization stays in the shared core service.
use super::{errors::ApiError, WebState};
use axum::{extract::State, Json};
pub(super) async fn status(
    State(state): State<WebState>,
) -> Result<Json<macc_core::session_store::usage::UsageView>, ApiError> {
    macc_core::session_store::usage::inspect(&state.paths.root)
        .map(Json)
        .map_err(ApiError::from)
}
