//! Structured phase diagnostics and eligibility for retained review recovery.
use super::model::Task;
pub fn decision_precondition(reason: &str) -> bool {
    [
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
}
/// Internal refresh sentinels never identify an answerable native request.
pub fn decision_wait_id(task: &Task) -> Option<&str> {
    task.task_runtime
        .extra
        .get("decision_wait")
        .and_then(serde_json::Value::as_str)
        .filter(|id| !id.is_empty() && *id != "context_refresh")
}
pub fn context_refresh_blocked(task: &Task) -> bool {
    decision_wait_id(task).is_none()
        && task.task_runtime.last_error_code.as_deref() == Some("E903")
        && matches!(
            task.task_runtime.last_error_origin.as_deref(),
            Some("decisions" | "decision_context_refresh")
        )
        && (task
            .task_runtime
            .extra
            .get("decision_wait")
            .and_then(serde_json::Value::as_str)
            == Some("context_refresh")
            || task
                .task_runtime
                .extra
                .get("native_context_refresh")
                .and_then(serde_json::Value::as_bool)
                == Some(true)
            || task
                .task_runtime
                .last_error_message
                .as_deref()
                .is_some_and(|s| s.contains("DM_STALE:")))
}
pub fn retained_review_protocol(task: &Task) -> bool {
    task.task_runtime.last_error_code.as_deref() == Some("E103")
        && task.task_runtime.last_error_origin.as_deref() == Some("output_protocol")
        && task.task_runtime.current_phase.as_deref() == Some("review")
        && task.task_runtime.pid.is_none()
        && task.has_worktree_attached()
}
/// Historical runners incorrectly converted this native fault into E906.
pub fn missing_result_protocol(task: &Task) -> bool {
    let runtime = &task.task_runtime;
    (runtime.last_error_code.as_deref() == Some("E103")
        && matches!(
            runtime.last_error_origin.as_deref(),
            Some("output_protocol" | "runner")
        )
        || runtime.last_error_code.as_deref() == Some("E906"))
        && [
            runtime.last_error.as_deref(),
            runtime.last_error_message.as_deref(),
        ]
        .into_iter()
        .flatten()
        .any(|s| s.starts_with("Missing explicit MACC_TASK_RESULT;"))
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn free_text_cannot_replace_a_native_phase_error_code() {
        let mut task = Task::default();
        task.task_runtime.last_error=Some("Validation error: Decision control: Decision old-id has no passing independent compliance evidence".into());
        assert!(effective_code(&task).is_none());
        assert!(task.task_runtime.last_error_code.is_none());
        task.task_runtime.last_error = Some("build failed".into());
        assert!(effective_code(&task).is_none());
        task.task_runtime.last_error_code = Some("E903".into());
        assert_eq!(effective_code(&task), Some("E903"));
    }
}
