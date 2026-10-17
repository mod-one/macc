use serde::{Deserialize, Serialize};
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Access {
    pub task_id: Option<String>,
    pub feature: Option<String>,
    pub worktree: Option<String>,
    pub tool_id: Option<String>,
    pub user_id: Option<String>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Evidence {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub binding: Option<Binding>,
    pub kind: String,
    pub reference: String,
    pub sha256: Option<String>,
    pub revision: Option<String>,
    pub summary: String,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Proposal {
    pub kind: String,
    pub title: String,
    pub body: String,
    pub scope: String,
    pub scope_ref: Option<String>,
    pub confidence: f64,
    pub evidence: Vec<Evidence>,
    #[serde(default)]
    pub tags: Vec<String>,
    pub source_task: Option<String>,
    pub source_revision: Option<String>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Memory {
    pub id: String,
    pub project_id: String,
    pub status: String,
    pub proposal: Proposal,
    pub created_at: i64,
    pub updated_at: i64,
    pub verified_revision: Option<String>,
    pub reviewed_by: Option<String>,
    pub staleness_reason: Option<String>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Search {
    #[serde(default)]
    pub query: String,
    #[serde(default)]
    pub access: Access,
    #[serde(default)]
    pub include_inactive: bool,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Review {
    pub id: String,
    pub status: String,
    pub reason: String,
    pub confirmed: bool,
    pub expected_checksum: String,
    #[serde(default)]
    pub evidence: Option<Vec<Evidence>>,
}
#[derive(Debug, Clone, Serialize)]
pub struct Hit {
    pub memory: Memory,
    pub checksum: String,
    pub rank_reason: String,
    pub score: i64,
}

/// Optional, checksummed provenance. Legacy evidence omits this field unchanged.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum Binding {
    Symbol { provider: String, symbol: String },
    Tool { tool_id: String, observed_at: i64 },
}
#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Resolution {
    KeepLeft,
    KeepRight,
    #[default]
    BothStale,
    Dismiss,
}
