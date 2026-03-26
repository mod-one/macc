use super::errors::ApiError;
use super::plan::{build_overrides, build_plan, build_plan_response};
use super::types::{ApiApplyRequest, ApiApplyResponse, ApiApplyResult, ApiPlanResponse};
use super::WebState;
use axum::body::Bytes;
use axum::extract::State;
use axum::Json;
use macc_core::plan::{ActionStatus, PlannedOp, PlannedOpKind, Scope};
use macc_core::MaccError;

#[derive(serde::Serialize)]
#[serde(untagged)]
pub(super) enum ApplyEndpointResponse {
    DryRun(ApiPlanResponse),
    Applied(ApiApplyResponse),
}

pub(super) async fn run_apply_handler(
    State(state): State<WebState>,
    body: Bytes,
) -> std::result::Result<Json<ApplyEndpointResponse>, ApiError> {
    let request: ApiApplyRequest = serde_json::from_slice(&body).map_err(|err| {
        ApiError::from(MaccError::Validation(format!(
            "Invalid apply request body: {}",
            err
        )))
    })?;

    validate_request(&request)?;

    let canonical = state
        .engine
        .load_canonical_config(&state.paths)
        .map_err(ApiError::from)?;
    if !request.dry_run && !request.confirmed.unwrap_or(false) {
        return Err(ApiError::confirmation_required(
            "Confirmation required before apply can write changes.",
            None,
        ));
    }

    let plan_request = super::types::ApiPlanRequest {
        scope: request.scope,
        tools: request.tools.clone(),
        worktrees: Vec::new(),
        allow_user_scope: request.allow_user_scope,
        offline: None,
        include_diff: Some(true),
        explain: Some(true),
    };
    let overrides = build_overrides(&state, &plan_request).map_err(ApiError::from)?;
    let mut plan = build_plan(&state, &canonical, &overrides).map_err(ApiError::from)?;
    let ops = state.engine.plan_operations(&state.paths, &plan);

    if request.dry_run {
        return Ok(Json(ApplyEndpointResponse::DryRun(build_plan_response(
            &state.paths,
            &plan,
            &ops,
            true,
            true,
        ))));
    }

    let report = state
        .engine
        .apply(&state.paths, &mut plan, false)
        .map_err(ApiError::from)?;
    macc_core::service::context::mark_apply_completed(&state.paths).map_err(ApiError::from)?;

    Ok(Json(ApplyEndpointResponse::Applied(build_apply_response(
        &ops, &report,
    ))))
}

fn validate_request(request: &ApiApplyRequest) -> std::result::Result<(), ApiError> {
    if request.scope == Some(Scope::User) || request.allow_user_scope.unwrap_or(false) {
        return Err(ApiError::from(MaccError::UserScopeNotAllowed(
            "Web apply endpoint does not support user-scope operations.".to_string(),
        )));
    }
    Ok(())
}

fn build_apply_response(ops: &[PlannedOp], report: &macc_core::ApplyReport) -> ApiApplyResponse {
    let mut backup_locations = Vec::new();
    if let Some(path) = &report.backup_dir {
        backup_locations.push(path.display().to_string());
    }
    if let Some(user_report) = &report.user_backup_report {
        backup_locations.push(user_report.root.display().to_string());
    }

    let changed_files = report
        .outcomes
        .values()
        .filter(|status| matches!(status, ActionStatus::Created | ActionStatus::Updated))
        .count();

    let results = report
        .outcomes
        .iter()
        .map(|(path, status)| {
            let op = ops.iter().find(|candidate| candidate.path == *path);
            ApiApplyResult {
                path: path.clone(),
                kind: op.map(|item| item.kind).unwrap_or(PlannedOpKind::Other),
                success: !matches!(status, ActionStatus::Unknown),
                message: Some(status_message(*status)),
                backup_location: report.backup_dir.as_ref().and_then(|root| {
                    let candidate = root.join(path);
                    candidate.exists().then(|| candidate.display().to_string())
                }),
            }
        })
        .collect();

    ApiApplyResponse {
        dry_run: false,
        applied_actions: report.outcomes.len(),
        changed_files,
        backup_locations,
        results,
        warnings: Vec::new(),
    }
}

fn status_message(status: ActionStatus) -> String {
    match status {
        ActionStatus::Created => "created".to_string(),
        ActionStatus::Updated => "updated".to_string(),
        ActionStatus::Unchanged => "unchanged".to_string(),
        ActionStatus::Noop => "noop".to_string(),
        ActionStatus::Unknown => "unknown".to_string(),
    }
}
