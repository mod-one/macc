use super::*;
use crate::{
    coordinator::{model::TaskRegistry, RuntimeStatus, WorkflowState},
    Result,
};
use serde_json::json;
use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
    time::Duration,
};

pub fn controller_lock(root: &Path) -> Result<Option<crate::fs_lock::AdvisoryLock>> {
    if !policy(root)?.enabled {
        return Ok(None);
    }
    let path = Store::location(root)?.with_file_name("controller.lock");
    Ok(Some(crate::fs_lock::AdvisoryLock::acquire(
        &path,
        Duration::ZERO,
        "durable decision controller",
    )?))
}
pub fn recover(root: &Path) -> Result<()> {
    if !policy(root)?.enabled {
        return Ok(());
    }
    let mut store = Store::open(root)?;
    store.project(root)?;
    Ok(())
}
pub fn publication_lock(root: &Path) -> Result<Option<crate::fs_lock::AdvisoryLock>> {
    if !policy(root)?.enabled {
        return Ok(None);
    }
    Ok(Some(Store::open(root)?.mutation_lock()?))
}
pub fn sync(root: &Path, prd: &Path, registry: &mut TaskRegistry) -> Result<()> {
    let p = policy(root)?;
    if !p.enabled {
        return Ok(());
    }
    let cfg = crate::load_canonical_config(&crate::ProjectPaths::from_root(root).config_path)?;
    let reference =
        crate::config::CoordinatorConfigResolved::resolve(cfg.automation.coordinator.as_ref())
            .reference_branch;
    let mut store = Store::open(root)?;
    store.project(root)?;
    let mut plan = store.ensure_plan(root, prd, &reference, registry)?;
    store.import_legacy(root, &plan)?;
    store.import_planner_requests(&plan, &p)?;
    let mut decisions = store.applicable_decisions(&plan)?;
    if p.human_question_min_risk == Consultation::NeverAsk {
        for decision in decisions.iter().filter(|d| d.status == "pending_human") {
            store.resolve_delegated(&decision.id, &p)?;
        }
        plan = store.plan(&plan.id)?;
        store.project(root)?;
        decisions = store.applicable_decisions(&plan)?;
    }
    registry
        .extra
        .entry("decision_execution_id".into())
        .or_insert_with(|| json!(uuid::Uuid::new_v4().to_string()));
    let mut satisfied = crate::coordinator::task_selector::satisfied_dependency_ids(registry);
    satisfied.extend(registry.external_merged_task_ids.iter().cloned());
    for task in &mut registry.tasks {
        if task.is_merged() {
            continue;
        }
        let active = task
            .task_runtime
            .pid
            .is_some_and(crate::coordinator::helpers::is_pid_running);
        let source = super::graph::task(&plan.graph, &task.id)?;
        if source.get("decision_disposition").is_some() {
            task.task_runtime
                .extra
                .insert("decision_quarantine".into(), json!(true));
            if !active {
                task.set_workflow_state(WorkflowState::Abandoned);
                task.task_runtime.set_status(RuntimeStatus::Idle);
            }
            continue;
        }
        let pending = decisions.iter().find(|d| {
            d.status == "pending_human"
                && super::context::applies(&plan, d, &task.id).unwrap_or(false)
        });
        if let Some(decision) = pending {
            task.task_runtime
                .extra
                .insert("decision_wait".into(), json!(decision.id));
            if !active {
                task.set_workflow_state(WorkflowState::Blocked);
                task.task_runtime.set_last_error_details(
                    "DM_PENDING",
                    "decisions",
                    format!(
                        "Decision {} awaits a human answer: {}",
                        decision.id, decision.request.question
                    ),
                );
                task.task_runtime.set_status(RuntimeStatus::Idle);
            }
            continue;
        }
        let refresh = task.task_runtime.extra.contains_key("decision_wait")
            || task
                .task_runtime
                .last_error_message
                .as_deref()
                .is_some_and(|s| s.contains("DM_STALE"));
        if refresh && !active && !task.is_human_approval_gate() {
            if task.has_worktree_attached() {
                let base = crate::coordinator::repair_handoff::reconcile(root, task, &reference)?;
                task.task_runtime.extra.insert(crate::coordinator::repair_handoff::RESUME_KEY.into(),json!({"base":base,"evidence":"Durable decision resolved; resume with current context"}));
            }
            task.set_workflow_state(WorkflowState::Todo);
            task.task_runtime.set_status(RuntimeStatus::Idle);
            task.task_runtime.pid = None;
            task.task_runtime.clear_last_error_details();
            task.task_runtime.last_error = None;
            task.task_runtime.extra.remove("decision_wait");
        }
        if p.human_question_min_risk != Consultation::NeverAsk {
            task.task_runtime.extra.remove("delegated_gate_resolution");
        }
        if task.is_human_approval_gate() && p.human_question_min_risk == Consultation::NeverAsk {
            // The gate outcome is delegated, not a human vote. Revalidate its
            // exact current subject every cycle before deriving satisfaction.
            let gate = task.gate.as_ref().unwrap();
            let subject = gate.subject_task.as_deref().unwrap_or_default();
            if !task.dependencies.iter().all(|d| satisfied.contains(d)) {
                continue;
            }
            if let Some(revision) =
                crate::coordinator::approval::subject_revision(root, &reference, subject)
            {
                let request:Request=serde_json::from_value(json!({"idempotency_key":format!("gate:{}:{}:{}",task.id,revision,digest(&p)),"task_id":task.id,"kind":"approval","question":format!("Resolve approval of {subject} at {revision}"),"context":gate.description,"investigated":[format!("Verified subject {subject} integrated at {revision}; prerequisites satisfied")],"recommendation":format!("Proceed under explicit never_ask delegation for {subject} at {revision}; this is not a human vote"),"risk":"high","impact":{"evidence":[revision]}})).map_err(|e|error(e.to_string()))?;
                let resolution = store.submit(&plan, request, "coordinator", "coordinator", &p)?;
                task.task_runtime.extra.insert("delegated_gate_resolution".into(),json!({"decision_id":resolution.id,"origin":"delegated_autonomous","subject_revision":revision,"policy_digest":digest(&p),"subject_task":subject}));
                task.task_runtime.approval = None;
                task.set_workflow_state(WorkflowState::Approved);
                task.task_runtime.set_status(RuntimeStatus::Idle);
                satisfied.insert(task.id.clone());
            }
        }
    }
    registry
        .extra
        .insert("decision_plan_id".into(), json!(plan.id));
    Ok(())
}

