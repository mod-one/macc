//! Native unresolved prerequisites; a wait is not a provider failure.
use super::*;
use serde::{Deserialize, Serialize};
use std::path::Path;
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct PendingDecision {
    pub id: String,
    pub version: u64,
    pub status: String,
    pub task_id: String,
    pub question: String,
    #[serde(default)]
    pub owner_task_id: String,
    #[serde(default)]
    pub action: Option<String>,
    #[serde(default)]
    pub inaccessible_reason: Option<String>,
}
pub fn pending_for_task(root: &Path, task: &str) -> crate::Result<Option<PendingDecision>> {
    find_pending(root, task, false)
}

pub(crate) fn pending_owned_intervention(
    root: &Path,
    task: &str,
) -> crate::Result<Option<PendingDecision>> {
    find_pending(root, task, true)
}

fn find_pending(
    root: &Path,
    task: &str,
    owned_intervention_only: bool,
) -> crate::Result<Option<PendingDecision>> {
    if !policy(root)?.enabled {
        return Ok(None);
    }
    let (store, plan) = active_plan(root)?;
    find_pending_in_plan(&store, &plan, task, owned_intervention_only)
}

pub(crate) fn check_unblock_owner(
    root: &Path,
    task: &crate::coordinator::model::Task,
) -> crate::Result<()> {
    if !policy(root)?.enabled {
        return Ok(());
    }
    let Some(store) = Store::inspect_registered(root)? else {
        return Ok(());
    };
    let cfg = crate::load_canonical_config(&crate::ProjectPaths::from_root(root).config_path)?;
    let cfg = cfg.automation.coordinator.as_ref();
    let reference = crate::config::CoordinatorConfigResolved::resolve(cfg).reference_branch;
    let bound = task
        .task_runtime
        .extra
        .get("decision_context")
        .and_then(|ctx| ctx["plan_id"].as_str());
    let plans = store.plans()?;
    let owners: Vec<_> = plans
        .iter()
        .filter(|p| {
            p.reference_branch == reference
                && bound.is_none_or(|id| id == p.id)
                && p.graph["tasks"]
                    .as_array()
                    .is_some_and(|ts| ts.iter().any(|t| t["id"] == task.id))
        })
        .collect();
    let [plan] = owners.as_slice() else {
        return Err(error(format!("Cannot identify a unique decision plan for task {}. Matching PRDs: {:?}. Run `macc coordinator resume`.",task.id,owners.iter().map(|p| &p.source).collect::<Vec<_>>())));
    };
    if let Some(wait) = find_pending_in_plan(&store, plan, &task.id, false)? {
        return Err(error(format!("{} in {}. {}. Run `macc coordinator resume` to reconcile progress and inspect the outstanding decision.",task.id,plan.source,wait.reason())));
    }
    if crate::prd_queue::current(root, cfg).path != plan.source {
        return Err(error(format!(
            "Task {} belongs to {}. Run `macc coordinator resume` to restore the queue position.",
            task.id, plan.source
        )));
    }
    Ok(())
}

fn find_pending_in_plan(
    store: &Store,
    plan: &Plan,
    task: &str,
    owned_intervention_only: bool,
) -> crate::Result<Option<PendingDecision>> {
    for d in store.applicable_decisions(plan)? {
        if owned_intervention_only
            && (d.request.task_id != task
                || !matches!(d.status.as_str(), "blocked_external" | "pending_human")
                || d.request.intervention.is_none())
        {
            continue;
        }
        if matches!(
            d.status.as_str(),
            "pending_human"
                | "blocked_external"
                | "conditional"
                | "expired"
                | "rejected"
                | "conflict"
                | "waiting_dependency"
                | "investigating"
        ) && store.decision_applies(plan, &d, task)?
        {
            return Ok(Some(PendingDecision {
                id: d.id,
                version: d.version,
                status: d.status,
                task_id: task.into(),
                question: d.request.question,
                owner_task_id: d.request.task_id,
                action: d.request.intervention.as_ref().map(|i| i.action.clone()),
                inaccessible_reason: d
                    .request
                    .intervention
                    .as_ref()
                    .map(|i| i.inaccessible_reason.clone()),
            }));
        }
    }
    Ok(None)
}
/// A propagated gate must not replace a consumer's independent failure.
pub(super) fn independent_consumer_failure(
    task: &crate::coordinator::model::Task,
    owner: &str,
) -> bool {
    if task.state != "blocked" || task.id == owner {
        return false;
    }
    match task.task_runtime.last_error_code.as_deref() {
        None | Some("E905") => false,
        Some("E903") => {
            task.task_runtime.last_error_origin.as_deref() == Some("precondition")
                || crate::coordinator::phase_diagnostics::compliance_block(task).is_some()
        }
        Some(_) => true,
    }
}
pub(super) fn clear_wait_binding(runtime: &mut crate::coordinator::model::TaskRuntime) {
    for key in [
        "decision_wait",
        "decision_wait_status",
        "decision_wait_owner",
        "decision_wait_action",
        "decision_wait_reason",
        "native_context_refresh",
    ] {
        runtime.extra.remove(key);
    }
}

