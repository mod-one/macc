use super::errors::ApiError;
use super::types::{ApiWorktree, ApiWorktreeCreateRequest};
use super::WebState;
use async_stream::stream;
use axum::body::Bytes;
use axum::extract::{Path, Query, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::sse::{Event, Sse};
use axum::Json;
use macc_core::service::worktree::{
    canonicalize_path_fallback, delete_branch, git_worktree_is_dirty, load_worktree_session_labels,
    resolve_worktree_path, WorktreeFetchMaterializer, WorktreeSetupOptions,
};
use macc_core::{MaccError, Result, WorktreeCreateResult, WorktreeCreateSpec, WorktreeEntry};
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::collections::{BTreeMap, BTreeSet, HashSet};
use std::convert::Infallible;
use std::path::{Path as StdPath, PathBuf};
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

const WORKTREE_LOG_POLL_INTERVAL: Duration = Duration::from_millis(250);
const WORKTREE_LOG_HEARTBEAT_INTERVAL: Duration = Duration::from_secs(15);

struct WebFetchMaterializer;

impl WorktreeFetchMaterializer for WebFetchMaterializer {
    fn materialize_fetch_units(
        &self,
        paths: &macc_core::ProjectPaths,
        units: Vec<macc_core::resolve::FetchUnit>,
        quiet: bool,
        offline: bool,
    ) -> Result<Vec<macc_core::resolve::MaterializedFetchUnit>> {
        macc_adapter_shared::fetch::materialize_fetch_units(paths, units, quiet, offline)
    }
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct WorktreeRunResponse {
    status: &'static str,
    worktree_id: String,
    path: String,
}

#[derive(Clone, Debug)]
struct WorktreeLogTarget {
    worktree_id: String,
    log_path: PathBuf,
}

#[derive(Debug, Serialize)]
struct WorktreeLogEventData {
    timestamp: String,
    level: String,
    message: String,
}

#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct DeleteWorktreeQuery {
    confirmed: Option<bool>,
    force: Option<bool>,
    #[serde(alias = "remove_branch")]
    remove_branch: Option<bool>,
    #[serde(alias = "force_confirmed")]
    force_confirmed: Option<bool>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
struct DeleteWorktreeBody {
    confirmed: Option<bool>,
    force: Option<bool>,
    #[serde(alias = "remove_branch")]
    remove_branch: Option<bool>,
    #[serde(alias = "force_confirmed")]
    force_confirmed: Option<bool>,
}

pub(super) async fn list_worktrees_handler(
    State(state): State<WebState>,
) -> std::result::Result<Json<Vec<ApiWorktree>>, ApiError> {
    let entries = state
        .engine
        .list_worktrees(&state.paths.root)
        .map_err(ApiError::from)?;
    let session_labels =
        load_worktree_session_labels(Some(&state.paths)).map_err(ApiError::from)?;
    let root = canonicalize_path_fallback(&state.paths.root);
    let mut worktrees = Vec::new();

    for entry in entries {
        if canonicalize_path_fallback(&entry.path) == root {
            continue;
        }
        worktrees.push(map_entry_to_api(&entry, &session_labels)?);
    }

    Ok(Json(worktrees))
}

pub(super) async fn create_worktree_handler(
    State(state): State<WebState>,
    body: Bytes,
) -> std::result::Result<Json<Vec<ApiWorktree>>, ApiError> {
    let request: ApiWorktreeCreateRequest = serde_json::from_slice(&body).map_err(|err| {
        ApiError::from(MaccError::Validation(format!(
            "Invalid worktree create request body: {}",
            err
        )))
    })?;
    validate_create_request(&request)?;

    let spec = WorktreeCreateSpec {
        slug: request.slug.trim().to_string(),
        tool: request.tool.trim().to_string(),
        count: request.count,
        base: request.base.trim().to_string(),
        dir: PathBuf::from(".macc/worktree"),
        scope: normalize_optional(request.scope),
        feature: normalize_optional(request.feature),
    };

    let created = state
        .engine
        .worktree_setup_workflow(
            &WebFetchMaterializer,
            &state.paths.root,
            &spec,
            WorktreeSetupOptions {
                skip_apply: request.skip_apply.unwrap_or(false),
                allow_user_scope: request.allow_user_scope.unwrap_or(false),
            },
        )
        .map_err(ApiError::from)?;

    let created_paths = created
        .iter()
        .map(|entry| canonicalize_path_fallback(&entry.path))
        .collect::<BTreeSet<_>>();
    let entries = state
        .engine
        .list_worktrees(&state.paths.root)
        .map_err(ApiError::from)?;
    let session_labels =
        load_worktree_session_labels(Some(&state.paths)).map_err(ApiError::from)?;
    let by_path = entries
        .into_iter()
        .map(|entry| (canonicalize_path_fallback(&entry.path), entry))
        .collect::<BTreeMap<_, _>>();

    let mut response = Vec::with_capacity(created.len());
    for created_entry in created {
        let path = canonicalize_path_fallback(&created_entry.path);
        if !created_paths.contains(&path) {
            continue;
        }
        if let Some(entry) = by_path.get(&path) {
            response.push(map_entry_to_api(entry, &session_labels)?);
        } else {
            response.push(map_created_to_api(&created_entry, &session_labels)?);
        }
    }

    Ok(Json(response))
}

pub(super) async fn delete_worktree_handler(
    State(state): State<WebState>,
    Path(id): Path<String>,
    Query(query): Query<DeleteWorktreeQuery>,
    body: Bytes,
) -> std::result::Result<Json<serde_json::Value>, ApiError> {
    let body = if body.is_empty() {
        DeleteWorktreeBody::default()
    } else {
        serde_json::from_slice(&body).map_err(|err| {
            ApiError::from(MaccError::Validation(format!(
                "Invalid worktree delete request body: {}",
                err
            )))
        })?
    };
    let options = merge_delete_options(query, body);

    if !options.confirmed.unwrap_or(false) {
        return Err(ApiError::confirmation_required(
            "Confirmation required before deleting worktree",
            Some(json!({ "worktreeId": id, "force": options.force.unwrap_or(false) })),
        ));
    }
    if options.force.unwrap_or(false) && !options.force_confirmed.unwrap_or(false) {
        return Err(ApiError::confirmation_required(
            "Force delete requires a second confirmation",
            Some(json!({ "worktreeId": id, "force": true, "forceConfirmed": false })),
        ));
    }

    let worktree_path = resolve_worktree_path(&state.paths.root, &id).map_err(ApiError::from)?;
    let entries = state
        .engine
        .list_worktrees(&state.paths.root)
        .map_err(ApiError::from)?;
    let root = canonicalize_path_fallback(&state.paths.root);
    let target = canonicalize_path_fallback(&worktree_path);
    if target == root {
        return Err(ApiError::worktree_conflict(
            "The repository root worktree cannot be deleted through this endpoint",
            Some(json!({ "worktreeId": id, "path": worktree_path })),
        ));
    }

    let matched = entries
        .iter()
        .find(|entry| canonicalize_path_fallback(&entry.path) == target)
        .ok_or_else(|| {
            ApiError::worktree_not_found(
                format!("worktree '{}' was not found", id),
                Some(json!({ "worktreeId": id, "path": worktree_path })),
            )
        })?;

    state
        .engine
        .remove_worktree(
            &state.paths.root,
            &matched.path,
            options.force.unwrap_or(false),
        )
        .map_err(|err| map_delete_error(err, &id, &matched.path, "worktree_remove"))?;

    if options.remove_branch.unwrap_or(false) {
        delete_branch(
            &state.paths.root,
            matched.branch.as_deref(),
            options.force.unwrap_or(false),
        )
        .map_err(|err| map_delete_error(err, &id, &matched.path, "branch_delete"))?;
    }

    Ok(Json(json!({
        "status": "ok",
        "message": format!("Removed worktree '{}'", id),
        "id": id,
        "path": matched.path,
        "force": options.force.unwrap_or(false),
        "removeBranch": options.remove_branch.unwrap_or(false),
    })))
}

pub(super) async fn run_worktree_handler(
    State(state): State<WebState>,
    Path(id): Path<String>,
) -> std::result::Result<(StatusCode, Json<WorktreeRunResponse>), ApiError> {
    let worktree_path = resolve_existing_worktree_path(&state, &id)?;
    let worktree_key = canonicalize_path_fallback(&worktree_path);

    {
        let mut active = active_worktree_runs().lock().expect("active worktree runs");
        if !active.insert(worktree_key.clone()) {
            return Err(ApiError::worktree_conflict(
                format!("worktree '{}' is already running", id),
                Some(json!({ "worktreeId": id, "path": worktree_path })),
            ));
        }
    }

    let state_for_run = state.clone();
    let id_for_run = id.clone();
    tokio::task::spawn_blocking(move || {
        let outcome = state_for_run
            .engine
            .worktree_run_task(&state_for_run.paths, &id_for_run);
        if let Err(err) = &outcome {
            tracing::warn!("web worktree run failed for {}: {}", id_for_run, err);
        }
        active_worktree_runs()
            .lock()
            .expect("active worktree runs")
            .remove(&worktree_key);
    });

    Ok((
        StatusCode::ACCEPTED,
        Json(WorktreeRunResponse {
            status: "started",
            worktree_id: id,
            path: worktree_path.to_string_lossy().into_owned(),
        }),
    ))
}

pub(super) async fn worktree_logs_handler(
    State(state): State<WebState>,
    Path(id): Path<String>,
    headers: HeaderMap,
) -> std::result::Result<
    Sse<impl tokio_stream::Stream<Item = std::result::Result<Event, Infallible>>>,
    ApiError,
> {
    let worktree_path = resolve_existing_worktree_path(&state, &id)?;
    let target = resolve_latest_worktree_log_target(&state, &id, &worktree_path)?;
    let last_event_id = headers
        .get("last-event-id")
        .and_then(|value| value.to_str().ok())
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned);

    Ok(Sse::new(worktree_log_stream(
        target,
        last_event_id,
        WORKTREE_LOG_POLL_INTERVAL,
        WORKTREE_LOG_HEARTBEAT_INTERVAL,
    )))
}

fn validate_create_request(
    request: &ApiWorktreeCreateRequest,
) -> std::result::Result<(), ApiError> {
    if request.slug.trim().is_empty() {
        return Err(ApiError::from(MaccError::Validation(
            "worktree slug must not be empty".to_string(),
        )));
    }
    if request.tool.trim().is_empty() {
        return Err(ApiError::from(MaccError::Validation(
            "worktree tool must not be empty".to_string(),
        )));
    }
    if request.base.trim().is_empty() {
        return Err(ApiError::from(MaccError::Validation(
            "worktree base must not be empty".to_string(),
        )));
    }
    if request.count == 0 {
        return Err(ApiError::from(MaccError::Validation(
            "worktree count must be >= 1".to_string(),
        )));
    }
    Ok(())
}

fn active_worktree_runs() -> &'static Mutex<HashSet<PathBuf>> {
    static ACTIVE: OnceLock<Mutex<HashSet<PathBuf>>> = OnceLock::new();
    ACTIVE.get_or_init(|| Mutex::new(HashSet::new()))
}

