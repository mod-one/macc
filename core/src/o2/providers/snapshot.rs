//! Canonical source fingerprint shared by all code providers.
use super::*;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
#[derive(Debug, Serialize, Deserialize)]
pub struct SourceSnapshot {
    pub version: u32,
    pub project_root: String,
    pub revision: String,
    pub files: BTreeMap<String, String>,
    pub digest: String,
    pub excluded: usize,
    pub truncated: bool,
}
pub fn source_snapshot(
    root: &Path,
    p: &super::super::policy::CodePolicy,
) -> Result<SourceSnapshot> {
    let mut files = BTreeMap::new();
    let mut excluded = 0;
    let names = crate::git::tracked_source_files(root)?;
    let truncated = names.len() > p.max_files as usize;
    for name in names.into_iter().take(p.max_files as usize) {
        match paths::source(root, &name, p.max_file_bytes) {
            Ok((_, data)) => {
                if std::str::from_utf8(&data).is_ok()
                    && paths::safe_text(std::str::from_utf8(&data).unwrap()).is_ok()
                {
                    files.insert(name, digest(&data));
                } else {
                    excluded += 1;
                }
            }
            Err(_) => excluded += 1,
        }
    }
    let digest = digest(
        serde_json::to_string(&files)
            .map_err(|e| error(e.to_string()))?
            .as_bytes(),
    );
    Ok(SourceSnapshot {
        version: 1,
        project_root: root
            .canonicalize()
            .map_err(|e| error(e.to_string()))?
            .display()
            .to_string(),
        revision: crate::git::head_commit(root)?,
        files,
        digest,
        excluded,
        truncated,
    })
}
