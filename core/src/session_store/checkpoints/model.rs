use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ContextRequest {
    pub identity: Value,
    pub phase: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Checkpoint {
    pub schema_version: u32,
    pub id: String,
    pub project_id: String,
    pub task_id: String,
    pub role: String,
    pub tool_id: String,
    pub phase: String,
    pub worktree: String,
    pub branch: String,
    pub source_revision: String,
    pub worktree_dirty_digest: String,
    pub worktree_content_digest: String,
    pub source_hashes: std::collections::BTreeMap<String, String>,
    pub acceptance_criteria_ids: Vec<String>,
    pub mandatory_task: Value,
    pub completed_steps: Vec<String>,
    pub remaining_steps: Vec<String>,
    pub changed_files: Vec<String>,
    pub decisions: Vec<Value>,
    pub test_results: Vec<Value>,
    pub blocking_issues: Vec<String>,
    pub review_objections: Value,
    pub memory_refs: Vec<String>,
    pub source_refs: Vec<String>,
    pub next_action: String,
    pub reason: String,
    pub validation: Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CheckpointView {
    pub id: String,
    pub task_id: String,
    pub role: String,
    pub tool_id: String,
    pub phase: String,
    pub reason: String,
    pub validation_state: String,
    pub created_at: i64,
    pub content_sha256: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PreparedContext {
    pub enabled: bool,
    pub checkpoint_id: Option<String>,
    pub prompt: String,
    pub acknowledgement: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct FinishRequest {
    #[serde(flatten)]
    pub key: super::super::LeaseKey,
    pub phase: String,
    pub result: String,
    pub acknowledged: bool,
}
