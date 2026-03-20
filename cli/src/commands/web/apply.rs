use super::errors::ApiError;
use super::types::{
    ApiApplyRequest, ApiApplyResponse, ApiApplyResult, ApiPlanConsent, ApiPlanDiff, ApiPlanFile,
    ApiPlanResponse, ApiPlanSummary,
};
use super::WebState;
use axum::body::Bytes;
use axum::extract::State;
use axum::Json;
use macc_core::config::CanonicalConfig;
use macc_core::plan::{ActionPlan, ActionStatus, PlannedOp, PlannedOpKind, Scope};
use macc_core::resolve::{resolve, resolve_fetch_units, CliOverrides};
use macc_core::{MaccError, ProjectPaths};

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

    let overrides = build_overrides(&state, &request).map_err(ApiError::from)?;
    let mut plan = build_plan(&state, &canonical, &overrides).map_err(ApiError::from)?;
    let ops = state.engine.plan_operations(&state.paths, &plan);

    if request.dry_run {
        return Ok(Json(ApplyEndpointResponse::DryRun(build_plan_response(
            &state.paths,
            &plan,
            &ops,
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

fn build_overrides(state: &WebState, request: &ApiApplyRequest) -> macc_core::Result<CliOverrides> {
    if request.tools.is_empty() {
        return Ok(CliOverrides::default());
    }

    let (descriptors, _diagnostics) = state.engine.list_tools(&state.paths);
    let allowed_tools: Vec<String> = descriptors.into_iter().map(|d| d.id).collect();
    CliOverrides::from_tools_csv(&request.tools.join(","), &allowed_tools)
}

fn build_plan(
    state: &WebState,
    canonical: &CanonicalConfig,
    overrides: &CliOverrides,
) -> macc_core::Result<ActionPlan> {
    let resolved = resolve(canonical, overrides);
    let fetch_units = resolve_fetch_units(&state.paths, &resolved)?;
    let materialized_units = macc_adapter_shared::fetch::materialize_fetch_units(
        &state.paths,
        fetch_units,
        resolved.settings.quiet,
        resolved.settings.offline,
    )?;
    state
        .engine
        .plan(&state.paths, canonical, &materialized_units, overrides)
}

fn build_plan_response(
    paths: &ProjectPaths,
    plan: &ActionPlan,
    ops: &[PlannedOp],
    include_diff: bool,
) -> ApiPlanResponse {
    ApiPlanResponse {
        summary: ApiPlanSummary {
            total_actions: plan.actions.len(),
            files_write: ops
                .iter()
                .filter(|op| op.kind == PlannedOpKind::Write)
                .count(),
            files_merge: ops
                .iter()
                .filter(|op| op.kind == PlannedOpKind::Merge)
                .count(),
            consent_required: ops.iter().filter(|op| op.consent_required).count(),
            backup_required: ops.iter().filter(|op| op.metadata.backup_required).count(),
            backup_path: paths.backups_dir.display().to_string(),
        },
        files: ops
            .iter()
            .map(|op| ApiPlanFile {
                path: op.path.clone(),
                kind: op.kind,
                scope: op.scope,
                consent_required: op.consent_required,
                backup_required: op.metadata.backup_required,
                set_executable: op.metadata.set_executable,
                explain: Some(explain_operation(op)),
            })
            .collect(),
        diffs: ops
            .iter()
            .map(|op| {
                let view = if include_diff {
                    macc_core::plan::render_diff(op)
                } else {
                    macc_core::plan::DiffView {
                        kind: macc_core::plan::DiffViewKind::Unsupported,
                        diff: String::new(),
                        truncated: false,
                    }
                };
                ApiPlanDiff {
                    path: op.path.clone(),
                    diff_kind: match view.kind {
                        macc_core::plan::DiffViewKind::Text => "text".to_string(),
                        macc_core::plan::DiffViewKind::Json => "json".to_string(),
                        macc_core::plan::DiffViewKind::Unsupported => "unsupported".to_string(),
                    },
                    diff: (!view.diff.is_empty()).then_some(view.diff),
                    diff_truncated: view.truncated,
                }
            })
            .collect(),
        risks: build_plan_risks(ops),
        consents: ops
            .iter()
            .filter(|op| op.consent_required)
            .map(|op| ApiPlanConsent {
                id: format!("consent:{}", op.path),
                scope: op.scope,
                message: format!(
                    "Apply will modify {}-scope path '{}'.",
                    scope_label(op.scope),
                    op.path
                ),
                paths: vec![op.path.clone()],
            })
            .collect(),
    }
}

fn build_plan_risks(ops: &[PlannedOp]) -> Vec<String> {
    let mut risks = Vec::new();
    if ops.iter().any(|op| op.metadata.backup_required) {
        risks.push("Existing files may be backed up before mutation.".to_string());
    }
    if ops.iter().any(|op| op.kind == PlannedOpKind::Merge) {
        risks.push("Structured merge operations may update existing generated files.".to_string());
    }
    if ops.iter().any(|op| op.scope == Scope::User) {
        risks.push("User-scope operations require a separate consent flow.".to_string());
    }
    risks
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

fn explain_operation(op: &PlannedOp) -> String {
    match op.kind {
        PlannedOpKind::Write => {
            if op.path == ".gitignore" {
                "ensures required ignore patterns are present".to_string()
            } else {
                "writes generated configuration/content".to_string()
            }
        }
        PlannedOpKind::Merge => "merges generated JSON fragment into existing file".to_string(),
        PlannedOpKind::Mkdir => "creates required directory structure".to_string(),
        PlannedOpKind::Delete => "deletes stale managed artifact".to_string(),
        PlannedOpKind::Other => "normalization or supplementary operation".to_string(),
    }
}

fn scope_label(scope: Scope) -> &'static str {
    match scope {
        Scope::Project => "project",
        Scope::User => "user",
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
