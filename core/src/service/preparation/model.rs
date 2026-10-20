//! Additive preparation DTOs shared by CLI, TUI and Web.
use super::*;
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Plan {
    #[serde(default)]
    pub mcp_cleanup: Vec<crate::tool::scoped_mcp::RegistrationSpec>,
    #[serde(default)]
    pub inputs: Inputs,
    pub revision: String,
    pub tools: Vec<tools::Requirement>,
    pub providers: Vec<crate::o2::providers::preparation::Requirement>,
    pub steps: Vec<String>,
    pub network: bool,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Report {
    #[serde(default)]
    pub accepted_degraded: bool,
    #[serde(default)]
    pub run_id: String,
    #[serde(default)]
    pub output_kind: Option<String>,
    #[serde(default)]
    pub warnings: Vec<String>,
    #[serde(default)]
    pub current: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub live_output: Option<String>,
    pub revision: String,
    pub status: String,
    pub completed: Vec<String>,
    pub error: Option<String>,
    #[serde(default)]
    pub question: Option<Question>,
}
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Inputs {
    #[serde(default)]
    pub accept_degraded: bool,
    pub semantic_suite: Option<serde_json::Value>,
    pub semantic_approval: Option<Approval>,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Approval {
    pub report_id: String,
    pub checksum: String,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Question {
    pub kind: String,
    pub message: String,
    pub data: serde_json::Value,
}
