use serde::{Deserialize, Serialize};
use super::repository_identity::RepositoryIdentity;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum MatchStrength {
    None,
    Weak,
    Medium,
    Strong,
}

impl MatchStrength {
    pub fn as_str(&self) -> &'static str {
        match self {
            MatchStrength::None => "none",
            MatchStrength::Weak => "weak",
            MatchStrength::Medium => "medium",
            MatchStrength::Strong => "strong",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct SaveBundleManifest {
    pub version: u32,
    pub kind: String,
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    pub created_at: String,
    pub updated_at: String,
    pub macc_version: String,
    pub repository: RepositoryIdentity,
    pub includes: SaveIncludes,
    pub excludes: SaveExcludes,
    pub paths: SavePaths,
    pub hashes: SaveHashes,
    pub security: SaveSecurity,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct SaveIncludes {
    pub config: bool,
    pub coordinator_sessions: bool,
    pub catalogs: bool,
    pub logs: bool,
    pub prd: bool,
    pub automation_state: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct SaveExcludes {
    pub worktrees: bool,
    pub cache: bool,
    pub generated_files: bool,
    pub secrets: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct SavePaths {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub config: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub coordinator_sessions: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub logs_archive: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub task_registry: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct SaveHashes {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub config: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub coordinator_sessions: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub task_registry: Option<String>,
    pub manifest_payload: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct SaveSecurity {
    pub secret_scan: SecretScanMetadata,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct SecretScanMetadata {
    pub performed: bool,
    pub findings: usize,
    pub redacted_logs: bool,
}
