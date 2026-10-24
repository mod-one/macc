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
    /// Restart-only recoveries per crash fingerprint: the same stop recurring
    /// after a restart escalates instead of looping.
    #[serde(default)]
    pub restarts: BTreeMap<String, u32>,
}

/// Restart-only recoveries allowed for one crash fingerprint.
pub const MAX_RESTARTS_PER_FINGERPRINT: u32 = 1;

/// Identifies "the same stop": its reason and the incident tasks with their
/// states, independent of the run id and epoch that change on each restart.
pub fn crash_fingerprint(incident: &Incident) -> String {
    let mut tasks: Vec<String> = incident
        .tasks
        .iter()
        .map(|t| format!("{}:{}", t.id, t.state))
        .collect();
    tasks.sort();
    format!("{}|{}", incident.reason.trim(), tasks.join(","))
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
        "out_of_scope_fix":task.task_runtime.extra.get(macc_core::coordinator::precondition_scope::OUT_OF_SCOPE_FIX_KEY),
        "extra":task.extra,"blocked_on_external":task.blocked_on_external
    })).collect();
    // Repairs publish to the reference branch: its movement is new evidence too.
    let reference =
        super::workspace::target(&paths.root, canonical.automation.coordinator.as_ref())?;
    Ok(
        json!({"runtime_build":super::product_findings::runtime_identity(),"tasks":tasks,"head":git(&paths.root,&["rev-parse","HEAD"])?,
        "reference":{"branch":reference.branch,"tip":reference.base},
        "source_status":git(&paths.root,&["status","--porcelain"])?,
        "source_diff":git(&paths.root,&["diff","HEAD","--binary"])?,
        "tools":canonical.tools,"supervisor":canonical.automation.supervisor,
        "availability":availability}),
    )
}

