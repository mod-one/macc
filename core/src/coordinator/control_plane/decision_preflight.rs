//! Check exact decision prompt cost before acquiring or preparing a worker.
use crate::coordinator::{decisions, model::TaskRegistry, RuntimeStatus, WorkflowState};
use crate::Result;
use std::{collections::BTreeMap, path::Path};

pub(super) fn ready(root: &Path, registry: &mut serde_json::Value, task_id: &str) -> Result<bool> {
    let Some(budget) = decisions::context_budget(root, task_id, "dev")? else {
        return Ok(true);
    };
    tracing::info!(
        task_id,
        required_bytes = budget.required_bytes,
        budget_bytes = budget.budget_bytes,
        prerequisites = budget.prerequisite_count,
        bindings = budget.binding_count,
        "Decision context preflight"
    );
    if budget.fits {
        return Ok(true);
    }
    let detail = budget.check().unwrap_err().to_string();
    let mut typed = TaskRegistry::from_value(registry)?;
    let task = typed
        .find_task_mut(task_id)
        .ok_or_else(|| decisions::error("Preflight task disappeared"))?;
    if task.task_runtime.pid.is_some() {
        return Err(decisions::error("Preflight cannot replace an active task"));
    }
    task.set_workflow_state(WorkflowState::Blocked);
    task.touch_state_changed(&crate::coordinator::helpers::now_iso_coordinator());
    let runtime = task.ensure_runtime();
    runtime.set_status(RuntimeStatus::Failed);
    runtime.current_phase = Some("dev".into());
    runtime.last_error = Some(detail.clone());
    runtime.set_last_error_details("E903", "decision_context_budget", detail.clone());
    runtime.completion_kind = Some("precondition_unmet".into());
    runtime.unmet_preconditions = vec![detail.clone()];
    runtime.result_explanation = Some(
        "Decision prompt exceeds its configured byte budget; no worker or provider was started"
            .into(),
    );
    runtime.extra.insert(
        "decision_context_budget".into(),
        serde_json::to_value(&budget).map_err(|e| decisions::error(e.to_string()))?,
    );
    *registry = typed.to_value()?;
    crate::coordinator::helpers::recompute_resource_locks_from_tasks(registry);
    crate::coordinator::state::coordinator_state_registry_save(root, &BTreeMap::new(), registry)?;
    crate::coordinator::helpers::append_coordinator_event_with_severity(
        root,
        "decision_context_budget_exceeded",
        task_id,
        "dev",
        "blocked",
        &detail,
        "warning",
    )?;
    Ok(false)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn budget_failure_is_recorded_before_creating_any_claim_or_worker() {
        let (tmp, _store, _plan, mut p) = decisions::tests_context_budget::large_graph();
        p.max_context_bytes = 2048;
        decisions::configure(tmp.path(), p).unwrap();
        let mut registry = crate::coordinator::state::coordinator_state_registry_load(
            tmp.path(),
            &BTreeMap::new(),
        )
        .unwrap();
        assert!(!ready(tmp.path(), &mut registry, "B").unwrap());
        let tasks = registry["tasks"].as_array().unwrap();
        let task = tasks.iter().find(|t| t["id"] == "B").unwrap();
        assert_eq!(task["state"], "blocked");
        assert_eq!(task["task_runtime"]["last_error_code"], "E903");
        assert_eq!(task["task_runtime"]["current_phase"], "dev");
        assert_eq!(
            task["task_runtime"]["last_error_origin"],
            "decision_context_budget"
        );
        assert_eq!(
            task["task_runtime"]["decision_context_budget"]["prerequisite_count"],
            4
        );
        assert!(task["task_runtime"]["claim_id"].is_null());
        assert!(!tmp.path().join(".macc/worktree").exists());
    }
}
