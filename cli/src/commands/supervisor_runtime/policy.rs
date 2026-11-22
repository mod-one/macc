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
    /// Restart-only recoveries per semantic fingerprint: the same stop recurring
    /// with unchanged retained work and applicable decisions escalates.
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

pub fn recovery_fingerprint(paths: &ProjectPaths, incident: &Incident) -> Result<String> {
    let roots: Vec<_> = incident
        .tasks
        .iter()
        .filter(|t| macc_core::supervisor::incident::repair_candidate(t))
        .collect();
    if !roots.is_empty()
        && roots
            .iter()
            .all(|t| macc_core::coordinator::phase_diagnostics::context_refresh_blocked(t))
    {
        let ids = roots.iter().map(|t| t.id.clone()).collect::<Vec<_>>();
        match macc_core::session_store::checkpoints::recovery::restart_revision(&paths.root, &ids) {
            Ok(revision) if revision.as_array().is_some_and(|r| !r.is_empty()) => {
                return Ok(format!(
                    "context-refresh-v2:{}",
                    macc_core::coordinator::decisions::digest(&revision)
                ));
            }
            // Malformed or unavailable evidence cannot renew a restart budget.
            // Let native recovery produce the specific durable refusal instead
            // of aborting the entire supervisor while computing a ledger key.
            Err(error) => {
                tracing::warn!(%error, "Cannot fingerprint native recovery evidence; retaining legacy restart budget")
            }
            _ => {}
        }
    }
    Ok(crash_fingerprint(incident))
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
    #[serde(default)]
    pub trigger: Value,
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
    let scoped_prds: BTreeMap<_, _> = incident
        .tasks
        .iter()
        .filter_map(|task| {
            macc_core::coordinator::scoped_prd::recovery_revision(&paths.root, task)
                .map(|revision| (&task.id, revision))
        })
        .collect();
    // Repairs publish to the reference branch: its movement is new evidence too.
    let reference =
        super::workspace::target(&paths.root, canonical.automation.coordinator.as_ref())?;
    let mut context = json!({"runtime_build":super::product_findings::runtime_identity(),"tasks":tasks,"head":git(&paths.root,&["rev-parse","HEAD"])?,
        "reference":{"branch":reference.branch,"tip":reference.base},
        "source_status":git(&paths.root,&["status","--porcelain"])?,
        "source_diff":git(&paths.root,&["diff","HEAD","--binary"])?,
        "tools":canonical.tools,"supervisor":canonical.automation.supervisor,
        "prd_queue":canonical.automation.coordinator.as_ref().map(|c|json!({"files":c.prd_files,"entries":macc_core::prd_queue::validate(&paths.root,&c.prd_files).ok(),"progress":macc_core::prd_queue::load(&paths.root).ok()})),
        "decision_gates":macc_core::coordinator::decisions::gate_revision(&paths.root,&incident.tasks.iter().map(|t|t.id.clone()).collect::<Vec<_>>())?,"availability":availability,"decision_policy":canonical.automation.coordinator.as_ref().and_then(|c|c.decisions.as_ref()),"checkpoint_revision":macc_core::session_store::checkpoints::recovery::revision(&paths.root,&incident.tasks.iter().map(|t|t.id.clone()).collect::<Vec<_>>())?});
    if !scoped_prds.is_empty() {
        context["scoped_prds"] = json!(scoped_prds);
    }
    Ok(context)
}

