//! Session operations share core services with CLI and TUI.
use super::{errors::ApiError, WebState};
use axum::body::Bytes;
use axum::{
    extract::{Path, State},
    http::{HeaderMap, StatusCode},
    Json,
};
use macc_core::session_store as store;
use serde::Deserialize;
fn request<T: serde::de::DeserializeOwned>(body: &Bytes) -> Result<T, ApiError> {
    serde_json::from_slice(body).map_err(|e| {
        ApiError::from(macc_core::MaccError::Validation(format!(
            "Invalid session request: {e}"
        )))
    })
}
fn failure(e: macc_core::MaccError) -> ApiError {
    if e.to_string().contains("revision conflict") {
        ApiError::new(
            StatusCode::CONFLICT,
            "MACC-WEB-3004",
            "Conflict",
            e.to_string(),
            false,
            Some("Reload settings and review the preserved draft".into()),
            None,
            None,
        )
    } else {
        ApiError::from(e)
    }
}
#[derive(Deserialize)]
pub(super) struct PolicyRequest {
    revision: String,
    policy: serde_json::Value,
}
#[derive(Deserialize)]
pub(super) struct MigrationRequest {
    #[serde(default)]
    apply: bool,
    source_sha256: Option<String>,
}
#[derive(Deserialize)]
pub(super) struct Operation {
    action: String,
    #[serde(default)]
    confirm: bool,
    id: Option<String>,
}
pub(super) async fn status(
    State(state): State<WebState>,
) -> Result<Json<store::StoreView>, ApiError> {
    store::inspect(&state.paths.root)
        .map(Json)
        .map_err(ApiError::from)
}
pub(super) async fn policy(
    State(state): State<WebState>,
    headers: HeaderMap,
    body: Bytes,
) -> Result<Json<store::StoreView>, ApiError> {
    super::mutation_gate::require_project_owner(&state, &headers)?;
    let request: PolicyRequest = request(&body)?;
    let policy: Option<store::SessionPolicy> = serde_json::from_value(request.policy)
        .map_err(|e| ApiError::from(macc_core::MaccError::Validation(e.to_string())))?;
    match policy {
        Some(p) => store::update_policy(&state.paths.root, &request.revision, &p),
        None => store::remove_policy_override(&state.paths.root, &request.revision),
    }
    .map(Json)
    .map_err(failure)
}
pub(super) async fn migration(
    State(state): State<WebState>,
    Path(direction): Path<String>,
    headers: HeaderMap,
    body: Bytes,
) -> Result<Json<store::MigrationReport>, ApiError> {
    let request: MigrationRequest = request(&body)?;
    if request.apply {
        super::mutation_gate::require_project_owner(&state, &headers)?;
    }
    let op = match direction.as_str() {
        "migrate" => store::migrate,
        "rollback" => store::rollback,
        "upgrade" => store::checkpoints::upgrade,
        _ => {
            return Err(ApiError::from(macc_core::MaccError::Validation(
                "Unknown session migration".into(),
            )))
        }
    };
    op(
        &state.paths.root,
        request.apply,
        request.source_sha256.as_deref(),
    )
    .map(Json)
    .map_err(ApiError::from)
}
pub(super) async fn action(
    State(state): State<WebState>,
    headers: HeaderMap,
    body: Bytes,
) -> Result<Json<serde_json::Value>, ApiError> {
    super::mutation_gate::require_project_owner(&state, &headers)?;
    let request: Operation = request(&body)?;
    let result = match request.action.as_str() {
        "recover" => {
            serde_json::json!({"quarantined":store::recover(&state.paths.root).map_err(ApiError::from)?})
        }
        "backup" => {
            serde_json::json!({"backup":store::backup(&state.paths.root).map_err(ApiError::from)?})
        }
        "close" => {
            if !request.confirm {
                return Err(ApiError::new(
                    StatusCode::BAD_REQUEST,
                    super::errors::WEB_ERR_CONFIRMATION_REQUIRED,
                    "validation",
                    "Review and confirm session closure".into(),
                    false,
                    None,
                    None,
                    None,
                ));
            }
            store::close(
                &state.paths.root,
                request.id.as_deref().ok_or_else(|| {
                    ApiError::from(macc_core::MaccError::Validation(
                        "Session ID required".into(),
                    ))
                })?,
            )
            .map_err(ApiError::from)?;
            serde_json::json!({"closed":true})
        }
        _ => {
            return Err(ApiError::from(macc_core::MaccError::Validation(
                "Unknown session action".into(),
            )))
        }
    };
    Ok(Json(result))
}
pub(super) async fn events(
    State(state): State<WebState>,
    Path(id): Path<String>,
) -> Result<Json<Vec<serde_json::Value>>, ApiError> {
    store::events(&state.paths.root, &id)
        .map(Json)
        .map_err(ApiError::from)
}

pub(super) async fn plan(
    State(state): State<WebState>,
    Path(direction): Path<String>,
) -> Result<Json<store::MigrationReport>, ApiError> {
    let op = match direction.as_str() {
        "migrate" => store::migrate,
        "rollback" => store::rollback,
        "upgrade" => store::checkpoints::upgrade,
        _ => {
            return Err(ApiError::from(macc_core::MaccError::Validation(
                "Unknown session migration".into(),
            )))
        }
    };
    op(&state.paths.root, false, None)
        .map(Json)
        .map_err(ApiError::from)
}
pub(super) async fn preview(
    State(state): State<WebState>,
    body: Bytes,
) -> Result<Json<serde_json::Value>, ApiError> {
    let request: PolicyRequest = request(&body)?;
    let policy: Option<store::SessionPolicy> = serde_json::from_value(request.policy)
        .map_err(|e| ApiError::from(macc_core::MaccError::Validation(e.to_string())))?;
    if let Some(p) = &policy {
        p.validate().map_err(ApiError::from)?;
    }
    let view = store::inspect(&state.paths.root).map_err(ApiError::from)?;
    if view.revision != request.revision {
        return Err(failure(macc_core::MaccError::Validation(
            "Configuration revision conflict".into(),
        )));
    }
    Ok(Json(
        serde_json::json!({"before":view.policy,"after":policy.clone().unwrap_or_default(),"source":if policy.is_some(){"project"}else{"defaults"},"activation":"next session claim; active leases unchanged"}),
    ))
}

#[derive(Deserialize)]
pub(super) struct ReconcileRequest {
    expected_sha256: String,
}
pub(super) async fn checkpoint(
    State(state): State<WebState>,
    Path(id): Path<String>,
) -> Result<Json<store::checkpoints::Checkpoint>, ApiError> {
    store::checkpoints::get(&state.paths.root, &id)
        .map(Json)
        .map_err(ApiError::from)
}
pub(super) async fn reconciliation_plan(
    State(state): State<WebState>,
    Path(id): Path<String>,
) -> Result<Json<serde_json::Value>, ApiError> {
    store::checkpoints::reconcile(&state.paths.root, &id, None)
        .map(Json)
        .map_err(ApiError::from)
}
pub(super) async fn reconciliation_apply(
    State(state): State<WebState>,
    Path(id): Path<String>,
    headers: HeaderMap,
    body: Bytes,
) -> Result<Json<serde_json::Value>, ApiError> {
    super::mutation_gate::require_project_owner(&state, &headers)?;
    let r: ReconcileRequest = request(&body)?;
    store::checkpoints::reconcile(&state.paths.root, &id, Some(&r.expected_sha256))
        .map(Json)
        .map_err(ApiError::from)
}