fn resolve_existing_worktree_path(
    state: &WebState,
    id: &str,
) -> std::result::Result<PathBuf, ApiError> {
    let worktree_path = resolve_worktree_path(&state.paths.root, id).map_err(ApiError::from)?;
    let target = canonicalize_path_fallback(&worktree_path);
    let root = canonicalize_path_fallback(&state.paths.root);
    let entries = state
        .engine
        .list_worktrees(&state.paths.root)
        .map_err(ApiError::from)?;

    if target == root {
        return Err(ApiError::worktree_not_found(
            format!("worktree '{}' was not found", id),
            Some(json!({ "worktreeId": id, "path": worktree_path })),
        ));
    }

    entries
        .iter()
        .find(|entry| canonicalize_path_fallback(&entry.path) == target)
        .map(|entry| entry.path.clone())
        .ok_or_else(|| {
            ApiError::worktree_not_found(
                format!("worktree '{}' was not found", id),
                Some(json!({ "worktreeId": id, "path": worktree_path })),
            )
        })
}

fn resolve_latest_worktree_log_target(
    state: &WebState,
    worktree_id: &str,
    worktree_path: &StdPath,
) -> std::result::Result<WorktreeLogTarget, ApiError> {
    let worktree_name = worktree_path
        .file_name()
        .and_then(|value| value.to_str())
        .unwrap_or(worktree_id);
    let direct_dir = state
        .paths
        .macc_dir
        .join("log")
        .join("performer")
        .join(worktree_name);
    let worktree_local_dir = worktree_path.join(".macc").join("log").join("performer");
    let aggregated_dir = state.paths.macc_dir.join("log").join("performer");

    let latest = if let Some(path) = latest_log_in_dir(&direct_dir)? {
        Some(path)
    } else if let Some(path) = latest_log_in_dir(&worktree_local_dir)? {
        Some(path)
    } else {
        latest_aggregated_log(&aggregated_dir, worktree_name)?
    };

    latest
        .map(|log_path| WorktreeLogTarget {
            worktree_id: worktree_id.to_string(),
            log_path,
        })
        .ok_or_else(|| {
            ApiError::log_not_found(
                format!("performer log for worktree '{}' was not found", worktree_id),
                Some(json!({ "worktreeId": worktree_id, "path": worktree_path })),
            )
        })
}

