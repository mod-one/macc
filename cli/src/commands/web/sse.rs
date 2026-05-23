use super::errors::ApiError;
use super::WebState;
use async_stream::stream;
use axum::extract::{Query, State};
use axum::http::HeaderMap;
use axum::response::sse::{Event, Sse};
use macc_core::coordinator::COORDINATOR_EVENT_SCHEMA_VERSION;
use macc_core::engine::CoordinatorEvent;
use macc_core::process_ownership::{ClientIdentity, ClientKind, ProcessHandle};
use serde::Deserialize;
use std::collections::VecDeque;
use std::convert::Infallible;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

const SSE_POLL_INTERVAL: Duration = Duration::from_millis(250);
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
    let initial_events = state
        .engine
        .get_coordinator_events(&state.paths)
        .map_err(ApiError::from)?;
    let last_event_id = query.last_event_id.clone().or_else(|| {
        headers
            .get("last-event-id")
            .and_then(|value| value.to_str().ok())
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(ToOwned::to_owned)
    });
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
                    match state.engine.get_coordinator_events(&state.paths) {
                        Ok(events) => {
                            pending_events = pending_events_after(&events, source_seq_cursor);
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

fn web_client_id(query: &EventsQuery, headers: &HeaderMap) -> Option<String> {
    query.client_id.clone().or_else(|| {
        headers
            .get(WEB_CLIENT_HEADER)
            .and_then(|value| value.to_str().ok())
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(ToOwned::to_owned)
    })
}

fn register_web_viewers(
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

fn pending_events_after(
    events: &[CoordinatorEvent],
    source_seq_cursor: i64,
) -> VecDeque<CoordinatorEvent> {
    events
        .iter()
        .filter(|event| event.seq > source_seq_cursor)
        .cloned()
        .collect()
}

fn resolve_source_seq_cursor(events: &[CoordinatorEvent], last_event_id: Option<&str>) -> i64 {
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

fn build_coordinator_sse_event(event: &CoordinatorEvent) -> Event {
    Event::default()
        .id(coordinator_event_sse_id(event))
        .event("coordinator_event")
        .json_data(event.raw.clone())
        .expect("serialize coordinator event payload")
}

fn build_heartbeat_sse_event(source_seq_cursor: i64) -> Event {
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
