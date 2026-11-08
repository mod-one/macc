//! One native interpretation of current invocation output for all adapters.
use super::{
    error_normalizer::{NormalizerRegistry, ToolError},
    result_protocol,
};
use serde::Serialize;
#[derive(Debug, Serialize)]
pub struct Inspection {
    pub assistant_output: String,
    pub terminal_output: String,
    pub provider_output: String,
    pub provider_failure: bool,
    pub error: Option<ToolError>,
}
pub fn inspect(tool: &str, exit_code: i32, raw: &str) -> Inspection {
    let provider_output = result_protocol::provider_output(raw);
    let error = if provider_output.is_empty() {
        None
    } else {
        NormalizerRegistry::from_inventory()
            .get(tool)
            .and_then(|n| n.normalize(exit_code, &provider_output, ""))
    };
    Inspection {
        assistant_output: result_protocol::projection(raw),
        terminal_output: result_protocol::completion_projection(raw),
        provider_failure: !provider_output.is_empty(),
        provider_output,
        error,
    }
}
