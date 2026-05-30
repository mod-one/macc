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
                        normalize_session_json(&mut val, &paths.root);
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

fn recompute_worktree_path(old_path_str: &str, current_project_root: &std::path::Path) -> String {
    let old_path = std::path::Path::new(old_path_str);
    if let Some(pos) = old_path_str.find(".macc/worktrees") {
        let rel = &old_path_str[pos + ".macc/worktrees".len()..];
        let rel_clean = rel.trim_start_matches('/').trim_start_matches('\\');
        current_project_root.join(".macc/worktree").join(rel_clean).to_string_lossy().to_string()
    } else if let Some(pos) = old_path_str.find(".macc/worktree") {
        let rel = &old_path_str[pos + ".macc/worktree".len()..];
        let rel_clean = rel.trim_start_matches('/').trim_start_matches('\\');
        current_project_root.join(".macc/worktree").join(rel_clean).to_string_lossy().to_string()
    } else {
        if let Some(last_component) = old_path.file_name() {
            current_project_root.join(".macc/worktree").join(last_component).to_string_lossy().to_string()
        } else {
            old_path_str.to_string()
        }
    }
}

fn normalize_session_json(value: &mut serde_json::Value, project_root: &std::path::Path) {
    match value {
        serde_json::Value::Object(map) => {
            map.remove("pid");
            map.remove("active_worktree");
            map.remove("owner_pid");
            map.remove("owner_task_id");
            map.remove("owner_worktree");
            
            if let Some(status_val) = map.get_mut("status") {
                if let Some(status_str) = status_val.as_str() {
                    if status_str == "active" {
                        *status_val = serde_json::Value::String("available".to_string());
                    }
                }
            }
            
            if map.contains_key("heartbeat_epoch") {
                map.insert("heartbeat_epoch".to_string(), serde_json::json!(0));
            }
            
            let mut keys_to_rename = Vec::new();
            for (k, v) in map.iter_mut() {
                if k.contains('/') || k.contains('\\') || k.contains(".macc") {
                    let new_k = recompute_worktree_path(k, project_root);
                    if new_k != *k {
                        keys_to_rename.push((k.clone(), new_k));
                    }
                }
                normalize_session_json(v, project_root);
            }
            
            for (old_k, new_k) in keys_to_rename {
                if let Some(val) = map.remove(&old_k) {
                    map.insert(new_k, val);
                }
            }
        }
        serde_json::Value::Array(arr) => {
            for v in arr.iter_mut() {
                normalize_session_json(v, project_root);
            }
        }
        _ => {}
    }
}


#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::path::Path;

    #[test]
    fn test_recompute_worktree_path() {
        let current_root = Path::new("/new/project");
        
        // Absolute path with .macc/worktree
        let old_wt_1 = "/old/project/.macc/worktree/worker-01";
        let new_wt_1 = recompute_worktree_path(old_wt_1, current_root);
        assert_eq!(new_wt_1, "/new/project/.macc/worktree/worker-01");

        // Absolute path with .macc/worktrees
        let old_wt_2 = "/old/project/.macc/worktrees/worker-02";
        let new_wt_2 = recompute_worktree_path(old_wt_2, current_root);
        assert_eq!(new_wt_2, "/new/project/.macc/worktree/worker-02");

        // Plain name
        let old_wt_3 = "worker-03";
        let new_wt_3 = recompute_worktree_path(old_wt_3, current_root);
        assert_eq!(new_wt_3, "/new/project/.macc/worktree/worker-03");
    }

    #[test]
    fn test_normalize_session_json() {
        let current_root = Path::new("/new/project");
        let mut session_data = json!({
            "pid": 9999,
            "active_worktree": "/old/project/.macc/worktree/worker-01",
            "tools": {
                "codex": {
                    "sessions": {
                        "/old/project/.macc/worktree/worker-01": {
                            "session_id": "sid-old",
                            "updated_at": "2026-01-01T00:00:00Z"
                        },
                        "session-new": {
                            "status": "active",
                            "created_at": "2026-01-01T00:00:00Z",
                            "heartbeat_epoch": 1234567,
                            "owner_task_id": "TASK-A",
                            "owner_pid": "1234",
                            "owner_worktree": "/old/project/.macc/worktree/worker-01"
                        }
                    }
                }
            }
        });

        normalize_session_json(&mut session_data, current_root);

        let expected = json!({
            "tools": {
                "codex": {
                    "sessions": {
                        "/new/project/.macc/worktree/worker-01": {
                            "session_id": "sid-old",
                            "updated_at": "2026-01-01T00:00:00Z"
                        },
                        "session-new": {
                            "status": "available",
                            "created_at": "2026-01-01T00:00:00Z",
                            "heartbeat_epoch": 0
                        }
                    }
                }
            }
        });

        assert_eq!(session_data, expected);
    }
}
