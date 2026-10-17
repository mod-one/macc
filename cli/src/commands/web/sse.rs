use super::errors::ApiError;
use super::WebState;
use async_stream::stream;
use axum::extract::{Query, State};
use axum::http::HeaderMap;
use axum::response::sse::{Event, Sse};
use macc_core::coordinator::COORDINATOR_EVENT_SCHEMA_VERSION;
use macc_core::coordinator_storage::{CoordinatorStoragePaths, SqliteStorage};
use macc_core::engine::CoordinatorEvent;
use macc_core::process_ownership::{ClientIdentity, ClientKind, ProcessHandle};
use serde::Deserialize;
use std::collections::VecDeque;
use std::convert::Infallible;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

const SSE_POLL_INTERVAL: Duration = Duration::from_millis(250);
/// Events sent on connect when the client resumes nothing.
const SSE_INITIAL_BACKLOG: usize = 500;
/// Most events read per poll; a backlog drains over the next polls.
const SSE_POLL_BATCH: usize = 1000;
const SSE_HEARTBEAT_INTERVAL: Duration = Duration::from_secs(5);
const WEB_CLIENT_HEADER: &str = "X-Macc-Client-Id";

#[derive(Debug, Default, Deserialize)]
pub(super) struct EventsQuery {
    #[serde(default)]
    pub(super) last_event_id: Option<String>,
    #[serde(default)]
    pub(super) client_id: Option<String>,
}

pub(super) async fn events_handler(
    State(state): State<WebState>,
    Query(query): Query<EventsQuery>,
    headers: HeaderMap,
) -> std::result::Result<
    Sse<impl tokio_stream::Stream<Item = std::result::Result<Event, Infallible>>>,
    ApiError,
> {
    let last_event_id = query.last_event_id.clone().or_else(|| {
        headers
            .get("last-event-id")
            .and_then(|value| value.to_str().ok())
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(ToOwned::to_owned)
    });
    let (initial_events, last_event_id) = initial_coordinator_events(&state, last_event_id)
        .await
        .map_err(ApiError::from)?;
    let viewer_guards =
        register_web_viewers(&state, web_client_id(&query, &headers)).map_err(|err| *err)?;

    Ok(Sse::new(coordinator_event_stream(
        state,
        initial_events,
        last_event_id,
        viewer_guards,
        SSE_POLL_INTERVAL,
        SSE_HEARTBEAT_INTERVAL,
    )))
}

pub(super) fn coordinator_event_stream(
    state: WebState,
    initial_events: Vec<CoordinatorEvent>,
    last_event_id: Option<String>,
    viewer_guards: Vec<macc_core::service::process_ownership::ProcessViewerGuard>,
    poll_interval: Duration,
    heartbeat_interval: Duration,
) -> impl tokio_stream::Stream<Item = std::result::Result<Event, Infallible>> {
    stream! {
        let _viewer_guards = viewer_guards;
        let mut source_seq_cursor = resolve_source_seq_cursor(&initial_events, last_event_id.as_deref());
        let mut pending_events = pending_events_after(&initial_events, source_seq_cursor);
        let mut poll_tick = tokio::time::interval(poll_interval);
        let mut heartbeat_tick = tokio::time::interval(heartbeat_interval);
        poll_tick.tick().await;
        heartbeat_tick.tick().await;

        loop {
            while let Some(event) = pending_events.pop_front() {
                source_seq_cursor = source_seq_cursor.max(event.seq);
                yield Ok(build_coordinator_sse_event(&event));
            }

            tokio::select! {
                _ = poll_tick.tick() => {
                    match coordinator_events_after(&state, source_seq_cursor).await {
                        Ok(events) => {
                            pending_events = events;
                        }
                        Err(err) => {
                            tracing::warn!("failed to refresh coordinator SSE events: {}", err);
                        }
                    }
                }
                _ = heartbeat_tick.tick() => {
                    yield Ok(build_heartbeat_sse_event(source_seq_cursor));
                }
            }
        }
    }
}

pub(super) fn web_client_id(query: &EventsQuery, headers: &HeaderMap) -> Option<String> {
    query.client_id.clone().or_else(|| {
        headers
            .get(WEB_CLIENT_HEADER)
            .and_then(|value| value.to_str().ok())
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(ToOwned::to_owned)
    })
}

pub(super) fn register_web_viewers(
    state: &WebState,
    client_id: Option<String>,
) -> Result<Vec<macc_core::service::process_ownership::ProcessViewerGuard>, Box<ApiError>> {
    let Some(client_id) = client_id else {
        return Ok(Vec::new());
    };
    let records = state
        .engine
        .process_list_running(&state.paths.root)
        .map_err(|err| Box::new(ApiError::from(err)))?;
    let mut viewer_guards = Vec::with_capacity(records.len());

    for record in records {
        let now = chrono::Utc::now().to_rfc3339();
        let identity = ClientIdentity {
            client_id: client_id.clone(),
            kind: ClientKind::Web,
            connected_at: now.clone(),
            last_heartbeat: now,
        };
        let handle = ProcessHandle {
            kind: record.process.kind,
            project_root: record.process.project_root,
            pid: record.process.pid,
        };
        let guard = state
            .engine
            .process_register_viewer(&state.paths.root, handle, identity)
            .map_err(|err| Box::new(ApiError::from(err)))?;
        viewer_guards.push(guard);
    }

    Ok(viewer_guards)
}

