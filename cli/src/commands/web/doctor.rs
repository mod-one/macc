use super::errors::ApiError;
use super::types::{
    ApiDoctorFixIssueResult, ApiDoctorFixRequest, ApiDoctorFixResponse, ApiDoctorIssue,
    ApiDoctorReport,
};
use super::WebState;
use axum::body::Bytes;
use axum::extract::State;
use axum::Json;
use macc_core::doctor::{ToolCheck, ToolStatus};
use macc_core::service::interaction::InteractionHandler;
use macc_core::tool::spec::{CheckSeverity, DoctorCheckKind};
use macc_core::MaccError;
use std::collections::{BTreeMap, BTreeSet};

#[derive(Default)]
struct SilentInteraction;

impl InteractionHandler for SilentInteraction {}

pub(super) async fn get_doctor_handler(
    State(state): State<WebState>,
) -> std::result::Result<Json<ApiDoctorReport>, ApiError> {
    let checks = state.engine.doctor(&state.paths);
    Ok(Json(build_report(&checks)))
}

pub(super) async fn run_doctor_fix_handler(
    State(state): State<WebState>,
    body: Bytes,
) -> std::result::Result<Json<ApiDoctorFixResponse>, ApiError> {
    let request = parse_fix_request(&body)?;
    if request.issue_codes.is_some() {
        return Err(ApiError::from(MaccError::Validation(
            "Selective doctor fixes are not supported; omit issueCodes to run all safe fixes."
                .to_string(),
        )));
    }

    let checks_before = state.engine.doctor(&state.paths);
    let issues_before = build_issues(&checks_before);
    let fixable_issue_codes = issues_before
        .iter()
        .filter(|issue| issue.fix_available)
        .map(|issue| issue.code.clone())
        .collect::<BTreeSet<_>>();

    if !fixable_issue_codes.is_empty() {
        let interaction = SilentInteraction;
        match state
            .engine
            .project_run_doctor(&state.paths, true, &interaction)
        {
            Ok(()) => {}
            Err(MaccError::Validation(_)) => {}
            Err(err) => return Err(ApiError::from(err)),
        }
    }

    let checks_after = state.engine.doctor(&state.paths);
    let report = build_report(&checks_after);
    let after_codes = report
        .issues
        .iter()
        .map(|issue| issue.code.as_str())
        .collect::<BTreeSet<_>>();

    let mut fixed_count = 0usize;
    let mut failed_count = 0usize;
    let mut results = Vec::new();

    for issue in issues_before.iter().filter(|issue| issue.fix_available) {
        let code = &issue.code;
        let fixed = !after_codes.contains(code.as_str());
        if fixed {
            fixed_count += 1;
        } else {
            failed_count += 1;
        }
        results.push(ApiDoctorFixIssueResult {
            code: code.clone(),
            fixed,
            message: if fixed {
                format!("Issue '{}' is no longer reported after doctor fix.", code)
            } else {
                format!("Issue '{}' is still reported after doctor fix.", code)
            },
            severity: issue.severity.clone(),
        });
    }

    let status = if failed_count == 0 {
        "ok"
    } else if fixed_count == 0 {
        "failed"
    } else {
        "partial"
    };
    let message = if fixable_issue_codes.is_empty() {
        "No doctor issues support automatic fixes.".to_string()
    } else if failed_count == 0 {
        format!("Doctor fix resolved {} issue(s).", fixed_count)
    } else if fixed_count == 0 {
        format!(
            "Doctor fix did not resolve the {} selected issue(s).",
            failed_count
        )
    } else {
        format!(
            "Doctor fix resolved {} issue(s) and left {} issue(s) unresolved.",
            fixed_count, failed_count
        )
    };

    Ok(Json(ApiDoctorFixResponse {
        status: status.to_string(),
        message,
        attempted_count: fixable_issue_codes.len(),
        fixed_count,
        failed_count,
        backup_location: None,
        results,
        report,
    }))
}

fn parse_fix_request(body: &Bytes) -> std::result::Result<ApiDoctorFixRequest, ApiError> {
    if body.is_empty() {
        return Ok(ApiDoctorFixRequest::default());
    }

    serde_json::from_slice(body).map_err(|err| {
        ApiError::from(MaccError::Validation(format!(
            "Invalid doctor fix request body: {}",
            err
        )))
    })
}

