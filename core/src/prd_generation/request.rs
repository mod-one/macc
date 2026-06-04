use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum ModelRoutingMode {
    #[default]
    Auto,
    Manual,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelSelection {
    pub mode: ModelRoutingMode,
    /// Only respected when `mode = Manual`.
    pub model: Option<String>,
}

impl Default for ModelSelection {
    fn default() -> Self {
        Self {
            mode: ModelRoutingMode::Auto,
            model: None,
        }
    }
}

/// Unified request object used by CLI, TUI, Web, and Web UI.
/// No `performer` or `skill` fields — those are fixed internally.
#[derive(Debug, Clone)]
pub struct PrdGenerateRequest {
    /// Required: path to the brief file.
    pub from_path: PathBuf,
    pub tool: Option<String>,
    pub model_selection: Option<ModelSelection>,
    pub instructions: Option<String>,
    pub instructions_file: Option<PathBuf>,
    /// Override the output directory. Defaults to DEFAULT_TARGET_DIR/<run-id>.
    pub target_dir: Option<PathBuf>,
    /// Existing PRD to update rather than creating from scratch.
    pub update_path: Option<PathBuf>,
    pub dry_run: bool,
    pub promote: bool,
    pub yes: bool,
}
