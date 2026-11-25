//! Durable outcome of the supervisor's worktree cleanup decision.
use serde::{Deserialize, Serialize};
use std::{fmt, path::Path};

#[derive(Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum Cleanup {
    Removed,
    Retained { reason: Retention },
    Failed { error: String },
}

#[derive(Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Retention {
    RetryScheduled,
    PendingRestore,
    CustomizedConfig { path: String },
    UntrackedFile { path: String },
    ChangedFile { path: String },
    IgnoredFile { path: String },
    UnmergedCommit { commit: String },
    MissingTarget { branch: String },
}

impl fmt::Display for Retention {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::RetryScheduled => write!(f, "another attempt is scheduled"),
            Self::PendingRestore => write!(f, "repair restoration pending"),
            Self::CustomizedConfig { path } => write!(f, "customized configuration {path:?}"),
            Self::UntrackedFile { path } => write!(f, "untracked file {path:?}"),
            Self::ChangedFile { path } => write!(f, "uncommitted change to {path:?}"),
            Self::IgnoredFile { path } => {
                write!(f, "ignored file retained for safekeeping {path:?}")
            }
            Self::UnmergedCommit { commit } => write!(f, "unmerged commit {commit}"),
            Self::MissingTarget { branch } => {
                write!(f, "integration target {branch:?} unavailable")
            }
        }
    }
}

impl Cleanup {
    pub fn summary(&self, path: &Path) -> String {
        let slot = path
            .file_name()
            .unwrap_or(path.as_os_str())
            .to_string_lossy();
        match self {
            Self::Removed => format!("{slot} removed after archiving evidence"),
            Self::Retained { reason } => format!("{slot} retained · {reason}"),
            Self::Failed { error } => format!("{slot} cleanup failed · {error}"),
        }
    }

    pub fn from_intervention(value: &serde_json::Value) -> Option<Self> {
        serde_json::from_value(value.get("worktree_cleanup")?.clone()).ok()
    }
}