impl PendingDecision {
    pub fn reason(&self) -> String {
        if self.status == "blocked_external" {
            return format!("DM_PENDING: blocked: external intervention required; decision {}: {}. Action: {}. Unavailable: {}. Retained work is preserved", self.id, self.question, self.action.as_deref().unwrap_or(&self.question), self.inaccessible_reason.as_deref().unwrap_or("Inspect the exact native request"));
        }
        format!(
            "DM_PENDING: Decision {} ({}) awaits operator resolution: {}",
            self.id, self.status, self.question
        )
    }
    pub(super) fn record_binding(&self, runtime: &mut crate::coordinator::model::TaskRuntime) {
        runtime
            .extra
            .insert("decision_wait".into(), serde_json::json!(self.id));
        runtime.extra.insert(
            "decision_wait_status".into(),
            serde_json::json!(self.status),
        );
        runtime.extra.insert(
            "decision_wait_owner".into(),
            serde_json::json!(self.owner_task_id),
        );
        if let Some(action) = &self.action {
            runtime
                .extra
                .insert("decision_wait_action".into(), serde_json::json!(action));
        } else {
            runtime.extra.remove("decision_wait_action");
        }
        runtime.extra.insert(
            "decision_wait_reason".into(),
            serde_json::json!(self.reason()),
        );
    }
    pub fn apply(&self, runtime: &mut crate::coordinator::model::TaskRuntime) {
        self.record_binding(runtime);
        runtime.set_status(crate::coordinator::RuntimeStatus::Idle);
        if self.task_id == self.owner_task_id
            && runtime.last_error_origin.as_deref() == Some("precondition")
            && runtime.last_error_code.as_deref() == Some("E903")
            && (!runtime.unmet_preconditions.is_empty()
                || !crate::coordinator::result_protocol::missing_precondition_explanation(
                    runtime.result_explanation.as_deref(),
                ))
        {
            // Preserve the performer's actual result. The durable request and action
            // are still available for diagnostics and normal decision reconciliation.
            return;
        }
        runtime.set_last_error_details("E903", "decisions", self.reason());
        runtime.last_error = Some(self.reason());
        runtime.completion_kind = Some("precondition_unmet".into());
        runtime.result_explanation = Some(self.reason());
        runtime.unmet_preconditions = vec![self.reason()];
    }
}

/// Catch requests created after the cycle's initial reconciliation.
pub fn refresh_waits(
    root: &Path,
    registry: &mut crate::coordinator::model::TaskRegistry,
) -> crate::Result<bool> {
    let mut changed = false;
    for task in &mut registry.tasks {
        if task.task_runtime.last_error_code.as_deref() == Some("E905") {
            // A dependency explanation is not this task's native invocation.
            // Repair legacy projected sentinels without touching real requests.
            if task
                .task_runtime
                .extra
                .get("decision_wait")
                .and_then(serde_json::Value::as_str)
                == Some("context_refresh")
            {
                clear_wait_binding(&mut task.task_runtime);
                changed = true;
            }
            continue;
        }
        if task.state == "blocked" && !task.task_runtime.extra.contains_key("decision_wait") {
            if let Some(pending) = pending_for_task(root, &task.id)? {
                if independent_consumer_failure(task, &pending.owner_task_id) {
                    continue;
                }
                pending.apply(&mut task.task_runtime);
                changed = true;
            } else if task
                .task_runtime
                .last_error_message
                .as_deref()
                .is_some_and(|e| e.contains("DM_STALE:"))
            {
                task.task_runtime
                    .extra
                    .insert("decision_wait".into(), serde_json::json!("context_refresh"));
                changed = true;
            }
        }
    }
    Ok(changed)
}

/// Only applicable native revisions are relevant to a cached human-gate diagnosis.
pub fn gate_revision(root: &Path, tasks: &[String]) -> crate::Result<Vec<serde_json::Value>> {
    if !policy(root)?.enabled {
        return Ok(vec![]);
    }
    let (store, plan) = active_plan(root)?;
    let mut result = Vec::new();
    for d in store.applicable_decisions(&plan)? {
        let mut applies = false;
        for task in tasks {
            applies |= store.decision_applies(&plan, &d, task)?;
        }
        if applies {
            result.push(serde_json::json!({"id":d.id,"version":d.version,"status":d.status}));
        }
    }
    result.sort_by_key(|v| v["id"].as_str().unwrap_or_default().to_owned());
    Ok(result)
}

/// Real human decisions keep an idle engine waiting. A scheduled native context
/// refresh also gets a cycle to run sync; sync clears that marker on refusal or
/// budget exhaustion. External recovery belongs to a stopped engine/supervisor.
pub fn human_wait_count(registry: &crate::coordinator::model::TaskRegistry) -> usize {
    let native_waits = registry.tasks.iter().filter(|task| {
        let runtime = &task.task_runtime;
        if runtime.last_error_code.as_deref() == Some("E905") {
            return false;
        }
        let human = runtime
            .extra
            .get("decision_wait_status")
            .and_then(serde_json::Value::as_str)
            == Some("pending_human");
        let refreshing = runtime
            .extra
            .get("native_context_refresh")
            .and_then(serde_json::Value::as_bool)
            == Some(true);
        match task.state.as_str() {
            "blocked" => human || refreshing,
            "todo" => human,
            _ => false,
        }
    });
    registry.waiting_approval_count() + native_waits.count()
}

