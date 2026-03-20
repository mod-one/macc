use super::errors::ApiError;
use super::types::{ApiLogContent, ApiLogFile};
use super::WebState;
use async_stream::stream;
use axum::extract::{Path, Query, State};
use axum::http::HeaderMap;
use axum::response::sse::{Event, Sse};
use axum::Json;
use chrono::{DateTime, SecondsFormat, Utc};
use serde::Deserialize;
use serde_json::json;
use std::collections::HashMap;
use std::convert::Infallible;
use std::fs;
use std::fs::Metadata;
use std::io::SeekFrom;
use std::path::{Component, Path as StdPath, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use tokio::io::{AsyncReadExt, AsyncSeekExt};

const CATEGORY_COORDINATOR: &str = "coordinator";
const CATEGORY_PERFORMER: &str = "performer";
const DEFAULT_LIMIT: usize = 200;
const MAX_LIMIT: usize = 1_000;
const TAIL_POLL_INTERVAL: Duration = Duration::from_millis(500);
const TAIL_HEARTBEAT_INTERVAL: Duration = Duration::from_secs(15);
const MAX_TAIL_STREAMS_PER_CLIENT: usize = 25;

#[derive(Debug, Deserialize, Default)]
pub(super) struct ReadLogQuery {
    offset: Option<usize>,
    limit: Option<usize>,
    search: Option<String>,
}

#[derive(Clone, Default)]
pub(super) struct TailStreamLimiter {
    inner: Arc<Mutex<HashMap<String, usize>>>,
}

#[derive(Debug, Deserialize)]
pub(super) struct TailLogQuery {
    path: String,
}

pub(super) struct TailStreamPermit {
    limiter: TailStreamLimiter,
    client_key: String,
}

#[derive(Debug, Clone, Copy)]
struct TailCursor {
    offset: u64,
    file_identity: Option<FileIdentity>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct FileIdentity {
    #[cfg(unix)]
    dev: u64,
    #[cfg(unix)]
    ino: u64,
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

pub(super) async fn tail_log_handler(
    State(state): State<WebState>,
    headers: HeaderMap,
    Query(query): Query<TailLogQuery>,
) -> std::result::Result<
    Sse<impl tokio_stream::Stream<Item = std::result::Result<Event, Infallible>>>,
    ApiError,
> {
    let (requested_path, file_path) = resolve_log_path(&state, &query.path)?;
    let last_event_id = headers
        .get("last-event-id")
        .and_then(|value| value.to_str().ok())
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned);
    let client_key = client_stream_key(&headers);
    let permit = state
        .tail_stream_limiter
        .try_acquire(client_key.clone())
        .map_err(|_| {
            ApiError::conflict(
                format!(
                    "too many concurrent log tail streams for client '{}'",
                    client_key
                ),
                Some(json!({
                    "client": client_key,
                    "limit": MAX_TAIL_STREAMS_PER_CLIENT
                })),
            )
        })?;

    Ok(Sse::new(tail_log_stream(
        requested_path,
        file_path,
        last_event_id,
        permit,
        TAIL_POLL_INTERVAL,
        TAIL_HEARTBEAT_INTERVAL,
    )))
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

impl TailStreamLimiter {
    pub(super) fn try_acquire(&self, client_key: String) -> Result<TailStreamPermit, ()> {
        let mut guard = self.inner.lock().map_err(|_| ())?;
        let active = guard.entry(client_key.clone()).or_insert(0);
        if *active >= MAX_TAIL_STREAMS_PER_CLIENT {
            return Err(());
        }
        *active += 1;
        drop(guard);

        Ok(TailStreamPermit {
            limiter: self.clone(),
            client_key,
        })
    }

    fn release(&self, client_key: &str) {
        let Ok(mut guard) = self.inner.lock() else {
            return;
        };
        let Some(active) = guard.get_mut(client_key) else {
            return;
        };
        if *active <= 1 {
            guard.remove(client_key);
        } else {
            *active -= 1;
        }
    }
}

impl Drop for TailStreamPermit {
    fn drop(&mut self) {
        self.limiter.release(&self.client_key);
    }
}

pub(super) fn tail_log_stream(
    requested_path: String,
    file_path: PathBuf,
    last_event_id: Option<String>,
    permit: TailStreamPermit,
    poll_interval: Duration,
    heartbeat_interval: Duration,
) -> impl tokio_stream::Stream<Item = std::result::Result<Event, Infallible>> {
    stream! {
        let _permit = permit;
        let mut cursor = resolve_tail_cursor(&file_path, last_event_id.as_deref()).await;
        let mut partial_line = Vec::new();
        let mut poll_tick = tokio::time::interval(poll_interval);
        let mut heartbeat_tick = tokio::time::interval(heartbeat_interval);
        poll_tick.tick().await;
        heartbeat_tick.tick().await;

        loop {
            match read_available_log_lines(&file_path, &requested_path, &mut cursor, &mut partial_line).await {
                Ok(events) => {
                    for event in events {
                        yield Ok(event);
                    }
                }
                Err(err) => {
                    tracing::warn!(
                        path = %requested_path,
                        error = %err,
                        "failed to tail web log file"
                    );
                }
            }

            tokio::select! {
                _ = poll_tick.tick() => {}
                _ = heartbeat_tick.tick() => {
                    yield Ok(build_log_heartbeat_event(&requested_path, cursor.offset));
                }
            }
        }
    }
}

fn client_stream_key(headers: &HeaderMap) -> String {
    for header_name in ["x-forwarded-for", "x-real-ip"] {
        if let Some(value) = headers
            .get(header_name)
            .and_then(|value| value.to_str().ok())
        {
            let client = value
                .split(',')
                .next()
                .map(str::trim)
                .filter(|value| !value.is_empty());
            if let Some(client) = client {
                return client.to_string();
            }
        }
    }

    "anonymous".to_string()
}

async fn resolve_tail_cursor(file_path: &StdPath, last_event_id: Option<&str>) -> TailCursor {
    let metadata = tokio::fs::metadata(file_path).await.ok();
    let size = metadata.as_ref().map(Metadata::len).unwrap_or_default();
    let offset = last_event_id
        .and_then(parse_tail_event_id)
        .map(|cursor| cursor.offset.min(size))
        .unwrap_or(size);

    TailCursor {
        offset,
        file_identity: metadata.as_ref().and_then(file_identity),
    }
}

async fn read_available_log_lines(
    file_path: &StdPath,
    requested_path: &str,
    cursor: &mut TailCursor,
    partial_line: &mut Vec<u8>,
) -> std::result::Result<Vec<Event>, macc_core::MaccError> {
    let metadata =
        tokio::fs::metadata(file_path)
            .await
            .map_err(|err| macc_core::MaccError::Io {
                path: file_path.to_string_lossy().into_owned(),
                action: "stat tailed web log file".into(),
                source: err,
            })?;
    let current_identity = file_identity(&metadata);
    if cursor.file_identity.is_some()
        && current_identity.is_some()
        && cursor.file_identity != current_identity
    {
        cursor.offset = 0;
        partial_line.clear();
    }
    if metadata.len() < cursor.offset {
        cursor.offset = 0;
        partial_line.clear();
    }
    cursor.file_identity = current_identity;

    let mut file =
        tokio::fs::File::open(file_path)
            .await
            .map_err(|err| macc_core::MaccError::Io {
                path: file_path.to_string_lossy().into_owned(),
                action: "open tailed web log file".into(),
                source: err,
            })?;
    file.seek(SeekFrom::Start(cursor.offset))
        .await
        .map_err(|err| macc_core::MaccError::Io {
            path: file_path.to_string_lossy().into_owned(),
            action: "seek tailed web log file".into(),
            source: err,
        })?;

    let mut chunk = Vec::new();
    file.read_to_end(&mut chunk)
        .await
        .map_err(|err| macc_core::MaccError::Io {
            path: file_path.to_string_lossy().into_owned(),
            action: "read tailed web log file".into(),
            source: err,
        })?;
    if chunk.is_empty() {
        return Ok(Vec::new());
    }

    let mut events = Vec::new();
    let base_offset = cursor.offset;
    let mut buffer = std::mem::take(partial_line);
    buffer.extend_from_slice(&chunk);
    let mut consumed = 0usize;
    for (index, byte) in buffer.iter().enumerate() {
        if *byte != b'\n' {
            continue;
        }
        let line = &buffer[consumed..index];
        consumed = index + 1;
        let content = String::from_utf8_lossy(line)
            .trim_end_matches('\r')
            .to_string();
        let line_offset = base_offset + consumed as u64;
        events.push(build_log_line_event(requested_path, line_offset, content));
    }
    if consumed == 0 {
        *partial_line = buffer;
        return Ok(events);
    }

    *partial_line = buffer[consumed..].to_vec();
    cursor.offset = base_offset + consumed as u64;

    Ok(events)
}

fn file_identity(metadata: &Metadata) -> Option<FileIdentity> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;

        Some(FileIdentity {
            dev: metadata.dev(),
            ino: metadata.ino(),
        })
    }

    #[cfg(not(unix))]
    {
        let _ = metadata;
        None
    }
}

fn parse_tail_event_id(value: &str) -> Option<TailCursor> {
    if let Some(offset) = value.strip_prefix("off-") {
        return offset.parse::<u64>().ok().map(|offset| TailCursor {
            offset,
            file_identity: None,
        });
    }

    if let Some(heartbeat) = value.strip_prefix("hb-") {
        let offset = heartbeat.split('-').next()?;
        return offset.parse::<u64>().ok().map(|offset| TailCursor {
            offset,
            file_identity: None,
        });
    }

    None
}

fn build_log_line_event(requested_path: &str, offset: u64, content: String) -> Event {
    let payload = json!({
        "path": requested_path,
        "timestamp": Utc::now().to_rfc3339_opts(SecondsFormat::Secs, true),
        "content": content
    });

    Event::default()
        .id(format!("off-{offset}"))
        .event("log_line")
        .json_data(payload)
        .expect("serialize log line payload")
}

fn build_log_heartbeat_event(requested_path: &str, offset: u64) -> Event {
    let event_id = format!("hb-{}-{}", offset, unix_timestamp_millis());
    let payload = json!({
        "path": requested_path,
        "timestamp": Utc::now().to_rfc3339_opts(SecondsFormat::Secs, true),
        "offset": offset,
        "type": "heartbeat",
        "status": "ok"
    });

    Event::default()
        .id(event_id)
        .event("heartbeat")
        .json_data(payload)
        .expect("serialize log heartbeat payload")
}

fn unix_timestamp_millis() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
}
