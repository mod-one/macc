use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
#[serde(default, deny_unknown_fields)]
pub struct SessionPolicy {
    pub lease: LeasePolicy,
    pub lifecycle: LifecyclePolicy,
}
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(default, deny_unknown_fields)]
pub struct LeasePolicy {
    pub ttl_seconds: u64,
    pub heartbeat_seconds: u64,
}
impl Default for LeasePolicy {
    fn default() -> Self {
        Self {
            ttl_seconds: 1800,
            heartbeat_seconds: 30,
        }
    }
}
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(default, deny_unknown_fields)]
pub struct LifecyclePolicy {
    pub max_idle_hours: u64,
}
impl Default for LifecyclePolicy {
    fn default() -> Self {
        Self { max_idle_hours: 24 }
    }
}
impl SessionPolicy {
    pub fn validate(&self) -> crate::Result<()> {
        if !(10..=86400).contains(&self.lease.ttl_seconds)
            || self.lease.heartbeat_seconds == 0
            || self.lease.heartbeat_seconds > self.lease.ttl_seconds / 3
            || !(1..=720).contains(&self.lifecycle.max_idle_hours)
        {
            return Err(super::error("Lease TTL must be 10..86400 seconds, heartbeat 1..TTL/3, and idle age 1..720 hours"));
        }
        Ok(())
    }
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Authority {
    pub version: u32,
    pub mode: String,
    pub project_id: String,
    pub project_root: String,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AcquireRequest {
    pub identity: Value,
    pub request_id: String,
    pub owner_pid: i64,
    #[serde(default)]
    pub preferred_provider_id: Option<String>,
    #[serde(default)]
    pub generate_provider_id: bool,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LeaseKey {
    pub session_id: String,
    pub lease_token: String,
    pub generation: i64,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LeaseGrant {
    #[serde(flatten)]
    pub key: LeaseKey,
    pub provider_session_id: Option<String>,
    pub is_new: bool,
    pub heartbeat_seconds: u64,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionView {
    pub id: String,
    pub project_id: String,
    pub task_id: String,
    pub role: String,
    pub tool_id: String,
    pub provider_session_id: Option<String>,
    pub lineage_id: String,
    pub status: String,
    pub generation: i64,
    pub use_count: i64,
    pub owner_pid: Option<i64>,
    pub expires_at: Option<i64>,
    pub updated_at: i64,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StoreView {
    pub project_root: String,
    pub backend: String,
    pub project_id: Option<String>,
    pub revision: String,
    pub policy: SessionPolicy,
    pub policy_source: String,
    pub fields: Vec<FieldDescriptor>,
    pub sessions: Vec<SessionView>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FieldDescriptor {
    pub path: String,
    pub label: String,
    pub default: u64,
    pub min: u64,
    pub max: u64,
    pub unit: String,
    pub help: String,
    pub activation: String,
}
pub fn fields() -> Vec<FieldDescriptor> {
    [
        (
            "lease.ttl_seconds",
            "Lease TTL",
            1800,
            10,
            86400,
            "seconds",
            "Lease validity; a live owner is never displaced by expiry.",
        ),
        (
            "lease.heartbeat_seconds",
            "Heartbeat interval",
            30,
            1,
            28800,
            "seconds",
            "Must not exceed one third of the lease TTL.",
        ),
        (
            "lifecycle.max_idle_hours",
            "Maximum idle age",
            24,
            1,
            720,
            "hours",
            "Older released transcripts start a fresh session.",
        ),
    ]
    .into_iter()
    .map(
        |(path, label, default, min, max, unit, help)| FieldDescriptor {
            path: path.into(),
            label: label.into(),
            default,
            min,
            max,
            unit: unit.into(),
            help: help.into(),
            activation: "next session claim".into(),
        },
    )
    .collect()
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MigrationReport {
    pub direction: String,
    pub source_sha256: String,
    pub imported: usize,
    pub quarantined: usize,
    pub backup: Option<String>,
    pub applied: bool,
}
