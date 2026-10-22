use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct HooksConfig {
    pub overrides: BTreeMap<String, bool>,
    pub external: Vec<ExternalHook>,
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ExternalHook {
    pub definition: Hook,
    pub sha256: String,
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Hook {
    pub id: String,
    pub name: String,
    pub description: String,
    pub version: String,
    pub event: Event,
    #[serde(default)]
    pub default_enabled: bool,
    pub handler: Handler,
    /// Explicit skill output contracts. Empty means invocation by bundle ID only.
    #[serde(default)]
    pub skills: Vec<String>,
    pub source: Source,
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Source {
    pub repository: String,
    pub subpath: String,
    pub distribution: String,
}
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Event {
    ToolOutput,
    CoordinatorEvidence,
    PerformerEvidence,
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum Handler {
    Builtin { name: String },
    Rules { steps: Vec<Step> },
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum Step {
    RetainLines {
        pattern: String,
    },
    Replace {
        pattern: String,
        replacement: String,
    },
}
pub fn checksum(hook: &Hook) -> crate::Result<String> {
    use sha2::{Digest, Sha256};
    let bytes = serde_json::to_vec(hook).map_err(|e| super::error(e.to_string()))?;
    Ok(format!("{:x}", Sha256::digest(bytes)))
}