/// Where live streams read events from. The coordinator database answers
/// "what is new since seq N" from an index; the engine call loads the whole
/// log and is kept for projects without one (and for engines in tests).
fn storage_with_events(paths: &macc_core::ProjectPaths) -> Option<SqliteStorage> {
    let storage_paths = CoordinatorStoragePaths::from_project_paths(paths);
    // Never create a database just to stream from it.
    if !storage_paths.sqlite_path.exists() {
        return None;
    }
    let sqlite = SqliteStorage::new(storage_paths);
    sqlite
        .has_snapshot_data()
        .unwrap_or(false)
        .then_some(sqlite)
}

/// The events to send on connect (those after the client's last event, or
/// the recent backlog) and the last event id the stream resumes from. A
/// resumed position comes back as `cursor-<seq>`, so the stream does not
/// restart from the first event when nothing new arrived yet.
pub(super) async fn initial_coordinator_events(
    state: &WebState,
    last_event_id: Option<String>,
) -> macc_core::Result<(Vec<CoordinatorEvent>, Option<String>)> {
    let state = state.clone();
    blocking(move || match storage_with_events(&state.paths) {
        Some(sqlite) => {
            let resumed = match last_event_id.as_deref() {
                Some(id) => sqlite
                    .event_seq(id)?
                    .or_else(|| parse_source_seq_from_sse_id(id)),
                None => None,
            };
            let records = match resumed {
                Some(seq) => sqlite.load_events_after(seq, SSE_POLL_BATCH)?,
                None => sqlite.load_recent_events(SSE_INITIAL_BACKLOG)?,
            };
            Ok((
                records
                    .into_iter()
                    .map(CoordinatorEvent::from_record)
                    .collect(),
                resumed.map(|seq| format!("cursor-{seq}")),
            ))
        }
        None => Ok((
            state.engine.get_coordinator_events(&state.paths)?,
            last_event_id,
        )),
    })
    .await
}

/// Events newer than `after_seq`, oldest first.
pub(super) async fn coordinator_events_after(
    state: &WebState,
    after_seq: i64,
) -> macc_core::Result<VecDeque<CoordinatorEvent>> {
    let state = state.clone();
    blocking(move || match storage_with_events(&state.paths) {
        Some(sqlite) => Ok(sqlite
            .load_events_after(after_seq, SSE_POLL_BATCH)?
            .into_iter()
            .map(CoordinatorEvent::from_record)
            .collect()),
        None => Ok(pending_events_after(
            &state.engine.get_coordinator_events(&state.paths)?,
            after_seq,
        )),
    })
    .await
}

/// Storage reads block: keep them off the async workers.
async fn blocking<T: Send + 'static>(
    work: impl FnOnce() -> macc_core::Result<T> + Send + 'static,
) -> macc_core::Result<T> {
    tokio::task::spawn_blocking(work)
        .await
        .unwrap_or_else(|err| {
            Err(macc_core::MaccError::Validation(format!(
                "event read task failed: {err}"
            )))
        })
}

pub(super) fn pending_events_after(
    events: &[CoordinatorEvent],
    source_seq_cursor: i64,
) -> VecDeque<CoordinatorEvent> {
    events
        .iter()
        .filter(|event| event.seq > source_seq_cursor)
        .cloned()
        .collect()
}

pub(super) fn resolve_source_seq_cursor(
    events: &[CoordinatorEvent],
    last_event_id: Option<&str>,
) -> i64 {
    last_event_id
        .and_then(|id| {
            events
                .iter()
                .find(|event| coordinator_event_sse_id(event) == id)
                .map(|event| event.seq)
                .or_else(|| parse_source_seq_from_sse_id(id))
        })
        .unwrap_or_default()
}

fn coordinator_event_sse_id(event: &CoordinatorEvent) -> String {
    event
        .event_id
        .clone()
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| format!("cursor-{}", event.seq))
}

fn parse_source_seq_from_sse_id(event_id: &str) -> Option<i64> {
    if let Some(seq) = event_id.strip_prefix("cursor-") {
        return seq.parse::<i64>().ok();
    }

    if let Some(heartbeat) = event_id.strip_prefix("hb-") {
        let seq = heartbeat.split('-').next()?;
        return seq.parse::<i64>().ok();
    }

    None
}

pub(super) fn build_coordinator_sse_event(event: &CoordinatorEvent) -> Event {
    Event::default()
        .id(coordinator_event_sse_id(event))
        .event("coordinator_event")
        .json_data(event.raw.clone())
        .expect("serialize coordinator event payload")
}

pub(super) fn build_heartbeat_sse_event(source_seq_cursor: i64) -> Event {
    let ts = chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Secs, true);
    let heartbeat_id = format!("hb-{}-{}", source_seq_cursor, unix_timestamp_millis());
    let payload = serde_json::json!({
        "schema_version": COORDINATOR_EVENT_SCHEMA_VERSION,
        "event_id": heartbeat_id,
        "seq": source_seq_cursor,
        "ts": ts,
        "source": "coordinator",
        "type": "heartbeat",
        "status": "ok"
    });

    Event::default()
        .id(payload["event_id"].as_str().expect("heartbeat id"))
        .event("heartbeat")
        .json_data(payload)
        .expect("serialize heartbeat payload")
}

fn unix_timestamp_millis() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
}
