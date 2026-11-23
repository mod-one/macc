//! Apply the save policy to staged files only. The checksum pass verifies the final bytes.
use super::{SaveBundleManifest, SaveOptions, SecretHandlingChoice};
use crate::{
    security::{scan_bytes, Finding, Severity},
    MaccError, Result,
};
use std::{fs, path::Path};

fn blocking(bytes: &[u8]) -> Vec<Finding> {
    scan_bytes("bundle", bytes)
        .into_iter()
        .filter(|f| f.severity == Severity::Error)
        .collect()
}

fn redact(text: &str, mut findings: Vec<Finding>) -> String {
    findings.sort_unstable_by_key(|f| f.range.start);
    let mut output = String::with_capacity(text.len());
    let mut end = 0;
    for f in findings {
        // Overlapping matches are redacted as one range, including either tail.
        if f.range.start >= end {
            output.push_str(&text[end..f.range.start]);
            output.push_str("[REDACTED]");
        }
        end = end.max(f.range.end);
    }
    output.push_str(&text[end..]);
    output
}

pub fn redact_secrets_in_text(text: &str) -> String {
    redact(text, blocking(text.as_bytes()))
}

fn io_error(path: &Path, source: std::io::Error) -> MaccError {
    MaccError::Io {
        path: path.display().to_string(),
        action: "process staged save payload".into(),
        source,
    }
}

pub(super) fn require_clean(bytes: &[u8]) -> Result<()> {
    if !blocking(bytes).is_empty() {
        return Err(MaccError::Validation(
            "Save refused: a blocking secret remains in the staged bundle".into(),
        ));
    }
    Ok(())
}

pub(super) fn secure_payloads(
    root: &Path,
    manifest: &mut SaveBundleManifest,
    opts: &SaveOptions,
) -> Result<()> {
    let files = super::integrity::staged_files(root)?;
    let mut log_files = files.iter().filter(|path| path.starts_with("logs")).count();
    for relative in files {
        if opts.dry_run {
            manifest.security.secret_scan.findings +=
                blocking(relative.to_string_lossy().as_bytes()).len();
        }
        let path = root.join(&relative);
        // Excluding a section can also remove other files enumerated in it.
        if !path.try_exists().map_err(|e| io_error(&path, e))? {
            continue;
        }
        let bytes = fs::read(&path).map_err(|e| io_error(&path, e))?;
        let findings = blocking(&bytes);
        if findings.is_empty() {
            continue;
        }
        if opts.dry_run {
            manifest.security.secret_scan.findings += findings.len();
            continue;
        }
        match opts.handle_secrets {
            None | Some(SecretHandlingChoice::Abort) => {
                return Err(MaccError::Validation("MACC-SAVE-1003: Secret scan failed in staged payloads. Choose redaction, exclusion or abort.".into()));
            }
            Some(SecretHandlingChoice::Exclude) => {
                exclude(root, &relative, manifest)?;
                if relative.starts_with("logs") {
                    log_files -= 1;
                }
            }
            Some(SecretHandlingChoice::Redact) => {
                redact_file(&path, &relative, &bytes, findings, manifest)?
            }
        }
    }
    manifest.includes.logs = log_files > 0;
    manifest.paths.logs_archive = manifest.includes.logs.then(|| "logs".into());
    if opts.dry_run {
        // The final checksum manifest contains the relative filenames as well.
        let yaml =
            serde_yaml::to_string(manifest).map_err(|e| MaccError::Validation(e.to_string()))?;
        manifest.security.secret_scan.findings += blocking(yaml.as_bytes()).len();
    }
    Ok(())
}

fn redact_file(
    path: &Path,
    relative: &Path,
    bytes: &[u8],
    findings: Vec<Finding>,
    manifest: &mut SaveBundleManifest,
) -> Result<()> {
    let config = relative == Path::new("config/macc.yaml");
    let session =
        relative == Path::new("state/tool-sessions.json") && manifest.session_store.is_none();
    if !(config || session || relative.starts_with("logs") || relative.starts_with("catalogs")) {
        return Err(MaccError::Validation("MACC-SAVE-1003: This structured snapshot cannot be text-redacted. Exclude it or abort.".into()));
    }
    let text = std::str::from_utf8(bytes).map_err(|_| {
        MaccError::Validation(
            "MACC-SAVE-1003: Binary payload cannot be text-redacted. Exclude it or abort.".into(),
        )
    })?;
    fs::write(path, redact(text, findings)).map_err(|e| io_error(path, e))?;
    if config || session {
        let hash = super::compute_file_sha256(path).map_err(|e| io_error(path, e))?;
        if config {
            manifest.hashes.config = Some(hash);
        } else {
            manifest.hashes.coordinator_sessions = Some(hash);
        }
    }
    Ok(())
}

fn exclude(root: &Path, relative: &Path, manifest: &mut SaveBundleManifest) -> Result<()> {
    let remove = |rel: &Path| {
        let path = root.join(rel);
        fs::remove_file(&path).map_err(|e| io_error(&path, e))
    };
    if relative.starts_with("config") {
        let path = root.join("config");
        fs::remove_dir_all(&path).map_err(|e| io_error(&path, e))?;
        manifest.includes.config = false;
        manifest.paths.config = None;
        manifest.hashes.config = None;
    } else if relative == Path::new("state/tool-sessions.json")
        || relative == Path::new("state/memory.sqlite")
    {
        remove(Path::new("state/tool-sessions.json"))?;
        if let Some(snapshot) = manifest.session_store.take() {
            remove(Path::new(&snapshot.database_path))?;
        }
        manifest.includes.coordinator_sessions = false;
        manifest.paths.coordinator_sessions = None;
        manifest.hashes.coordinator_sessions = None;
    } else if relative == Path::new(super::registry::PAYLOAD) {
        remove(relative)?;
        manifest.includes.automation_state = false;
        manifest.paths.task_registry = None;
        manifest.hashes.task_registry = None;
    } else if relative.starts_with("prds") {
        let path = root.join("prds");
        fs::remove_dir_all(&path).map_err(|e| io_error(&path, e))?;
        manifest.prd_files.clear();
        manifest.includes.prd = false;
    } else if relative.starts_with("logs") || relative.starts_with("catalogs") {
        remove(relative)?;
    } else {
        return Err(MaccError::Validation(
            "Cannot exclude a required save payload. Omit its section or abort.".into(),
        ));
    }
    Ok(())
}