pub fn prepare_context(
    root: &Path,
    task_id: &str,
    phase: &str,
    worktree: &Path,
    attempt: &str,
) -> Result<Option<(ContextPackage, PathBuf)>> {
    let p = policy(root)?;
    if !p.enabled {
        return Ok(None);
    }
    let (mut store, plan) = active_plan(root)?;
    let context = store.context(&plan, task_id, attempt, phase, &p)?;
    let semantic_digest = digest(&json!([
        context.contract_digest,
        context.bindings,
        context.policy
    ]));
    let mut registry = registry(root)?;
    if registry
        .find_task(task_id)
        .and_then(|t| t.task_runtime.extra.get("decision_bindings_digest"))
        .and_then(serde_json::Value::as_str)
        .is_some_and(|old| old != semantic_digest)
    {
        if crate::session_store::is_sqlite(root)? {
            let _session_lock = crate::session_store::lock(root)?;
            let (db, authority) = crate::session_store::active_database(root)?;
            store::sql(db.execute("UPDATE logical_sessions SET status='rotated' WHERE project_id=?1 AND task_id=?2 AND status='available'",rusqlite::params![authority.project_id,task_id]))?;
        }
        if let Some(task) = registry.find_task_mut(task_id) {
            task.task_runtime.active_session_id = None;
            task.task_runtime.last_session_id = None;
            task.task_runtime.last_session_tool = None;
        }
    }
    let issued = store.issue(&plan, task_id, attempt, phase, worktree, &p)?;
    if let Some(task) = registry.find_task_mut(task_id) {
        task.task_runtime
            .extra
            .insert("decision_bindings_digest".into(), json!(semantic_digest));
        task.task_runtime.extra.insert("decision_context".into(),json!({"plan_id":plan.id,"attempt_id":attempt,"phase":phase,"digest":issued.0.context_digest}));
        task.task_runtime.extra.insert(
            "decision_has_bindings".into(),
            json!(issued
                .0
                .bindings
                .iter()
                .any(|b| !b.resolution.as_ref().is_some_and(|r| r.advisory))),
        );
    }
    crate::coordinator::state::coordinator_state_registry_save(
        root,
        &BTreeMap::new(),
        &registry.to_value()?,
    )?;
    Ok(Some(issued))
}
pub fn merge_ready(root: &Path, task_id: &str) -> Result<()> {
    let p = policy(root)?;
    if !p.enabled {
        return Ok(());
    }
    let (store, plan) = active_plan(root)?;
    let registry = registry(root)?;
    let task = registry
        .find_task(task_id)
        .ok_or_else(|| error("Missing merge task"))?;
    let info = task
        .task_runtime
        .extra
        .get("decision_context")
        .ok_or_else(|| error("No decision context receipt for merge"))?;
    let attempt = info["attempt_id"].as_str().unwrap_or_default();
    let phase = info["phase"].as_str().unwrap_or_default();
    let context = store.read_context(&plan.id, attempt, phase)?;
    let fresh = store.context(&plan, task_id, attempt, phase, &p)?;
    if context.context_digest != fresh.context_digest {
        return Err(error(
            "DM_STALE: merge context no longer matches applicable decisions",
        ));
    }
    let receipt: Option<String> = store::sql(store.db.query_row(
        "SELECT receipt FROM contexts WHERE plan_id=?1 AND attempt_id=?2 AND phase=?3",
        rusqlite::params![plan.id, attempt, phase],
        |r| r.get(0),
    ))?;
    let receipt: ContextReceipt =
        store::decode(&receipt.ok_or_else(|| error("Context was not delivered"))?)?;
    if receipt.completion_status != "accepted" {
        return Err(error(
            "Context completion was not accepted; inspect freshness/compliance diagnostics",
        ));
    }
    if !receipt.acknowledged {
        return Err(error("Missing model context acknowledgement"));
    }
    if context
        .bindings
        .iter()
        .any(|b| !b.resolution.as_ref().is_some_and(|r| r.advisory))
        && phase != "review"
    {
        return Err(error(
            "Applicable decisions require independent review before merge",
        ));
    }
    if phase == "review" {
        let worktree = task
            .worktree_path()
            .ok_or_else(|| error("Missing reviewed worktree"))?;
        if receipt.reviewed_commit.as_deref()
            != Some(crate::git::head_commit(Path::new(worktree))?.as_str())
            || crate::git::is_dirty(Path::new(worktree))?
        {
            return Err(error("DM_STALE: reviewed artifacts changed; repeat independent review before publication"));
        }
    }
    for binding in &context.bindings {
        if !binding.resolution.as_ref().is_some_and(|r| r.advisory)
            && !receipt
                .compliance
                .get(&binding.id)
                .is_some_and(|c| c.status == "pass")
        {
            return Err(error("Missing passing decision compliance evidence"));
        }
    }
    Ok(())
}

