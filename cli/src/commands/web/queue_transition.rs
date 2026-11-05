//! Existing ownership gate plus explicit, revision-bound PRD queue consent.
use super::{errors::ApiError, WebState};
use axum::{extract::State, http::HeaderMap, Json};
use macc_core::prd_queue::review::{self, Review};
use serde::Deserialize;
#[derive(Deserialize)]
pub(super) struct Acceptance {
    revision: String,
    confirmed: bool,
}
pub(super) async fn inspect(State(state): State<WebState>) -> Result<Json<Review>, ApiError> {
    tokio::task::spawn_blocking(move || {
        let config = macc_core::load_canonical_config(&state.paths.config_path)?;
        review::inspect(
            &state.paths.root,
            &config.automation.coordinator.unwrap_or_default(),
        )
    })
    .await
    .map_err(|e| ApiError::validation(e.to_string()))?
    .map(Json)
    .map_err(ApiError::from)
}
pub(super) async fn accept(
    State(state): State<WebState>,
    headers: HeaderMap,
    Json(input): Json<Acceptance>,
) -> Result<Json<serde_json::Value>, ApiError> {
    super::mutation_gate::require_project_owner(&state, &headers)?;
    if !input.confirmed {
        return Err(ApiError::confirmation_required(
            "Confirm the PRD queue transition first",
            None,
        ));
    }
    tokio::task::spawn_blocking(move || {
        let config = macc_core::load_canonical_config(&state.paths.config_path)?;
        review::accept(
            &state.paths.root,
            &config.automation.coordinator.unwrap_or_default(),
            &input.revision,
            input.confirmed,
        )
    })
    .await
    .map_err(|e| ApiError::validation(e.to_string()))??;
    Ok(Json(serde_json::json!({"accepted":true})))
}
