//! Native provider bridges. Adapter capabilities choose paths and response protocols.
mod deployment;
mod dispatch;
mod inspection;
pub use deployment::{plan, registration_command};
pub use dispatch::dispatch;
pub use inspection::{inspect, verify, Deployment};
use serde::{Deserialize, Serialize};
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Spec {
    pub tool: String,
    pub config: String,
    pub launcher: String,
    pub protocol: Protocol,
}
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub enum Protocol {
    Replacement,
    Feedback,
}
pub fn specs() -> Vec<Spec> {
    let registry = crate::ToolRegistry::default_registry();
    registry
        .list_ids()
        .iter()
        .filter_map(|id| registry.get(id)?.native_output_hook())
        .collect()
}
pub(super) fn enabled(config: &crate::config::CanonicalConfig) -> crate::Result<bool> {
    Ok(super::status(config)?
        .hooks
        .iter()
        .any(|h| h.enabled && h.hook.event == super::Event::ToolOutput))
}
#[cfg(test)]
mod tests;