#[cfg(test)]
mod handoff_tests {
    use super::*;
    use crate::coordinator::model::TaskRegistry;
    #[test]
    fn dependent_refresh_sentinels_do_not_cancel_the_roots_recovery_cycle() {
        use crate::coordinator::engine::{
            ControlPlaneLoopConfig, CoordinatorCounts, CoordinatorRunController,
        };
        let mut reg=TaskRegistry::from_value(&serde_json::json!({"tasks":[{"id":"ROOT","state":"blocked","task_runtime":{"last_error_code":"E903","last_error_origin":"decision_context_refresh","decision_wait":"context_refresh","native_context_refresh":true}}]})).unwrap();
        for i in 0..10 {
            reg.tasks.push(serde_json::from_value(serde_json::json!({"id":format!("CHILD-{i}"),"state":"blocked","dependencies":["ROOT"],"task_runtime":{"last_error_code":"E905","last_error_origin":"dependency","last_error_message":"Dependency ROOT: DM_STALE: binding changed","decision_wait":"context_refresh"}})).unwrap());
        }
        assert_eq!(
            human_wait_count(&reg),
            1,
            "legacy dependent markers must not terminate the run"
        );
        let mut controller = CoordinatorRunController::new(ControlPlaneLoopConfig {
            timeout: None,
            max_no_progress_cycles: 5,
        });
        let counts = CoordinatorCounts {
            total: 11,
            todo: 0,
            active: 0,
            blocked: 11,
            merged: 0,
            waiting_approval: human_wait_count(&reg),
        };
        assert!(controller.on_cycle_counts(counts, None).is_ok());
        let root = tempfile::tempdir().unwrap();
        assert!(refresh_waits(root.path(), &mut reg).unwrap());
        assert!(
            !refresh_waits(root.path(), &mut reg).unwrap(),
            "cleanup is idempotent and never recopies the root error into children"
        );
        assert!(reg.tasks.iter().skip(1).all(|t| !t
            .task_runtime
            .extra
            .contains_key("decision_wait")
            && t.state == "blocked"));
        reg.tasks[0]
            .task_runtime
            .extra
            .remove("native_context_refresh");
        assert_eq!(
            human_wait_count(&reg),
            0,
            "a rejected root refresh must still stop"
        );
        reg.tasks[0]
            .task_runtime
            .extra
            .insert("native_context_refresh".into(), serde_json::json!(true));
        reg.tasks.push(serde_json::from_value(serde_json::json!({"id":"EXTERNAL","state":"blocked","task_runtime":{"last_error_code":"E903","decision_wait":"real-device-request","decision_wait_status":"blocked_external"}})).unwrap());
        assert_eq!(
            human_wait_count(&reg),
            1,
            "an external prerequisite must not cancel an independent refresh"
        );
        reg.tasks.last_mut().unwrap().task_runtime.extra.insert(
            "decision_wait_status".into(),
            serde_json::json!("pending_human"),
        );
        assert_eq!(human_wait_count(&reg), 2);
    }
    #[test]
    fn external_prerequisite_cannot_masquerade_as_a_human_approval_wait() {
        use crate::coordinator::{
            engine::{ControlPlaneLoopConfig, CoordinatorCounts, CoordinatorRunController},
            model::TaskRegistry,
        };
        let registry = |status: &str| {
            TaskRegistry::from_value(&serde_json::json!({"tasks":[{"id":"A","state":"blocked","task_runtime":{"decision_wait":"request","decision_wait_status":status}}]})).unwrap()
        };
        assert_eq!(human_wait_count(&registry("pending_human")), 1);
        let mut refreshing = registry("context_refresh");
        refreshing.tasks[0]
            .task_runtime
            .extra
            .insert("native_context_refresh".into(), serde_json::json!(true));
        assert_eq!(
            human_wait_count(&refreshing),
            1,
            "allow native sync one cycle before terminal stall evaluation"
        );
        refreshing.tasks[0]
            .task_runtime
            .extra
            .remove("native_context_refresh");
        assert_eq!(
            human_wait_count(&refreshing),
            0,
            "failed refresh must hand off instead of idling forever"
        );
        let blocked = registry("blocked_external");
        assert_eq!(human_wait_count(&blocked), 0);
        let mut controller = CoordinatorRunController::new(ControlPlaneLoopConfig {
            timeout: None,
            max_no_progress_cycles: 5,
        });
        let result = controller.on_cycle_counts(
            CoordinatorCounts {
                total: 1,
                todo: 0,
                active: 0,
                blocked: 1,
                merged: 0,
                waiting_approval: human_wait_count(&blocked),
            },
            None,
        );
        assert!(result.unwrap_err().to_string().contains("blocked"));
    }
}
