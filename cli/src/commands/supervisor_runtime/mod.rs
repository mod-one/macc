//! Production supervisor: canonical incidents, durable intervention and managed restart.
mod recovery;
mod repair;
mod report;
use recovery::intervene;
pub use report::{record_failure, record_shutdown};
use report::{slug, write_json, write_report, Intervention};
mod tool;
use macc_core::config::CanonicalConfig;
use macc_core::coordinator_storage::{CoordinatorStoragePaths, SqliteStorage};
use macc_core::supervisor::incident;
use macc_core::{MaccError, ProjectPaths, Result};
use serde_json::json;
use std::path::Path;
use std::time::Duration;

pub async fn run(paths: ProjectPaths, canonical: CanonicalConfig) -> Result<()> {
    let config = canonical.automation.supervisor.clone().unwrap_or_default();
    loop {
        match monitor(paths.clone(), canonical.clone()).await {
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

async fn monitor(paths: ProjectPaths, canonical: CanonicalConfig) -> Result<()> {
    let config = canonical.automation.supervisor.clone().unwrap_or_default();
    let ledger_path = paths.root.join(".macc/state/supervisor-interventions.json");
    let mut ledger: serde_json::Value = if ledger_path.exists() {
        serde_json::from_str(
            &std::fs::read_to_string(&ledger_path).map_err(|e| io(&ledger_path, e))?,
        )
        .map_err(|e| {
            MaccError::Validation(format!("Invalid supervisor intervention ledger: {e}"))
        })?
    } else {
        json!({"attempts":0,"completed":[]})
    };
    loop {
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
            &json!({"checked_at":chrono::Utc::now().to_rfc3339(),"health":{"status":status},"coordinator_pid":run.as_ref().map(|r|r.pid),"tool":config.tool.as_ref().or(canonical.tools.enabled.first()),"last_intervention":ledger.get("last_status")}),
        )?;
        if status == "completed" {
            ledger["attempts"] = json!(0);
            write_json(&ledger_path, &ledger)?;
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
                let completed = ledger["completed"]
                    .as_array()
                    .is_some_and(|a| a.iter().any(|v| v.as_str() == Some(&incident.id)));
                let attempts = ledger["attempts"].as_u64().unwrap_or(0) as u32;
                if !completed && attempts < config.max_restart_attempts {
                    let _lock = macc_core::fs_lock::AdvisoryLock::acquire(
                        &paths.root.join(".macc/state/supervisor-intervention.lock"),
                        Duration::ZERO,
                        "supervisor intervention",
                    )?;
                    let attempt = attempts + 1;
                    ledger["attempts"] = json!(attempt);
                    write_json(&ledger_path, &ledger)?;
                    let dir = paths
                        .root
                        .join(".macc/log/supervisor")
                        .join(format!("{}-attempt-{attempt}", slug(&incident.id)));
                    let mut report = Intervention {
                        incident,
                        attempt,
                        status: "detected".into(),
                        tool: None,
                        diagnosis: None,
                        worktree: None,
                        validation: Vec::new(),
                        commit: None,
                        restart_pid: None,
                        detail: String::new(),
                    };
                    write_report(&paths, &config, &dir, &report)?;
                    match intervene(&paths, &canonical, &config, &mut report, &dir, &_lock).await {
                        Ok(()) => {
                            ledger["completed"]
                                .as_array_mut()
                                .ok_or_else(|| {
                                    MaccError::Validation(
                                        "Invalid supervisor completed ledger".into(),
                                    )
                                })?
                                .push(json!(report.incident.id));
                        }
                        Err(e) => {
                            report.status = "escalated".into();
                            report.detail = e.to_string();
                        }
                    }
                    ledger["last_status"] = json!(report.status);
                    write_report(&paths, &config, &dir, &report)?;
                    write_json(&ledger_path, &ledger)?;
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
