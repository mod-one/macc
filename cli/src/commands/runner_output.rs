//! Read-only provider boundary inspection; no project initialization or AI call.
use macc_core::{MaccError, Result};
use std::path::Path;
pub fn run(
    file: &Path,
    tool: &str,
    exit_code: i32,
    root: Option<&Path>,
    task: Option<&str>,
    source: Option<&str>,
) -> Result<()> {
    let raw = std::fs::read_to_string(file).map_err(|source| MaccError::Io {
        path: file.display().to_string(),
        action: "read current runner output".into(),
        source,
    })?;
    let mut result = macc_core::coordinator::runner_output::inspect(tool, exit_code, &raw);
    let native_wait = match (root, task, source) {
        (Some(root), Some(task), Some(source)) => {
            macc_core::session_store::decision_wait::waiting_for_invocation(root, task, source)?
        }
        _ => None,
    };
    if let Some(wait) = &native_wait {
        // A lease-fenced receipt outranks an interrupted transcript. It never
        // supplies assistant ACK/compliance or creates a successful outcome.
        let reason = wait.reason.replace(['\r', '\n'], " ");
        result.terminal_output = format!("MACC_TASK_PRECONDITION: {reason}\nMACC_TASK_RESULT_EXP: {reason}\nMACC_TASK_RESULT: precondition_unmet\n");
        result.provider_failure = false;
        result.provider_output.clear();
        result.error = None;
    }
    let mut result =
        serde_json::to_value(result).map_err(|e| MaccError::Validation(e.to_string()))?;
    result["native_wait"] =
        serde_json::to_value(native_wait).map_err(|e| MaccError::Validation(e.to_string()))?;
    println!(
        "{}",
        serde_json::to_string(&result).map_err(|e| MaccError::Validation(e.to_string()))?
    );
    Ok(())
}
