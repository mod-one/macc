use super::{
    binding, error, observations, packets, verifier, Category, CheckRef, Outcome, Packet, Status,
};
use crate::config::{CoordinatorConfig, CoordinatorConfigResolved};
use crate::coordinator::{
    decisions as dm, helpers,
    model::{ExternalBlockResolution, Task, TaskRegistry},
    repair_handoff, retained_reconciliation, state, task_selector,
    types::CoordinatorEnvConfig,
    RuntimeStatus,
};
use crate::Result;
use serde_json::json;
use std::{
    path::Path,
    time::{Duration, Instant},
};

fn verify_binding(root: &Path, prd: &Path, task: &Task, packet: &Packet) -> Result<()> {
    if binding::version(root, prd, task, &packet.proposal)? != packet.binding {
        return Err(error(
            "The recorded block or candidate changed. A fresh diagnosis is required",
        ));
    }
    Ok(())
}

/// A new supervisor incident can reuse a still-current wait without another AI call.
pub fn waiting_check<'a>(root: &Path, prd: &Path, task: &'a Task) -> Option<&'a CheckRef> {
    let check = task.task_runtime.resume_check.as_deref()?;
    if check.category != Category::TechnicalCondition
        || check
            .last
            .as_ref()
            .is_some_and(|outcome| outcome.status != Status::Waiting)
    {
        return None;
    }
    let packet = packets::load(root, check).ok()?;
    verify_binding(root, prd, task, &packet).ok()?;
    Some(check)
}

fn observe(
    root: &Path,
    task: &Task,
    packet: &Packet,
    digest: &mut Option<String>,
) -> Result<Outcome> {
    let p = &packet.proposal;
    let v = p
        .verifier
        .as_ref()
        .ok_or_else(|| error("Technical condition has no script"))?;
    let candidate = binding::candidate(root, task)?;
    let commit = crate::git::head_commit(&candidate)?;
    let deadline = Instant::now() + Duration::from_secs(3);
    let observation = match observations::collect(&v.source, &candidate, deadline)? {
        observations::Reading::Observed(observation) => observation,
        observations::Reading::Missing(path) => {
            return Ok(Outcome {
                status: Status::Waiting,
                detail: format!(
                    "Supply {path} in the retained working directory for {}",
                    p.target
                ),
            })
        }
    };
    *digest = Some(dm::digest(&observation));
    verifier::evaluate(
        &v.script,
        &json!({"observation":observation,"target":p.target,"commit":commit,"now_unix":chrono::Utc::now().timestamp()}),
        deadline,
    )
}

fn check_task(
    root: &Path,
    prd: &Path,
    config: Option<&CoordinatorConfig>,
    task: &mut Task,
    reference: &mut CheckRef,
) -> Result<Outcome> {
    let packet = packets::load(root, reference)?;
    verify_binding(root, prd, task, &packet)?;
    let outcome = observe(root, task, &packet, &mut reference.observation_sha256)?;
    if outcome.status != Status::Ready {
        return Ok(outcome);
    }
    let p = &packet.proposal;
    // Stage recovery in a copy. A failed reconciliation keeps the original task blocked.
    let mut next = task.clone();
    if p.decision.is_none() {
        crate::coordinator::worktree_recovery::recover_task(root, &mut next)?;
        crate::session_store::checkpoints::recovery::ensure_task_ready(root, &next.id)?;
        if next.has_worktree_attached() {
            let branch = CoordinatorConfigResolved::resolve(config).reference_branch;
            let base = repair_handoff::reconcile_task(root, &mut next, &branch)?;
            next.task_runtime.extra.insert(
                repair_handoff::RESUME_KEY.into(),
                json!({"base":base,"evidence":reference.id}),
            );
            crate::session_store::checkpoints::recovery::ensure_task_ready(root, &next.id)?;
        }
    }
    verify_binding(root, prd, &next, &packet)?;
    if p.phase == "merge" {
        dm::merge_ready(root, &next.id)?
    }
    if let Some(decision) = &p.decision {
        let head = packet.binding["commit"]
            .as_str()
            .ok_or_else(|| error("Missing candidate commit"))?;
        if next.has_worktree_attached() {
            let branch = CoordinatorConfigResolved::resolve(config).reference_branch;
            if !crate::git::merge_base_is_ancestor(root, &format!("refs/heads/{branch}"), head)? {
                return Err(error("The retained candidate needs reconciliation with its reference branch before this decision can resume"));
            }
        }
        dm::Store::open(root)?.record_observed_condition(
            decision,
            &next.id,
            &p.criterion,
            &reference.id,
            &outcome.detail,
            head,
        )?;
        // Native decision sync rechecks all other decisions and reconciles the exact retained phase.
    } else {
        if let Some(block) = &next.blocked_on_external {
            next.task_runtime.external_block_resolution = Some(ExternalBlockResolution {
                block_digest: Some(dm::digest(block)),
                evidence: format!("{}: {}", reference.id, outcome.detail),
                resolved_at: helpers::now_iso_coordinator(),
                block_reason: block.reason.clone(),
                tracking_id: block.tracking_id.clone(),
            });
        }
        let workflow = retained_reconciliation::resume_state(root, &mut next);
        next.set_workflow_state(workflow);
        let rt = &mut next.task_runtime;
        rt.set_status(RuntimeStatus::Idle);
        rt.pid = None;
        rt.last_error = None;
        rt.clear_last_error_details();
        *task = next;
    }
    Ok(outcome)
}

pub fn replay_at_startup(
    root: &Path,
    prd: &Path,
    config: Option<&CoordinatorConfig>,
    env: &CoordinatorEnvConfig,
) -> Result<()> {
    let args = packets::storage_args(config, env);
    let mut registry =
        TaskRegistry::from_value(&state::coordinator_state_registry_load(root, &args)?)?;
    let mut checked = false;
    let mut released = false;
    for task in &mut registry.tasks {
        if task.state != "blocked" {
            continue;
        }
        let Some(mut reference) = task
            .task_runtime
            .resume_check
            .clone()
            .filter(|r| r.category == Category::TechnicalCondition)
        else {
            continue;
        };
        reference.observation_sha256 = None;
        let outcome =
            check_task(root, prd, config, task, &mut reference).unwrap_or_else(|e| Outcome {
                status: Status::Error,
                detail: e.to_string(),
            });
        checked = true;
        released |= outcome.status == Status::Ready;
        helpers::append_coordinator_event_with_severity(
            root,
            "resume_check",
            &task.id,
            &reference.phase,
            outcome.status.label(),
            &format!(
                "{} (observation {}): {}",
                reference.id,
                reference
                    .observation_sha256
                    .as_deref()
                    .unwrap_or("unavailable"),
                outcome.detail
            ),
            if outcome.status == Status::Error {
                "warning"
            } else {
                "info"
            },
        )?;
        reference.last = Some(outcome);
        reference.checked_at = Some(helpers::now_iso_coordinator());
        task.task_runtime.resume_check = Some(reference);
    }
    if checked {
        state::coordinator_state_registry_save(root, &args, &registry.to_value()?)?;
    }
    if released {
        // Persist receipts before dependent projections. Startup can recover after either write.
        dm::sync(root, prd, &mut registry)?;
        task_selector::reconcile_task_blocks(&mut registry, &helpers::now_iso_coordinator());
        state::coordinator_state_registry_save(root, &args, &registry.to_value()?)?;
    }
    Ok(())
}
