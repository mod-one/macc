use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

pub struct PromoteOptions {
    pub source_path: PathBuf,
    pub dest_path: PathBuf,
    /// Skip confirmation when destination already exists.
    pub yes: bool,
    pub backup_dir: Option<PathBuf>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PromoteResult {
    pub promoted: bool,
    pub source: String,
    pub destination: String,
    pub backup_created: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub warnings: Vec<String>,
}

/// Promote a generated PRD file to its destination.
/// Returns `Err` if the destination exists and `yes` is false.
pub fn promote_prd(options: &PromoteOptions) -> crate::Result<PromoteResult> {
    use crate::MaccError;

    if !options.source_path.exists() {
        return Err(MaccError::Validation(format!(
            "PRD-GEN-PROMOTE-CONFLICT: source '{}' does not exist",
            options.source_path.display()
        )));
    }

    // Inspect one source snapshot and publish exactly those validated bytes.
    let raw = std::fs::read_to_string(&options.source_path).map_err(|e| MaccError::Io {
        path: options.source_path.to_string_lossy().into(),
        action: "read promotion source".into(),
        source: e,
    })?;
    let destination = crate::coordinator::decisions::promotion_destination(&options.dest_path)?;
    let _guard = crate::coordinator::decisions::guard_prd_promotion(&destination)?;
    let validation = super::validation::validate_prd_content(&raw, &destination);
    if !validation.ok {
        return Err(MaccError::Validation(format!(
            "PRD-GEN-PROMOTE-INVALID: {}",
            validation.errors.join("; ")
        )));
    }
    let dest_exists = destination.exists();

    if dest_exists && !options.yes {
        return Err(MaccError::Validation(format!(
            "PRD-GEN-PROMOTE-CONFLICT: '{}' already exists. Use --yes to overwrite.",
            options.dest_path.display()
        )));
    }

    let backup_created = if dest_exists {
        if let Some(backup_dir) = &options.backup_dir {
            let ts = chrono::Local::now().format("%Y%m%d-%H%M%S").to_string();
            let filename = options
                .dest_path
                .file_name()
                .map(|n| n.to_string_lossy().to_string())
                .unwrap_or_else(|| "prd.json".to_string());
            let backup_path = backup_dir.join(&ts).join(&filename);
            if let Some(parent) = backup_path.parent() {
                std::fs::create_dir_all(parent).map_err(|e| MaccError::Io {
                    path: parent.to_string_lossy().into(),
                    action: "create backup dir".into(),
                    source: e,
                })?;
            }
            std::fs::copy(&destination, &backup_path).map_err(|e| MaccError::Io {
                path: options.dest_path.to_string_lossy().into(),
                action: "backup existing PRD".into(),
                source: e,
            })?;
            Some(backup_path.to_string_lossy().to_string())
        } else {
            None
        }
    } else {
        None
    };

    crate::atomic_write(
        &crate::ProjectPaths::from_root(destination.parent().unwrap()),
        &destination,
        raw.as_bytes(),
    )?;

    Ok(PromoteResult {
        promoted: true,
        source: options.source_path.to_string_lossy().to_string(),
        destination: options.dest_path.to_string_lossy().to_string(),
        backup_created,
        warnings: validation.warnings,
    })
}

/// List all generation run directories newest-first under the given base.
/// Returns `(run_id, run_dir_path)` pairs.
pub fn list_generation_runs(base_dir: &Path) -> Vec<(String, PathBuf)> {
    if !base_dir.exists() {
        return vec![];
    }
    let mut runs: Vec<(String, PathBuf)> = std::fs::read_dir(base_dir)
        .ok()
        .into_iter()
        .flatten()
        .flatten()
        .filter_map(|e| {
            let path = e.path();
            if path.is_dir() {
                Some((e.file_name().to_string_lossy().to_string(), path))
            } else {
                None
            }
        })
        .collect();
    runs.sort_by(|a, b| b.0.cmp(&a.0));
    runs
}