pub fn ready(pending: &mut Pending, context: Value, now: i64, max_attempts: u32) -> bool {
    if pending.context != context {
        // A settings-only change supplies missing checks or enables a phase;
        // it does not invalidate the source diagnosis or spend another AI call.
        let mut previous_evidence = pending.context.clone();
        let mut next_evidence = context.clone();
        if let Some(v) = previous_evidence.as_object_mut() {
            v.remove("supervisor");
        }
        if let Some(v) = next_evidence.as_object_mut() {
            v.remove("supervisor");
        }
        let diagnosis = if previous_evidence == next_evidence {
            pending
                .diagnosis
                .clone()
                .filter(|d| d.repairable && !d.requires_human && !d.restart_only)
        } else {
            None
        };
        let sequence = pending.sequence;
        *pending = Pending {
            diagnosis,
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
    if matches!(report.status.as_str(), "recovered" | "repair_integrated") {
        pending.status = report.status.clone();
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

const RETRY: &str = "macc supervisor stop && macc supervisor start --daemon --retry";

/// The exact step that moves a settled, unrecovered intervention forward.
pub fn next_action(pending: &Pending, report: &Intervention, max_attempts: u32) -> Option<String> {
    if report.status == "recovered" {
        return None;
    }
    if report.status == "repair_integrated" {
        return Some("Repair integrated and retained work reconciled. Automatic restart is disabled; start `macc coordinator run` when ready.".into());
    }
    if pending.status == "retrying" && pending.attempts < max_attempts {
        return Some(format!(
            "No action needed: automatic retry {} of {} is scheduled.",
            pending.attempts + 1,
            max_attempts
        ));
    }
    let kept = report
        .worktree
        .as_ref()
        .filter(|path| path.exists())
        .map(|path| format!(" Repair work is kept in {}.", path.display()))
        .unwrap_or_default();
    let human = report.diagnosis.as_ref().is_some_and(|d| d.requires_human)
        || report.detail.contains(super::repair::HUMAN_DECISION);
    let action = if report
        .diagnosis
        .as_ref()
        .is_some_and(|d| d.macc_repair_required)
    {
        format!("Fix and validate MACC in its source repository; install the corrected build, then run `{RETRY}`. Product findings: .macc/reports/macc-defects/. Application edits cannot repair the installed runner.")
    } else if report
        .diagnosis
        .as_ref()
        .is_some_and(|d| !d.external_prerequisites.is_empty())
    {
        format!("Supply the recorded external devices, environment or authority, then run `{RETRY}`. Source repair cannot replace these verification requirements.")
    } else if human {
        format!("Make the human decision named in the reason, then run `{RETRY}`.")
    } else if let Some(time) = pending
        .next_check
        .and_then(|time| chrono::DateTime::from_timestamp(time, 0))
    {
        format!(
            "No action needed: rechecks automatically at {}. To retry sooner run `{RETRY}`.",
            time.to_rfc3339()
        )
    } else if report.detail.contains("requires validation_commands")
        || report
            .detail
            .contains("Tests cannot be enabled without validation_commands")
        || report.detail.contains("No project validation scripts")
        || report.detail.contains("requires a test command")
    {
        "Configure automation.supervisor.validation_commands (relative subproject test commands), or explicitly disable tests_enabled. The recorded diagnosis is reused after a supervisor configuration change. Run `macc supervisor config check` to inspect readiness.".into()
    } else if report.detail.contains("Automatic repair is disabled") {
        "Enable automation.supervisor.repair_enabled to continue with the recorded diagnosis."
            .into()
    } else if report.detail.contains("Automatic restart is disabled") {
        "Start the coordinator manually, or enable automation.supervisor.auto_restart.".into()
    } else if report.detail.contains("Validation failed")
        || report.detail.contains("Validation timed out")
    {
        format!("Fix the failing command or set automation.supervisor.validation_commands in .macc/macc.yaml (a config change is retried automatically), otherwise run `{RETRY}`.")
    } else if report.detail.contains("Git identity is missing") {
        format!("Run `macc doctor --fix --git-name \"Your Name\" --git-email \"you@example.com\"`, then `{RETRY}`.")
    } else if report.detail.contains("Coordinator failed during startup")
        || report.detail.contains("Coordinator readiness timed out")
    {
        format!("Fix the coordinator startup error in .macc/log/coordinator/daemon-stderr.log (`macc coordinator run --preflight-only` shows reference-branch problems), then run `{RETRY}`.")
    } else if report.detail.contains("Repair verification rejected") {
        format!("The repair agent could not satisfy the independent verification in {} round(s). Finish the repair in the kept worktree as the reason describes (or raise automation.supervisor.max_repair_rounds), then run `{RETRY}`.", report.repair_rounds.max(1))
    } else if report.detail.contains("integration deferred") {
        format!(
            "Commit or stash the overlapping changes on the reference branch, then run `{RETRY}`."
        )
    } else {
        format!("Resolve the reason above, then run `{RETRY}`.")
    };
    Some(format!("{action}{kept}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn settings_only_changes_reuse_diagnosis_but_source_changes_invalidate_it() {
        let diagnosis: Diagnosis =
            serde_json::from_value(json!({"summary":"missing API", "repairable":true})).unwrap();
        let mut p = Pending {
            context: json!({"head":"a", "supervisor":{"tests_enabled":true}}),
            diagnosis: Some(diagnosis),
            status: "waiting".into(),
            attempts: 3,
            ..Default::default()
        };
        assert!(ready(
            &mut p,
            json!({"head":"a", "supervisor":{"tests_enabled":false}}),
            10,
            3
        ));
        assert_eq!(p.diagnosis.as_ref().unwrap().summary, "missing API");
        assert!(ready(
            &mut p,
            json!({"head":"b", "supervisor":{"tests_enabled":false}}),
            10,
            3
        ));
        assert!(p.diagnosis.is_none());
    }
    #[test]
    fn the_same_stop_after_a_restart_has_the_same_fingerprint() {
        let incident = |run: &str, state: &str| Incident {
            id: format!("{run}-1"),
            run_id: run.into(),
            epoch: 1,
            reason: "Coordinator error [invalid_transition]: from=testing".into(),
            tasks: vec![macc_core::coordinator::model::Task {
                id: "L4L-BASELINE-001".into(),
                state: state.into(),
                ..Default::default()
            }],
            recent_events: Vec::new(),
            run_status: Some("failed".into()),
        };
        assert_eq!(
            crash_fingerprint(&incident("run-a", "testing")),
            crash_fingerprint(&incident("run-b", "testing"))
        );
        assert_ne!(
            crash_fingerprint(&incident("run-a", "testing")),
            crash_fingerprint(&incident("run-a", "reviewing"))
        );
    }
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