fn build_report(checks: &[ToolCheck]) -> ApiDoctorReport {
    let issues = build_issues(checks);
    let mut issues_by_severity = BTreeMap::new();
    let mut issues_by_category = BTreeMap::new();

    for issue in &issues {
        *issues_by_severity
            .entry(severity_label(&issue.severity).to_string())
            .or_insert(0) += 1;
        *issues_by_category
            .entry(issue.category.clone())
            .or_insert(0) += 1;
    }

    ApiDoctorReport {
        health_score: compute_health_score(checks),
        issues_by_severity,
        issues_by_category,
        issues,
    }
}

fn build_issues(checks: &[ToolCheck]) -> Vec<ApiDoctorIssue> {
    checks
        .iter()
        .filter(|check| !matches!(check.status, ToolStatus::Installed))
        .map(map_issue)
        .collect()
}

fn map_issue(check: &ToolCheck) -> ApiDoctorIssue {
    ApiDoctorIssue {
        severity: check.severity.clone(),
        code: issue_code(check),
        category: category_label(&check.kind).to_string(),
        description: issue_description(check),
        current_state: current_state_label(&check.status),
        expected_state: expected_state_label(&check.kind),
        fix_available: !matches!(check.kind, DoctorCheckKind::Custom),
    }
}

fn compute_health_score(checks: &[ToolCheck]) -> u8 {
    if checks.is_empty() {
        return 100;
    }

    let penalty = checks
        .iter()
        .filter(|check| !matches!(check.status, ToolStatus::Installed))
        .map(|check| match check.severity {
            CheckSeverity::Error => 40u16,
            CheckSeverity::Warning => 20u16,
        })
        .sum::<u16>();

    100u8.saturating_sub(penalty.min(100) as u8)
}

fn issue_code(check: &ToolCheck) -> String {
    let scope = check.tool_id.as_deref().unwrap_or(&check.name);
    format!(
        "doctor.{}.{}.{}",
        slug(scope),
        kind_label(&check.kind),
        slug(&check.check_target)
    )
}

fn issue_description(check: &ToolCheck) -> String {
    match check.kind {
        DoctorCheckKind::Which => format!(
            "{} requires '{}' to be available in PATH.",
            check.name, check.check_target
        ),
        DoctorCheckKind::PathExists => format!(
            "{} requires path '{}' to exist.",
            check.name, check.check_target
        ),
        DoctorCheckKind::Custom => format!(
            "{} reported a custom diagnostic failure for '{}'.",
            check.name, check.check_target
        ),
    }
}

fn current_state_label(status: &ToolStatus) -> String {
    match status {
        ToolStatus::Installed => "installed".to_string(),
        ToolStatus::Missing => "missing".to_string(),
        ToolStatus::Error(message) => format!("error: {}", message),
    }
}

fn expected_state_label(kind: &DoctorCheckKind) -> String {
    match kind {
        DoctorCheckKind::Which => "binary is available in PATH".to_string(),
        DoctorCheckKind::PathExists => "path exists".to_string(),
        DoctorCheckKind::Custom => "custom diagnostic passes".to_string(),
    }
}

fn severity_label(severity: &CheckSeverity) -> &'static str {
    match severity {
        CheckSeverity::Error => "error",
        CheckSeverity::Warning => "warning",
    }
}

fn category_label(kind: &DoctorCheckKind) -> &'static str {
    match kind {
        DoctorCheckKind::Which => "tooling",
        DoctorCheckKind::PathExists => "filesystem",
        DoctorCheckKind::Custom => "custom",
    }
}

fn kind_label(kind: &DoctorCheckKind) -> &'static str {
    match kind {
        DoctorCheckKind::Which => "which",
        DoctorCheckKind::PathExists => "path_exists",
        DoctorCheckKind::Custom => "custom",
    }
}

fn slug(value: &str) -> String {
    let mut slug = String::new();
    let mut previous_separator = false;
    for ch in value.chars() {
        let normalized = ch.to_ascii_lowercase();
        if normalized.is_ascii_alphanumeric() {
            slug.push(normalized);
            previous_separator = false;
        } else if !previous_separator {
            slug.push('-');
            previous_separator = true;
        }
    }
    slug.trim_matches('-').to_string()
}
