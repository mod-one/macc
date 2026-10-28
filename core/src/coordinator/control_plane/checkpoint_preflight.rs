//! Reject checkpoint drift before launching a performer process.
use crate::coordinator::{model::TaskRegistry, RuntimeStatus, WorkflowState};
use crate::{session_store::checkpoints::recovery, Result};
use std::{collections::BTreeMap, path::Path};

pub(super) fn ready(
    root: &Path,
    registry_value: &mut serde_json::Value,
    task_id: &str,
    prd: &Path,
) -> Result<bool> {
    let mut registry = TaskRegistry::from_value(registry_value)?;
    if !root.join(".macc/memory.db").exists() {
        return Ok(true);
    }
    if let Some(task) = registry.find_task_mut(task_id) {
        task.ensure_runtime()
            .extra
            .insert("checkpoint_prd_file".into(), serde_json::json!(prd));
    }
    crate::coordinator::state::coordinator_state_registry_save(
        root,
        &BTreeMap::new(),
        &registry.to_value()?,
    )?;
    *registry_value = registry.to_value()?;
    match recovery::ensure_task_ready(root, task_id) {
        Ok(()) => Ok(true),
        Err(error) => {
            let message = format!("{}: {}", recovery::CODE, error);
            if let Some(task) = registry.find_task_mut(task_id) {
                task.set_workflow_state(WorkflowState::Blocked);
                let runtime = task.ensure_runtime();
                runtime.set_status(RuntimeStatus::Failed);
                runtime.pid = None;
                runtime.set_last_error_details("E903", "checkpoint_preflight", message.clone());
                runtime.last_error = Some(message.clone());
                runtime.unmet_preconditions = vec![message.clone()];
                runtime.result_explanation = Some("Checkpoint recovery is required before a provider invocation; existing work was preserved".into());
            }
            *registry_value = registry.to_value()?;
            crate::coordinator::helpers::recompute_resource_locks_from_tasks(registry_value);
            crate::coordinator::state::coordinator_state_registry_save(
                root,
                &BTreeMap::new(),
                registry_value,
            )?;
            crate::coordinator::helpers::append_coordinator_event_with_severity(
                root,
                "checkpoint_context_drift",
                task_id,
                "dispatch",
                "blocked",
                &message,
                "warning",
            )?;
            tracing::warn!(
                code = recovery::CODE,
                task_id,
                "Dispatch blocked by checkpoint preflight; no performer started"
            );
            Ok(false)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn source_drift_blocks_before_a_performer_is_started() {
        let (tmp, _, _) = crate::session_store::checkpoints::recovery::tests::setup();
        let root = tmp.path();
        let prd = root.join("prd.json");
        std::fs::write(
            &prd,
            std::fs::read_to_string(&prd)
                .unwrap()
                .replace("Validate", "New scope"),
        )
        .unwrap();
        let mut registry =
            crate::coordinator::state::coordinator_state_registry_load(root, &BTreeMap::new())
                .unwrap();
        assert!(!ready(root, &mut registry, "A", &prd).unwrap());
        let task = &registry["tasks"][0];
        assert_eq!(task["state"], "blocked");
        assert_eq!(task["task_runtime"]["last_error_code"], "E903");
        assert!(task["task_runtime"]["pid"].is_null());
        assert!(task["task_runtime"]["last_error_message"]
            .as_str()
            .unwrap()
            .contains(recovery::CODE));
    }
}
