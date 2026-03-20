use super::errors::ApiError;
use super::types::{
    ApiPlanConsent, ApiPlanDiff, ApiPlanFile, ApiPlanRequest, ApiPlanResponse, ApiPlanRisk,
    ApiPlanSummary, ApiRiskLevel,
};
use super::WebState;
use axum::body::Bytes;
use axum::extract::State;
use axum::Json;
use macc_core::config::CanonicalConfig;
use macc_core::plan::{ActionPlan, PlannedOp, PlannedOpKind, Scope};
use macc_core::resolve::{resolve, resolve_fetch_units, CliOverrides};
use macc_core::MaccError;
use macc_core::ProjectPaths;

pub(super) async fn run_plan_handler(
    State(state): State<WebState>,
    body: Bytes,
) -> std::result::Result<Json<ApiPlanResponse>, ApiError> {
    let request: ApiPlanRequest = serde_json::from_slice(&body).map_err(|err| {
        ApiError::from(macc_core::MaccError::Validation(format!(
            "Invalid plan request body: {}",
            err
        )))
    })?;

    let canonical = state
        .engine
        .load_canonical_config(&state.paths)
        .map_err(ApiError::from)?;
    let overrides = build_overrides(&state, &request).map_err(ApiError::from)?;
    let plan = build_plan(&state, &canonical, &overrides).map_err(ApiError::from)?;
    validate_preview_plan(&plan, request.allow_user_scope.unwrap_or(false))
        .map_err(ApiError::from)?;
    let ops = state.engine.plan_operations(&state.paths, &plan);

    Ok(Json(build_plan_response(
        &state.paths,
        &plan,
        &ops,
        request.include_diff.unwrap_or(true),
        request.explain.unwrap_or(true),
    )))
}

pub(super) fn build_overrides(
    state: &WebState,
    request: &ApiPlanRequest,
) -> macc_core::Result<CliOverrides> {
    let mut overrides = CliOverrides {
        offline: request.offline,
        ..CliOverrides::default()
    };

    if request.tools.is_empty() {
        return Ok(overrides);
    }

    let (descriptors, _diagnostics) = state.engine.list_tools(&state.paths);
    let allowed_tools: Vec<String> = descriptors.into_iter().map(|d| d.id).collect();
    overrides.tools = CliOverrides::from_tools_csv(&request.tools.join(","), &allowed_tools)?.tools;
    Ok(overrides)
}

pub(super) fn build_plan(
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

pub(super) fn build_plan_response(
    paths: &ProjectPaths,
    plan: &ActionPlan,
    ops: &[PlannedOp],
    include_diff: bool,
    explain: bool,
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
                risk_level: classify_op_risk(op),
                content_preview: content_preview(op.after.as_deref()),
                explain: explain.then(|| explain_operation(op)),
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
                classification: classify_op_risk(op),
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

fn validate_preview_plan(plan: &ActionPlan, allow_user_scope: bool) -> macc_core::Result<()> {
    if allow_user_scope {
        return Ok(());
    }

    if let Some(path) = plan
        .actions
        .iter()
        .find(|action| action.scope() == Scope::User)
        .map(|action| action.path().to_string())
    {
        return Err(MaccError::UserScopeNotAllowed(format!(
            "Action for path '{}' is User scope",
            path
        )));
    }

    Ok(())
}

fn build_plan_risks(ops: &[PlannedOp]) -> Vec<ApiPlanRisk> {
    let mut risks = Vec::new();
    if ops.iter().any(|op| op.metadata.backup_required) {
        risks.push(ApiPlanRisk {
            level: ApiRiskLevel::Caution,
            message: "Existing files may be backed up before mutation.".to_string(),
        });
    }
    if ops.iter().any(|op| op.kind == PlannedOpKind::Merge) {
        risks.push(ApiPlanRisk {
            level: ApiRiskLevel::Caution,
            message: "Structured merge operations may update existing generated files.".to_string(),
        });
    }
    if ops.iter().any(|op| op.scope == Scope::User) {
        risks.push(ApiPlanRisk {
            level: ApiRiskLevel::Dangerous,
            message: "User-scope operations require a separate consent flow.".to_string(),
        });
    }
    if risks.is_empty() {
        risks.push(ApiPlanRisk {
            level: ApiRiskLevel::Safe,
            message: "No elevated risks detected for this plan preview.".to_string(),
        });
    }
    risks
}

fn classify_op_risk(op: &PlannedOp) -> ApiRiskLevel {
    if op.scope == Scope::User {
        return ApiRiskLevel::Dangerous;
    }
    if op.kind == PlannedOpKind::Merge || op.metadata.backup_required || op.metadata.set_executable
    {
        return ApiRiskLevel::Caution;
    }
    ApiRiskLevel::Safe
}

fn content_preview(bytes: Option<&[u8]>) -> Option<String> {
    let bytes = bytes?;
    let text = String::from_utf8_lossy(bytes);
    let mut preview = String::new();
    for ch in text.chars().take(240) {
        preview.push(ch);
    }
    if text.chars().count() > 240 {
        preview.push_str("...");
    }
    Some(preview)
}

pub(super) fn explain_operation(op: &PlannedOp) -> String {
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
