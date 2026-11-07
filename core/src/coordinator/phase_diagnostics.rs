//! Compatibility projection for untyped phase failures from older runtimes.
use super::model::Task;
pub fn decision_precondition(reason: &str) -> bool {
    reason.contains("no passing independent compliance evidence")
        || [
            "DM_STALE:",
            "DM_COMPLIANCE:",
            "DM_EXTERNAL_VERIFICATION:",
            "DM_PENDING:",
        ]
        .iter()
        .any(|code| reason.contains(code))
}
pub fn effective_code(task: &Task) -> Option<&str> {
    task.task_runtime
        .last_error_code
        .as_deref()
        .filter(|v| !v.is_empty())
        .or_else(|| {
            let reason = task.task_runtime.last_error.as_deref()?;
            if decision_precondition(reason) {
                Some("E903")
            } else {
                None
            }
        })
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn legacy_compliance_errors_are_projected_without_rewriting_registry_state() {
        let mut task = Task::default();
        task.task_runtime.last_error=Some("Validation error: Decision control: Decision old-id has no passing independent compliance evidence".into());
        assert_eq!(effective_code(&task), Some("E903"));
        assert!(task.task_runtime.last_error_code.is_none());
        task.task_runtime.last_error = Some("build failed".into());
        assert!(effective_code(&task).is_none());
    }
}
