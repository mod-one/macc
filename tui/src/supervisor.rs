//! Read-only supervisor status strip for the coordinator client.
use macc_core::ProjectPaths;
use serde_json::Value;
pub fn status_strip(paths: &ProjectPaths) -> Option<String> {
    let raw =
        std::fs::read_to_string(paths.root.join(".macc/state/supervisor-health.json")).ok()?;
    let value: Value = serde_json::from_str(&raw).ok()?;
    let pid = std::fs::read_to_string(paths.root.join(".macc/state/supervisor.pid"))
        .ok()
        .and_then(|p| p.trim().parse::<i64>().ok());
    let alive = pid.is_some_and(macc_core::coordinator::helpers::is_pid_running);
    let status = if alive {
        value
            .pointer("/health/status")
            .and_then(Value::as_str)
            .unwrap_or("running")
    } else {
        "stopped"
    };
    let last = std::fs::read_to_string(paths.root.join(".macc/log/supervisor/latest.json"))
        .ok()
        .and_then(|s| serde_json::from_str::<Value>(&s).ok());
    let phase = last
        .as_ref()
        .and_then(|v| v.pointer("/intervention/status"))
        .and_then(Value::as_str)
        .unwrap_or("none");
    Some(format!(
        "Supervisor: {status} | intervention: {phase} | macc supervisor report"
    ))
}

/// Why the latest intervention is not recovered, the exact next step, and the
/// MACC improvements it recommended. Empty while nothing needs the operator.
pub fn detail_lines(paths: &ProjectPaths) -> Vec<String> {
    std::fs::read_to_string(paths.root.join(".macc/log/supervisor/latest.json"))
        .ok()
        .and_then(|raw| serde_json::from_str::<Value>(&raw).ok())
        .map(|report| detail_lines_from(&report))
        .unwrap_or_default()
}

fn detail_lines_from(report: &Value) -> Vec<String> {
    let Some(intervention) = report.get("intervention") else {
        return Vec::new();
    };
    let text = |key: &str| {
        intervention
            .get(key)
            .and_then(Value::as_str)
            // One header line each: keep the first line of multi-line detail.
            .and_then(|value| value.lines().map(str::trim).find(|line| !line.is_empty()))
    };
    let mut lines = Vec::new();
    if matches!(text("status"), Some("escalated" | "interrupted")) {
        if let Some(reason) = text("detail") {
            lines.push(format!("Supervisor reason: {reason}"));
        }
        if let Some(next) = text("next_action") {
            lines.push(format!("Supervisor next: {next}"));
        }
    }
    let findings: Vec<&str> = intervention
        .pointer("/diagnosis/macc_findings")
        .and_then(Value::as_array)
        .map(|items| items.iter().filter_map(Value::as_str).collect())
        .unwrap_or_default();
    if let Some(first) = findings.first() {
        lines.push(format!(
            "MACC findings ({}, all in `macc supervisor report`): {first}",
            findings.len()
        ));
    }
    lines
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn waiting_escalation_shows_reason_next_step_and_findings() {
        let lines = detail_lines_from(&json!({"intervention":{
            "status":"escalated",
            "detail":"Validation error: Validation failed: pnpm run test\nsee logs",
            "next_action":"Fix the failing command, otherwise run `macc supervisor stop && macc supervisor start --daemon --retry`.",
            "diagnosis":{"macc_findings":["Surface unmet preconditions first","Deduplicate root logs"]}
        }}));
        assert_eq!(
            lines[0],
            "Supervisor reason: Validation error: Validation failed: pnpm run test"
        );
        assert!(lines[1].starts_with("Supervisor next: Fix the failing command"));
        assert_eq!(
            lines[2],
            "MACC findings (2, all in `macc supervisor report`): Surface unmet preconditions first"
        );
    }

    #[test]
    fn recovered_intervention_keeps_only_its_findings() {
        let lines = detail_lines_from(&json!({"intervention":{
            "status":"recovered","detail":"proof","diagnosis":{"macc_findings":["Improve X"]}
        }}));
        assert_eq!(lines.len(), 1);
        assert!(lines[0].starts_with("MACC findings (1"));
        assert!(detail_lines_from(&json!({})).is_empty());
    }
}
