use super::errors::ApiError;
use super::WebState;
use axum::http::HeaderMap;
use macc_core::process_ownership::{ProcessHandle, ProcessKind};
use macc_core::service::process_ownership_gate::{gate_owner_action, ClientContext};

const WEB_CLIENT_HEADER: &str = "X-Macc-Client-Id";

/// L6-OWN-008 + spec "all mutating actions" requirement: every mutating web
/// endpoint must pass this gate. Read-only endpoints bypass it.
///
/// Behavior:
/// * If there is no `ProcessKind::Project` lease for the project, the gate
///   silently passes (backwards-compatible / single-user mode).
/// * If a project lease exists but has no owner, the gate passes (no
///   controller — a controller can be claimed via /api/v1/processes).
/// * If a project lease has an owner, only the owning client may mutate;
///   `MaccError::NotProcessOwner` is returned otherwise (mapped to HTTP 403
///   by `errors::ApiError::from(MaccError)`).
pub(super) fn require_project_owner(
    state: &WebState,
    headers: &HeaderMap,
) -> std::result::Result<(), ApiError> {
    let client_id = match client_id_from_headers(headers) {
        Some(id) => id,
        // Missing client identity is treated as "no controller present", which
        // matches single-user CLI parity. Mutations are allowed.
        None => return Ok(()),
    };
    let handle = ProcessHandle {
        kind: ProcessKind::Project,
        project_root: state.paths.root.clone(),
        pid: None,
    };
    // Skip gate if no project lease has been registered.
    let record =
        macc_core::service::process_ownership::get_record(&state.paths.root, &handle)?;
    if record.is_none() {
        return Ok(());
    }
    gate_owner_action(
        &ClientContext {
            client_id,
            project_root: state.paths.root.clone(),
        },
        &handle,
    )?;
    Ok(())
}

fn client_id_from_headers(headers: &HeaderMap) -> Option<String> {
    headers
        .get(WEB_CLIENT_HEADER)
        .and_then(|v| v.to_str().ok())
        .map(|v| v.trim())
        .filter(|v| !v.is_empty())
        .map(str::to_string)
}
