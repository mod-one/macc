use super::io;
use super::repair;
use super::report::{write_json, Intervention};
use super::task_requeue::requeue;
use super::tool;
use macc_core::config::CanonicalConfig;
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
    if report
        .incident
        .reason
        .contains(macc_core::prd_queue::transition::CODE)
    {
        return Err(MaccError::Validation(format!(
            "{}: review and explicitly accept the PRD queue transition; no application repair or AI diagnosis is appropriate", macc_core::prd_queue::transition::CODE
        )));
    }
    let progress = super::report::Progress { paths, config, dir };
    config.validate()?;
    if super::scoped_prd_recovery::handle(paths, canonical, config, report, &progress)? {
        if config.auto_restart && restart_allowed {
            report.restart_only = true;
            restart(paths, canonical, report, lock, &progress).await?;
        } else {
            report.status = "ready_to_resume".into();
            if config.auto_restart {
                report
                    .detail
                    .push_str(" The automatic restart budget is exhausted.");
            }
        }
        return Ok(());
    }
    stop_orphan_jobs(&report.incident).await?;
    macc_core::session_store::checkpoints::recovery::maintain(&paths.root)?;
    if let Some(diagnosis) = super::compliance_review::diagnosis(&paths.root, &report.incident)? {
        write_json(
            &dir.join("review-compliance-block.json"),
            &json!({"diagnosis":diagnosis,"source_repair_attempted":false,"compliance_accepted":false}),
        )?;
        let detail = diagnosis.summary.clone();
        report.diagnosis = Some(diagnosis);
        return Err(MaccError::Validation(detail));
    }
    if let Some(diagnosis) = super::resume_checks::recorded_wait(
        &paths.root,
        canonical.automation.coordinator.as_ref(),
        &report.incident,
    ) {
        report.status = "waiting_condition".into();
        report.detail = diagnosis.summary.clone();
        report.diagnosis = Some(diagnosis);
        return Ok(());
    }
    if let Some(diagnosis) =
        super::external_verification::pending_diagnosis(&paths.root, &report.incident)?
    {
        let detail = format!(
            "{} {}",
            diagnosis.summary,
            diagnosis.external_prerequisites.join("; ")
        );
        report.diagnosis = Some(diagnosis);
        return Err(MaccError::Validation(detail));
    }
    if let Some(diagnosis) = super::external_verification::diagnosis(&report.incident) {
        report.detail = diagnosis.summary.clone();
        report.diagnosis = Some(diagnosis);
        return Err(MaccError::Validation("DM_EXTERNAL_VERIFICATION: diagnostic report retained; provide and independently verify the selected assistance, or explicitly reconcile the decision. Source edits and an unchanged restart cannot satisfy it.".into()));
    }
    // The engine stopped while every incident task was between phases: no
    // task failed and nothing needs repairing — the restarted run resumes
    // their pending test, review, fix or merge. No AI call is needed.
    if incident::resumable_by_restart(&report.incident) {
        if !config.auto_restart {
            return Err(MaccError::Validation("Automatic restart is disabled; coordinator phases are intact and can be resumed manually".into()));
        }
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
    if config.repair_enabled && config.auto_restart && restart_allowed {
        if let Some(ids) = super::output_protocol_recovery::recover(paths, &report.incident, dir)? {
            report.detail="MACC output protocol recovery after runtime update/explicit retry; retained work must pass native validation and delivery".into();
            return restart_only(paths, canonical, report, lock, &progress, &ids).await;
        }
    }
    if config.repair_enabled && config.auto_restart && restart_allowed {
        if let Some(ids) = super::review_protocol_recovery::recover(paths, &report.incident, dir)? {
            report.detail="MACC-REVIEW-PROTOCOL-REFRESH: restarting the retained review after runtime update or explicit retry; no acceptance inferred".into();
            return restart_only(paths, canonical, report, lock, &progress, &ids).await;
        }
    }
    if config.repair_enabled && config.auto_restart && restart_allowed {
        if let Some(ids) = super::decision_budget_recovery::recover(paths, &report.incident, dir)? {
            report.detail=format!("{}: current prompts fit the configured policy; resuming {} without a source repair or budget override",macc_core::coordinator::decisions::CONTEXT_BUDGET_CODE,ids.join(", "));
            return restart_only(paths, canonical, report, lock, &progress, &ids).await;
        }
    }
    if config.repair_enabled && config.auto_restart {
        let checkpoint_recovery = if restart_allowed {
            super::checkpoint_recovery::recover(paths, &report.incident, dir)
        } else {
            super::checkpoint_recovery::preview(paths, &report.incident, dir)
        };
        if let Err(error) = &checkpoint_recovery {
            report.diagnosis = Some(repair::Diagnosis {resume_check_ids:Default::default(),
                summary: format!("MACC-CHECKPOINT-CONTEXT-DRIFT: retained checkpoint recovery was refused: {error}"),
                repairable: false, requires_human: true, restart_only: false,
                task_ids: report.incident.tasks.iter().filter(|t| incident::repair_candidate(t)).map(|t|t.id.clone()).collect(),
                external_prerequisites: vec![],
                retry_at: None, unavailable_tool: None, related_task_ids: vec![],
                macc_findings: vec!["Checkpoint recovery stopped before application repair; inspect checkpoint-recovery-blocked.json for the exact changed field. Worktree content drift may indicate a tool still writing after invocation shutdown; it does not establish an ownership conflict.".into()],
                macc_repair_required: false,
            });
        }
        if let Some(ids) = checkpoint_recovery? {
            if !restart_allowed {
                let message = format!("{}: native checkpoint recovery validated, but the restart budget for this unchanged work and decision binding is exhausted. Evidence: checkpoint-recovery.json. No checkpoint was reconciled and retained work is preserved; inspect the recurring refresh before explicitly retrying.", super::checkpoint_recovery::RESTART_LIMIT);
                report.diagnosis = Some(repair::Diagnosis {
                    resume_check_ids: Default::default(),
                    summary: message.clone(),
                    repairable: false,
                    requires_human: false,
                    restart_only: false,
                    task_ids: ids,
                    external_prerequisites: vec![],
                    retry_at: None,
                    unavailable_tool: None,
                    related_task_ids: vec![],
                    macc_findings: vec![
                        "Native context refresh recurred with unchanged semantic recovery evidence"
                            .into(),
                    ],
                    macc_repair_required: true,
                });
                return Err(MaccError::Validation(message));
            }
            report.detail = format!("MACC-CHECKPOINT-CONTEXT-DRIFT: execution context recovered without changing task requirements for {}",ids.join(", "));
            return restart_only(paths, canonical, report, lock, &progress, &ids).await;
        }
    }
    // The supervisor commits the repair itself: fail before spending AI calls.
    let identity = macc_core::git::missing_git_identity_fields(&paths.root);
    if config.repair_enabled && !identity.is_empty() {
        return Err(MaccError::Validation(format!(
            "Git identity is missing ({}); the supervisor cannot commit a repair",
            identity.join(", ")
        )));
    }
    let agent = tool::Agent::resolve(&paths.root, canonical, config)?;
    report.tool = Some(agent.id.clone());
    report.model = (!agent.model.is_empty()).then(|| agent.model.clone());
    report.effort = (!agent.effort.is_empty()).then(|| agent.effort.clone());
    let target = super::workspace::target(&paths.root, canonical.automation.coordinator.as_ref())?;
    report.target_branch = target.branch.clone();
    let worktree = super::workspace::prepare(
        &paths.root,
        &report.incident.id,
        &target,
        config.retain_repair_branches,
    )?;
    report.worktree = Some(worktree.clone());
    agent.prepare_settings(&worktree)?;
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
        repair::diagnose(
            &agent,
            &paths.root,
            &worktree,
            dir,
            &report.incident,
            canonical.automation.coordinator.as_ref(),
        )
        .await?
    };
    super::policy::enrich(
        &mut diagnosis,
        &report.incident,
        chrono::Utc::now().timestamp(),
    );
    report.detail = diagnosis.summary.clone();
    report.diagnosis = Some(diagnosis.clone());
    super::product_findings::publish(paths, report, &diagnosis)?;
    if !diagnosis.macc_repair_required && diagnosis.waiting_for_checks() {
        report.status = "waiting_condition".into();
        return Ok(());
    }
    super::product_findings::ensure_application_repair(&diagnosis)?;
    if diagnosis.restart_only && !diagnosis.requires_human {
        if !config.auto_restart {
            return Err(MaccError::Validation(
                "Automatic restart is disabled; start the coordinator manually when ready".into(),
            ));
        }
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
    if !diagnosis.requires_human
        && !diagnosis.repairable
        && config.repair_enabled
        && config.auto_restart
        && restart_allowed
    {
        if let Some(ids) =
            super::checkpoint_recovery::after_diagnosis(paths, &report.incident, dir)?
        {
            report.detail="Native checkpoint decision context reconciled; resuming retained phases without source repair".into();
            return restart_only(paths, canonical, report, lock, &progress, &ids).await;
        }
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
    if !config.repair_enabled {
        return Err(MaccError::Validation(
            "Automatic repair is disabled; diagnosis retained. Enable repair_enabled to continue"
                .into(),
        ));
    }
    let source_repair_refs =
        match super::external_verification::source_repair_refs(&paths.root, &diagnosis.task_ids) {
            Ok(refs) => refs,
            Err(error) => {
                diagnosis.requires_human = true;
                diagnosis.repairable = false;
                diagnosis.summary = format!("DM_PENDING: {error}");
                diagnosis.external_prerequisites.push(error.to_string());
                report.diagnosis = Some(diagnosis);
                return Err(error);
            }
        };
    if !source_repair_refs.is_empty() && !config.ai_verification_enabled {
        return Err(MaccError::Validation("Native source prerequisite recovery requires independent AI verification; no external decision will be cleared by tests alone".into()));
    }
    report.validation = repair::validation_commands(&paths.root, config)?;
    let proof = repair::fix(&agent, &progress, &worktree, &diagnosis, report).await?;
    report.detail = format!(
        "{proof}; tests={}; AI verification={}",
        if config.tests_enabled {
            "enabled"
        } else {
            "skipped by configuration"
        },
        if config.ai_verification_enabled {
            "enabled"
        } else {
            "skipped by configuration"
        }
    );
    let destination = target.branch.as_deref().unwrap_or("the project HEAD");
    progress.enter(
        report,
        "integrating",
        format!("Committing the repair and publishing it to {destination}"),
    )?;
    ensure_not_stopped(paths)?;
    report.commit = Some(repair::integrate(
        &paths.root,
        &worktree,
        &target,
        &report.incident,
        report.repair_evidence.as_ref().ok_or_else(|| {
            MaccError::Validation("Missing native verified repair source snapshot".into())
        })?,
    )?);
    progress.enter(
        report,
        "requeuing",
        format!("Requeuing {}", diagnosis.task_ids.join(", ")),
    )?;
    ensure_not_stopped(paths)?;
    if !source_repair_refs.is_empty() {
        let mut store = macc_core::coordinator::decisions::Store::open(&paths.root)?;
        store.complete_source_repairs(
            &source_repair_refs,
            &target.base,
            report.commit.as_deref().expect("published repair"),
            &proof,
        )?;
    }
    requeue(paths, canonical, &diagnosis.task_ids, &report.detail)?;
    if !config.auto_restart {
        report.status = "repair_integrated".into();
        report.next_action = Some("Repair integrated and retained work reconciled. Automatic restart is disabled; start `macc coordinator run` when ready.".into());
        report.detail = format!("{}; repair integrated and tasks requeued; automatic restart disabled, coordinator remains stopped", report.detail);
        progress.step(
            report,
            "Repair integrated; start the coordinator manually when ready",
        )?;
        return Ok(());
    }
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
    progress.enter(
        report,
        "restart_requested",
        "Restarting the coordinator; recovery not yet verified",
    )?;
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
    let ids = report
        .diagnosis
        .as_ref()
        .map(|d| d.task_ids.clone())
        .filter(|ids| !ids.is_empty())
        .unwrap_or_else(|| report.incident.tasks.iter().map(|t| t.id.clone()).collect());
    progress.enter(
        report,
        "restart_requested",
        "Coordinator ready; checking that repaired tasks resume",
    )?;
    super::restart_observation::wait(
        paths,
        &report.incident.run_id,
        &ids,
        Duration::from_secs(15),
    )
    .await?;
    report.status = "recovered".into();
    Ok(())
}
/// A crashed engine can leave performers alive. Signal only their recorded groups.
async fn stop_orphan_jobs(incident: &macc_core::supervisor::incident::Incident) -> Result<()> {
    for task in incident.tasks.iter().filter(|task| task.state != "todo") {
        let runtime = &task.task_runtime;
        if let Some(pid) = runtime.pid.or_else(|| {
            runtime
                .performer_group
                .as_ref()
                .map(|g| i64::from(g.leader))
        }) {
            let task = task.clone();
            tokio::task::spawn_blocking(move || {
                macc_core::service::coordinator_workflow::stop_task_process(&task, pid)
            })
            .await
            .map_err(|error| {
                MaccError::Validation(format!("Stopping orphan performer: {error}"))
            })??;
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

#[cfg(all(test, target_os = "linux"))]
mod tests {
    use super::*;
    use macc_core::process_ownership::launched_group::{LaunchedGroup, Scope};
    use std::{os::unix::process::CommandExt, process::Command};

    #[tokio::test]
    async fn orphan_recovery_uses_the_registered_group_even_after_the_pid_was_cleared() {
        let mut child = Command::new("sleep")
            .arg("60")
            .process_group(0)
            .spawn()
            .unwrap();
        let pid = child.id() as i32;
        let group = LaunchedGroup::from_child(
            pid,
            "ff2edfb5-cb3c-4878-815e-a87eddc84513".parse().unwrap(),
            Scope::PerformerGroup,
        );
        let incident = incident::Incident {
            id: "incident".into(),
            run_id: "run".into(),
            epoch: 1,
            reason: "coordinator exited".into(),
            run_status: Some("crashed".into()),
            recent_events: vec![],
            tasks: vec![serde_json::from_value(json!({
                "id":"T","state":"claimed","task_runtime":{"performer_group":group}
            }))
            .unwrap()],
        };
        let result = stop_orphan_jobs(&incident).await;
        // Reap this fixture even if the regression returns without signalling it.
        let stopped = child.try_wait().unwrap().is_some();
        if !stopped {
            let _ = child.kill();
        }
        child.wait().unwrap();
        result.unwrap();
        assert!(stopped, "registered performer survived orphan recovery");
    }
}
