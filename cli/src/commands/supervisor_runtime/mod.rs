//! Production supervisor: canonical incidents, durable intervention and managed restart.
mod policy;
mod recovery;
mod repair;
mod report;
mod shutdown;
mod workspace;
use recovery::intervene;
pub use report::{record_failure, record_shutdown};
use report::{slug, write_json, write_report, Intervention};
pub use shutdown::request_stop;
pub fn request_retry(paths: &ProjectPaths) -> Result<()> {
    write_json(
        &paths.root.join(".macc/state/supervisor-retry-request.json"),
        &json!({"requested_at":chrono::Utc::now().to_rfc3339()}),
    )
}
mod tool;
use macc_core::config::CanonicalConfig;
use macc_core::coordinator_storage::{CoordinatorStoragePaths, SqliteStorage};
use macc_core::supervisor::incident;
use macc_core::{MaccError, ProjectPaths, Result};
use serde_json::json;
use std::path::Path;
use std::time::Duration;

pub async fn run(paths: ProjectPaths, canonical: CanonicalConfig, attached: bool) -> Result<()> {
    tokio::select! {
        result = run_monitor(paths.clone(), canonical) => result,
        result = shutdown::wait_for_operator_stop(&paths, attached) => result,
    }
}

async fn run_monitor(paths: ProjectPaths, canonical: CanonicalConfig) -> Result<()> {
    let config = canonical.automation.supervisor.clone().unwrap_or_default();
    loop {
        match monitor(paths.clone()).await {
            Ok(()) => return Ok(()),
            Err(error) => {
                record_failure(&paths, &config, &error.to_string())?;
                eprintln!("Supervisor monitoring failed; retrying: {error}");
                tokio::time::sleep(Duration::from_secs(config.watchdog_interval_seconds.max(1)))
                    .await;
            }
        }
    }
}

