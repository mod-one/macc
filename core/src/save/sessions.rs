//! Session payload packaging, shared by every save client.
use super::{bundle::compute_file_sha256, manifest::*};
use crate::{MaccError, ProjectPaths, Result};
use std::{fs, path::Path};

pub(super) fn capture(
    paths: &ProjectPaths,
    directory: &Path,
    includes: &mut SaveIncludes,
    payload_paths: &mut SavePaths,
    hashes: &mut SaveHashes,
) -> Result<Option<crate::session_store::SessionStoreSnapshot>> {
    let _guard = crate::session_store::lock(&paths.root)?;
    let snapshot = if crate::session_store::is_sqlite(&paths.root)? {
        Some(crate::session_store::bundle::capture_locked(
            &paths.root,
            directory,
        )?)
    } else {
        let source = paths.macc_dir.join("state/tool-sessions.json");
        if !source.exists() {
            return Ok(None);
        }
        let destination = directory.join("state/tool-sessions.json");
        crate::session_store::write(
            &paths.root,
            &destination,
            &crate::session_store::bytes(&source)?,
        )?;
        None
    };
    let destination = directory.join("state/tool-sessions.json");
    hashes.coordinator_sessions =
        Some(
            compute_file_sha256(&destination).map_err(|e| MaccError::Io {
                path: destination.display().to_string(),
                action: "hash session payload".into(),
                source: e,
            })?,
        );
    payload_paths.coordinator_sessions = Some("state/tool-sessions.json".into());
    includes.coordinator_sessions = true;
    // Old JSON-only readers must never publish the SQLite authority envelope.
    if snapshot.is_some() {
        hashes.coordinator_sessions = None;
        payload_paths.coordinator_sessions = None;
    }
    Ok(snapshot)
}
pub(super) fn findings(directory: &Path, snapshot: bool) -> Result<usize> {
    let mut count = 0;
    for rel in ["state/tool-sessions.json", "state/memory.sqlite"] {
        if rel.ends_with(".sqlite") && !snapshot {
            continue;
        }
        let path = directory.join(rel);
        if path.exists() {
            // SQLite pages contain non-UTF8 bytes. Scan their lossy text view
            // so embedded ASCII credentials are not silently skipped.
            let bytes = crate::session_store::bytes(&path)?;
            let text = String::from_utf8_lossy(&bytes);
            count += crate::security::scan_bytes(rel, text.as_bytes())
                .iter()
                .filter(|f| f.severity == crate::security::Severity::Error)
                .count();
        }
    }
    Ok(count)
}
/// Manifest paths are relative regular files inside the bundle, never symlinks.
pub(super) fn payload(directory: &Path, relative: &str) -> Result<std::path::PathBuf> {
    let p = Path::new(relative);
    if p.components()
        .any(|c| !matches!(c, std::path::Component::Normal(_)))
        || p.as_os_str().is_empty()
    {
        return Err(MaccError::Validation(
            "MACC-RESTORE-2003: Invalid bundle payload path".into(),
        ));
    }
    let mut path = directory.to_path_buf();
    for part in p.components() {
        path.push(part);
        let metadata = fs::symlink_metadata(&path).map_err(|e| {
            MaccError::Validation(format!("MACC-RESTORE-2003: Missing bundle payload: {e}"))
        })?;
        if metadata.file_type().is_symlink() {
            return Err(MaccError::Validation(
                "MACC-RESTORE-2003: Symlink bundle payload rejected".into(),
            ));
        }
    }
    if !path.is_file() {
        return Err(MaccError::Validation(
            "MACC-RESTORE-2003: Expected regular bundle payload".into(),
        ));
    }
    Ok(path)
}
