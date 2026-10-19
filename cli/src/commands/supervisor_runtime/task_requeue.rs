//! Resume repaired roots through the existing coordinator state contract.
use macc_core::config::CanonicalConfig;
use macc_core::coordinator::types::CoordinatorEnvConfig;
use macc_core::supervisor::incident;
use macc_core::{MaccError, ProjectPaths, Result};
pub(super) fn requeue(
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
            if task.has_worktree_attached() {
                let reference =
                    macc_core::config::CoordinatorConfigResolved::resolve(config).reference_branch;
                macc_core::coordinator::repair_handoff::reconcile(&paths.root, task, &reference)?;
            }
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
            let reference =
                macc_core::config::CoordinatorConfigResolved::resolve(config).reference_branch;
            let base = if task.has_worktree_attached() {
                Some(macc_core::coordinator::repair_handoff::reconcile(
                    &paths.root,
                    task,
                    &reference,
                )?)
            } else {
                None
            };
            task.set_workflow_state(macc_core::coordinator::WorkflowState::Todo);
            if let Some(base) = base {
                task.ensure_runtime().extra.insert(
                    macc_core::coordinator::repair_handoff::RESUME_KEY.into(),
                    serde_json::json!({"base":base,"evidence":proof}),
                );
            }
            task.ensure_runtime().pid = None;
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
