//! Durable, per-incident decisions: evidence changes, external waits and transient retries.
use super::{
    repair::{git, Diagnosis},
    report::Intervention,
};
use macc_core::{
    config::CanonicalConfig, supervisor::incident::Incident, MaccError, ProjectPaths, Result,
};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::BTreeMap;

#[derive(Default, Serialize, Deserialize)]
pub struct Ledger {
    #[serde(default)]
    pub attempts: u32, // Legacy status consumers.
    #[serde(default)]
    pub completed: Vec<String>,
    #[serde(default)]
    pub last_status: String,
    #[serde(default)]
    pub incidents: BTreeMap<String, Pending>,
}
#[derive(Default, Serialize, Deserialize)]
pub struct Pending {
    pub context: Value,
    #[serde(default)]
    pub attempts: u32,
    #[serde(default)]
    pub sequence: u32,
    #[serde(default)]
    pub status: String,
    #[serde(default)]
    pub diagnosis: Option<Diagnosis>,
    #[serde(default)]
    pub next_check: Option<i64>,
    #[serde(default)]
    pub reset_rechecked: bool,
}

pub fn context(
    paths: &ProjectPaths,
    incident: &Incident,
    canonical: &CanonicalConfig,
    now: i64,
) -> Result<Value> {
    let db_paths =
        macc_core::coordinator_storage::CoordinatorStoragePaths::from_project_paths(paths);
    let throttle = if db_paths.sqlite_path.exists() {
        macc_core::coordinator_storage::SqliteStorage::new(db_paths).load_throttle_registry()?
    } else {
        Default::default()
    };
    let availability: BTreeMap<_, _> = canonical
        .tools
        .enabled
        .iter()
        .map(|tool| {
            (
                tool,
                !throttle
                    .get(tool)
                    .is_some_and(|t| t.throttled_until > now.max(0) as u64),
            )
        })
        .collect();
    let tasks: Vec<_> = incident.tasks.iter().map(|task| json!({
        "id":task.id,"state":task.state,"tool":task.tool,"worktree":task.worktree,
        "last_error":task.task_runtime.last_error,"last_error_code":task.task_runtime.last_error_code,
        "unmet_preconditions":task.task_runtime.unmet_preconditions,"result_explanation":task.task_runtime.result_explanation,
        "extra":task.extra,"blocked_on_external":task.blocked_on_external
    })).collect();
    Ok(
        json!({"tasks":tasks,"head":git(&paths.root,&["rev-parse","HEAD"])?,
        "source_status":git(&paths.root,&["status","--porcelain"])?,
        "source_diff":git(&paths.root,&["diff","HEAD","--binary"])?,
        "tools":canonical.tools,"supervisor":canonical.automation.supervisor,
        "availability":availability}),
    )
}

pub fn ready(pending: &mut Pending, context: Value, now: i64, max_attempts: u32) -> bool {
    if pending.context != context {
        let sequence = pending.sequence;
        *pending = Pending {
            context,
            sequence,
            ..Default::default()
        };
    }
    if pending.status == "waiting" {
        if pending.next_check.is_some_and(|time| now >= time) && !pending.reset_rechecked {
            pending.reset_rechecked = true;
            pending.next_check = None;
            pending.diagnosis = None;
            pending.status.clear();
            pending.attempts = 0;
        } else {
            return false;
        }
    }
    pending.attempts < max_attempts && pending.next_check.is_none_or(|time| now >= time)
}

pub fn transient(error: &MaccError) -> bool {
    match error {
        MaccError::Coordinator { code, .. } => matches!(*code, "E101" | "E601" | "E603" | "E901"),
        MaccError::Io { source, .. } => matches!(
            source.kind(),
            std::io::ErrorKind::Interrupted
                | std::io::ErrorKind::WouldBlock
                | std::io::ErrorKind::TimedOut
                | std::io::ErrorKind::ConnectionReset
                | std::io::ErrorKind::ConnectionAborted
                | std::io::ErrorKind::ConnectionRefused
        ),
        _ => false,
    }
}

