use super::WebState;
use axum::extract::State;
use axum::Json;
use macc_core::engine::Engine;
use serde_json::Value;

pub(crate) async fn get_snapshot_handler(
    State(state): State<WebState>,
) -> Result<Json<Value>, super::errors::ApiError> {
    let snapshot = state
        .engine
        .runtime_snapshot(&state.paths)
        .map_err(|e| super::errors::ApiError::validation(e.to_string()))?;
    let json = serde_json::to_value(&snapshot)
        .map_err(|e| super::errors::ApiError::validation(e.to_string()))?;
    Ok(Json(json))
}

pub(crate) async fn get_worker_snapshot_handler(
    State(state): State<WebState>,
    axum::extract::Path(worker_id): axum::extract::Path<String>,
) -> Result<Json<Value>, super::errors::ApiError> {
    let snapshot = state
        .engine
        .runtime_snapshot(&state.paths)
        .map_err(|e| super::errors::ApiError::validation(e.to_string()))?;
    let worker = snapshot.workers.iter().find(|w| w.id == worker_id);
    match worker {
        Some(w) => {
            let json = serde_json::to_value(w)
                .map_err(|e| super::errors::ApiError::validation(e.to_string()))?;
            Ok(Json(json))
        }
        None => Err(super::errors::ApiError::not_found(
            format!("Worker '{}' not found", worker_id),
            None,
        )),
    }
}
