//! Project an exact native review objection without changing its authority.
use crate::{
    coordinator::{decisions, model::TaskRegistry},
    Result,
};
pub(super) fn attach(
    root: &std::path::Path,
    registry: &mut serde_json::Value,
    task_id: &str,
) -> Result<()> {
    let mut tasks = TaskRegistry::from_value(registry)?;
    let Some(task) = tasks.find_task_mut(task_id) else {
        return Ok(());
    };
    if let Some(block) = decisions::review_compliance_for_task(root, task)? {
        // Keep the whole review objection and exact versions, not only the
        // first non-passing binding that supplied the phase's error string.
        let message = block.describe();
        task.task_runtime.last_error = Some(message.clone());
        task.task_runtime.last_error_message = Some(message.clone());
        task.task_runtime.unmet_preconditions = vec![message];
        task.task_runtime.extra.insert(
            decisions::COMPLIANCE_BLOCK_KEY.into(),
            serde_json::json!(block),
        );
    }
    *registry = tasks.to_value()?;
    Ok(())
}
