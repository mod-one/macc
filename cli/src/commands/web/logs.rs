use super::errors::ApiError;
use super::types::{ApiLogContent, ApiLogFile};
use super::WebState;
use axum::extract::{Path, Query, State};
use axum::Json;
use chrono::{DateTime, SecondsFormat, Utc};
use serde::Deserialize;
use serde_json::json;
use std::fs;
use std::path::{Component, Path as StdPath, PathBuf};

const CATEGORY_COORDINATOR: &str = "coordinator";
const CATEGORY_PERFORMER: &str = "performer";
const DEFAULT_LIMIT: usize = 200;
const MAX_LIMIT: usize = 1_000;

#[derive(Debug, Deserialize, Default)]
pub(super) struct ReadLogQuery {
    offset: Option<usize>,
    limit: Option<usize>,
    search: Option<String>,
}

pub(super) async fn list_logs_handler(
    State(state): State<WebState>,
) -> std::result::Result<Json<Vec<ApiLogFile>>, ApiError> {
    maybe_aggregate_performer_logs(&state);

    let log_root = state.paths.macc_dir.join("log");
    if !log_root.exists() {
        return Ok(Json(Vec::new()));
    }
    let canonical_root = fs::canonicalize(&log_root).map_err(|err| macc_core::MaccError::Io {
        path: log_root.to_string_lossy().into_owned(),
        action: "canonicalize web log root".into(),
        source: err,
    })?;
    let mut files = Vec::new();
    collect_category_logs(&log_root, &canonical_root, CATEGORY_COORDINATOR, &mut files)?;
    collect_category_logs(&log_root, &canonical_root, CATEGORY_PERFORMER, &mut files)?;
    files.sort_by(|left, right| {
        left.category
            .cmp(&right.category)
            .then_with(|| left.path.cmp(&right.path))
    });

    Ok(Json(files))
}

pub(super) async fn read_log_handler(
    State(state): State<WebState>,
    Path(path): Path<String>,
    Query(query): Query<ReadLogQuery>,
) -> std::result::Result<Json<ApiLogContent>, ApiError> {
    let (requested_path, file_path) = resolve_log_path(&state, &path)?;
    let content = fs::read_to_string(&file_path).map_err(|err| macc_core::MaccError::Io {
        path: file_path.to_string_lossy().into_owned(),
        action: "read web log file".into(),
        source: err,
    })?;

    let offset = query.offset.unwrap_or(0);
    let limit = query.limit.unwrap_or(DEFAULT_LIMIT).min(MAX_LIMIT);
    let search = query.search.as_deref().filter(|value| !value.is_empty());

    let filtered = content
        .lines()
        .filter(|line| search.is_none_or(|needle| line.contains(needle)))
        .map(str::to_string)
        .collect::<Vec<_>>();
    let total = filtered.len();
    let start = offset.min(total);
    let end = start.saturating_add(limit).min(total);
    let lines = filtered[start..end].to_vec();

    Ok(Json(ApiLogContent {
        path: requested_path,
        lines,
        total,
        has_more: end < total,
    }))
}

fn maybe_aggregate_performer_logs(state: &WebState) {
    if let Err(err) = macc_core::coordinator::logs::aggregate_performer_logs(&state.paths.root) {
        tracing::warn!(
            "performer log aggregation failed for web logs endpoint: {}",
            err
        );
    }
}

fn collect_category_logs(
    log_root: &StdPath,
    canonical_root: &StdPath,
    category: &str,
    out: &mut Vec<ApiLogFile>,
) -> std::result::Result<(), ApiError> {
    let category_root = log_root.join(category);
    if !category_root.exists() {
        return Ok(());
    }

    collect_logs_recursive(log_root, canonical_root, &category_root, category, out)
}

