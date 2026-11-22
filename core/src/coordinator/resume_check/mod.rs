//! Version-bound, supervisor-authored observations for retained task phases.
mod adb;
mod binding;
mod observations;
mod packets;
mod replay;
mod verifier;
pub use packets::DiagnosisSnapshot;
pub use replay::{replay_at_startup, waiting_check};

use super::decisions::DecisionRef;
use crate::MaccError;
use serde::{Deserialize, Serialize};

fn error(message: impl std::fmt::Display) -> MaccError {
    MaccError::Validation(format!("Resume check: {message}"))
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Category {
    TechnicalCondition,
    ExternalAction,
    HumanApproval,
    ReviewCorrection,
}
impl Category {
    pub fn label(self) -> &'static str {
        match self {
            Self::TechnicalCondition => "Technical condition",
            Self::ExternalAction => "External action",
            Self::HumanApproval => "Human approval",
            Self::ReviewCorrection => "Review correction",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum Source {
    AdbDevices,
    JsonFile {
        path: String,
    },
    TextFile {
        path: String,
    },
    PathInfo {
        path: String,
    },
    Sha256File {
        path: String,
    },
    Inputs {
        fields: std::collections::BTreeMap<String, Source>,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct Verifier {
    pub source: Source,
    pub script: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct Proposal {
    pub task_id: String,
    pub phase: String,
    pub category: Category,
    pub target: String,
    pub criterion: String,
    pub decision: Option<DecisionRef>,
    pub verifier: Option<Verifier>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Status {
    Ready,
    Waiting,
    Error,
}
impl Status {
    pub fn label(self) -> &'static str {
        match self {
            Self::Ready => "ready",
            Self::Waiting => "waiting",
            Self::Error => "error",
        }
    }
}
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct Outcome {
    pub status: Status,
    pub detail: String,
}

/// The registry holds a reference. Source and binding are in a content-addressed local packet.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct CheckRef {
    pub id: String,
    pub category: Category,
    pub phase: String,
    pub criterion: String,
    pub last: Option<Outcome>,
    pub checked_at: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub observation_sha256: Option<String>,
}

impl CheckRef {
    pub(crate) fn next_action(&self, task_id: &str) -> Option<String> {
        let action = match self.category {
            Category::TechnicalCondition => match self.last.as_ref().map(|r| r.status) {
                Some(Status::Ready) => return None,
                Some(Status::Error) => "Inspect the verifier error, restore its input or refresh the diagnosis, then restart the coordinator",
                _ => "Restore the condition and restart the coordinator",
            },
            Category::ExternalAction => "Supply the recorded access or external action",
            Category::HumanApproval => "Open the Inbox and submit the required review votes",
            Category::ReviewCorrection => "Address the review findings in the retained worktree through the correction workflow",
        };
        let condition = self
            .last
            .as_ref()
            .map_or(self.criterion.as_str(), |r| r.detail.as_str());
        Some(format!(
            "{task_id} · {condition}. {action} for the retained {} phase.",
            self.phase
        ))
    }
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Packet {
    proposal: Proposal,
    binding: serde_json::Value,
    script_sha256: Option<String>,
}

#[cfg(test)]
mod tests;
