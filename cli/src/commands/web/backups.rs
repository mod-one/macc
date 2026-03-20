use super::errors::ApiError;
use super::types::{ApiBackup, ApiBackupFile, ApiBackupRestoreRequest, ApiBackupRestoreResult};
use super::WebState;
use axum::extract::{Path, State};
use axum::Json;
use chrono::Local;
use serde_json::json;
use std::fs;
use std::path::{Path as StdPath, PathBuf};

pub(super) async fn list_backups_handler(
    State(state): State<WebState>,
) -> std::result::Result<Json<Vec<ApiBackup>>, ApiError> {
    let root =
        macc_core::domain::backups::backup_root(&state.paths, false).map_err(ApiError::from)?;
    let sets = macc_core::domain::backups::list_backup_sets(&root).map_err(ApiError::from)?;
    let backups = sets
        .iter()
        .map(|set| backup_set_to_api(set))
        .collect::<std::result::Result<Vec<_>, _>>()?;
    Ok(Json(backups))
}

pub(super) async fn restore_backup_handler(
    State(state): State<WebState>,
    Path(id): Path<String>,
    Json(payload): Json<ApiBackupRestoreRequest>,
) -> std::result::Result<Json<ApiBackupRestoreResult>, ApiError> {
    if !payload.confirmed {
        return Err(ApiError::confirmation_required(
            "Confirmation required",
            Some(json!({ "backupId": id })),
        ));
    }

    let backup_root =
        macc_core::domain::backups::backup_root(&state.paths, false).map_err(ApiError::from)?;
    let backup_set = backup_root.join(&id);
    if !backup_set.is_dir() {
        return Err(ApiError::backup_not_found(
            format!("backup '{}' was not found", id),
            Some(json!({ "backupId": id })),
        ));
    }

    let files =
        macc_core::domain::backups::collect_files_recursive(&backup_set).map_err(ApiError::from)?;
    let restore_backup_id = create_pre_restore_backup(&state.paths, &backup_set, &files)?;
    let restored_files = restore_backup_files(&state.paths.root, &backup_set, &files)?;

    Ok(Json(ApiBackupRestoreResult {
        status: "ok".to_string(),
        message: format!(
            "Restored {} file(s) from backup '{}' after creating restore backup '{}'",
            restored_files, id, restore_backup_id
        ),
        backup_id: id,
        restore_backup_id,
        restored_files,
    }))
}

fn backup_set_to_api(set: &PathBuf) -> std::result::Result<ApiBackup, ApiError> {
    let files = macc_core::domain::backups::collect_files_recursive(set).map_err(ApiError::from)?;
    let mut entries = Vec::with_capacity(files.len());
    let mut total_size = 0u64;
    for file in files {
        let rel = file.strip_prefix(set).map_err(|err| {
            ApiError::from(macc_core::MaccError::Validation(format!(
                "failed to compute backup relative path for {}: {}",
                file.display(),
                err
            )))
        })?;
        let metadata = fs::metadata(&file).map_err(|err| macc_core::MaccError::Io {
            path: file.to_string_lossy().into_owned(),
            action: "stat backup file".into(),
            source: err,
        })?;
        total_size += metadata.len();
        entries.push(ApiBackupFile {
            path: rel.to_string_lossy().into_owned(),
            size: metadata.len(),
        });
    }

    let id = set
        .file_name()
        .and_then(|value| value.to_str())
        .unwrap_or_default()
        .to_string();
    Ok(ApiBackup {
        timestamp: id.clone(),
        id,
        files: entries.len(),
        entries,
        total_size,
        path: set.to_string_lossy().into_owned(),
        user_scope: false,
    })
}

fn create_pre_restore_backup(
    paths: &macc_core::ProjectPaths,
    backup_set: &StdPath,
    files: &[PathBuf],
) -> std::result::Result<String, ApiError> {
    let restore_backup_id = Local::now().format("%Y%m%d-%H%M%S").to_string();
    let restore_root = paths.backups_dir.join(&restore_backup_id);
    fs::create_dir_all(&restore_root).map_err(|err| macc_core::MaccError::Io {
        path: restore_root.to_string_lossy().into_owned(),
        action: "create restore backup directory".into(),
        source: err,
    })?;

    for file in files {
        let rel = file.strip_prefix(backup_set).map_err(|err| {
            ApiError::from(macc_core::MaccError::Validation(format!(
                "failed to compute restore backup relative path for {}: {}",
                file.display(),
                err
            )))
        })?;
        let current_path = paths.root.join(rel);
        if !current_path.is_file() {
            continue;
        }
        let backup_path = restore_root.join(rel);
        if let Some(parent) = backup_path.parent() {
            fs::create_dir_all(parent).map_err(|err| macc_core::MaccError::Io {
                path: parent.to_string_lossy().into_owned(),
                action: "create restore backup parent directory".into(),
                source: err,
            })?;
        }
        fs::copy(&current_path, &backup_path).map_err(|err| macc_core::MaccError::Io {
            path: current_path.to_string_lossy().into_owned(),
            action: format!("copy to restore backup {}", backup_path.display()),
            source: err,
        })?;
    }

    Ok(restore_backup_id)
}

fn restore_backup_files(
    project_root: &StdPath,
    backup_set: &StdPath,
    files: &[PathBuf],
) -> std::result::Result<usize, ApiError> {
    let mut restored = 0usize;
    for file in files {
        let rel = file.strip_prefix(backup_set).map_err(|err| {
            ApiError::from(macc_core::MaccError::Validation(format!(
                "failed to compute restore relative path for {}: {}",
                file.display(),
                err
            )))
        })?;
        let destination = project_root.join(rel);
        if let Some(parent) = destination.parent() {
            fs::create_dir_all(parent).map_err(|err| macc_core::MaccError::Io {
                path: parent.to_string_lossy().into_owned(),
                action: "create restore parent directory".into(),
                source: err,
            })?;
        }
        fs::copy(file, &destination).map_err(|err| macc_core::MaccError::Io {
            path: file.to_string_lossy().into_owned(),
            action: format!("restore to {}", destination.display()),
            source: err,
        })?;
        restored += 1;
    }
    Ok(restored)
}