pub fn settle(
    pending: &mut Pending,
    report: &Intervention,
    error: Option<&MaccError>,
    now: i64,
    delay: u64,
) {
    pending.diagnosis = report.diagnosis.clone();
    if report.status == "recovered" {
        pending.status = "recovered".into();
        return;
    }
    if error.is_some_and(transient) {
        pending.status = "retrying".into();
        pending.next_check = Some(now.saturating_add(delay.max(1) as i64));
    } else {
        pending.status = "waiting".into();
        pending.next_check = if pending.reset_rechecked {
            None
        } else {
            report
                .diagnosis
                .as_ref()
                .filter(|d| !d.requires_human)
                .and_then(|d| d.retry_at.as_deref())
                .and_then(|time| chrono::DateTime::parse_from_rfc3339(time).ok())
                .map(|time| time.timestamp())
        };
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn unchanged_diagnosis_waits_across_serialization_and_wakes_once_for_reset() {
        let ctx = json!({"head":"a","available":false});
        let mut record = Pending {
            context: ctx.clone(),
            status: "waiting".into(),
            next_check: Some(100),
            attempts: 1,
            ..Default::default()
        };
        assert!(!ready(&mut record, ctx.clone(), 99, 3));
        record = serde_json::from_value(serde_json::to_value(record).unwrap()).unwrap();
        assert!(ready(&mut record, ctx.clone(), 100, 3));
        record.status = "waiting".into();
        assert!(!ready(&mut record, ctx.clone(), 200, 3));
        assert!(ready(
            &mut record,
            json!({"head":"a","available":true}),
            200,
            3
        ));
        assert!(!record.reset_rechecked);
    }
    #[test]
    fn retry_budget_is_for_transient_errors_and_resets_on_new_evidence() {
        assert!(transient(&MaccError::Coordinator {
            code: "E101",
            message: "network".into()
        }));
        assert!(!transient(&MaccError::Validation(
            "verification rejected".into()
        )));
        assert!(!transient(&MaccError::Io {
            path: "tool".into(),
            action: "spawn".into(),
            source: std::io::Error::from(std::io::ErrorKind::NotFound)
        }));
        let mut p = Pending {
            context: json!("old"),
            attempts: 3,
            status: "retrying".into(),
            ..Default::default()
        };
        assert!(!ready(&mut p, json!("old"), 100, 3));
        assert!(ready(&mut p, json!("new"), 100, 3));
    }
}

/// Interpret original provider evidence using the existing adapters, without source repair.
pub fn enrich(diagnosis: &mut Diagnosis, incident: &Incident, now: i64) {
    if diagnosis.requires_human || diagnosis.repairable || diagnosis.retry_at.is_some() {
        return;
    }
    let registry = macc_core::coordinator::error_normalizer::NormalizerRegistry::from_inventory();
    for task in &incident.tasks {
        let Some(tool) = task.tool.as_deref() else {
            continue;
        };
        let Some(error) = registry.get(tool).and_then(|n| {
            n.normalize(
                1,
                &incident.reason,
                task.task_runtime.last_error.as_deref().unwrap_or(""),
            )
        }) else {
            continue;
        };
        if matches!(error.error_code.as_str(), "E601" | "E602") {
            if let Some(seconds) = error.retry_after_seconds {
                diagnosis.retry_at =
                    chrono::DateTime::from_timestamp(now.saturating_add(seconds as i64), 0)
                        .map(|dt| dt.to_rfc3339());
                diagnosis.unavailable_tool = Some(tool.into());
            }
        }
    }
}

/// Migrate a conclusive report from the former global retry ledger without another AI call.
pub fn restore_previous(
    paths: &ProjectPaths,
    incident: &Incident,
    context: &Value,
    now: i64,
) -> Option<Pending> {
    let text = std::fs::read_to_string(paths.root.join(".macc/log/supervisor/latest.json")).ok()?;
    let value: Value = serde_json::from_str(&text).ok()?;
    let report: Intervention = serde_json::from_value(value.get("intervention")?.clone()).ok()?;
    if report.incident.id != incident.id || report.status != "escalated" {
        return None;
    }
    let mut diagnosis = report.diagnosis?;
    if diagnosis.repairable && !diagnosis.requires_human {
        return None;
    }
    enrich(&mut diagnosis, incident, now);
    let next_check = if diagnosis.requires_human {
        None
    } else {
        diagnosis
            .retry_at
            .as_deref()
            .and_then(|time| chrono::DateTime::parse_from_rfc3339(time).ok())
            .map(|time| time.timestamp())
    };
    Some(Pending {
        context: context.clone(),
        attempts: 1,
        sequence: report.attempt,
        status: "waiting".into(),
        diagnosis: Some(diagnosis),
        next_check,
        reset_rechecked: false,
    })
}