fn latest_log_in_dir(dir: &StdPath) -> std::result::Result<Option<PathBuf>, ApiError> {
    if !dir.is_dir() {
        return Ok(None);
    }

    let mut latest: Option<(SystemTime, PathBuf)> = None;
    for entry in std::fs::read_dir(dir).map_err(|err| macc_core::MaccError::Io {
        path: dir.to_string_lossy().into_owned(),
        action: "read worktree performer log directory".into(),
        source: err,
    })? {
        let entry = entry.map_err(|err| macc_core::MaccError::Io {
            path: dir.to_string_lossy().into_owned(),
            action: "iterate worktree performer log directory".into(),
            source: err,
        })?;
        let path = entry.path();
        if !path.is_file() || !is_log_file(&path) {
            continue;
        }
        let modified = entry
            .metadata()
            .map_err(|err| macc_core::MaccError::Io {
                path: path.to_string_lossy().into_owned(),
                action: "stat worktree performer log".into(),
                source: err,
            })?
            .modified()
            .unwrap_or(UNIX_EPOCH);
        if latest
            .as_ref()
            .map(|(current, _)| modified > *current)
            .unwrap_or(true)
        {
            latest = Some((modified, path));
        }
    }

    Ok(latest.map(|(_, path)| path))
}

fn latest_aggregated_log(
    dir: &StdPath,
    worktree_name: &str,
) -> std::result::Result<Option<PathBuf>, ApiError> {
    if !dir.is_dir() {
        return Ok(None);
    }

    let prefix = format!("{worktree_name}--");
    let mut latest: Option<(SystemTime, PathBuf)> = None;
    for entry in std::fs::read_dir(dir).map_err(|err| macc_core::MaccError::Io {
        path: dir.to_string_lossy().into_owned(),
        action: "read aggregated performer log directory".into(),
        source: err,
    })? {
        let entry = entry.map_err(|err| macc_core::MaccError::Io {
            path: dir.to_string_lossy().into_owned(),
            action: "iterate aggregated performer log directory".into(),
            source: err,
        })?;
        let path = entry.path();
        let file_name = path
            .file_name()
            .and_then(|value| value.to_str())
            .unwrap_or("");
        if !path.is_file() || !file_name.starts_with(&prefix) || !is_log_file(&path) {
            continue;
        }
        let modified = entry
            .metadata()
            .map_err(|err| macc_core::MaccError::Io {
                path: path.to_string_lossy().into_owned(),
                action: "stat aggregated performer log".into(),
                source: err,
            })?
            .modified()
            .unwrap_or(UNIX_EPOCH);
        if latest
            .as_ref()
            .map(|(current, _)| modified > *current)
            .unwrap_or(true)
        {
            latest = Some((modified, path));
        }
    }

    Ok(latest.map(|(_, path)| path))
}

