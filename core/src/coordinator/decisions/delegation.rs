//! One durable authority check for native delegation and legacy gate projections.
use super::*;
use crate::coordinator::model::{Task, TaskRegistry};
use std::{collections::BTreeSet, path::Path};

fn human_veto(
    store: &Store,
    plan: &Plan,
    task: &Task,
    root: &Path,
    revision: &str,
) -> crate::Result<Option<crate::coordinator::approval::GateEvaluation>> {
    let votes = store.verified_approvals(root, plan)?;
    let gate = task
        .gate
        .as_ref()
        .ok_or_else(|| error("Missing approval gate"))?;
    Ok(crate::coordinator::approval::evaluate_gate(
        gate,
        true,
        Some(revision),
        votes.get(&task.id).map(Vec::as_slice).unwrap_or_default(),
        &chrono::Utc::now().to_rfc3339(),
    )
    .filter(|eval| {
        eval.state == crate::coordinator::WorkflowState::Rejected
            || eval.status == crate::coordinator::model::ApprovalStatus::ChangesRequested
    }))
}

fn valid(decision: &Decision, task: &Task, plan: &Plan, policy: &Policy, revision: &str) -> bool {
    let Ok(node) = super::graph::task(&plan.graph, &task.id) else {
        return false;
    };
    let Ok(gate) =
        serde_json::from_value::<crate::coordinator::model::TaskGate>(node["gate"].clone())
    else {
        return false;
    };
    if task.gate.as_ref() != Some(&gate)
        || decision.subject_digest != digest(&super::graph::contract(node))
    {
        return false;
    }
    decision.plan_id == plan.id
        && decision.request.task_id == task.id
        && decision.request.kind == "approval"
        && decision.status == "resolved"
        && !decision.requires_human_renewal
        && decision.resolution.as_ref().is_some_and(|r| {
            matches!(
                r.origin.as_str(),
                "delegated_autonomous" | "coordinator_reconciliation" | "verified_existing_policy"
            ) && r.policy_digest == digest(policy)
                && r.source_revision.as_deref() == Some(revision)
                && r.conditions.is_empty()
        })
}
pub(crate) fn verified(root: &Path, registry: &TaskRegistry) -> crate::Result<BTreeSet<String>> {
    let p = policy(root)?;
    if !p.enabled || p.human_question_min_risk != Consultation::NeverAsk {
        return Ok(BTreeSet::new());
    }
    let (store, plan) = active_plan(root)?;
    let decisions = store.decisions(&plan.id)?;
    let mut allowed = BTreeSet::new();
    for task in registry.tasks.iter().filter(|t| t.is_human_approval_gate()) {
        let Some(proof) = task.task_runtime.extra.get("delegated_gate_resolution") else {
            continue;
        };
        let subject = task
            .gate
            .as_ref()
            .and_then(|g| g.subject_task.as_deref())
            .unwrap_or_default();
        let Some(revision) =
            crate::coordinator::approval::subject_revision(root, &plan.reference_branch, subject)
        else {
            continue;
        };
        if proof["origin"] == "delegated_autonomous"
            && proof["subject_revision"] == revision
            && proof["subject_task"] == subject
            && proof["policy_digest"] == digest(&p)
            && human_veto(&store, &plan, task, root, &revision)?.is_none()
            && decisions
                .iter()
                .any(|d| proof["decision_id"] == d.id && valid(d, task, &plan, &p, &revision))
        {
            allowed.insert(task.id.clone());
        }
    }
    Ok(allowed)
}
pub(super) fn resolve(
    store: &mut Store,
    plan: &Plan,
    task: &mut Task,
    p: &Policy,
    reference: &str,
    root: &Path,
) -> crate::Result<bool> {
    let gate = task
        .gate
        .as_ref()
        .ok_or_else(|| error("Missing delegated approval gate"))?;
    let subject = gate.subject_task.as_deref().unwrap_or_default();
    let Some(revision) = crate::coordinator::approval::subject_revision(root, reference, subject)
    else {
        return Ok(false);
    };
    if let Some(veto) = human_veto(store, plan, task, root, &revision)? {
        task.task_runtime.extra.remove("delegated_gate_resolution");
        task.set_workflow_state(veto.state);
        task.task_runtime
            .set_status(crate::coordinator::RuntimeStatus::WaitingForUser);
        task.task_runtime
            .set_last_error_details("E909", "approval", veto.note);
        return Ok(false);
    }
    let request: Request = serde_json::from_value(serde_json::json!({"idempotency_key":format!("gate:{}:{}:{}",task.id,revision,digest(p)),"task_id":task.id,"kind":"approval","question":format!("Resolve approval of {subject} at {revision}"),"context":gate.description.as_deref().unwrap_or_default(),"investigated":[format!("Verified subject {subject} integrated at {revision}; prerequisites satisfied")],"recommendation":format!("Proceed under explicit never_ask delegation for {subject} at {revision}; this is not a human vote"),"risk":"high","impact":{"evidence":[revision]}})).map_err(|e|error(e.to_string()))?;
    let decision = store.submit(plan, request, "coordinator", "coordinator", p)?;
    if !valid(&decision, task, plan, p, &revision) {
        task.task_runtime.extra.remove("delegated_gate_resolution");
        return Ok(false);
    }
    task.task_runtime.extra.insert("delegated_gate_resolution".into(),serde_json::json!({"decision_id":decision.id,"origin":"delegated_autonomous","subject_revision":revision,"policy_digest":digest(p),"subject_task":subject}));
    task.task_runtime.approval = None;
    if task.task_runtime.last_error_code.as_deref() == Some("E909") {
        task.task_runtime.clear_last_error_details();
    }
    task.set_workflow_state(crate::coordinator::WorkflowState::Approved);
    task.task_runtime
        .set_status(crate::coordinator::RuntimeStatus::Idle);
    Ok(true)
}
