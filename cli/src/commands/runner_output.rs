//! Read-only provider boundary inspection; no project initialization or AI call.
use macc_core::{MaccError, Result};
use std::path::Path;
#[derive(Debug, Clone, Copy, clap::ValueEnum)]
pub enum View {
    Inspection,
    /// Read a JSONL provider event stream and print only its error records
    ProviderErrors,
    /// Read an isolated final assistant response and print eligible protocol fields
    FinalProtocol,
}

pub fn run(
    view: View,
    file: &Path,
    tool: &str,
    exit_code: i32,
    root: Option<&Path>,
    task: Option<&str>,
    source: Option<&str>,
) -> Result<()> {
    let read_error = |source| MaccError::Io {
        path: file.display().to_string(),
        action: "read current runner output".into(),
        source,
    };
    if matches!(view, View::ProviderErrors) {
        use std::io::{BufRead, BufReader};
        let reader = BufReader::new(std::fs::File::open(file).map_err(read_error)?);
        for line in reader.lines() {
            let line = line.map_err(read_error)?;
            if macc_core::coordinator::result_protocol::is_provider_error_record(&line) {
                println!("{line}");
            }
        }
        return Ok(());
    }
    let raw = std::fs::read_to_string(file).map_err(read_error)?;
    if matches!(view, View::FinalProtocol) {
        print!(
            "{}",
            macc_core::coordinator::result_protocol::isolated_protocol(&raw)
        );
        return Ok(());
    }
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