pub fn ready(pending: &mut Pending, context: Value, now: i64, max_attempts: u32) -> bool {
    // A known human gate cannot be repaired by unrelated Git/runtime activity.
    if pending.status == "waiting"
        && pending
            .diagnosis
            .as_ref()
            .is_some_and(|d| d.requires_human && d.summary.starts_with("DM_PENDING:"))
        && pending
            .context
            .get("decision_gates")
            .is_some_and(|v| v == &context["decision_gates"])
        && pending.context["decision_policy"] == context["decision_policy"]
    {
        return false;
    }
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
        let changed_fields: std::collections::BTreeSet<_> = pending
            .context
            .as_object()
            .into_iter()
            .flat_map(|o| o.keys())
            .chain(context.as_object().into_iter().flat_map(|o| o.keys()))
            .filter(|k| pending.context.get(*k) != context.get(*k))
            .cloned()
            .collect();
        let trigger = json!({"reason":if pending.context.is_null(){"initial_or_explicit_retry"}else{"evidence_changed"},"previous_revision":macc_core::coordinator::decisions::digest(&pending.context),"next_revision":macc_core::coordinator::decisions::digest(&context),"changed_fields":changed_fields,"runtime_updated":pending.context.get("runtime_build").is_some_and(|old|old!=&context["runtime_build"]),"previous_runtime":pending.context.get("runtime_build"),"next_runtime":context.get("runtime_build"),"at":now});
        tracing::info!(%trigger,"Supervisor diagnosis invalidation");
        let sequence = pending.sequence;
        *pending = Pending {
            diagnosis,
            trigger,
            context,
            sequence,
            ..Default::default()
        };
    }
    if pending.status == "waiting" {
        if pending.next_check.is_some_and(|time| now >= time) && !pending.reset_rechecked {
            pending.trigger = json!({"reason":"quota_reset","at":now});
            pending.reset_rechecked = true;
            pending.next_check = None;
            pending.diagnosis = None;
            pending.status.clear();
            pending.attempts = 0;
        } else {
            return false;
        }
    }
    if pending.status == "retrying" {
        pending.trigger = json!({"reason":"transient_retry","at":now});
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
    if report.status == "ready_to_resume" {
        return Some("Worker recovery completed. Start `macc coordinator run` when ready.".into());
    }
    if report.status == "waiting_condition" {
        return Some("Supply the outstanding condition, then run `macc coordinator run`. MACC will check each target and resume its retained phase when ready.".into());
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
    if report
        .incident
        .tasks
        .iter()
        .any(macc_core::coordinator::scoped_prd::is_dispatch_failure)
    {
        return Some(format!("Inspect the retained worker and its scoped PRD. MACC archives proven historical markers in .macc/persistent/scoped-prd-archive/. Preserve any other edits, then retry the native recovery (`{RETRY}`)."));
    }
    let mut paths: Vec<_> = report
        .incident
        .tasks
        .iter()
        .filter_map(|t| t.worktree_path().map(std::path::Path::new))
        .filter(|p| p.exists())
        .map(|p| p.display().to_string())
        .collect();
    paths.sort();
    paths.dedup();
    let mut kept = if paths.is_empty() {
        String::new()
    } else {
        format!(" Retained task work: {}.", paths.join(", "))
    };
    if report.repair_rounds > 0 || report.commit.is_some() {
        if let Some(path) = report.worktree.as_ref().filter(|p| p.exists()) {
            kept.push_str(&format!(" Supervisor repair work: {}.", path.display()));
        }
    }
    let human = report.diagnosis.as_ref().is_some_and(|d| d.requires_human)
        || report.detail.contains(super::repair::HUMAN_DECISION);
    let action = if report.detail.contains(super::repair_evidence::EMPTY)
        || report.detail.contains(super::repair_evidence::OUTSIDE)
    {
        format!("Inspect repair-source-refused.json and *-retained-work.json in the incident log directory. Keep the original task worktree, commits, index and uncommitted files. Resume the task's native correction and independent review in that worker; an empty supervisor base cannot stand in for its delivery. Reconcile ownership through native recovery before retrying (`{RETRY}`). No source repair or passing compliance is inferred from a model's explanation.")
    } else if report
        .detail
        .contains(super::checkpoint_recovery::RESTART_LIMIT)
    {
        format!("Inspect checkpoint-recovery.json in the incident log directory: the same work and applicable decision binding already used their automatic restart. No checkpoint was changed by this preview. Investigate the recurring context refresh, then explicitly retry native recovery (`{RETRY}`). A new applicable decision or new checkpointed work has a separate bounded recovery budget.")
    } else if report.diagnosis.as_ref().is_some_and(|d| {
        d.summary
            .contains(macc_core::session_store::checkpoints::recovery::WORKTREE_DRIFT)
    }) {
        format!("Inspect checkpoint-recovery-blocked.json: retained Git content/index changed after the checkpoint. Confirm all tools from that invocation have stopped and inspect late or external writes. Reconcile reviewed changes through `macc session-store reconcile <checkpoint-id>`, then explicitly retry native recovery (`{RETRY}`). Original work is preserved; no device, approval or automatic acceptance is implied.")
    } else if report.diagnosis.as_ref().is_some_and(|d| {
        d.summary
            .starts_with(macc_core::session_store::checkpoints::recovery::CODE)
    }) {
        format!("Inspect checkpoint-recovery-blocked.json in the incident log directory: verify the named checkpoint, retained phase, ownership and changed field. Reconcile reviewed changes through `macc session-store reconcile <checkpoint-id>` before retrying native recovery (`{RETRY}`). Original work is retained; this infrastructure guard is not a device or approval request.")
    } else if report
        .diagnosis
        .as_ref()
        .is_some_and(|d| d.macc_repair_required)
    {
        format!("Fix and validate MACC in its source repository; install the corrected build, then run `{RETRY}`. Product findings: .macc/reports/macc-defects/. Application edits cannot repair the installed runner.")
    } else if report
        .diagnosis
        .as_ref()
        .is_some_and(|d| d.summary.starts_with("DM_COMPLIANCE:"))
    {
        "Open Decisions for the exact decision/version and independent finding in the diagnosis. Reconcile the recorded selection through a governed revision, or supply the missing independent evidence, then resume the retained phase with fresh context and review. This is a governance/compliance block; source repair or an unchanged restart cannot approve it.".into()
    } else if report
        .diagnosis
        .as_ref()
        .is_some_and(|d| d.summary.starts_with("DM_PENDING:"))
    {
        "Open Decisions and answer or revise the exact native request listed in the reason. The supervisor waits for its revision; no application repair is needed. Then resume the retained task phase.".into()
    } else if report
        .diagnosis
        .as_ref()
        .is_some_and(|d| !d.external_prerequisites.is_empty())
    {
        let ids: std::collections::BTreeSet<_> = report
            .incident
            .tasks
            .iter()
            .filter_map(macc_core::coordinator::phase_diagnostics::decision_wait_id)
            .collect();
        let evidence = if ids.is_empty() {
            String::new()
        } else {
            format!(" Open Decisions and attach the requested evidence to {} for independent verification.",ids.into_iter().collect::<Vec<_>>().join(", "))
        };
        format!("Supply the recorded external devices, environment or authority.{evidence} Then run `{RETRY}`. Source repair cannot replace these verification requirements.")
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
    fn legacy_checkpoint_guard_report_is_not_external_evidence_or_a_context_refresh_question() {
        let report:Intervention=serde_json::from_value(json!({"incident":{"id":"incident","run_id":"run","epoch":1,"reason":"DM_STALE","tasks":[{"id":"VERIFY","state":"blocked","task_runtime":{"decision_wait":"context_refresh"}}],"recent_events":[]},"attempt":1,"status":"escalated","tool":null,"diagnosis":{"summary":"MACC-CHECKPOINT-CONTEXT-DRIFT: decision-only refresh refused","repairable":false,"requires_human":true,"external_prerequisites":["Review conflicting retained files or changed task requirements"]},"worktree":null,"validation":[],"commit":null,"restart_pid":null,"detail":""})).unwrap();
        let action = next_action(&Pending::default(), &report, 3).unwrap();
        assert!(action.contains("checkpoint-recovery-blocked.json"));
        assert!(
            !action.contains("Open Decisions")
                && !action.contains("Supply the recorded external")
                && !action.contains("to context_refresh")
        );
        let mut drift: Intervention =
            serde_json::from_value(serde_json::to_value(&report).unwrap()).unwrap();
        drift
            .diagnosis
            .as_mut()
            .unwrap()
            .summary
            .push_str(macc_core::session_store::checkpoints::recovery::WORKTREE_DRIFT);
        let action = next_action(&Pending::default(), &drift, 3).unwrap();
        assert!(action.contains("all tools from that invocation have stopped"));
        assert!(action.contains("late or external writes"));
        assert!(!action.contains("ownership"));
        let mut recurring = report;
        recurring.detail = super::super::checkpoint_recovery::RESTART_LIMIT.into();
        let action = next_action(&Pending::default(), &recurring, 3).unwrap();
        assert!(action.contains("checkpoint-recovery.json") && action.contains("explicitly retry"));
        assert!(!action.contains("Reconcile reviewed changes") && !action.contains("Open Decisions"), "a validated exhausted budget is distinct from a checkpoint guard failure or unanswered question");
    }

    #[test]
    fn external_handoff_points_to_the_native_request_and_actual_retained_work() {
        let root = tempfile::tempdir().unwrap();
        let worker = root.path().join("worker-04");
        let supervisor = root.path().join("supervisor-03");
        std::fs::create_dir_all(&worker).unwrap();
        std::fs::create_dir_all(&supervisor).unwrap();
        let mut report:Intervention=serde_json::from_value(json!({"incident":{"id":"incident","run_id":"run","epoch":1,"reason":"blocked","tasks":[{"id":"MOBILE","state":"blocked","worktree":{"worktree_path":worker},"task_runtime":{"decision_wait":"device-request"}}],"recent_events":[]},"attempt":1,"status":"escalated","tool":"fake","diagnosis":{"summary":"Device required","repairable":false,"requires_human":true,"external_prerequisites":["Android observation"]},"worktree":supervisor,"validation":[],"commit":null,"restart_pid":null,"detail":""})).unwrap();
        let action = next_action(&Pending::default(), &report, 3).unwrap();
        assert!(action.contains("device-request") && action.contains("independent verification"));
        assert!(action.contains(worker.to_str().unwrap()));
        assert!(!action.contains(supervisor.to_str().unwrap()));
        report.repair_rounds = 1;
        assert!(next_action(&Pending::default(), &report, 3)
            .unwrap()
            .contains(supervisor.to_str().unwrap()));
    }

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
    if report.incident.id != incident.id
        || !matches!(report.status.as_str(), "escalated" | "waiting_condition")
    {
        return None;
    }
    // An older model diagnosis can say this requires human intervention.
    // With no retry ledger, let the native handler inspect the current proof.
    if incident
        .tasks
        .iter()
        .any(macc_core::coordinator::scoped_prd::is_dispatch_failure)
    {
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
        trigger: json!({"reason":"restored_previous_report"}),
    })
}

#[cfg(test)]
mod human_wait_tests {
    use super::*;
    #[test]
    fn native_human_wait_ignores_unrelated_changes_and_logs_the_relevant_revision() {
        let diagnosis=serde_json::from_value(json!({"summary":"DM_PENDING: waiting for exact answer","requires_human":true,"repairable":false})).unwrap();
        let mut record = Pending {
            context: json!({"head":"a","availability":false,"decision_gates":[{"id":"d","version":1,"status":"pending_human"}],"decision_policy":{}}),
            diagnosis: Some(diagnosis),
            status: "waiting".into(),
            ..Default::default()
        };
        let mut context = record.context.clone();
        context["head"] = json!("b");
        context["availability"] = json!(true);
        assert!(!ready(&mut record, context.clone(), 100, 3));
        context["decision_gates"][0]["version"] = json!(2);
        context["decision_gates"][0]["status"] = json!("resolved");
        assert!(ready(&mut record, context, 101, 3));
        assert_eq!(record.trigger["reason"], "evidence_changed");
        assert!(record.trigger["changed_fields"]
            .as_array()
            .unwrap()
            .contains(&json!("decision_gates")));
        assert_ne!(
            record.trigger["previous_revision"],
            record.trigger["next_revision"]
        );
    }
}
