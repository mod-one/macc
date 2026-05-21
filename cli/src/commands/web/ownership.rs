// Stub handlers — full implementations delivered by L6-WEB-001.
use super::WebState;
use axum::extract::{Path, State};
use axum::http::StatusCode;

pub async fn list_processes_handler(State(_state): State<WebState>) -> StatusCode {
    StatusCode::NOT_IMPLEMENTED
}

pub async fn get_process_ownership_handler(
    State(_state): State<WebState>,
    Path(_handle): Path<String>,
) -> StatusCode {
    StatusCode::NOT_IMPLEMENTED
}

pub async fn claim_ownership_handler(
    State(_state): State<WebState>,
    Path(_handle): Path<String>,
) -> StatusCode {
    StatusCode::NOT_IMPLEMENTED
}

pub async fn release_ownership_handler(
    State(_state): State<WebState>,
    Path(_handle): Path<String>,
) -> StatusCode {
    StatusCode::NOT_IMPLEMENTED
}

pub async fn add_viewer_handler(
    State(_state): State<WebState>,
    Path(_handle): Path<String>,
) -> StatusCode {
    StatusCode::NOT_IMPLEMENTED
}

pub async fn remove_viewer_handler(
    State(_state): State<WebState>,
    Path(_handle): Path<String>,
) -> StatusCode {
    StatusCode::NOT_IMPLEMENTED
}

pub async fn request_takeover_handler(
    State(_state): State<WebState>,
    Path(_handle): Path<String>,
) -> StatusCode {
    StatusCode::NOT_IMPLEMENTED
}

pub async fn respond_takeover_handler(
    State(_state): State<WebState>,
    Path(_handle): Path<String>,
) -> StatusCode {
    StatusCode::NOT_IMPLEMENTED
}

pub async fn heartbeat_handler(
    State(_state): State<WebState>,
    Path(_handle): Path<String>,
) -> StatusCode {
    StatusCode::NOT_IMPLEMENTED
}
