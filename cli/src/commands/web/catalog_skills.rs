/// Web API handlers for the Skills & Catalog lifecycle (spec §16).
use super::errors::ApiError;
use super::WebState;
use axum::extract::State;
use axum::Json;

// ── GET /api/v1/catalog/skills/available ─────────────────────────────────────

/// `GET /api/v1/catalog/skills/available`
///
/// Lists skills available from configured catalogs.
pub(super) async fn available_handler(
    State(state): State<WebState>,
) -> std::result::Result<Json<serde_json::Value>, ApiError> {
    let entries = state.engine.catalog_skills_available(&state.paths, None);
    Ok(Json(serde_json::json!({ "skills": entries })))
}

// ── GET /api/v1/catalog/mcp/available ────────────────────────────────────────

/// `GET /api/v1/catalog/mcp/available`
///
/// Lists MCP servers available from configured catalogs.
pub(super) async fn mcp_available_handler(
    State(state): State<WebState>,
) -> std::result::Result<Json<serde_json::Value>, ApiError> {
    let catalog =
        macc_core::catalog::load_effective_mcp_catalog(&state.paths).map_err(ApiError::from)?;
    Ok(Json(serde_json::json!({ "mcp": catalog.entries })))
}

// ── GET /api/v1/catalog/skills/status ────────────────────────────────────────

/// `GET /api/v1/catalog/skills/status`
///
/// Returns installed status for all locked skills.
pub(super) async fn status_handler(
    State(state): State<WebState>,
) -> std::result::Result<Json<serde_json::Value>, ApiError> {
    let statuses = state
        .engine
        .skills_status(&state.paths, None)
        .map_err(|e| ApiError::validation(e.to_string()))?;

    let warnings: Vec<String> = statuses.iter().flat_map(|s| s.warnings.clone()).collect();

    Ok(Json(serde_json::json!({
        "skills": statuses,
        "warnings": warnings,
    })))
}

// ── GET /api/v1/catalog/skills/installed ─────────────────────────────────────

/// `GET /api/v1/catalog/skills/installed`
///
/// Returns the raw skills lockfile entries.
pub(super) async fn installed_handler(
    State(state): State<WebState>,
) -> std::result::Result<Json<serde_json::Value>, ApiError> {
    let lockfile = state
        .engine
        .skills_lockfile(&state.paths)
        .map_err(|e| ApiError::validation(e.to_string()))?;
    Ok(Json(
        serde_json::json!({ "skills": lockfile.skills, "version": lockfile.version }),
    ))
}

// ── POST /api/v1/catalog/skills/verify ───────────────────────────────────────

/// `POST /api/v1/catalog/skills/verify`
///
/// Runs verification and returns drift findings.
pub(super) async fn verify_handler(
    State(state): State<WebState>,
) -> std::result::Result<Json<serde_json::Value>, ApiError> {
    let findings = state
        .engine
        .skills_verify(&state.paths)
        .map_err(|e| ApiError::validation(e.to_string()))?;

    let ok = findings.is_empty();
    Ok(Json(serde_json::json!({
        "ok": ok,
        "findings": findings,
        "finding_count": findings.len(),
    })))
}

// ── GET /api/v1/catalog/skills/lockfile ──────────────────────────────────────

/// `GET /api/v1/catalog/skills/lockfile`
///
/// Returns the full lockfile.
pub(super) async fn lockfile_handler(
    State(state): State<WebState>,
) -> std::result::Result<Json<serde_json::Value>, ApiError> {
    let lockfile = state
        .engine
        .skills_lockfile(&state.paths)
        .map_err(|e| ApiError::validation(e.to_string()))?;
    Ok(Json(serde_json::to_value(&lockfile).unwrap_or_default()))
}
