use chrono::Utc;
use sha2::{Digest, Sha256};
use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};

use super::classifier::copy_dir_all;
use super::logs::{parse_duration_to_seconds, parse_size_to_bytes};
use super::manifest::{
    SaveBundleManifest, SaveExcludes, SaveHashes, SaveIncludes, SavePaths, SaveSecurity,
    SecretScanMetadata,
};
use super::repository_identity::get_repository_identity;
use super::scanner::redact_secrets_in_text;
use super::SecretHandlingChoice;
use crate::{MaccError, ProjectPaths, Result};

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

fn write_checksums_file(tmp_save_dir: &Path) -> Result<()> {
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

pub fn create_save_bundle(
    paths: &ProjectPaths,
    name: &str,
    opts: &super::SaveOptions,
) -> Result<SaveBundleManifest> {
    if name.is_empty()
        || name
            .chars()
            .any(|c| !c.is_alphanumeric() && c != '-' && c != '_')
    {
        return Err(MaccError::Validation(format!(
            "MACC-SAVE-1000: Invalid save name: {}",
            name
        )));
    }

    let saves_dir = super::user_saves_dir().ok_or(MaccError::HomeDirNotFound)?;
    let target_save_dir = saves_dir.join(name);

    if target_save_dir.exists() && !opts.overwrite {
        return Err(MaccError::Validation(format!(
            "MACC-SAVE-1001: Save already exists: {}",
            name
        )));
    }

    if !paths.config_path.exists() {
        return Err(MaccError::Validation(
            "MACC-SAVE-1002: No .macc/macc.yaml found in project root.".to_string(),
        ));
    }

    // Determine what to include
    let only_sections: Option<BTreeSet<String>> = opts
        .only
        .as_ref()
        .map(|s| s.split(',').map(|sec| sec.trim().to_string()).collect());

    let include_config = only_sections
        .as_ref()
        .map(|s| s.contains("config"))
        .unwrap_or(true);
    let include_sessions = !opts.no_sessions
        && only_sections
            .as_ref()
            .map(|s| s.contains("sessions"))
            .unwrap_or(true);
    let include_catalogs = only_sections
        .as_ref()
        .map(|s| s.contains("catalogs"))
        .unwrap_or(true);
    let include_logs = opts.include_logs
        || only_sections
            .as_ref()
            .map(|s| s.contains("logs"))
            .unwrap_or(false);
    let include_prd = opts.include_prd
        || only_sections
            .as_ref()
            .map(|s| s.contains("prd"))
            .unwrap_or(false);
    let include_state = opts.include_state
        || only_sections
            .as_ref()
            .map(|s| s.contains("automation_state"))
            .unwrap_or(false);

    let tmp_parent = saves_dir.join(".tmp");
    fs::create_dir_all(&tmp_parent).ok();
    let tmp_save_dir = tmp_parent.join(format!("{}.{}", name, uuid::Uuid::new_v4()));
    fs::create_dir_all(&tmp_save_dir).map_err(|e| MaccError::Io {
        path: tmp_save_dir.to_string_lossy().into(),
        action: "create temp save directory".into(),
        source: e,
    })?;

    let mut includes = SaveIncludes {
        config: false,
        coordinator_sessions: false,
        catalogs: false,
        logs: false,
        prd: false,
        automation_state: false,
    };

    let mut paths_meta = SavePaths {
        config: None,
        coordinator_sessions: None,
        logs_archive: None,
        task_registry: None,
    };

    let mut hashes = SaveHashes {
        config: None,
        coordinator_sessions: None,
        task_registry: None,
        manifest_payload: "".to_string(),
    };

    let mut findings_count = 0;

    // 1. Copy config
    if include_config && paths.config_path.exists() {
        let config_dir = tmp_save_dir.join("config");
        fs::create_dir_all(&config_dir).ok();
        let dest = config_dir.join("macc.yaml");
        fs::copy(&paths.config_path, &dest).map_err(|e| MaccError::Io {
            path: dest.to_string_lossy().into(),
            action: "copy config file".into(),
            source: e,
        })?;

        let config_bytes = fs::read(&dest).unwrap();
        let findings = crate::security::scan_bytes("config/macc.yaml", &config_bytes);
        findings_count += findings
            .iter()
            .filter(|f| f.severity == crate::security::Severity::Error)
            .count();

        let sha = compute_file_sha256(&dest).unwrap();
        hashes.config = Some(sha);
        paths_meta.config = Some("config/macc.yaml".to_string());
        includes.config = true;
    }

    // 2. Copy sessions
    let sessions_src = paths.macc_dir.join("state").join("tool-sessions.json");
    if include_sessions && sessions_src.exists() {
        let state_dir = tmp_save_dir.join("state");
        fs::create_dir_all(&state_dir).ok();
        let dest = state_dir.join("tool-sessions.json");
        fs::copy(&sessions_src, &dest).map_err(|e| MaccError::Io {
            path: dest.to_string_lossy().into(),
            action: "copy tool-sessions file".into(),
            source: e,
        })?;

        let sessions_bytes = fs::read(&dest).unwrap();
        let findings = crate::security::scan_bytes("state/tool-sessions.json", &sessions_bytes);
        findings_count += findings
            .iter()
            .filter(|f| f.severity == crate::security::Severity::Error)
            .count();

        let sha = compute_file_sha256(&dest).unwrap();
        hashes.coordinator_sessions = Some(sha);
        paths_meta.coordinator_sessions = Some("state/tool-sessions.json".to_string());
        includes.coordinator_sessions = true;
    }

    // 3. Copy catalogs
    if include_catalogs && paths.project_catalog_dir().exists() {
        let catalogs_dest = tmp_save_dir.join("catalogs");
        if let Err(e) = copy_dir_all(paths.project_catalog_dir(), &catalogs_dest, paths) {
            fs::remove_dir_all(&tmp_save_dir).ok();
            return Err(MaccError::Io {
                path: catalogs_dest.to_string_lossy().into(),
                action: "copy catalog directory".into(),
                source: e,
            });
        }
        includes.catalogs = true;
    }

    // 4. Copy task registry / state
    let registry_src = paths.macc_dir.join("automation").join("task");
    if (include_prd || include_state) && registry_src.exists() {
        let registry_dest = tmp_save_dir.join("automation").join("task");
        fs::create_dir_all(registry_dest.parent().unwrap()).ok();
        if let Err(e) = copy_dir_all(&registry_src, &registry_dest, paths) {
            fs::remove_dir_all(&tmp_save_dir).ok();
            return Err(MaccError::Io {
                path: registry_dest.to_string_lossy().into(),
                action: "copy task registry directory".into(),
                source: e,
            });
        }
        includes.prd = include_prd;
        includes.automation_state = include_state;
        paths_meta.task_registry = Some("automation/task".to_string());
    }

    // 5. Copy logs
    let logs_src = paths.macc_dir.join("log");
    if include_logs && logs_src.exists() {
        let logs_dest = tmp_save_dir.join("logs");
        fs::create_dir_all(&logs_dest).ok();

        // Custom copy logs with size limit, time range, and redaction
        let max_bytes = parse_size_to_bytes(&opts.log_max_size);
        let max_age_seconds = parse_duration_to_seconds(&opts.log_since);
        let mut total_copied = 0;

        let mut read_logs = |dir: &Path| -> std::io::Result<()> {
            if !dir.exists() {
                return Ok(());
            }
            for entry in fs::read_dir(dir)? {
                let entry = entry?;
                let path = entry.path();
                if path.is_file() {
                    let meta = entry.metadata()?;
                    if let Ok(modified_time) = meta.modified() {
                        if let Ok(elapsed) = modified_time.elapsed() {
                            if elapsed.as_secs() > max_age_seconds {
                                continue;
                            }
                        }
                    }
                    if total_copied + meta.len() > max_bytes {
                        break;
                    }
                    if let Ok(mut text) = fs::read_to_string(&path) {
                        if opts.redact_logs {
                            text = redact_secrets_in_text(&text);
                        }
                        let rel = path.strip_prefix(&logs_src).unwrap();
                        let target_path = logs_dest.join(rel);
                        fs::create_dir_all(target_path.parent().unwrap()).ok();
                        fs::write(&target_path, text)?;
                        total_copied += meta.len();
                    }
                }
            }
            Ok(())
        };

        let _ = read_logs(&logs_src);
        let _ = read_logs(&logs_src.join("coordinator"));
        let _ = read_logs(&logs_src.join("performer"));

        includes.logs = true;
        paths_meta.logs_archive = Some("logs".to_string());
    }

    // If dry run, clean up temp dir and abort
    if opts.dry_run {
        fs::remove_dir_all(&tmp_save_dir).ok();
        return Ok(SaveBundleManifest {
            version: 1,
            kind: "macc.save_bundle".to_string(),
            name: name.to_string(),
            description: opts.description.clone(),
            created_at: Utc::now().to_rfc3339(),
            updated_at: Utc::now().to_rfc3339(),
            macc_version: crate::version().to_string(),
            repository: get_repository_identity(&paths.root),
            includes,
            excludes: SaveExcludes {
                worktrees: true,
                cache: true,
                generated_files: true,
                secrets: true,
            },
            paths: paths_meta,
            hashes,
            security: SaveSecurity {
                secret_scan: SecretScanMetadata {
                    performed: true,
                    findings: findings_count,
                    redacted_logs: opts.redact_logs,
                },
            },
        });
    }

    // Abort or redact/exclude if secrets were found in config/session files
    if findings_count > 0 {
        match opts.handle_secrets {
            Some(SecretHandlingChoice::Redact) => {
                let config_dest = tmp_save_dir.join("config").join("macc.yaml");
                if config_dest.exists() {
                    if let Ok(content) = fs::read_to_string(&config_dest) {
                        let redacted = redact_secrets_in_text(&content);
                        fs::write(&config_dest, &redacted).ok();
                        let sha = compute_file_sha256(&config_dest).unwrap();
                        hashes.config = Some(sha);
                    }
                }
                let sessions_dest = tmp_save_dir.join("state").join("tool-sessions.json");
                if sessions_dest.exists() {
                    if let Ok(content) = fs::read_to_string(&sessions_dest) {
                        let redacted = redact_secrets_in_text(&content);
                        fs::write(&sessions_dest, &redacted).ok();
                        let sha = compute_file_sha256(&sessions_dest).unwrap();
                        hashes.coordinator_sessions = Some(sha);
                    }
                }
                findings_count = 0;
            }
            Some(SecretHandlingChoice::Exclude) => {
                let config_dest = tmp_save_dir.join("config").join("macc.yaml");
                if config_dest.exists() {
                    if let Ok(bytes) = fs::read(&config_dest) {
                        let findings = crate::security::scan_bytes("config/macc.yaml", &bytes);
                        if findings
                            .iter()
                            .any(|f| f.severity == crate::security::Severity::Error)
                        {
                            fs::remove_dir_all(config_dest.parent().unwrap()).ok();
                            includes.config = false;
                            hashes.config = None;
                            paths_meta.config = None;
                        }
                    }
                }
                let sessions_dest = tmp_save_dir.join("state").join("tool-sessions.json");
                if sessions_dest.exists() {
                    if let Ok(bytes) = fs::read(&sessions_dest) {
                        let findings =
                            crate::security::scan_bytes("state/tool-sessions.json", &bytes);
                        if findings
                            .iter()
                            .any(|f| f.severity == crate::security::Severity::Error)
                        {
                            fs::remove_dir_all(sessions_dest.parent().unwrap()).ok();
                            includes.coordinator_sessions = false;
                            hashes.coordinator_sessions = None;
                            paths_meta.coordinator_sessions = None;
                        }
                    }
                }
                findings_count = 0;
            }
            _ => {
                fs::remove_dir_all(&tmp_save_dir).ok();
                return Err(MaccError::Validation(format!("MACC-SAVE-1003: Secret scan failed. Found {} potential secrets in config/session files. Choose [R/E/A] to proceed.", findings_count)));
            }
        }
    }

    // Write manifest
    let manifest_path = tmp_save_dir.join("manifest.yaml");
    let mut manifest = SaveBundleManifest {
        version: 1,
        kind: "macc.save_bundle".to_string(),
        name: name.to_string(),
        description: opts.description.clone(),
        created_at: Utc::now().to_rfc3339(),
        updated_at: Utc::now().to_rfc3339(),
        macc_version: crate::version().to_string(),
        repository: get_repository_identity(&paths.root),
        includes,
        excludes: SaveExcludes {
            worktrees: true,
            cache: true,
            generated_files: true,
            secrets: true,
        },
        paths: paths_meta,
        hashes: hashes.clone(),
        security: SaveSecurity {
            secret_scan: SecretScanMetadata {
                performed: true,
                findings: findings_count,
                redacted_logs: opts.redact_logs,
            },
        },
    };

    // Calculate manifest payload hash by serializing with empty payload first
    let manifest_str =
        serde_yaml::to_string(&manifest).map_err(|e| MaccError::Validation(e.to_string()))?;
    let payload_hash = compute_manifest_payload_hash(&manifest_str);
    manifest.hashes.manifest_payload = payload_hash;

    // Write final manifest with the calculated payload hash
    let manifest_str =
        serde_yaml::to_string(&manifest).map_err(|e| MaccError::Validation(e.to_string()))?;
    fs::write(&manifest_path, manifest_str).map_err(|e| MaccError::Io {
        path: manifest_path.to_string_lossy().into(),
        action: "write final manifest".into(),
        source: e,
    })?;

    // Write checksums file (sha256sums.txt)
    write_checksums_file(&tmp_save_dir)?;

    // Atomic move to saves dir
    if target_save_dir.exists() {
        let backup_dir = saves_dir.join(format!("{}.old.{}", name, uuid::Uuid::new_v4()));
        fs::rename(&target_save_dir, &backup_dir).ok();
        if let Err(e) = fs::rename(&tmp_save_dir, &target_save_dir) {
            fs::rename(&backup_dir, &target_save_dir).ok(); // Rollback
            return Err(MaccError::Io {
                path: target_save_dir.to_string_lossy().into(),
                action: "atomic replace save directory".into(),
                source: e,
            });
        }
        fs::remove_dir_all(&backup_dir).ok();
    } else {
        fs::create_dir_all(&saves_dir).ok();
        fs::rename(&tmp_save_dir, &target_save_dir).map_err(|e| MaccError::Io {
            path: target_save_dir.to_string_lossy().into(),
            action: "atomic create save directory".into(),
            source: e,
        })?;
    }

    Ok(manifest)
}
