use super::super::LeaseKey;
use serde::{Deserialize, Serialize};
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct Counters {
    pub input_uncached: Option<u64>,
    pub cached_read: Option<u64>,
    pub cached_write: Option<u64>,
    pub output: Option<u64>,
    pub reasoning: Option<u64>,
    pub cost: Option<u64>,
}
impl Counters {
    pub(super) fn values(&self) -> [Option<u64>; 6] {
        [
            self.input_uncached,
            self.cached_read,
            self.cached_write,
            self.output,
            self.reasoning,
            self.cost,
        ]
    }
    pub(super) fn from_values(v: [Option<u64>; 6]) -> Self {
        Self {
            input_uncached: v[0],
            cached_read: v[1],
            cached_write: v[2],
            output: v[3],
            reasoning: v[4],
            cost: v[5],
        }
    }
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UsageRecord {
    pub invocation_id: String,
    pub logical_session_id: String,
    pub generation: i64,
    pub task_id: String,
    pub role: String,
    pub tool_id: String,
    pub phase: String,
    pub model: Option<String>,
    pub provider_version: Option<String>,
    pub schema: Option<String>,
    pub measurement_source: String,
    pub token_quality: String,
    pub cost_quality: String,
    pub currency: Option<String>,
    pub reported: Counters,
    pub incremental: Counters,
    pub notes: Vec<String>,
    pub status: String,
    pub outcome: Option<String>,
    pub elapsed_ms: Option<u64>,
}
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct UsageView {
    pub ready: bool,
    pub records: Vec<UsageRecord>,
    pub totals: Vec<UsageTotal>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UsageTotal {
    pub task_id: String,
    pub tool_id: String,
    pub phase: String,
    pub currency: Option<String>,
    pub cost_quality: String,
    pub invocations: u64,
    pub measured_invocations: u64,
    pub coverage: Counters,
    pub counters: Counters,
}
#[derive(Debug, Deserialize)]
pub struct BeginRequest {
    #[serde(flatten)]
    pub key: LeaseKey,
    pub phase: String,
}
#[derive(Debug, Deserialize)]
pub struct EndRequest {
    #[serde(flatten)]
    pub key: LeaseKey,
    pub outcome: String,
    pub raw: String,
    #[serde(default)]
    pub omitted: bool,
}
#[derive(Debug, Serialize, Deserialize)]
pub struct CapturePlan {
    pub enabled: bool,
    pub structured: bool,
    pub max_output_bytes: u64,
    pub profile: Option<crate::tool::usage::UsageSpec>,
}
#[derive(Debug, Default)]
pub(super) struct Observation {
    pub counters: Counters,
    pub notes: Vec<String>,
    pub model: Option<String>,
    pub session: Option<String>,
}
