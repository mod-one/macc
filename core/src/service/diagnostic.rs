use crate::coordinator::state_runtime;
use crate::coordinator::CoordinatorEventRecord;
use crate::coordinator_storage::{
    CoordinatorSnapshot, CoordinatorStorage, CoordinatorStoragePaths, JsonStorage, SqliteStorage,
};
use crate::{ProjectPaths, Result};
use chrono::{DateTime, Utc};

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum FailureKind {
    ProcessError,
    ConfigurationError,
    #[default]
    InternalError,
}

#[derive(Debug, Clone, Default)]
pub struct FailureReport {
    pub message: String,
    pub task_id: Option<String>,
    pub phase: Option<String>,
    pub source: String,
    pub blocking: bool,
    pub event_type: Option<String>,
    pub kind: FailureKind,
    pub suggested_fixes: Vec<String>,
}

pub fn analyze_last_failure(paths: &ProjectPaths) -> Result<Option<FailureReport>> {
    let storage_paths = CoordinatorStoragePaths::from_project_paths(paths);
    let sqlite = SqliteStorage::new(storage_paths.clone());
    let since = sqlite
        .get_latest_coordinator_run()?
        .and_then(|run| parse_timestamp(&run.started_at));
    analyze_failure(paths, since, storage_paths, sqlite)
}

/// Analyze only failures emitted during the managed command being polled.
/// This prevents a completed reconcile/sync from inheriting an unrelated error
/// from an earlier coordinator run.
pub fn analyze_last_failure_since(
    paths: &ProjectPaths,
    started_at: &str,
) -> Result<Option<FailureReport>> {
    let storage_paths = CoordinatorStoragePaths::from_project_paths(paths);
    let sqlite = SqliteStorage::new(storage_paths.clone());
    let Some(started_at) = parse_timestamp(started_at) else {
        return Ok(None);
    };
    analyze_failure(paths, Some(started_at), storage_paths, sqlite)
}

fn analyze_failure(
    paths: &ProjectPaths,
    since: Option<DateTime<Utc>>,
    storage_paths: CoordinatorStoragePaths,
    sqlite: SqliteStorage,
) -> Result<Option<FailureReport>> {
    if let Some(pause) = state_runtime::read_coordinator_pause_file(&paths.root)? {
        if timestamp_is_in_scope(&pause.updated_at, since.as_ref()) {
            let message = if pause.reason.is_empty() {
                "Coordinator paused due to a blocking error.".to_string()
            } else {
                pause.reason.clone()
            };
            return Ok(Some(build_failure_report(
                message,
                Some(pause.task_id),
                Some(pause.phase),
                "pause_file",
                true,
                Some("pause".to_string()),
            )));
        }
    }

    let snapshot: CoordinatorSnapshot = if sqlite.has_snapshot_data()? {
        sqlite.load_snapshot()?
    } else {
        JsonStorage::new(storage_paths).load_snapshot()?
    };

    if let Some(report) = report_from_events(&snapshot.events, since.as_ref()) {
        return Ok(Some(report));
    }

    // Legacy logs do not carry a reliable structured timestamp on every line.
    // They remain a fallback only for unscoped legacy state; using them for a
    // managed command would reintroduce cross-run error attribution.
    if since.is_none() {
        let logs = crate::service::logs::read_log_content(paths, "coordinator", None, None)
            .unwrap_or_default();
        if let Some(report) = report_from_logs(&logs) {
            return Ok(Some(report));
        }
    }

    Ok(None)
}

fn parse_timestamp(value: &str) -> Option<DateTime<Utc>> {
    DateTime::parse_from_rfc3339(value)
        .ok()
        .map(|value| value.with_timezone(&Utc))
}

fn timestamp_is_in_scope(value: &str, since: Option<&DateTime<Utc>>) -> bool {
    since.is_none_or(|since| parse_timestamp(value).is_some_and(|value| value >= *since))
}

#[cfg(test)]
mod tests {
    use super::report_from_events;
    use crate::coordinator::CoordinatorEventRecord;
    use chrono::{DateTime, Utc};
    use serde_json::json;

    fn event(ts: &str, message: &str) -> CoordinatorEventRecord {
        serde_json::from_value(json!({
            "event_id": format!("event-{ts}"),
            "ts": ts,
            "type": "command_error",
            "status": "failed",
            "severity": "blocking",
            "task_id": "-",
            "payload": {"message": message}
        }))
        .expect("event")
    }

    #[test]
    fn scoped_analysis_never_attributes_an_older_failure() {
        let events = vec![event("2026-08-29T18:00:00Z", "old coordinator run failed")];
        let since = DateTime::parse_from_rfc3339("2026-08-30T00:48:59Z")
            .expect("timestamp")
            .with_timezone(&Utc);
        assert!(report_from_events(&events, Some(&since)).is_none());
        assert_eq!(
            report_from_events(&events, None)
                .expect("legacy unscoped failure")
                .message,
            "old coordinator run failed"
        );
    }
}

