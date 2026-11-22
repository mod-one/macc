//! Route pre-spawn scoped PRD incidents through the owner's revision-checked unblock.
use super::{
    repair::Diagnosis,
    report::{Intervention, Progress},
};
use macc_core::{
    config::CanonicalConfig,
    coordinator::{control::Request, scoped_prd},
    service::coordinator_workflow,
    supervisor::{incident, SupervisorConfig},
    MaccError, ProjectPaths, Result,
};

pub(super) fn handle(
    paths: &ProjectPaths,
    canonical: &CanonicalConfig,
    config: &SupervisorConfig,
    report: &mut Intervention,
    progress: &Progress<'_>,
) -> Result<bool> {
    let roots: Vec<_> = report
        .incident
        .tasks
        .iter()
        .filter(|t| {
            (incident::repair_candidate(t) || scoped_prd::requeued(t))
                && !incident::in_flight_phase(t)
        })
        .collect();
    if roots.is_empty()
        || !roots
            .iter()
            .all(|t| scoped_prd::is_dispatch_failure(t) || scoped_prd::requeued(t))
    {
        return Ok(false);
    }
    let mut requests = Vec::new();
    let mut descriptions = Vec::new();
    let task_ids = roots.iter().map(|t| t.id.clone()).collect();
    for task in roots {
        if task.task_runtime.run_id.as_deref() != Some(&report.incident.run_id)
            || task.task_runtime.coordinator_epoch != Some(report.incident.epoch)
        {
            return Err(MaccError::Validation(
                "Scoped PRD incident belongs to an earlier run or epoch".into(),
            ));
        }
        if scoped_prd::requeued(task) {
            descriptions.push(format!(
                "{} has already passed native worker recovery.",
                task.id
            ));
            continue;
        }
        let description = scoped_prd::describe(&paths.root, task).map(|i| i.to_string())
            .unwrap_or_else(||format!("Inspecting the retained scoped PRD for {} after dispatch failed before launch.", task.id));
        requests.push(Request::new(task, &description)?);
        descriptions.push(description);
    }
    report.diagnosis = Some(Diagnosis {
        summary: descriptions.join("\n"),
        task_ids,
        ..Default::default()
    });
    if !requests.is_empty() && !config.repair_enabled {
        return Err(MaccError::Validation(
            "Automatic repair is disabled. Inspect the retained PRD and supervisor settings."
                .into(),
        ));
    }
    if report
        .incident
        .tasks
        .iter()
        .filter_map(|t| t.task_runtime.pid)
        .any(macc_core::coordinator::helpers::is_pid_running)
    {
        return Err(MaccError::Io {
            path: paths.root.display().to_string(),
            action: "wait for active performers before worker recovery".into(),
            source: std::io::ErrorKind::WouldBlock.into(),
        });
    }
    progress.enter(
        report,
        "requeuing",
        "Checking worker ownership and PRD provenance before resuming the task",
    )?;
    for request in requests {
        coordinator_workflow::submit_unblock_request(
            paths,
            canonical.automation.coordinator.as_ref(),
            &Default::default(),
            &request,
        )?;
    }
    report.detail = "Worker preflight recovery completed. Tasks are queued for dispatch.".into();
    Ok(true)
}
