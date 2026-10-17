use super::*;
/// Restrict tuning to existing bounded retrieval/compaction controls, never safety flags.
pub const FIELDS: &[&str] = &[
    "memory.ranking_task_weight",
    "memory.ranking_reference_weight",
    "memory.ranking_authority_weight",
    "memory.ranking_confidence_weight",
    "memory.ranking_evidence_weight",
    "memory.ranking_length_penalty",
    "memory.ranking_duplicate_penalty",
    "memory.semantic_weight",
    "context_engine.push_memory_limit_tokens",
    "context_engine.pull_limit_tokens",
    "adaptive_compaction.warning_ratio",
    "adaptive_compaction.action_ratio",
    "adaptive_compaction.guard_ratio",
    "adaptive_compaction.min_turns",
];
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Case {
    pub task_id: String,
    pub revision: String,
}
impl Case {
    /// Distinct identities prevent cross-arm checkpoint/session restoration.
    pub fn execution_task(&self, arm: &str) -> String {
        format!("{}--{}", self.task_id, arm)
    }
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Plan {
    pub field: String,
    pub candidate: f64,
    pub cases: Vec<Case>,
    /// Safe repository file describing checks, representative tasks and controlled conditions.
    pub protocol_path: String,
    pub protocol_sha256: String,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Trial {
    pub id: String,
    pub project: String,
    pub plan: Plan,
    pub baseline: policy::Policy,
    pub candidate: policy::Policy,
    pub ordinal: u64,
    pub alpha: f64,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Quality {
    pub accepted: bool,
    pub tests_passed: bool,
    pub spec_fidelity: bool,
    pub max_finding_severity: u8,
    pub retries: u64,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Observation {
    pub task_id: String,
    pub baseline: Quality,
    pub candidate: Quality,
    pub evidence_path: String,
    pub evidence_sha256: String,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub(super) struct Stamp {
    pub invocation: String,
    pub task: String,
    pub arm: String,
    pub policy: String,
    pub revision: String,
    pub clean: bool,
    pub execution: String,
    pub environment: String,
    pub generation: i64,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PairResult {
    pub task_id: String,
    pub baseline_cost: u64,
    pub candidate_cost: u64,
    pub baseline_ms: u64,
    pub candidate_ms: u64,
    pub currency: String,
    pub quality_passed: bool,
    pub telemetry_sha256: String,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Report {
    pub id: String,
    pub trial_id: String,
    pub pairs: Vec<PairResult>,
    pub passed: bool,
    pub wins: usize,
    pub losses: usize,
    pub ties: usize,
    pub p_value: f64,
    pub alpha: f64,
    pub cost_reduction: f64,
    pub latency_ratio: f64,
    pub reasons: Vec<String>,
    pub method: String,
}
