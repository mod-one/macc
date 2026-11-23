use chrono::Utc;
use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};

use super::classifier::copy_dir_all;
use super::manifest::{
    SaveBundleManifest, SaveExcludes, SaveHashes, SaveIncludes, SavePaths, SaveSecurity,
    SecretScanMetadata,
};
use super::repository_identity::get_repository_identity;
use crate::{MaccError, ProjectPaths, Result};

use super::integrity::write_checksums_file;
pub use super::integrity::{compute_file_sha256, compute_manifest_payload_hash};

struct StagingDirectory(PathBuf);
impl Drop for StagingDirectory {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn bundle_version(registry: bool, decisions: bool, sessions: bool) -> u32 {
    match (registry, decisions, sessions) {
        (true, _, _) => 4,
        (_, true, _) => 3,
        (_, _, true) => 2,
        _ => 1,
    }
}

pub fn create_save_bundle(
    paths: &ProjectPaths,
    name: &str,
    opts: &super::SaveOptions,
) -> Result<SaveBundleManifest> {
    let saves_dir = super::user_saves_dir().ok_or(MaccError::HomeDirNotFound)?;
    create_in(paths, name, opts, &saves_dir)
}

pub(super) fn create_in(
    paths: &ProjectPaths,
    name: &str,
    opts: &super::SaveOptions,
    saves_dir: &Path,
) -> Result<SaveBundleManifest> {
    let name = super::name::SaveName::parse(name)?;
    let target_save_dir = name.path(saves_dir);

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
    let tmp_save_dir = tmp_parent.join(format!("{}.{}", name, uuid::Uuid::new_v4()));
    fs::create_dir_all(&tmp_save_dir).map_err(|e| MaccError::Io {
        path: tmp_save_dir.to_string_lossy().into(),
        action: "create temp save directory".into(),
        source: e,
    })?;

    let _cleanup = StagingDirectory(tmp_save_dir.clone());

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

    // 1. Copy config
    if include_config && paths.config_path.exists() {
        let config_dir = tmp_save_dir.join("config");
        fs::create_dir_all(&config_dir).map_err(|source| MaccError::Io {
            path: config_dir.display().to_string(),
            action: "stage configuration".into(),
            source,
        })?;
        let dest = config_dir.join("macc.yaml");
        fs::copy(&paths.config_path, &dest).map_err(|e| MaccError::Io {
            path: dest.to_string_lossy().into(),
            action: "copy config file".into(),
            source: e,
        })?;

        let sha = compute_file_sha256(&dest).map_err(|source| MaccError::Io {
            path: dest.display().to_string(),
            action: "hash staged configuration".into(),
            source,
        })?;
        hashes.config = Some(sha);
        paths_meta.config = Some("config/macc.yaml".to_string());
        includes.config = true;
        super::skill_locks::capture(paths, &tmp_save_dir)?;
    }

    // 2. Capture sessions through the canonical authority service.
    let decision_store = if include_sessions {
        crate::coordinator::decisions::capture_backup(&paths.root, &tmp_save_dir)?
    } else {
        None
    };
    let session_store = if include_sessions {
        super::sessions::capture(
            paths,
            &tmp_save_dir,
            &mut includes,
            &mut paths_meta,
            &mut hashes,
        )?
    } else {
        None
    };

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
    if include_prd && registry_src.exists() {
        let registry_dest = tmp_save_dir.join("automation").join("task");
        if let Err(e) = copy_dir_all(&registry_src, &registry_dest, paths) {
            fs::remove_dir_all(&tmp_save_dir).ok();
            return Err(MaccError::Io {
                path: registry_dest.to_string_lossy().into(),
                action: "copy task registry directory".into(),
                source: e,
            });
        }
        includes.prd = include_prd;

        paths_meta.task_registry = Some("automation/task".to_string());
    }

    if include_state {
        hashes.task_registry = Some(super::registry::capture(paths, &tmp_save_dir)?);
        paths_meta.task_registry = Some(super::registry::PAYLOAD.into());
        includes.automation_state = true;
    }

    let logs_omitted_by_size = if include_logs {
        super::logs::capture(
            &paths.macc_dir.join("log"),
            &tmp_save_dir.join("logs"),
            opts,
        )?
    } else {
        Vec::new()
    };

    // Write manifest
    let manifest_path = tmp_save_dir.join("manifest.yaml");
    let mut manifest = SaveBundleManifest {
        version: bundle_version(
            includes.automation_state,
            decision_store.is_some(),
            session_store.is_some(),
        ),
        decision_store,
        session_store,
        kind: "macc.save_bundle".to_string(),
        name: name.to_string(),
        description: opts.description.clone(),
        created_at: Utc::now().to_rfc3339(),
        updated_at: Utc::now().to_rfc3339(),
        macc_version: crate::version().to_string(),
        repository: get_repository_identity(&paths.root),
        includes,
        logs_omitted_by_size,
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
                findings: 0,
                redacted_logs: opts.redact_logs,
            },
        },
    };

    super::scanner::secure_payloads(&tmp_save_dir, &mut manifest, opts)?;
    if opts.dry_run {
        return Ok(manifest);
    }
    manifest.version = bundle_version(
        manifest.includes.automation_state,
        manifest.decision_store.is_some(),
        manifest.session_store.is_some(),
    );

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
        if !opts.overwrite {
            return Err(MaccError::Validation(format!(
                "MACC-SAVE-1001: Save already exists: {name}"
            )));
        }
        name.existing_bundle(saves_dir)?;
        let backup_dir = saves_dir.join(format!("{}.old.{}", name, uuid::Uuid::new_v4()));
        fs::rename(&target_save_dir, &backup_dir).map_err(|source| MaccError::Io {
            path: target_save_dir.display().to_string(),
            action: "move previous save bundle".into(),
            source,
        })?;
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
        fs::rename(&tmp_save_dir, &target_save_dir).map_err(|e| MaccError::Io {
            path: target_save_dir.to_string_lossy().into(),
            action: "atomic create save directory".into(),
            source: e,
        })?;
    }

    Ok(manifest)
}
