use super::io;
use super::repair;
use super::report::{write_json, Intervention};
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
    restart_allowed: bool,
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
    let progress = super::report::Progress { paths, config, dir };
    // The engine stopped while every incident task was between phases: no
    // task failed and nothing needs repairing — the restarted run resumes
    // their pending test, review, fix or merge. No AI call is needed.
    if incident::resumable_by_restart(&report.incident) {
        if restart_allowed {
            let ids: Vec<String> = report.incident.tasks.iter().map(|t| t.id.clone()).collect();
            report.detail = format!(
                "The coordinator stopped ({}) while {} {} between phases; no task failed, so it was restarted to resume {}.",
                report.incident.reason,
                describe_tasks(&report.incident),
                if ids.len() == 1 { "was" } else { "were" },
                if ids.len() == 1 { "it" } else { "them" },
            );
            return restart_only(paths, canonical, report, lock, &progress, &ids).await;
        }
        report.detail = format!(
            "The coordinator already stopped the same way after a supervisor restart ({}); diagnosing instead of restarting again.",
            report.incident.reason
        );
    }
    // The supervisor commits the repair itself: fail before spending AI calls.
    let identity = macc_core::git::missing_git_identity_fields(&paths.root);
    if !identity.is_empty() {
        return Err(MaccError::Validation(format!(
            "Git identity is missing ({}); the supervisor cannot commit a repair",
            identity.join(", ")
        )));
    }
    let agent = tool::Agent::resolve(&paths.root, canonical, config)?;
    report.tool = Some(agent.id.clone());
    let target = super::workspace::target(&paths.root, canonical.automation.coordinator.as_ref())?;
    report.target_branch = target.branch.clone();
    let worktree = super::workspace::prepare(&paths.root, &report.incident.id, &target)?;
    report.worktree = Some(worktree.clone());
    let activity = if report.diagnosis.is_some() {
        "Reusing the recorded diagnosis".to_string()
    } else {
        format!(
            "AI diagnosis with {} (logs, task requirements, source)",
            agent.id
        )
    };
    progress.enter(report, "diagnosing", activity)?;
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
    if diagnosis.restart_only && !diagnosis.requires_human {
        if !restart_allowed {
            return Err(MaccError::Validation(format!(
                "The coordinator already stopped the same way after a supervisor restart; not restarting again: {}",
                diagnosis.summary
            )));
        }
        let ids = if diagnosis.task_ids.is_empty() {
            report.incident.tasks.iter().map(|t| t.id.clone()).collect()
        } else {
            diagnosis.task_ids.clone()
        };
        return restart_only(paths, canonical, report, lock, &progress, &ids).await;
    }
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
    let proof = repair::fix(
        &agent,
        &progress,
        &worktree,
        &diagnosis,
        config.intervention_timeout_seconds,
        report,
    )
    .await?;
    report.detail = proof;
    let destination = target.branch.as_deref().unwrap_or("the project HEAD");
    progress.enter(
        report,
        "integrating",
        format!("Committing the repair and publishing it to {destination}"),
    )?;
    ensure_not_stopped(paths)?;
    report.commit = Some(repair::integrate(&paths.root, &worktree, &target)?);
    progress.enter(
        report,
        "requeuing",
        format!("Requeuing {}", diagnosis.task_ids.join(", ")),
    )?;
    ensure_not_stopped(paths)?;
    requeue(paths, canonical, &diagnosis.task_ids, &report.detail)?;
    restart(paths, canonical, report, lock, &progress).await
}

/// Recovery without a source repair: requeue the incident's roots (tasks
/// between phases keep their state) and restart the coordinator.
async fn restart_only(
    paths: &ProjectPaths,
    canonical: &CanonicalConfig,
    report: &mut Intervention,
    lock: &macc_core::fs_lock::AdvisoryLock,
    progress: &super::report::Progress<'_>,
    ids: &[String],
) -> Result<()> {
    report.restart_only = true;
    progress.enter(
        report,
        "requeuing",
        format!("Resuming {} without a source repair", ids.join(", ")),
    )?;
    ensure_not_stopped(paths)?;
    let proof = format!("supervisor restart: {}", report.incident.reason);
    requeue(paths, canonical, ids, &proof)?;
    restart(paths, canonical, report, lock, progress).await
}

fn describe_tasks(incident: &incident::Incident) -> String {
    incident
        .tasks
        .iter()
        .map(|t| format!("{} ({})", t.id, t.state))
        .collect::<Vec<_>>()
        .join(", ")
}

/// Restart the managed coordinator and wait until it is ready.
async fn restart(
    paths: &ProjectPaths,
    canonical: &CanonicalConfig,
    report: &mut Intervention,
    lock: &macc_core::fs_lock::AdvisoryLock,
    progress: &super::report::Progress<'_>,
) -> Result<()> {
    progress.enter(report, "restarting", "Restarting the coordinator")?;
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
        } else if incident::in_flight_phase(task) {
            // Between phases: the restarted run resumes the pending phase from
            // this state. Resetting it to todo would discard the delivered
            // work's progress (tests passed, review recorded). Only drop the
            // dead process reference.
            let mut registry = registry;
            let task = registry.find_task_mut(id).unwrap();
            task.ensure_runtime().pid = None;
            macc_core::coordinator::state::coordinator_state_registry_save(
                &paths.root,
                &args,
                &registry.to_value()?,
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
