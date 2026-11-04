//! Validate the identifier shared by context files, receipts and runner environment.
use super::error;

pub(crate) fn validate(task: &str, attempt: &str, phase: &str) -> crate::Result<()> {
    if attempt.is_empty()
        || attempt.len() > 128
        || !attempt
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_'))
    {
        return Err(error(format!("Invalid context attempt for task {task:?}, phase {phase:?}: expected 1..128 ASCII letters, digits, '-' or '_' (received {} bytes)",attempt.len())));
    }
    if !matches!(phase, "dev" | "test" | "review" | "fix" | "merge_fix") {
        return Err(error(format!("Invalid context phase {phase:?} for task {task:?}: expected dev, test, review, fix or merge_fix")));
    }
    Ok(())
}
