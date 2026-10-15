//! Save payload hashing and checksum manifest generation.
use crate::{MaccError, Result};
use sha2::{Digest, Sha256};
use std::{
    fs,
    path::{Path, PathBuf},
};

pub fn compute_file_sha256(path: &Path) -> std::io::Result<String> {
    let content = fs::read(path)?;
    let mut hasher = Sha256::new();
    hasher.update(&content);
    Ok(format!("sha256:{:x}", hasher.finalize()))
}

pub fn compute_manifest_payload_hash(content: &str) -> String {
    let mut hasher = Sha256::new();
    for line in content.lines() {
        if line.contains("manifest_payload:") {
            continue;
        }
        hasher.update(line.as_bytes());
        hasher.update(b"\n");
    }
    format!("sha256:{:x}", hasher.finalize())
}

pub(super) fn write_checksums_file(tmp_save_dir: &Path) -> Result<()> {
    let checksums_dir = tmp_save_dir.join("checksums");
    fs::create_dir_all(&checksums_dir).map_err(|e| MaccError::Io {
        path: checksums_dir.to_string_lossy().into(),
        action: "create checksums directory".into(),
        source: e,
    })?;

    let mut files = Vec::new();

    fn visit_dirs(dir: &Path, files: &mut Vec<PathBuf>) -> std::io::Result<()> {
        if dir.is_dir() {
            for entry in fs::read_dir(dir)? {
                let entry = entry?;
                let path = entry.path();
                if path.is_dir() {
                    if path.file_name().map(|n| n == "checksums").unwrap_or(false) {
                        continue;
                    }
                    visit_dirs(&path, files)?;
                } else {
                    files.push(path);
                }
            }
        }
        Ok(())
    }

    visit_dirs(tmp_save_dir, &mut files).map_err(|e| MaccError::Io {
        path: tmp_save_dir.to_string_lossy().into(),
        action: "traverse save bundle files for checksums".into(),
        source: e,
    })?;

    files.sort();

    let mut lines = Vec::new();
    for file_path in files {
        let sha = compute_file_sha256(&file_path).map_err(|e| MaccError::Io {
            path: file_path.to_string_lossy().into(),
            action: "calculate file hash for checksums".into(),
            source: e,
        })?;
        let rel_path = file_path.strip_prefix(tmp_save_dir).unwrap();
        let rel_path_str = rel_path.to_string_lossy().replace('\\', "/");
        let clean_sha = sha.strip_prefix("sha256:").unwrap_or(&sha);
        lines.push(format!("{}  {}", clean_sha, rel_path_str));
    }

    let checksums_file_path = checksums_dir.join("sha256sums.txt");
    let checksums_content = lines.join("\n") + "\n";
    fs::write(&checksums_file_path, checksums_content).map_err(|e| MaccError::Io {
        path: checksums_file_path.to_string_lossy().into(),
        action: "write sha256sums.txt file".into(),
        source: e,
    })?;

    Ok(())
}