fn collect_logs_recursive(
    log_root: &StdPath,
    canonical_root: &StdPath,
    current_dir: &StdPath,
    category: &str,
    out: &mut Vec<ApiLogFile>,
) -> std::result::Result<(), ApiError> {
    for entry in fs::read_dir(current_dir).map_err(|err| macc_core::MaccError::Io {
        path: current_dir.to_string_lossy().into_owned(),
        action: "read web log directory".into(),
        source: err,
    })? {
        let entry = entry.map_err(|err| macc_core::MaccError::Io {
            path: current_dir.to_string_lossy().into_owned(),
            action: "iterate web log directory".into(),
            source: err,
        })?;
        let path = entry.path();
        if path.is_dir() {
            let canonical_dir =
                fs::canonicalize(&path).map_err(|err| macc_core::MaccError::Io {
                    path: path.to_string_lossy().into_owned(),
                    action: "canonicalize web log directory".into(),
                    source: err,
                })?;
            if !canonical_dir.starts_with(canonical_root) {
                continue;
            }

            collect_logs_recursive(log_root, canonical_root, &path, category, out)?;
            continue;
        }
        if !path.is_file() {
            continue;
        }
        let extension = path
            .extension()
            .and_then(|value| value.to_str())
            .unwrap_or("");
        if !matches!(extension, "jsonl" | "log" | "txt" | "md") {
            continue;
        }

        let canonical_path = fs::canonicalize(&path).map_err(|err| macc_core::MaccError::Io {
            path: path.to_string_lossy().into_owned(),
            action: "canonicalize web log file".into(),
            source: err,
        })?;
        if !canonical_path.starts_with(&canonical_root) {
            continue;
        }

        let relative = path.strip_prefix(log_root).map_err(|err| {
            macc_core::MaccError::Validation(format!(
                "failed to compute web log relative path for {}: {}",
                path.display(),
                err
            ))
        })?;
        let metadata = fs::metadata(&path).map_err(|err| macc_core::MaccError::Io {
            path: path.to_string_lossy().into_owned(),
            action: "stat web log file".into(),
            source: err,
        })?;

        out.push(ApiLogFile {
            path: relative.to_string_lossy().replace('\\', "/"),
            category: category.to_string(),
            size: metadata.len(),
            modified: metadata.modified().ok().map(format_system_time_rfc3339),
        });
    }

    Ok(())
}

fn resolve_log_path(
    state: &WebState,
    raw_path: &str,
) -> std::result::Result<(String, PathBuf), ApiError> {
    if raw_path.trim().is_empty() {
        return Err(ApiError::log_validation(
            "log path must not be empty",
            Some(json!({ "path": raw_path })),
        ));
    }

    let relative = sanitize_relative_log_path(raw_path)?;
    let display_path = relative.to_string_lossy().replace('\\', "/");
    let target = state.paths.macc_dir.join("log").join(&relative);

    if !target.is_file() {
        return Err(ApiError::log_not_found(
            format!("log '{}' was not found", display_path),
            Some(json!({ "path": display_path })),
        ));
    }

    let canonical_root = fs::canonicalize(state.paths.macc_dir.join("log")).map_err(|err| {
        macc_core::MaccError::Io {
            path: state
                .paths
                .macc_dir
                .join("log")
                .to_string_lossy()
                .into_owned(),
            action: "canonicalize web log root".into(),
            source: err,
        }
    })?;
    let canonical_target = fs::canonicalize(&target).map_err(|err| macc_core::MaccError::Io {
        path: target.to_string_lossy().into_owned(),
        action: "canonicalize requested web log path".into(),
        source: err,
    })?;
    if !canonical_target.starts_with(&canonical_root) {
        return Err(ApiError::log_validation(
            "log path must stay under .macc/log",
            Some(json!({ "path": display_path })),
        ));
    }

    Ok((display_path, canonical_target))
}

fn sanitize_relative_log_path(raw_path: &str) -> std::result::Result<PathBuf, ApiError> {
    let path = StdPath::new(raw_path);
    if path.is_absolute() {
        return Err(ApiError::log_validation(
            "log path must be relative",
            Some(json!({ "path": raw_path })),
        ));
    }

    let mut cleaned = PathBuf::new();
    for component in path.components() {
        match component {
            Component::Normal(part) => cleaned.push(part),
            Component::CurDir => {}
            Component::ParentDir | Component::RootDir | Component::Prefix(_) => {
                return Err(ApiError::log_validation(
                    "log path must not contain parent traversal or absolute prefixes",
                    Some(json!({ "path": raw_path })),
                ));
            }
        }
    }

    let category = cleaned
        .components()
        .next()
        .and_then(|component| match component {
            Component::Normal(part) => part.to_str(),
            _ => None,
        })
        .unwrap_or_default();
    if cleaned.as_os_str().is_empty()
        || (category != CATEGORY_COORDINATOR && category != CATEGORY_PERFORMER)
    {
        return Err(ApiError::log_validation(
            "log path must target coordinator or performer logs",
            Some(json!({ "path": raw_path })),
        ));
    }

    Ok(cleaned)
}

fn format_system_time_rfc3339(value: std::time::SystemTime) -> String {
    DateTime::<Utc>::from(value).to_rfc3339_opts(SecondsFormat::Secs, true)
}
