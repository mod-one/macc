use super::io;
use super::repair;
use super::report::{write_json, write_report, Intervention};
use super::tool;
use macc_core::config::CanonicalConfig;
use macc_core::coordinator::types::CoordinatorEnvConfig;
use macc_core::supervisor::{incident, SupervisorConfig};
use macc_core::{MaccError, ProjectPaths, Result};
use serde_json::json;
use std::path::Path;
use std::time::Duration;
pub(super) async fn intervene(
    paths: &ProjectPaths,
    canonical: &CanonicalConfig,
    config: &SupervisorConfig,
    report: &mut Intervention,
    dir: &Path,
    lock: &macc_core::fs_lock::AdvisoryLock,
) -> Result<()> {
    let current = incident::inspect_with_config(paths, canonical.automation.coordinator.as_ref())?
        .ok_or_else(|| {
            MaccError::Validation("Coordinator incident changed before intervention".into())
        })?;
    if current.id != report.incident.id {
        return Err(MaccError::Validation(
            "Coordinator epoch changed before intervention".into(),
        ));
    }
    ensure_not_stopped(paths)?;
    stop_orphan_jobs(&report.incident).await?;
    let agent = tool::Agent::resolve(&paths.root, canonical, config)?;
    report.tool = Some(agent.id.clone());
    let (worktree, base) = super::workspace::prepare(&paths.root, &report.incident.id)?;
    report.worktree = Some(worktree.clone());
    report.status = "diagnosing".into();
    write_report(paths, config, dir, report)?;
    let mut diagnosis = if let Some(cached) = report.diagnosis.clone() {
        cached
    } else {
        repair::diagnose(&agent, &paths.root, &worktree, dir, &report.incident).await?
    };
    super::policy::enrich(
        &mut diagnosis,
        &report.incident,
        chrono::Utc::now().timestamp(),
    );
    report.detail = diagnosis.summary.clone();
    report.diagnosis = Some(diagnosis.clone());
    if diagnosis.requires_human || !diagnosis.repairable {
        return Err(MaccError::Validation(format!(
            "Intervention escalated: {}",
            diagnosis.summary
        )));
    }
    if diagnosis.task_ids.is_empty() && !report.incident.tasks.is_empty() {
        return Err(MaccError::Validation(
            "Diagnosis did not identify repaired root tasks".into(),
        ));
    }
    report.validation = repair::validation_commands(&paths.root, config)?;
    report.status = "repairing".into();
    write_report(paths, config, dir, report)?;
    let proof = repair::fix(
        &agent,
        &paths.root,
        &worktree,
        dir,
        &diagnosis,
        &report.validation,
        config.intervention_timeout_seconds,
    )
    .await?;
    report.status = "integrating".into();
    report.detail = proof;
    write_report(paths, config, dir, report)?;
    ensure_not_stopped(paths)?;
    report.commit = Some(repair::integrate(&paths.root, &worktree, &base)?);
    report.status = "requeuing".into();
    write_report(paths, config, dir, report)?;
    ensure_not_stopped(paths)?;
    requeue(paths, canonical, &diagnosis.task_ids, &report.detail)?;
    report.status = "restarting".into();
    write_report(paths, config, dir, report)?;
    ensure_not_stopped(paths)?;
    let pid = macc_core::service::coordinator::coordinator_restart_after_intervention(
        paths,
        canonical.automation.coordinator.as_ref(),
        lock,
    )?;
    report.restart_pid = Some(pid);
    // Avoid blocking the async runtime while readiness polls the persistent registry.
    let ready_paths = paths.clone();
    tokio::task::spawn_blocking(move || {
        macc_core::service::coordinator_readiness::wait(&ready_paths, pid, Duration::from_secs(15))
    })
    .await
    .map_err(|e| MaccError::Validation(e.to_string()))??;
    write_json(
        &paths.root.join(".macc/state/coordinator-supervisor.json"),
        &json!({"coordinator_pid":pid,"supervisor_pid":std::process::id()}),
    )?;
    std::fs::write(
        paths.root.join(".macc/state/coordinator.pid"),
        format!("{pid}\n"),
    )
    .map_err(|e| io(&paths.root, e))?;
    report.status = "recovered".into();
    Ok(())
}
fn requeue(
    paths: &ProjectPaths,
    canonical: &CanonicalConfig,
    ids: &[String],
    proof: &str,
) -> Result<()> {
    // Existing unblock API reconciles dependency descendants and records operator evidence.
    let config = canonical.automation.coordinator.as_ref();
    let env = CoordinatorEnvConfig::default();
    let mut args = std::collections::BTreeMap::new();
    if let Some(mode) = config.and_then(|c| c.storage_mode.as_ref()) {
        args.insert("storage-mode".into(), mode.clone());
    }
    for id in ids {
        let value =
            macc_core::coordinator::state::coordinator_state_registry_load(&paths.root, &args)?;
        let registry = macc_core::coordinator::model::TaskRegistry::from_value(&value)?;
        let task = registry
            .find_task(id)
            .ok_or_else(|| MaccError::Validation(format!("Unknown repaired task {id}")))?;
        if !incident::repair_candidate(task) {
            return Err(MaccError::Validation(format!(
                "Task {id} is not an eligible repair root"
            )));
        }
        if task.state == "blocked" {
            macc_core::service::coordinator_workflow::coordinator_unblock_task(
                paths, config, &env, id, proof,
            )?;
        } else {
            // Crash recovery runs only with the coordinator stopped; preserve committed work for reconciliation.
            let mut registry = registry;
            let task = registry.find_task_mut(id).unwrap();
            task.set_workflow_state(macc_core::coordinator::WorkflowState::Todo);
            task.ensure_runtime()
                .set_status(macc_core::coordinator::RuntimeStatus::Idle);
            macc_core::coordinator::state::coordinator_state_registry_save(
                &paths.root,
                &args,
                &registry.to_value()?,
            )?;
        }
    }
    Ok(())
}

/// A crashed engine can leave performers alive. Signal only their recorded groups.
async fn stop_orphan_jobs(incident: &macc_core::supervisor::incident::Incident) -> Result<()> {
    #[cfg(unix)]
    for task in &incident.tasks {
        if let Some(pid) = task.task_runtime.pid {
            if !macc_core::coordinator::helpers::is_pid_running(pid) {
                continue;
            }
            let group = task.task_runtime.process_group_id.ok_or_else(|| {
                MaccError::Validation(format!(
                    "Live orphan {} has no recorded process group; intervention deferred",
                    task.id
                ))
            })?;
            if group <= 1
                || unsafe { libc::getpgid(pid as i32) } as i64 != group
                || group == unsafe { libc::getpgrp() } as i64
            {
                return Err(MaccError::Validation(format!(
                    "Orphan {} process identity does not match its recorded group",
                    task.id
                )));
            }
            unsafe {
                libc::kill(-(group as i32), libc::SIGTERM);
            }
            let deadline = tokio::time::Instant::now() + Duration::from_secs(3);
            while macc_core::coordinator::helpers::is_pid_running(pid)
                && tokio::time::Instant::now() < deadline
            {
                tokio::time::sleep(Duration::from_millis(50)).await;
            }
            if macc_core::coordinator::helpers::is_pid_running(pid) {
                unsafe {
                    libc::kill(-(group as i32), libc::SIGKILL);
                }
            }
        }
    }
    Ok(())
}

fn ensure_not_stopped(paths: &ProjectPaths) -> Result<()> {
    if super::shutdown::requested(paths)? {
        return Err(MaccError::Validation(
            "Operator stopped coordinator; intervention canceled without restart".into(),
        ));
    }
    Ok(())
}
