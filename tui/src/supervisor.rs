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
