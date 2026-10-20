//! Non-generative account/runtime checks supplied by each tool adapter.
use serde::{Deserialize, Serialize};
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PreparationSpec {
    pub verify: super::ToolInstallCommand,
    #[serde(default)]
    pub expected_json: std::collections::BTreeMap<String, serde_json::Value>,
    /// Adapter-owned bounded runtime probe, independent from installation/login.
    #[serde(default = "default_timeout")]
    pub timeout_seconds: u64,
    #[serde(default)]
    pub require_output: bool,
    #[serde(default)]
    pub authentication_errors: Vec<String>,
    #[serde(default)]
    pub network_errors: Vec<String>,
    #[serde(default)]
    pub authentication_json: std::collections::BTreeMap<String, serde_json::Value>,
    pub setup: Option<super::ToolInstallCommand>,
}

fn default_timeout() -> u64 {
    15
}
