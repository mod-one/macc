use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SkillDefinition {
    pub id: String,
    pub title: String,
    pub kind: SkillKind,
    pub risk: SkillRisk,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub steps: Vec<SkillStep>,
    #[serde(default)]
    pub targets: HashMap<String, SkillTarget>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum SkillKind {
    #[default]
    LocalCommand,
    Prompt,
    Hybrid,
    Agent,
    Coordinator,
}

impl SkillKind {
    pub fn as_str(&self) -> &'static str {
        match self {
            SkillKind::LocalCommand => "local_command",
            SkillKind::Prompt => "prompt",
            SkillKind::Hybrid => "hybrid",
            SkillKind::Agent => "agent",
            SkillKind::Coordinator => "coordinator",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum SkillRisk {
    #[default]
    Safe,
    Caution,
    Dangerous,
}

impl SkillRisk {
    pub fn as_str(&self) -> &'static str {
        match self {
            SkillRisk::Safe => "safe",
            SkillRisk::Caution => "caution",
            SkillRisk::Dangerous => "dangerous",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct SkillStep {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub run: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub prompt: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct SkillTarget {
    pub strategy: String,
}

#[derive(Debug, Clone)]
pub struct SkillRunRequest {
    pub skill_id: String,
    pub tool_id: Option<String>,
    pub cwd: PathBuf,
    pub task_id: Option<String>,
    pub scope: Option<Vec<String>>,
    pub inputs: HashMap<String, String>,
    pub dry_run: bool,
    pub watch: bool,
    pub yes: bool,
}

/// Spec §5.6: metadata emitted when output was summarized before model context.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct SummaryMetadata {
    pub raw_size_chars: usize,
    pub summary_size_chars: usize,
    pub bundles_applied: Vec<String>,
    pub was_truncated: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SkillRunResult {
    pub skill_id: String,
    pub status: String,
    pub tool: Option<String>,
    pub started_at: String,
    pub duration_ms: u64,
    pub stdout: String,
    pub stderr: String,
    pub exit_code: Option<i32>,
    pub log_path: Option<PathBuf>,
    /// Populated when the summarization pipeline ran (spec §5.6).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub summary: Option<SummaryMetadata>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SkillDryRunPreview {
    pub skill_id: String,
    pub title: String,
    pub kind: String,
    pub tool: Option<String>,
    pub risk: String,
    pub commands: Vec<String>,
    pub writes: Vec<String>,
    pub context_estimate: Option<String>,
    pub logs_path: String,
}
