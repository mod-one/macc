#![allow(clippy::result_large_err)]

use super::errors::ApiError;
use super::WebState;
use axum::extract::{Path, Query, State};
use axum::http::HeaderMap;
use axum::Json;
use macc_core::process_ownership::{
    ClientIdentity, ClientKind, OwnershipRecord, OwnershipStatus, ProcessHandle, ProcessKind,
};
use serde::{Deserialize, Serialize};

const WEB_CLIENT_HEADER: &str = "X-Macc-Client-Id";

#[derive(Debug, Deserialize)]
pub(super) struct HandleQuery {
    #[serde(default)]
    pid: Option<i32>,
}

#[derive(Debug, Deserialize)]
pub(super) struct ClaimRequest {
    #[serde(default)]
    pid: Option<i32>,
}

#[derive(Debug, Deserialize)]
pub(super) struct ReleaseRequest {
    #[serde(default)]
    pid: Option<i32>,
}

#[derive(Debug, Deserialize)]
pub(super) struct ViewerRequest {
    #[serde(default)]
    pid: Option<i32>,
}

#[derive(Debug, Deserialize)]
pub(super) struct TakeoverRequestPayload {
    #[serde(default)]
    pid: Option<i32>,
}

#[derive(Debug, Deserialize)]
pub(super) struct TakeoverRespondPayload {
    #[serde(default)]
    pid: Option<i32>,
    request_id: String,
    accept: bool,
}

#[derive(Debug, Deserialize)]
pub(super) struct HeartbeatPayload {
    #[serde(default)]
    pid: Option<i32>,
}

#[derive(Debug, Serialize)]
pub(super) struct ClaimResponse {
    status: String,
}

#[derive(Debug, Serialize)]
pub(super) struct TakeoverRequestResponse {
    request_id: String,
}

pub async fn list_processes_handler(
    State(state): State<WebState>,
) -> Result<Json<Vec<OwnershipRecord>>, ApiError> {
    let records = state.engine.process_list_running(&state.paths.root)?;
    Ok(Json(records))
}

pub async fn get_process_ownership_handler(
    State(state): State<WebState>,
    Path(kind): Path<String>,
    Query(query): Query<HandleQuery>,
) -> Result<Json<Option<OwnershipRecord>>, ApiError> {
    let handle = build_handle(&state, &kind, query.pid)?;
    let record = state
        .engine
        .process_ownership_status(&state.paths.root, &handle)?;
    Ok(Json(record))
}

pub async fn claim_ownership_handler(
    State(state): State<WebState>,
    Path(kind): Path<String>,
    headers: HeaderMap,
    Json(req): Json<ClaimRequest>,
) -> Result<Json<ClaimResponse>, ApiError> {
    let identity = client_identity(&headers)?;
    let handle = build_handle(&state, &kind, req.pid)?;
    let (status, _owner_guard, _viewer_guard) =
        state
            .engine
            .process_ownership_claim(&state.paths.root, handle, identity)?;
    Ok(Json(ClaimResponse {
        status: status_label(status),
    }))
}

pub async fn release_ownership_handler(
    State(state): State<WebState>,
    Path(kind): Path<String>,
    headers: HeaderMap,
    Json(req): Json<ReleaseRequest>,
) -> Result<axum::http::StatusCode, ApiError> {
    let client_id = client_id_from_headers(&headers)?;
    let handle = build_handle(&state, &kind, req.pid)?;
    state
        .engine
        .process_ownership_release(&state.paths.root, &handle, &client_id)?;
    Ok(axum::http::StatusCode::NO_CONTENT)
}

pub async fn add_viewer_handler(
    State(state): State<WebState>,
    Path(kind): Path<String>,
    headers: HeaderMap,
    Json(req): Json<ViewerRequest>,
) -> Result<axum::http::StatusCode, ApiError> {
    let identity = client_identity(&headers)?;
    let handle = build_handle(&state, &kind, req.pid)?;
    state
        .engine
        .process_register_viewer(&state.paths.root, handle, identity)?;
    Ok(axum::http::StatusCode::NO_CONTENT)
}