fn worktree_log_stream(
    target: WorktreeLogTarget,
    last_event_id: Option<String>,
    poll_interval: Duration,
    heartbeat_interval: Duration,
) -> impl tokio_stream::Stream<Item = std::result::Result<Event, Infallible>> {
    stream! {
        let mut delivered_lines = parse_worktree_log_cursor(last_event_id.as_deref()).unwrap_or_default();
        let mut poll_tick = tokio::time::interval(poll_interval);
        let mut heartbeat_tick = tokio::time::interval(heartbeat_interval);
        poll_tick.tick().await;
        heartbeat_tick.tick().await;

        loop {
            tokio::select! {
                _ = poll_tick.tick() => {
                    match tokio::fs::read_to_string(&target.log_path).await {
                        Ok(content) => {
                            let lines = content.lines().collect::<Vec<_>>();
                            if lines.len() < delivered_lines {
                                delivered_lines = 0;
                            }
                            while delivered_lines < lines.len() {
                                let line_number = delivered_lines + 1;
                                let line = lines[delivered_lines].to_string();
                                delivered_lines = line_number;
                                yield Ok(build_worktree_log_event(&target.worktree_id, line_number, &line));
                            }
                        }
                        Err(err) => {
                            tracing::warn!(
                                "failed to read performer log stream for {} at {}: {}",
                                target.worktree_id,
                                target.log_path.display(),
                                err
                            );
                        }
                    }
                }
                _ = heartbeat_tick.tick() => {
                    yield Ok(build_worktree_log_heartbeat(delivered_lines));
                }
            }
        }
    }
}