fn report_from_events(
    events: &[CoordinatorEventRecord],
    since: Option<&DateTime<Utc>>,
) -> Option<FailureReport> {
    for raw in events.iter().rev() {
        if !timestamp_is_in_scope(&raw.ts, since) {
            continue;
        }
        let event_type = raw.event_type.as_str();
        let status = raw.status.as_str();
        let severity = raw.severity().unwrap_or_default();
        if !is_blocking_failure_event(event_type, status, severity) {
            continue;
        }
        let message = raw.message().unwrap_or(event_type).to_string();
        let task_id = raw.task_id.clone();
        let phase = raw
            .phase
            .clone()
            .or_else(|| infer_phase_from_status(status));
        return Some(build_failure_report(
            message,
            task_id,
            phase,
            "event",
            true,
            Some(event_type.to_string()),
        ));
    }
    None
}

fn report_from_logs(logs: &str) -> Option<FailureReport> {
    for line in logs.lines().rev() {
        let trimmed = line.trim();
        if let Some(rest) = trimmed.strip_prefix("- Run paused task=") {
            let mut task_id = None;
            let mut phase = None;
            let mut message = "Coordinator paused due to merge failure.".to_string();
            for segment in rest.split_whitespace() {
                if let Some(v) = segment.strip_prefix("task=") {
                    task_id = Some(v.to_string());
                } else if let Some(v) = segment.strip_prefix("phase=") {
                    phase = Some(v.to_string());
                } else if let Some(v) = segment.strip_prefix("reason=") {
                    message = v.to_string();
                }
            }
            return Some(build_failure_report(
                message,
                task_id,
                phase,
                "log",
                true,
                Some("run_paused".to_string()),
            ));
        }
        if let Some(rest) = trimmed.strip_prefix("- Merge failed task=") {
            let mut task_id = None;
            let mut message = "Local merge failed".to_string();
            for segment in rest.split_whitespace() {
                if let Some(v) = segment.strip_prefix("task=") {
                    task_id = Some(v.to_string());
                } else if let Some(v) = segment.strip_prefix("reason=") {
                    message = v.to_string();
                }
            }
            return Some(build_failure_report(
                message,
                task_id,
                Some("merge".to_string()),
                "log",
                true,
                Some("local_merge_failed".to_string()),
            ));
        }
    }
    None
}

fn build_failure_report(
    message: String,
    task_id: Option<String>,
    phase: Option<String>,
    source: &str,
    blocking: bool,
    event_type: Option<String>,
) -> FailureReport {
    let (kind, suggested_fixes) = classify_failure(
        &message,
        event_type.as_deref().unwrap_or_default(),
        phase.as_deref(),
    );
    FailureReport {
        message,
        task_id,
        phase,
        source: source.to_string(),
        blocking,
        event_type,
        kind,
        suggested_fixes,
    }
}

fn classify_failure(
    message: &str,
    event_type: &str,
    phase: Option<&str>,
) -> (FailureKind, Vec<String>) {
    let lower = message.to_ascii_lowercase();
    if lower.contains("merge")
        || lower.contains("conflict")
        || event_type.contains("merge")
        || event_type == "command_error"
    {
        return (
            FailureKind::ProcessError,
            vec![
                "Inspect merge/log details and resolve conflicting files.".to_string(),
                "Retry the failed phase from TUI or run `macc coordinator retry-phase ...`."
                    .to_string(),
            ],
        );
    }

    if lower.contains("config")
        || lower.contains("missing")
        || lower.contains("not found")
        || lower.contains("validation")
    {
        return (
            FailureKind::ConfigurationError,
            vec![
                "Verify `.macc/macc.yaml` and coordinator settings values.".to_string(),
                "Run `macc doctor` and fix reported tool/config issues.".to_string(),
            ],
        );
    }

    (
        FailureKind::InternalError,
        vec![
            "Inspect coordinator and performer logs for stack/context details.".to_string(),
            format!(
                "If state looks stale, run `macc coordinator reconcile` and retry{}.",
                phase.map(|p| format!(" phase `{}`", p)).unwrap_or_default()
            ),
        ],
    )
}

fn infer_phase_from_status(status: &str) -> Option<String> {
    match status {
        "claimed" | "in_progress" => Some("dev".to_string()),
        "pr_open" => Some("review".to_string()),
        "changes_requested" => Some("fix".to_string()),
        "queued" => Some("merge".to_string()),
        _ => None,
    }
}

pub fn is_blocking_failure_event(event: &str, status: &str, severity: &str) -> bool {
    let normalized_severity = if severity.is_empty() {
        if matches!(event, "command_error" | "task_blocked" | "failed")
            || (event == "phase_result" && matches!(status, "failed" | "error"))
        {
            "blocking"
        } else {
            "info"
        }
    } else {
        severity
    };
    normalized_severity.eq_ignore_ascii_case("blocking")
        && (matches!(
            event,
            "command_error" | "task_blocked" | "failed" | "phase_result"
        ) || matches!(status, "failed" | "error"))
}
