use crate::prd_generation::metadata::PRD_GENERATION_DEFAULT_TARGET_DIR;
use std::path::{Path, PathBuf};

/// Resolve the target directory for a generation run.
/// Validates the override stays within the project root (directory traversal guard).
pub fn resolve_target_dir(
    project_root: &Path,
    override_dir: Option<&Path>,
    run_id: &str,
) -> Result<PathBuf, String> {
    let base = match override_dir {
        Some(o) => {
            let candidate = if o.is_absolute() {
                o.to_path_buf()
            } else {
                project_root.join(o)
            };
            if !candidate.starts_with(project_root) {
                return Err(format!(
                    "Target directory '{}' is outside the project root.",
                    candidate.display()
                ));
            }
            candidate
        }
        None => project_root.join(PRD_GENERATION_DEFAULT_TARGET_DIR),
    };
    Ok(base.join(run_id))
}