pub async fn remove_viewer_handler(
    State(state): State<WebState>,
    Path(kind): Path<String>,
    headers: HeaderMap,
    Query(query): Query<HandleQuery>,
) -> Result<axum::http::StatusCode, ApiError> {
    let client_id = client_id_from_headers(&headers)?;
    let handle = build_handle(&state, &kind, query.pid)?;
    macc_core::service::process_ownership::unregister_viewer(
        &state.paths.root,
        &handle,
        &client_id,
    )?;
    Ok(axum::http::StatusCode::NO_CONTENT)
}

pub async fn request_takeover_handler(
    State(state): State<WebState>,
    Path(kind): Path<String>,
    headers: HeaderMap,
    Json(req): Json<TakeoverRequestPayload>,
) -> Result<Json<TakeoverRequestResponse>, ApiError> {
    let identity = client_identity(&headers)?;
    let handle = build_handle(&state, &kind, req.pid)?;
    let request_id =
        state
            .engine
            .process_ownership_request_takeover(&state.paths.root, &handle, identity)?;
    Ok(Json(TakeoverRequestResponse { request_id }))
}

pub async fn respond_takeover_handler(
    State(state): State<WebState>,
    Path(kind): Path<String>,
    headers: HeaderMap,
    Json(req): Json<TakeoverRespondPayload>,
) -> Result<axum::http::StatusCode, ApiError> {
    let client_id = client_id_from_headers(&headers)?;
    let handle = build_handle(&state, &kind, req.pid)?;
    state.engine.process_ownership_respond_takeover(
        &state.paths.root,
        &handle,
        &client_id,
        &req.request_id,
        req.accept,
    )?;
    Ok(axum::http::StatusCode::NO_CONTENT)
}

pub async fn heartbeat_handler(
    State(state): State<WebState>,
    Path(kind): Path<String>,
    headers: HeaderMap,
    Json(req): Json<HeartbeatPayload>,
) -> Result<axum::http::StatusCode, ApiError> {
    let client_id = client_id_from_headers(&headers)?;
    let handle = build_handle(&state, &kind, req.pid)?;
    state
        .engine
        .process_heartbeat(&state.paths.root, &handle, &client_id)?;
    Ok(axum::http::StatusCode::NO_CONTENT)
}

fn build_handle(state: &WebState, kind: &str, pid: Option<i32>) -> Result<ProcessHandle, ApiError> {
    let kind = parse_kind(kind)?;
    Ok(ProcessHandle {
        kind,
        project_root: state.paths.root.clone(),
        pid,
    })
}

fn parse_kind(raw: &str) -> Result<ProcessKind, ApiError> {
    match raw.to_ascii_lowercase().as_str() {
        "coordinator" => Ok(ProcessKind::Coordinator),
        "supervisor" => Ok(ProcessKind::Supervisor),
        "webserver" | "web-server" | "web_server" => Ok(ProcessKind::WebServer),
        "terminalsession" | "terminal-session" | "terminal_session" => {
            Ok(ProcessKind::TerminalSession)
        }
        "project" => Ok(ProcessKind::Project),
        other => Err(ApiError::validation(format!(
            "Unknown process kind '{other}'. Expected one of: coordinator, supervisor, webserver, terminalsession, project"
        ))),
    }
}

fn client_id_from_headers(headers: &HeaderMap) -> Result<String, ApiError> {
    headers
        .get(WEB_CLIENT_HEADER)
        .and_then(|v| v.to_str().ok())
        .map(|v| v.trim())
        .filter(|v| !v.is_empty())
        .map(str::to_string)
        .ok_or_else(|| {
            ApiError::validation(format!(
                "Missing required header '{}' (web client identity).",
                WEB_CLIENT_HEADER
            ))
        })
}

fn client_identity(headers: &HeaderMap) -> Result<ClientIdentity, ApiError> {
    let now = chrono::Utc::now().to_rfc3339();
    Ok(ClientIdentity {
        client_id: client_id_from_headers(headers)?,
        kind: ClientKind::Web,
        connected_at: now.clone(),
        last_heartbeat: now,
    })
}

fn status_label(status: OwnershipStatus) -> String {
    match status {
        OwnershipStatus::Owner => "owner".to_string(),
        OwnershipStatus::Viewer => "viewer".to_string(),
        OwnershipStatus::Unregistered => "unregistered".to_string(),
    }
}