async fn monitor(paths: ProjectPaths) -> Result<()> {
    let ledger_path = paths.root.join(".macc/state/supervisor-interventions.json");
    let mut ledger: policy::Ledger = if ledger_path.exists() {
        serde_json::from_str(
            &std::fs::read_to_string(&ledger_path).map_err(|e| io(&ledger_path, e))?,
        )
        .map_err(|e| {
            MaccError::Validation(format!("Invalid supervisor intervention ledger: {e}"))
        })?
    } else {
        policy::Ledger::default()
    };
    loop {
        let canonical = macc_core::load_canonical_config(&paths.config_path)?;
        let config = canonical.automation.supervisor.clone().unwrap_or_default();
        let storage = CoordinatorStoragePaths::from_project_paths(&paths);
        let mut run = if storage.sqlite_path.exists() {
            SqliteStorage::new(storage).get_latest_coordinator_run()?
        } else {
            None
        };
        let stall_path = paths.root.join(".macc/log/supervisor/stall.json");
        if let Some(r) = run.as_mut() {
            if !macc_core::coordinator::helpers::is_pid_running(r.pid)
                && matches!(
                    r.status.as_str(),
                    "running" | "draining" | "stopped_by_user"
                )
            {
                if let Ok(raw) = std::fs::read_to_string(&stall_path) {
                    if serde_json::from_str::<serde_json::Value>(&raw)
                        .ok()
                        .and_then(|v| v.get("run_id").and_then(|v| v.as_str()).map(str::to_owned))
                        .as_deref()
                        == Some(&r.run_id)
                    {
                        r.status = "crashed".into();
                        r.stop_reason=Some("Supervisor stopped a coordinator whose tick exceeded the stall threshold".into());
                        SqliteStorage::new(CoordinatorStoragePaths::from_project_paths(&paths))
                            .upsert_coordinator_run(r)?;
                    }
                }
            }
        }
        let running = run.as_ref().is_some_and(|r| {
            matches!(r.status.as_str(), "running" | "draining")
                && macc_core::coordinator::helpers::is_pid_running(r.pid)
        });
        if running
            && run
                .as_ref()
                .and_then(|r| r.last_tick_at.as_ref())
                .and_then(|t| chrono::DateTime::parse_from_rfc3339(t).ok())
                .is_some_and(|tick| {
                    chrono::Utc::now().signed_duration_since(tick).num_seconds()
                        > config.log_analysis_window_seconds.max(30) as i64
                })
        {
            write_json(
                &paths.root.join(".macc/log/supervisor/stall.json"),
                &json!({"timestamp":chrono::Utc::now().to_rfc3339(),"run_id":run.as_ref().map(|r|&r.run_id),"reason":"Coordinator tick exceeded stall threshold; stopping managed runtime before diagnosis"}),
            )?;
            macc_core::service::coordinator::coordinator_stop_managed_command_process(
                &paths, false,
            )?;
        }
        let status = if running {
            "healthy"
        } else if run
            .as_ref()
            .is_some_and(|r| r.status == "completed" || r.status == "success")
        {
            "completed"
        } else {
            "waiting"
        };
        write_json(
            &paths.root.join(".macc/state/supervisor-health.json"),
            &json!({"checked_at":chrono::Utc::now().to_rfc3339(),"health":{"status":status},"coordinator_pid":run.as_ref().map(|r|r.pid),"tool":config.tool.as_ref().or(canonical.tools.enabled.first()),"last_intervention":ledger.last_status}),
        )?;
        if status == "completed" {
            ledger.attempts = 0;
            write_json(
                &ledger_path,
                &serde_json::to_value(&ledger).map_err(|e| MaccError::Validation(e.to_string()))?,
            )?;
        }
        // Managed wrapper may be alive after a short terminal state update; wait until all run processes exited.
        let managed =
            macc_core::coordinator::managed_command_registry::list_managed_commands(&paths)?;
        let managed_running = managed
            .iter()
            .any(|r| macc_core::coordinator::helpers::is_pid_running(r.pid as i64));
        if !running && !managed_running {
            if let Some(incident) =
                incident::inspect_with_config(&paths, canonical.automation.coordinator.as_ref())?
            {
                let now = chrono::Utc::now().timestamp();
                let context = policy::context(&paths, &incident, &canonical, now)?;
                let completed = ledger.completed.contains(&incident.id);
                let retry_request = paths.root.join(".macc/state/supervisor-retry-request.json");
                if !retry_request.exists() && !ledger.incidents.contains_key(&incident.id) {
                    if let Some(previous) =
                        policy::restore_previous(&paths, &incident, &context, now)
                    {
                        ledger.incidents.insert(incident.id.clone(), previous);
                        write_json(
                            &ledger_path,
                            &serde_json::to_value(&ledger)
                                .map_err(|e| MaccError::Validation(e.to_string()))?,
                        )?;
                    }
                }
                let pending = ledger.incidents.entry(incident.id.clone()).or_default();
                if !completed && policy::ready(pending, context, now, config.max_restart_attempts) {
                    let _lock = macc_core::fs_lock::AdvisoryLock::acquire(
                        &paths.root.join(".macc/state/supervisor-intervention.lock"),
                        Duration::ZERO,
                        "supervisor intervention",
                    )?;
                    pending.attempts += 1;
                    let prefix = format!("{}-attempt-", slug(&incident.id));
                    let previous = std::fs::read_dir(paths.root.join(".macc/log/supervisor"))
                        .ok()
                        .into_iter()
                        .flatten()
                        .filter_map(|entry| entry.ok())
                        .filter_map(|entry| {
                            entry
                                .file_name()
                                .to_str()
                                .and_then(|name| name.strip_prefix(&prefix))
                                .and_then(|value| value.parse::<u32>().ok())
                        })
                        .max()
                        .unwrap_or(0);
                    pending.sequence = pending.sequence.max(previous) + 1;
                    let attempt = pending.sequence;
                    let cached = pending.diagnosis.clone();
                    ledger.attempts = attempt;
                    write_json(
                        &ledger_path,
                        &serde_json::to_value(&ledger)
                            .map_err(|e| MaccError::Validation(e.to_string()))?,
                    )?;
                    if retry_request.exists() {
                        std::fs::remove_file(&retry_request).map_err(|e| io(&retry_request, e))?;
                    }
                    let dir = paths
                        .root
                        .join(".macc/log/supervisor")
                        .join(format!("{}-attempt-{attempt}", slug(&incident.id)));
                    let mut report = Intervention {
                        log_dir: Some(dir.clone()),
                        incident,
                        attempt,
                        status: "detected".into(),
                        tool: None,
                        diagnosis: cached,
                        worktree: None,
                        validation: Vec::new(),
                        commit: None,
                        restart_pid: None,
                        detail: String::new(),
                    };
                    write_report(&paths, &config, &dir, &report)?;
                    let intervention_error =
                        match intervene(&paths, &canonical, &config, &mut report, &dir, &_lock)
                            .await
                        {
                            Ok(()) => {
                                ledger.completed.push(report.incident.id.clone());
                                None
                            }
                            Err(error) => {
                                report.status = "escalated".into();
                                report.detail = error.to_string();
                                Some(error)
                            }
                        };
                    let pending = ledger.incidents.get_mut(&report.incident.id).unwrap();
                    policy::settle(
                        pending,
                        &report,
                        intervention_error.as_ref(),
                        chrono::Utc::now().timestamp(),
                        config.watchdog_interval_seconds,
                    );
                    if let Ok(raw) = std::fs::read_to_string(dir.join("tool-error.json")) {
                        if let Ok(error) = serde_json::from_str::<
                            macc_core::coordinator::error_normalizer::ToolError,
                        >(&raw)
                        {
                            if let Some(delay) = error.retry_after_seconds {
                                pending.next_check = Some(
                                    chrono::Utc::now().timestamp().saturating_add(delay as i64),
                                );
                            }
                        }
                    }
                    ledger.last_status = report.status.clone();
                    if let Some(worktree) = report.worktree.as_ref().filter(|path| path.exists()) {
                        let close = pending.status != "retrying"
                            || pending.attempts >= config.max_restart_attempts;
                        match workspace::finish(&paths.root, worktree, &dir, close) {
                            Ok(removed) => {
                                if removed {
                                    report.detail.push_str(
                                        "; evidence archived; clean supervisor slot removed",
                                    );
                                }
                            }
                            Err(error) => report.detail.push_str(&format!(
                                "; evidence/cleanup failed; slot retained: {error}"
                            )),
                        }
                    }
                    write_report(&paths, &config, &dir, &report)?;
                    write_json(
                        &ledger_path,
                        &serde_json::to_value(&ledger)
                            .map_err(|e| MaccError::Validation(e.to_string()))?,
                    )?;
                }
            }
        }
        tokio::time::sleep(Duration::from_secs(config.watchdog_interval_seconds.max(1))).await;
    }
}
fn io(path: &Path, source: std::io::Error) -> MaccError {
    MaccError::Io {
        path: path.display().to_string(),
        action: "supervisor intervention persistence".into(),
        source,
    }
}