impl Store {
    pub fn resolve_delegated(&mut self, id: &str, policy: &Policy) -> Result<Decision> {
        let _fence = self.mutation_lock()?;
        use super::store::*;
        use rusqlite::TransactionBehavior;
        if policy.human_question_min_risk != Consultation::NeverAsk {
            return Err(error("Full autonomy delegation is not active"));
        }
        let pending = self.decision(id)?;
        let latest = self.plan(&pending.plan_id)?;
        if pending.request.task_id != "*"
            && digest(&super::graph::contract(super::graph::task(
                &latest.graph,
                &pending.request.task_id,
            )?)) != pending.subject_digest
        {
            return Err(error(
                "Pending request has a stale subject; reconcile before autonomous resolution",
            ));
        }
        let proposed_graph = if let Some(patch) = &pending.request.proposed_patch {
            Some(self.preview_patch(&latest, patch, &registry(&self.root)?)?)
        } else {
            None
        };
        let tx = sql(self
            .db
            .transaction_with_behavior(TransactionBehavior::Immediate))?;
        let mut d: Decision = decode(&sql(tx.query_row(
            "SELECT payload FROM requests WHERE id=?1",
            [id],
            |r| r.get::<_, String>(0),
        ))?)?;
        if d.status != "pending_human" {
            return Ok(d);
        }
        d.version += 1;
        d.status = "resolved".into();
        d.resolution = Some(Resolution {
            text: d.request.recommendation.clone(),
            selected_option_id: d.request.recommended_option_id.clone(),
            actor: "coordinator".into(),
            origin: "delegated_autonomous".into(),
            policy_digest: digest(policy),
            recorded_at: now(),
            advisory: false,
            source_revision: None,
        });
        sql(tx.execute(
            "UPDATE requests SET payload=?2 WHERE id=?1",
            rusqlite::params![id, encode(&d)?],
        ))?;
        let mut plan: Plan = decode(&sql(tx.query_row(
            "SELECT payload FROM plans WHERE id=?1",
            [&d.plan_id],
            |r| r.get::<_, String>(0),
        ))?)?;
        plan.decision_epoch += 1;
        if let (Some(patch), Some(graph)) = (&d.request.proposed_patch, proposed_graph) {
            super::graph::commit_graph(
                &tx,
                &mut plan,
                patch,
                graph,
                "coordinator delegated_autonomous",
            )?;
        }
        save_plan(&tx, &plan)?;
        event(&tx, &plan.id, "decision.resolved", &d)?;
        sql(tx.commit())?;
        Ok(d)
    }
}