fn parse_worktree_log_cursor(last_event_id: Option<&str>) -> Option<usize> {
    last_event_id.and_then(|value| {
        if let Some(raw) = value.strip_prefix("hb-") {
            raw.split('-').next()?.parse::<usize>().ok()
        } else {
            value.parse::<usize>().ok()
        }
    })
}

fn build_worktree_log_event(worktree_id: &str, line_number: usize, line: &str) -> Event {
    let payload = WorktreeLogEventData {
        timestamp: extract_log_timestamp(line).unwrap_or_else(|| {
            chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Secs, true)
        }),
        level: infer_log_level(line).to_string(),
        message: line.to_string(),
    };

    Event::default()
        .id(line_number.to_string())
        .event("log_line")
        .json_data(json!({
            "worktree_id": worktree_id,
            "line": line_number,
            "timestamp": payload.timestamp,
            "level": payload.level,
            "message": payload.message,
        }))
        .expect("serialize worktree log payload")
}

fn build_worktree_log_heartbeat(line_number: usize) -> Event {
    let heartbeat_id = format!("hb-{}-{}", line_number, unix_timestamp_millis());
    Event::default()
        .id(heartbeat_id.clone())
        .event("heartbeat")
        .json_data(json!({
            "event_id": heartbeat_id,
            "type": "heartbeat",
            "status": "ok",
            "line": line_number,
            "timestamp": chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Secs, true),
        }))
        .expect("serialize worktree log heartbeat")
}

fn extract_log_timestamp(line: &str) -> Option<String> {
    let candidate = line.split_whitespace().next()?;
    chrono::DateTime::parse_from_rfc3339(candidate)
        .ok()
        .map(|value| value.to_rfc3339_opts(chrono::SecondsFormat::Secs, true))
}

fn infer_log_level(line: &str) -> &'static str {
    let lowered = line.to_ascii_lowercase();
    if lowered.contains("error") || lowered.contains("failed") {
        "error"
    } else if lowered.contains("warn") {
        "warn"
    } else if lowered.contains("debug") {
        "debug"
    } else if lowered.contains("trace") {
        "trace"
    } else {
        "info"
    }
}

fn is_log_file(path: &StdPath) -> bool {
    matches!(
        path.extension().and_then(|value| value.to_str()),
        Some("md" | "log" | "txt" | "jsonl")
    )
}

fn unix_timestamp_millis() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
}

fn normalize_optional(value: Option<String>) -> Option<String> {
    value
        .map(|raw| raw.trim().to_string())
        .filter(|value| !value.is_empty())
}

