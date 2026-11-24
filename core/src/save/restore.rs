use super::bundle::{compute_file_sha256, compute_manifest_payload_hash};
pub(super) mod files;
use super::manifest::{MatchStrength, SaveBundleManifest};
use super::repository_identity::{compute_match_strength, get_repository_identity};
use crate::{MaccError, ProjectPaths, Result};

use std::fs;
use std::path::Path;

pub fn restore_save_bundle(
    paths: &ProjectPaths,
    name: &str,
    opts: &super::RestoreOptions,
) -> Result<()> {
    let saves_dir = super::user_saves_dir().ok_or(MaccError::HomeDirNotFound)?;
    restore_from(paths, name, opts, &saves_dir)
}

pub(super) fn restore_from(
    paths: &ProjectPaths,
    name: &str,
    opts: &super::RestoreOptions,
    saves_dir: &std::path::Path,
) -> Result<()> {
    let name = super::name::SaveName::parse(name)
        .map_err(|_| MaccError::Validation("MACC-RESTORE-2000: Invalid save name".into()))?;
    let target_save_dir = name.path(saves_dir);

    if !target_save_dir.exists() {
        return Err(MaccError::Validation(format!(
            "MACC-RESTORE-2000: Save bundle not found: {}",
            name
        )));
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
        MaccError::Validation(format!(
            "MACC-RESTORE-2001: Unsupported or malformed manifest: {}",
            e
        ))
    })?;

    if !manifest.supported_format() {
        return Err(MaccError::Validation(
            "MACC-RESTORE-2001: Unsupported bundle format".into(),
        ));
    }
    if let Some(meta) = &manifest.session_store {
        let source = super::sessions::payload(&target_save_dir, &meta.database_path)?;
        if compute_file_sha256(&source).map_err(|e| MaccError::Validation(e.to_string()))?
            != meta.database_sha256
        {
            return Err(MaccError::Validation(
                "MACC-RESTORE-2003: SQLite snapshot checksum mismatch".into(),
            ));
        }
    }
    if computed_payload_hash != manifest.hashes.manifest_payload {
        return Err(MaccError::Validation(
            "MACC-RESTORE-2003: Checksum mismatch. Manifest payload integrity verification failed."
                .to_string(),
        ));
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
            if line.trim().is_empty() {
                continue;
            }
            let (expected_sha, rel_path) = line.split_once("  ").ok_or_else(|| {
                MaccError::Validation(format!(
                    "MACC-RESTORE-2003: Checksum file format is invalid: {line}"
                ))
            })?;

            let target_path = super::sessions::payload(&target_save_dir, rel_path)?;

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
    if match_strength == MatchStrength::None {
        let reason = if manifest.repository.has_legacy_remote_hash() {
            "This legacy bundle cannot establish a cross-checkout match because its Git remote path was case-folded."
        } else {
            "The saved repository identity does not match this project."
        };
        if !opts.yes && !opts.config_only {
            return Err(MaccError::Validation(format!("MACC-RESTORE-2002: {reason} Choose --config-only to transfer settings or --yes to confirm a full cross-project restore.")));
        }
        tracing::warn!("{reason} Proceeding with the explicitly requested cross-project restore.");
    }

    for (section, relative, expected) in [
        (
            "configuration",
            &manifest.paths.config,
            &manifest.hashes.config,
        ),
        (
            "sessions",
            &manifest.paths.coordinator_sessions,
            &manifest.hashes.coordinator_sessions,
        ),
    ] {
        if let Some(relative) = relative {
            let source = super::sessions::payload(&target_save_dir, relative)?;
            if let Some(expected) = expected {
                let actual = compute_file_sha256(&source).map_err(|err| MaccError::Io {
                    path: source.display().to_string(),
                    action: format!("hash {section} payload"),
                    source: err,
                })?;
                if actual != *expected {
                    return Err(MaccError::Validation(format!(
                        "MACC-RESTORE-2003: Checksum mismatch. Save {section} file is corrupted."
                    )));
                }
            }
        }
    }

    let config_only = opts.config_only || match_strength == MatchStrength::Weak;
    let restore_sessions = !config_only
        && !opts.no_sessions
        && (opts.sessions || manifest.includes.coordinator_sessions);
    let sections = files::plan(
        paths,
        &target_save_dir,
        &name,
        &manifest,
        config_only,
        restore_sessions,
        opts.include_logs,
    )?;
    let registry = if manifest.includes.automation_state && !config_only {
        let config = if manifest.includes.config {
            super::sessions::payload(
                &target_save_dir,
                manifest.paths.config.as_deref().ok_or_else(|| {
                    MaccError::Validation("Missing configuration path in bundle".into())
                })?,
            )?
        } else {
            paths.config_path.clone()
        };
        let store = super::registry::RegistryStore::resolve(paths, &config)?;
        store.preflight(paths)?;
        Some((store, super::registry::unpack(&target_save_dir, &manifest)?))
    } else {
        None
    };
    let session_db = paths.macc_dir.join("memory.db");
    if restore_sessions && manifest.session_store.is_some() {
        files::destination("session database", &paths.root, &session_db, false)?;
        files::destination(
            "session authority",
            &paths.root,
            &crate::session_store::marker_path(&paths.root),
            false,
        )?;
    }
    let decision_db = if !config_only && !opts.no_sessions && manifest.decision_store.is_some() {
        let primary = crate::persistent::primary_root(&paths.root)?;
        let file = primary.join(".macc/persistent/control.sqlite");
        files::destination("decision database", &primary, &file, false)?;
        Some(file)
    } else {
        None
    };
    let _session_guard = if restore_sessions && !opts.dry_run {
        Some(crate::session_store::lock(&paths.root)?)
    } else {
        None
    };
    let sqlite_source = if restore_sessions {
        if let Some(meta) = &manifest.session_store {
            let source = super::sessions::payload(&target_save_dir, &meta.database_path)?;
            if opts.dry_run {
                crate::session_store::bundle::validate_snapshot(&paths.root, &source, meta)?;
            } else {
                crate::session_store::bundle::validate_locked(&paths.root, &source, meta)?;
            }
            Some(source)
        } else {
            if crate::session_store::is_sqlite(&paths.root)? {
                return Err(MaccError::Validation("SQLite authority is active; legacy session restore requires an explicit rollback first, or --no-sessions".into()));
            }
            None
        }
    } else {
        None
    };
    let decision_source = if !config_only && !opts.no_sessions {
        if let Some(meta) = &manifest.decision_store {
            let source = super::sessions::payload(&target_save_dir, &meta.database_path)?;
            if opts.dry_run {
                crate::coordinator::decisions::validated_snapshot(&paths.root, &source, meta)?;
            } else {
                crate::coordinator::decisions::validate_backup(&paths.root, &source, meta)?;
            }
            Some(source)
        } else {
            None
        }
    } else {
        None
    };
    if opts.dry_run {
        return Ok(());
    }
    let mut progress = Progress::default();
    if let (Some(source), Some(meta), Some(target)) =
        (&decision_source, &manifest.decision_store, decision_db)
    {
        progress.run("decision database", &target, || {
            crate::coordinator::decisions::restore_backup(&paths.root, source, meta)
        })?;
    }
    if let (Some(source), Some(meta)) = (&sqlite_source, &manifest.session_store) {
        progress.run("session database", &session_db, || {
            crate::session_store::bundle::restore_locked(&paths.root, source, meta)
        })?;
    }
    for section in sections {
        progress.run(section.name, &section.target, || section.restore(paths))?;
    }
    if let Some((store, registry)) = registry {
        progress.run("coordinator registry", store.target(), || {
            store.restore(&registry)
        })?;
    }
    Ok(())
}

