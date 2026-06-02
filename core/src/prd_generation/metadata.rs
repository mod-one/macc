use chrono::Local;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

pub const PRD_GENERATION_DEFAULT_TARGET_DIR: &str = ".macc/generated/prd/macc-prd-planner";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GenerationRunMetadata {
    pub run_id: String,
    pub from_path: String,
    pub tool: Option<String>,
    pub dry_run: bool,
    pub promoted: bool,
    pub created_at: String,
    pub files: Vec<String>,
}

impl GenerationRunMetadata {
    pub fn new(from_path: &str, tool: Option<String>, dry_run: bool) -> Self {
        let now = Local::now();
        Self {
            run_id: now.format("%Y-%m-%d-%H%M%S").to_string(),
            from_path: from_path.to_string(),
            tool,
            dry_run,
            promoted: false,
            created_at: now.to_rfc3339(),
            files: vec![],
        }
    }

    pub fn run_dir(&self, base_dir: &std::path::Path) -> PathBuf {
        base_dir.join(&self.run_id)
    }
}
