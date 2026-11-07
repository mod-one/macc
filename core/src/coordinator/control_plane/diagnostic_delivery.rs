//! Transfer only an authoritative native diagnostic receipt into task state.
use crate::{
    coordinator::{decisions, engine::phase_failure, model::TaskRegistry},
    Result,
};
use std::path::Path;
pub(super) fn attach(
    root: &Path,
    registry: &mut serde_json::Value,
    task_id: &str,
    reason: &str,
) -> Result<()> {
    if !reason.contains("DM_EXTERNAL_VERIFICATION:") {
        return Ok(());
    }
    let mut tasks = TaskRegistry::from_value(registry)?;
    let Some(task) = tasks.find_task_mut(task_id) else {
        return Ok(());
    };
    let delivery = (|| -> Result<Option<decisions::DiagnosticDelivery>> {
        let current = decisions::registry(root)?;
        let Some(current) = current.find_task(task_id) else {
            return Ok(None);
        };
        let Some(info) = current.task_runtime.extra.get("decision_context") else {
            return Ok(None);
        };
        if info["phase"] != "review" {
            return Ok(None);
        }
        let store = decisions::Store::open(root)?;
        let plan = info["plan_id"].as_str().unwrap_or_default();
        let attempt = info["attempt_id"].as_str().unwrap_or_default();
        if store.read_context(plan, attempt, "review")?.task_id != task_id {
            return Ok(None);
        }
        store.pending_diagnostic(
            plan,
            attempt,
            Path::new(task.worktree_path().unwrap_or_default()),
        )
    })();
    match delivery {
        Ok(Some(delivery)) => phase_failure::attach(task.ensure_runtime(), delivery),
        Ok(None) => {}
        Err(error) => phase_failure::record(
            task.ensure_runtime(),
            "review",
            &format!("E903: Diagnostic receipt requires revalidation: {error}"),
        ),
    }
    *registry = tasks.to_value()?;
    Ok(())
}
