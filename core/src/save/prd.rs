//! Ordered, portable snapshots of the coordinator's input files.
use super::manifest::SavedPrd;
use crate::{config::load_canonical_config, prd_queue, MaccError, ProjectPaths, Result};
use std::{
    fs, io,
    path::{Path, PathBuf},
};

fn io_error(path: &Path, source: io::Error) -> MaccError {
    MaccError::Io {
        action: "stage or verify PRD".into(),
        path: path.display().to_string(),
        source,
    }
}

fn payload_name(index: usize) -> String {
    format!("prds/{index}.json")
}

pub(super) fn verified_payload(bundle: &Path, index: usize, prd: &SavedPrd) -> Result<PathBuf> {
    let file = super::sessions::payload(bundle, &payload_name(index))?;
    if super::compute_file_sha256(&file).map_err(|e| io_error(&file, e))? != prd.sha256 {
        return Err(MaccError::Validation(format!(
            "PRD checksum mismatch for {}",
            prd.path
        )));
    }
    Ok(file)
}

pub(super) fn capture(paths: &ProjectPaths, config: &Path, bundle: &Path) -> Result<Vec<SavedPrd>> {
    let config = load_canonical_config(config)?
        .automation
        .coordinator
        .unwrap_or_default();
    let files = if config.prd_files.is_empty() {
        let file = prd_queue::active_path(&paths.root, Some(&config), None)?;
        if config.prd_file.is_none()
            && !file.try_exists().map_err(|e| io_error(&file, e))?
            && paths.macc_dir.join("prd.json").is_file()
        {
            vec![paths.macc_dir.join("prd.json")]
        } else {
            vec![file]
        }
    } else {
        config
            .prd_files
            .iter()
            .map(|file| paths.root.join(file))
            .collect()
    };
    let relative = files
        .iter()
        .map(|file| {
            let relative = file
                .strip_prefix(&paths.root)
                .ok()
                .and_then(Path::to_str)
                .and_then(crate::normalize_relative_path)
                .filter(|relative| paths.root.join(relative) == *file)
                .ok_or_else(|| {
                    MaccError::Validation(format!(
                        "PRD must be inside the project: {}",
                        file.display()
                    ))
                })?;
            crate::project_ownership::validate_path(paths, &relative)?;
            Ok(relative)
        })
        .collect::<Result<Vec<_>>>()?;
    let entries = prd_queue::validate(&paths.root, &relative)?;
    let directory = bundle.join("prds");
    fs::create_dir(&directory).map_err(|e| io_error(&directory, e))?;
    entries
        .into_iter()
        .enumerate()
        .map(|(index, entry)| {
            let source = paths.root.join(&entry.path);
            fs::copy(&source, bundle.join(payload_name(index)))
                .map_err(|e| io_error(&source, e))?;
            let prd = SavedPrd {
                path: entry.path,
                sha256: format!("sha256:{}", entry.fingerprint),
            };
            verified_payload(bundle, index, &prd)?;
            Ok(prd)
        })
        .collect()
}
