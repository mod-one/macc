use crate::commands::web::errors::ApiError;
use crate::commands::web::types::{ApiPrdResponse, ApiPrdTask, ApiPrdUpdateRequest};
use crate::commands::web::WebState;
use axum::{
    extract::{Query, State},
    Json,
};
use macc_core::MaccError;
use serde::Deserialize;
use std::collections::{BTreeMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Deserialize)]
pub(crate) struct PrdQuery {
    path: Option<String>,
}

fn resolve_prd_path(state: &WebState, path_param: Option<&str>) -> Result<PathBuf, ApiError> {
    let mut target = state.paths.root.clone();
    if let Some(p) = path_param {
        let p_path = Path::new(p);
        if p_path.is_absolute() {
            return Err(
                MaccError::Validation("path parameter must be relative".to_string()).into(),
            );
        }
        target.push(p_path);
    }

    // If the path points to a directory, append prd.json or worktree.prd.json
    // But typical usage is either path to file or we just append prd.json.
    // The spec says "Support an optional ?path= query param for worktree-specific PRDs."
    // Let's assume the path param points to the file directly if it ends with .json,
    // otherwise append prd.json. Wait, it's safer to just check if it's a dir.
    // Actually, worktree PRD is usually named "worktree.prd.json" inside the worktree dir,
    // or just "prd.json" inside the project dir.
    // If we just use the provided path exactly when provided:
    if !target.to_string_lossy().ends_with(".json") {
        if target == state.paths.root {
            target.push("prd.json");
        } else {
            // For worktree path, usually we append worktree.prd.json
            target.push("worktree.prd.json");
        }
    }

    Ok(target)
}

pub(crate) async fn get_prd_handler(
    State(state): State<WebState>,
    Query(query): Query<PrdQuery>,
) -> Result<Json<ApiPrdResponse>, ApiError> {
    let prd_path = resolve_prd_path(&state, query.path.as_deref())?;

    let content = fs::read_to_string(&prd_path).map_err(|e| {
        if e.kind() == std::io::ErrorKind::NotFound {
            MaccError::Io {
                path: prd_path.to_string_lossy().to_string(),
                action: "read PRD".into(),
                source: e,
            }
        } else {
            MaccError::Io {
                path: prd_path.to_string_lossy().to_string(),
                action: "read PRD".into(),
                source: e,
            }
        }
    })?;

    let mut parsed: serde_json::Value = serde_json::from_str(&content)
        .map_err(|e| MaccError::Validation(format!("Failed to parse PRD JSON: {}", e)))?;

    let tasks_val = parsed
        .as_object_mut()
        .and_then(|obj| obj.remove("tasks"))
        .unwrap_or_else(|| serde_json::json!([]));

    let tasks: Vec<ApiPrdTask> = serde_json::from_value(tasks_val)
        .map_err(|e| MaccError::Validation(format!("Invalid PRD tasks format: {}", e)))?;

    let metadata: BTreeMap<String, serde_json::Value> = serde_json::from_value(parsed)
        .map_err(|e| MaccError::Validation(format!("Invalid PRD metadata format: {}", e)))?;

    Ok(Json(ApiPrdResponse { tasks, metadata }))
}

pub(crate) async fn update_prd_handler(
    State(state): State<WebState>,
    Query(query): Query<PrdQuery>,
    Json(payload): Json<ApiPrdUpdateRequest>,
) -> Result<Json<ApiPrdResponse>, ApiError> {
    let prd_path = resolve_prd_path(&state, query.path.as_deref())?;

    // Validate JSON structure (unique task IDs, etc.)
    let mut seen_ids = HashSet::new();
    for task in &payload.tasks {
        if task.id.trim().is_empty() {
            return Err(MaccError::Validation("Task ID cannot be empty".to_string()).into());
        }
        if !seen_ids.insert(&task.id) {
            return Err(MaccError::Validation(format!("Duplicate task ID: {}", task.id)).into());
        }
    }

    // Reconstruct full JSON
    let mut full_json = serde_json::to_value(&payload.metadata)
        .map_err(|e| MaccError::Validation(format!("Failed to serialize metadata: {}", e)))?;

    let tasks_json = serde_json::to_value(&payload.tasks)
        .map_err(|e| MaccError::Validation(format!("Failed to serialize tasks: {}", e)))?;

    if let Some(obj) = full_json.as_object_mut() {
        obj.insert("tasks".to_string(), tasks_json);
    } else {
        return Err(MaccError::Validation("Metadata must be an object".to_string()).into());
    }

    let new_content = serde_json::to_string_pretty(&full_json)
        .map_err(|e| MaccError::Validation(format!("Failed to serialize new PRD: {}", e)))?;

    // Create backup
    if prd_path.exists() {
        let backups_dir = state.paths.root.join(".macc").join("backups");
        fs::create_dir_all(&backups_dir).map_err(|e| MaccError::Io {
            path: backups_dir.to_string_lossy().to_string(),
            action: "create backups directory".into(),
            source: e,
        })?;

        let timestamp = chrono::Utc::now().format("%Y%m%d%H%M%S").to_string();
        let backup_filename = format!(
            "{}.{}.bak",
            prd_path.file_name().unwrap_or_default().to_string_lossy(),
            timestamp
        );
        let backup_path = backups_dir.join(backup_filename);

        fs::copy(&prd_path, &backup_path).map_err(|e| MaccError::Io {
            path: prd_path.to_string_lossy().to_string(),
            action: "backup PRD file".into(),
            source: e,
        })?;
    } else {
        // Ensure parent directory exists for writing
        if let Some(parent) = prd_path.parent() {
            fs::create_dir_all(parent).map_err(|e| MaccError::Io {
                path: parent.to_string_lossy().to_string(),
                action: "create parent directory for PRD".into(),
                source: e,
            })?;
        }
    }

    // Write new content
    fs::write(&prd_path, new_content).map_err(|e| MaccError::Io {
        path: prd_path.to_string_lossy().to_string(),
        action: "write PRD file".into(),
        source: e,
    })?;

    Ok(Json(ApiPrdResponse {
        tasks: payload.tasks,
        metadata: payload.metadata,
    }))
}