fn map_entry_to_api(
    entry: &WorktreeEntry,
    session_labels: &BTreeMap<PathBuf, String>,
) -> std::result::Result<ApiWorktree, ApiError> {
    let metadata = macc_core::read_worktree_metadata(&entry.path).map_err(ApiError::from)?;
    let path_key = canonicalize_path_fallback(&entry.path);
    let dirty = git_worktree_is_dirty(&entry.path).map_err(ApiError::from)?;
    let (id, slug, branch, tool, base_branch, scope, feature) = if let Some(metadata) = metadata {
        (
            metadata.id.clone(),
            derive_slug_from_id(&metadata.id),
            Some(metadata.branch),
            Some(metadata.tool),
            Some(metadata.base),
            metadata.scope,
            metadata.feature,
        )
    } else {
        (
            fallback_worktree_id(&entry.path),
            None,
            entry.branch.as_deref().map(normalize_branch_name),
            None,
            None,
            None,
            None,
        )
    };

    Ok(ApiWorktree {
        id,
        slug,
        branch,
        tool,
        status: Some(derive_worktree_status(entry, dirty)),
        path: entry.path.to_string_lossy().into_owned(),
        base_branch,
        head: entry.head.clone(),
        scope,
        feature,
        locked: entry.locked,
        prunable: entry.prunable,
        session_label: session_labels.get(&path_key).cloned(),
    })
}

fn map_created_to_api(
    entry: &WorktreeCreateResult,
    session_labels: &BTreeMap<PathBuf, String>,
) -> std::result::Result<ApiWorktree, ApiError> {
    let metadata = macc_core::read_worktree_metadata(&entry.path).map_err(ApiError::from)?;
    let dirty = git_worktree_is_dirty(&entry.path).map_err(ApiError::from)?;
    let session_label = session_labels
        .get(&canonicalize_path_fallback(&entry.path))
        .cloned();

    Ok(ApiWorktree {
        id: entry.id.clone(),
        slug: derive_slug_from_id(&entry.id),
        branch: Some(entry.branch.clone()),
        tool: metadata.as_ref().map(|value| value.tool.clone()),
        status: Some(if dirty { "dirty" } else { "clean" }.to_string()),
        path: entry.path.to_string_lossy().into_owned(),
        base_branch: Some(entry.base.clone()),
        head: None,
        scope: metadata.as_ref().and_then(|value| value.scope.clone()),
        feature: metadata.as_ref().and_then(|value| value.feature.clone()),
        locked: false,
        prunable: false,
        session_label,
    })
}

fn normalize_branch_name(branch: &str) -> String {
    branch
        .strip_prefix("refs/heads/")
        .unwrap_or(branch)
        .to_string()
}

fn derive_worktree_status(entry: &WorktreeEntry, dirty: bool) -> String {
    if entry.prunable {
        "prunable".to_string()
    } else if entry.locked {
        "locked".to_string()
    } else if dirty {
        "dirty".to_string()
    } else {
        "clean".to_string()
    }
}

fn derive_slug_from_id(id: &str) -> Option<String> {
    let trimmed = id
        .rsplit_once('-')
        .map(|(prefix, suffix)| {
            if suffix.len() == 2 && suffix.chars().all(|ch| ch.is_ascii_digit()) {
                prefix
            } else {
                id
            }
        })
        .unwrap_or(id);
    trimmed
        .rsplit_once('-')
        .map(|(slug, _)| slug.to_string())
        .filter(|slug| !slug.is_empty())
}

fn fallback_worktree_id(path: &StdPath) -> String {
    path.file_name()
        .and_then(|value| value.to_str())
        .filter(|value| !value.is_empty())
        .map(ToString::to_string)
        .unwrap_or_else(|| path.to_string_lossy().into_owned())
}

fn merge_delete_options(
    query: DeleteWorktreeQuery,
    body: DeleteWorktreeBody,
) -> DeleteWorktreeQuery {
    DeleteWorktreeQuery {
        confirmed: body.confirmed.or(query.confirmed),
        force: body.force.or(query.force),
        remove_branch: body.remove_branch.or(query.remove_branch),
        force_confirmed: body.force_confirmed.or(query.force_confirmed),
    }
}

fn map_delete_error(err: MaccError, id: &str, path: &StdPath, operation: &str) -> ApiError {
    match err {
        MaccError::Git {
            operation: actual,
            message,
        } if actual == operation => ApiError::worktree_conflict(
            format!("worktree '{}' could not be removed: {}", id, message),
            Some(json!({ "worktreeId": id, "path": path, "operation": actual })),
        ),
        other => ApiError::from(other),
    }
}
