//! Save payload hashing and checksum manifest generation.
use crate::{MaccError, Result};
use sha2::{Digest, Sha256};
use std::{
    fs,
    path::{Path, PathBuf},
};

pub fn compute_file_sha256(path: &Path) -> std::io::Result<String> {
    let content = fs::read(path)?;
    Ok(sha256(&content))
}

pub(super) fn sha256(content: &[u8]) -> String {
    format!("sha256:{:x}", Sha256::digest(content))
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

pub(super) fn staged_files(root: &Path) -> Result<Vec<PathBuf>> {
    fn visit(root: &Path, relative: &Path, files: &mut Vec<PathBuf>) -> std::io::Result<()> {
        for entry in fs::read_dir(root.join(relative))? {
            let entry = entry?;
            let path = relative.join(entry.file_name());
            let kind = entry.file_type()?;
            if kind.is_dir() {
                visit(root, &path, files)?;
            } else if kind.is_file() {
                files.push(path);
            } else {
                return Err(std::io::Error::other(
                    "Unexpected non-regular entry in staged bundle",
                ));
            }
        }
        Ok(())
    }
    let mut files = Vec::new();
    visit(root, Path::new(""), &mut files).map_err(|source| MaccError::Io {
        path: root.display().to_string(),
        action: "enumerate staged bundle".into(),
        source,
    })?;
    files.sort();
    Ok(files)
}

pub(super) fn write_checksums_file(tmp_save_dir: &Path) -> Result<()> {
    let checksums_dir = tmp_save_dir.join("checksums");
    fs::create_dir_all(&checksums_dir).map_err(|source| MaccError::Io {
        path: checksums_dir.display().to_string(),
        action: "create checksums directory".into(),
        source,
    })?;
    let mut lines = Vec::new();
    for relative in staged_files(tmp_save_dir)? {
        if relative == Path::new("checksums/sha256sums.txt") {
            continue;
        }
        let path = tmp_save_dir.join(&relative);
        let bytes = fs::read(&path).map_err(|source| MaccError::Io {
            path: path.display().to_string(),
            action: "verify final save payload".into(),
            source,
        })?;
        super::scanner::require_clean(&bytes)?;
        let sha = sha256(&bytes);
        let rel_path_str = relative.to_string_lossy().replace('\\', "/");
        let clean_sha = sha.strip_prefix("sha256:").unwrap_or(&sha);
        lines.push(format!("{}  {}", clean_sha, rel_path_str));
    }

    let checksums_file_path = checksums_dir.join("sha256sums.txt");
    let checksums_content = lines.join("\n") + "\n";
    super::scanner::require_clean(checksums_content.as_bytes())?;
    fs::write(&checksums_file_path, checksums_content).map_err(|e| MaccError::Io {
        path: checksums_file_path.to_string_lossy().into(),
        action: "write sha256sums.txt file".into(),
        source: e,
    })?;

    Ok(())
}
