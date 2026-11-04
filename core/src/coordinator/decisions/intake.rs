//! Initial planner proposals: structural validation without creating authority.
use super::*;
use serde_json::Value;
use std::{
    collections::BTreeSet,
    path::{Component, Path},
};

/// Validate authored PRD intake before generation/promotion or runtime import.
/// No decisions are answered, artifacts read or graph effects applied here.
pub fn validate_initial_requests(graph: &Value, max_bytes: usize) -> crate::Result<Vec<Request>> {
    let Some(value) = graph.get("decision_requests") else {
        return Ok(Vec::new());
    };
    let rows = value
        .as_array()
        .ok_or_else(|| error("decision_requests must be an array"))?;
    if rows.len() > 256 {
        return Err(error("Planner decision intake exceeds 256 proposals"));
    }
    let mut keys = BTreeSet::new();
    let mut requests = Vec::new();
    for (i, value) in rows.iter().enumerate() {
        let request: Request = serde_json::from_value(value.clone())
            .map_err(|e| error(format!("decision_requests[{i}]: {e}")))?;
        validate_initial_request(graph, &request, max_bytes)
            .map_err(|e| error(format!("decision_requests[{i}]: {e}")))?;
        if !keys.insert(request.idempotency_key.clone()) {
            return Err(error(format!(
                "Duplicate planner idempotency key {}",
                request.idempotency_key
            )));
        }
        requests.push(request);
    }
    Ok(requests)
}

pub(super) fn validate_initial_request(
    graph: &Value,
    r: &Request,
    max_bytes: usize,
) -> crate::Result<()> {
    if serde_json::to_vec(r)
        .map_err(|e| error(e.to_string()))?
        .len()
        > max_bytes.min(262144)
    {
        return Err(error("Decision request exceeds the configured byte budget"));
    }
    if r.idempotency_key.trim().is_empty()
        || r.question.trim().is_empty()
        || r.recommendation.trim().is_empty()
        || r.investigated.is_empty()
        || r.investigated.iter().any(|s| s.trim().is_empty())
    {
        return Err(error("A proposal needs an idempotency key, concrete question, actual investigation and recommendation"));
    }
    if !matches!(
        r.kind.as_str(),
        "decision" | "clarification" | "approval" | "authorization" | "feedback" | "intervention"
    ) || !matches!(
        r.scope.as_str(),
        "task" | "project" | "paths" | "requirements"
    ) {
        return Err(error("Unknown decision kind/scope"));
    }
    super::graph::task(graph, &r.task_id)?;
    for id in &r.affected_tasks {
        if id != "*" {
            super::graph::task(graph, id)?;
        }
    }
    if r.has_effects() {
        return Err(error("Initial planner intake cannot contain proposed_patch or requires_replan; propose runtime effects after native plan identity is issued"));
    }
    if r.scope == "requirements" && r.requirement_refs.is_empty() {
        return Err(error("Requirements scope needs requirement_refs"));
    }
    if r.scope == "paths"
        && (r.affected_paths.is_empty()
            || r.affected_paths.iter().any(|p| {
                p.trim().is_empty()
                    || Path::new(p).is_absolute()
                    || Path::new(p)
                        .components()
                        .any(|c| matches!(c, Component::ParentDir))
            }))
    {
        return Err(error("Paths scope needs project-relative affected_paths"));
    }
    let mut options = BTreeSet::new();
    for option in &r.options {
        if option.id.trim().is_empty()
            || option.label.trim().is_empty()
            || !options.insert(option.id.as_str())
        {
            return Err(error(
                "Options need unique nonempty IDs and readable labels",
            ));
        }
    }
    if r.recommended_option_id
        .as_ref()
        .is_some_and(|id| !options.contains(id.as_str()))
    {
        return Err(error("Recommended option is not declared"));
    }
    if r.response_fields.len() > 32
        || r.supersedes.len() > 32
        || r.topic.as_ref().is_some_and(|s| s.len() > 256)
    {
        return Err(error(
            "Response fields, supersession or topic exceeds its structured budget",
        ));
    }
    let mut fields = BTreeSet::new();
    for field in &r.response_fields {
        if field.id.trim().is_empty()
            || field.id.starts_with("__")
            || field.label.trim().is_empty()
            || !fields.insert(&field.id)
        {
            return Err(error(
                "Response fields need unique non-reserved IDs and readable labels",
            ));
        }
    }
    if r.kind == "intervention" {
        let action = r.intervention.as_ref().ok_or_else(|| {
            error("Intervention requires action, inaccessible_reason and expected_result")
        })?;
        if action.action.trim().is_empty()
            || action.inaccessible_reason.trim().is_empty()
            || action.expected_result.trim().is_empty()
        {
            return Err(error(
                "Intervention requires an actual action, limitation and expected result",
            ));
        }
    } else if r.intervention.is_some() {
        return Err(error("Intervention details require kind intervention"));
    }
    if r.kind == "authorization" && r.operation_id.as_ref().is_none_or(|s| s.trim().is_empty()) {
        return Err(error("Authorization needs a stable operation_id"));
    }
    if let Some(at) = &r.expires_at {
        chrono::DateTime::parse_from_rfc3339(at)
            .map_err(|_| error("expires_at requires an RFC3339 timestamp with timezone"))?;
    }
    if r.artifacts.len() > 16 {
        return Err(error("At most 16 artifacts may be referenced"));
    }
    let mut artifacts = BTreeSet::new();
    for a in &r.artifacts {
        if a.id.trim().is_empty()
            || !artifacts.insert(&a.id)
            || a.digest.len() != 71
            || !a.digest.starts_with("sha256:")
            || !a.digest[7..].bytes().all(|b| b.is_ascii_hexdigit())
        {
            return Err(error(
                "Evidence requires unique retained IDs and exact SHA-256 references",
            ));
        }
    }
    Ok(())
}
