//! Graphical launch review; never falls back to stdin questions.
use super::{coordinator::ApiCoordinatorCommandResult, errors::ApiError, WebState};
use axum::{body::Bytes, extract::State, http::HeaderMap, Json};
use macc_core::service::{
    coordinator_workflow::CoordinatorCommandResult,
    launch_review::{LaunchDecision, LaunchReview},
};
use std::sync::atomic::Ordering;

pub(super) async fn review(State(state): State<WebState>) -> Result<Json<LaunchReview>, ApiError> {
    let engine = state.engine.clone();
    let paths = state.paths.clone();
    tokio::task::spawn_blocking(move || engine.coordinator_launch_review(&paths))
        .await
        .map_err(|e| ApiError::validation(e.to_string()))?
        .map(Json)
        .map_err(ApiError::from)
}
pub(super) async fn run(
    State(state): State<WebState>,
    headers: HeaderMap,
    body: Bytes,
) -> Result<Json<ApiCoordinatorCommandResult>, ApiError> {
    super::mutation_gate::require_project_owner(&state, &headers)?;
    if body.is_empty() {
        return Err(ApiError::confirmation_required(
            "Review and confirm the coordinator launch first.",
            None,
        ));
    }
    let decision: LaunchDecision = serde_json::from_slice(&body)
        .map_err(|e| ApiError::validation(format!("Invalid launch decision: {e}")))?;
    if !decision.confirmed {
        return Err(ApiError::confirmation_required(
            "Confirm the coordinator launch first.",
            None,
        ));
    }
    let engine = state.engine.clone();
    let paths = state.paths.clone();
    let client_id = super::mutation_gate::client_id_from_headers(&headers);
    let child_id = client_id.clone();
    tokio::task::spawn_blocking(move || {
        engine.coordinator_launch_confirmed(&paths, &decision, child_id.as_deref())
    })
    .await
    .map_err(|e| ApiError::validation(e.to_string()))??;
    if let Some(client_id) = client_id {
        let generation = state
            .coordinator_lease_generation
            .fetch_add(1, Ordering::SeqCst)
            .saturating_add(1);
        super::coordinator_lease::spawn_release_monitor(state.clone(), client_id, generation);
    }
    Ok(Json(ApiCoordinatorCommandResult::from(
        CoordinatorCommandResult::default(),
    )))
}
