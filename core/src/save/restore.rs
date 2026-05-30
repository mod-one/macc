use std::fs;
use chrono::Utc;
use crate::{MaccError, ProjectPaths, Result};
use super::manifest::{SaveBundleManifest, MatchStrength};
use super::bundle::{compute_file_sha256, compute_manifest_payload_hash};
use super::repository_identity::{get_repository_identity, compute_match_strength};
use super::classifier::copy_dir_all;

pub fn restore_save_bundle(paths: &ProjectPaths, name: &str, opts: &super::RestoreOptions) -> Result<()> {
    let saves_dir = super::user_saves_dir().ok_or(MaccError::HomeDirNotFound)?;
    let target_save_dir = saves_dir.join(name);
    
    if !target_save_dir.exists() {
        return Err(MaccError::Validation(format!("MACC-RESTORE-2000: Save bundle not found: {}", name)));
    }

    let manifest_path = target_save_dir.join("manifest.yaml");
    let manifest_str = fs::read_to_string(&manifest_path).map_err(|e| MaccError::Io {
        path: manifest_path.to_string_lossy().into(),
        action: "read manifest for restore".into(),
        source: e,
    })?;

    // Verify manifest payload integrity
    let computed_payload_hash = compute_manifest_payload_hash(&manifest_str);

    let manifest: SaveBundleManifest = serde_yaml::from_str(&manifest_str).map_err(|e| {
        MaccError::Validation(format!("MACC-RESTORE-2001: Unsupported or malformed manifest: {}", e))
    })?;

    if computed_payload_hash != manifest.hashes.manifest_payload {
        return Err(MaccError::Validation("MACC-RESTORE-2003: Checksum mismatch. Manifest payload integrity verification failed.".to_string()));
    }

    // Verify standalone checksums/sha256sums.txt if present
    let checksums_path = target_save_dir.join("checksums").join("sha256sums.txt");
    if checksums_path.exists() {
        let content = fs::read_to_string(&checksums_path).map_err(|e| MaccError::Io {
            path: checksums_path.to_string_lossy().into(),
            action: "read checksums file for validation".into(),
            source: e,
        })?;

        for line in content.lines() {
            let line = line.trim();
            if line.is_empty() {
                continue;
            }
            let parts: Vec<&str> = line.split("  ").collect();
            if parts.len() != 2 {
                return Err(MaccError::Validation(format!(
                    "MACC-RESTORE-2003: Checksum file format is invalid: {}",
                    line
                )));
            }
            let expected_sha = parts[0];
            let rel_path = parts[1];

            let target_path = target_save_dir.join(rel_path.replace('/', &std::path::MAIN_SEPARATOR.to_string()));

            if !target_path.exists() {
                return Err(MaccError::Validation(format!(
                    "MACC-RESTORE-2003: Checksum mismatch. File is missing: {}",
                    rel_path
                )));
            }

            let actual_sha = compute_file_sha256(&target_path).map_err(|e| MaccError::Io {
                path: target_path.to_string_lossy().into(),
                action: "calculate file hash for validation".into(),
                source: e,
            })?;
            let clean_actual_sha = actual_sha.strip_prefix("sha256:").unwrap_or(&actual_sha);

            if clean_actual_sha != expected_sha {
                return Err(MaccError::Validation(format!(
                    "MACC-RESTORE-2003: Checksum mismatch. File is corrupted: {}",
                    rel_path
                )));
            }
        }
    }

    // Repository match validation
    let current_repo = get_repository_identity(&paths.root);
    let match_strength = compute_match_strength(&current_repo, &manifest.repository);
    if match_strength == MatchStrength::None && !opts.yes {
        return Err(MaccError::Validation("MACC-RESTORE-2002: Repository mismatch. Choose config-only restore or confirm with yes.".to_string()));
    }

    // Verify config file checksum if present
    if let Some(config_rel) = &manifest.paths.config {
        let config_src = target_save_dir.join(config_rel);
        if config_src.exists() {
            let actual_sha = compute_file_sha256(&config_src).unwrap();
            if let Some(expected_sha) = &manifest.hashes.config {
                if &actual_sha != expected_sha {
                    return Err(MaccError::Validation("MACC-RESTORE-2003: Checksum mismatch. Save configuration file is corrupted.".to_string()));
                }
            }
        }
    }

    // Verify session file checksum if present
    if let Some(sessions_rel) = &manifest.paths.coordinator_sessions {
        let sessions_src = target_save_dir.join(sessions_rel);
        if sessions_src.exists() {
            let actual_sha = compute_file_sha256(&sessions_src).unwrap();
            if let Some(expected_sha) = &manifest.hashes.coordinator_sessions {
                if &actual_sha != expected_sha {
                    return Err(MaccError::Validation("MACC-RESTORE-2003: Checksum mismatch. Save sessions file is corrupted.".to_string()));
                }
            }
        }
    }

    if opts.dry_run {
        return Ok(());
    }

    // 1. Restore config
    let config_only = opts.config_only || match_strength == MatchStrength::Weak;
    if manifest.includes.config {
        if let Some(config_rel) = &manifest.paths.config {
            let config_src = target_save_dir.join(config_rel);
            if config_src.exists() {
                fs::create_dir_all(paths.config_path.parent().unwrap()).ok();
                
                // Back up overwritten file first
                if paths.config_path.exists() {
                    let backup_name = format!("macc.yaml.backup.{}", Utc::now().timestamp());
                    let backup_path = paths.config_path.parent().unwrap().join(backup_name);
                    fs::copy(&paths.config_path, &backup_path).ok();
                }

                fs::copy(&config_src, &paths.config_path).map_err(|e| MaccError::Io {
                    path: paths.config_path.to_string_lossy().into(),
                    action: "restore config file".into(),
                    source: e,
                })?;
            }
        }
    }

    if config_only {
        // Skip sessions, catalogs, logs, state
        return Ok(());
    }

    // 2. Restore sessions
    let restore_sessions = opts.sessions || (!opts.no_sessions && manifest.includes.coordinator_sessions);
    if restore_sessions {
        if let Some(sessions_rel) = &manifest.paths.coordinator_sessions {
            let sessions_src = target_save_dir.join(sessions_rel);
            if sessions_src.exists() {
                let sessions_dest = paths.macc_dir.join("state").join("tool-sessions.json");
                fs::create_dir_all(sessions_dest.parent().unwrap()).ok();
                
                // Session normalization: drop PIDs / stale leases
                if let Ok(content) = fs::read_to_string(&sessions_src) {
                    if let Ok(mut val) = serde_json::from_str::<serde_json::Value>(&content) {
                        if let Some(obj) = val.as_object_mut() {
                            obj.remove("pid");
                            obj.remove("active_worktree");
                        }
                        let normalized_bytes = serde_json::to_vec_pretty(&val).unwrap();
                        fs::write(&sessions_dest, normalized_bytes).ok();
                    } else {
                        fs::copy(&sessions_src, &sessions_dest).ok();
                    }
                }
            }
        }
    }

    // 3. Restore catalogs
    if manifest.includes.catalogs {
        let catalogs_src = target_save_dir.join("catalogs");
        if catalogs_src.exists() {
            let _ = copy_dir_all(&catalogs_src, &paths.project_catalog_dir(), paths);
        }
    }

    // 4. Restore logs to restored-logs directory
    if opts.include_logs && manifest.includes.logs {
        let logs_src = target_save_dir.join("logs");
        if logs_src.exists() {
            let restored_logs_dest = paths.macc_dir.join("restored-logs").join(name);
            let _ = copy_dir_all(&logs_src, &restored_logs_dest, paths);
        }
    }

    // 5. Restore task registry / state
    if manifest.includes.prd || manifest.includes.automation_state {
        let registry_src = target_save_dir.join("automation").join("task");
        if registry_src.exists() {
            let registry_dest = paths.macc_dir.join("automation").join("task");
            let _ = copy_dir_all(&registry_src, &registry_dest, paths);
        }
    }

    Ok(())
}