#[derive(Default)]
struct Progress {
    completed: Vec<&'static str>,
}
impl Progress {
    fn run(
        &mut self,
        section: &'static str,
        target: &Path,
        operation: impl FnOnce() -> Result<()>,
    ) -> Result<()> {
        if let Err(error) = operation() {
            let completed = if self.completed.is_empty() {
                "none".into()
            } else {
                self.completed.join(", ")
            };
            let retry = if self.completed.contains(&"session database")
                || self.completed.contains(&"legacy sessions")
            {
                "Correct the cause and retry with --no-sessions to retain restored session data."
            } else {
                "Correct the cause and retry the same bundle and options."
            };
            let context = format!(
                "Restore stopped in {section} at {}. Completed sections: {completed}. Retry point: {section}. {retry}",
                target.display()
            );
            return Err(match error {
                MaccError::Io {
                    path,
                    action,
                    source,
                } => MaccError::Io {
                    path,
                    action: format!("{action}. {context}"),
                    source,
                },
                other => MaccError::Validation(format!("{context} Cause: {other}")),
            });
        }
        self.completed.push(section);
        Ok(())
    }
}

fn recompute_worktree_path(old_path_str: &str, current_project_root: &std::path::Path) -> String {
    let old_path = std::path::Path::new(old_path_str);
    if let Some(pos) = old_path_str.find(".macc/worktrees") {
        let rel = &old_path_str[pos + ".macc/worktrees".len()..];
        let rel_clean = rel.trim_start_matches('/').trim_start_matches('\\');
        current_project_root
            .join(".macc/worktree")
            .join(rel_clean)
            .to_string_lossy()
            .to_string()
    } else if let Some(pos) = old_path_str.find(".macc/worktree") {
        let rel = &old_path_str[pos + ".macc/worktree".len()..];
        let rel_clean = rel.trim_start_matches('/').trim_start_matches('\\');
        current_project_root
            .join(".macc/worktree")
            .join(rel_clean)
            .to_string_lossy()
            .to_string()
    } else {
        if let Some(last_component) = old_path.file_name() {
            current_project_root
                .join(".macc/worktree")
                .join(last_component)
                .to_string_lossy()
                .to_string()
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
                        *status_val = serde_json::Value::String("abandoned".to_string());
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
                            "status": "abandoned",
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
