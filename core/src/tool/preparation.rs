//! Non-generative account/runtime checks supplied by each tool adapter.
use serde::{Deserialize, Serialize};
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PreparationSpec {
    pub verify: super::ToolInstallCommand,
    #[serde(default)]
    pub expected_json: std::collections::BTreeMap<String, serde_json::Value>,
    pub setup: Option<super::ToolInstallCommand>,
}
