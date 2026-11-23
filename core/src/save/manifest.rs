use super::repository_identity::RepositoryIdentity;
use serde::{Deserialize, Serialize};

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
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub session_store: Option<crate::session_store::SessionStoreSnapshot>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub decision_store: Option<crate::coordinator::decisions::ControlSnapshot>,
    pub kind: String,
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    pub created_at: String,
    pub updated_at: String,
    pub macc_version: String,
    pub repository: RepositoryIdentity,
    pub includes: SaveIncludes,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub prd_files: Vec<SavedPrd>,
    /// Paths relative to the log root, in budget allocation order.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub logs_omitted_by_size: Vec<String>,
    pub excludes: SaveExcludes,
    pub paths: SavePaths,
    pub hashes: SaveHashes,
    pub security: SaveSecurity,
}

impl SaveBundleManifest {
    pub(super) fn supported_format(&self) -> bool {
        let has_registry = self.includes.automation_state
            && self.paths.task_registry.as_deref() == Some(super::registry::PAYLOAD)
            && self.hashes.task_registry.is_some();
        let valid_version = match self.version {
            1 | 2 => {
                self.decision_store.is_none() && (self.version == 2) == self.session_store.is_some()
            }
            3 => self.decision_store.is_some(),
            4 => has_registry,
            5 => {
                self.includes.prd
                    && !self.prd_files.is_empty()
                    && (!self.includes.automation_state || has_registry)
            }
            _ => false,
        };
        valid_version
            && (self.version == 5 || self.prd_files.is_empty())
            && self.kind == "macc.save_bundle"
            && (self.session_store.is_none() || self.includes.coordinator_sessions)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct SavedPrd {
    pub path: String,
    pub sha256: String,
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
