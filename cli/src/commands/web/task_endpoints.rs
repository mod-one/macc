use super::errors::ApiError;
use super::types::ApiRegistryTask;
use super::WebState;
use async_stream::stream;
use axum::extract::{Path, Query, State};
use axum::http::HeaderMap;
use axum::response::sse::{Event, Sse};
use axum::Json;
use macc_core::coordinator::types::CoordinatorEnvConfig;
use macc_core::coordinator::RuntimeStatus;
use macc_core::engine::CoordinatorEvent;
use macc_core::service::coordinator_workflow::{CoordinatorCommand, CoordinatorCommandRequest};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::convert::Infallible;
use std::time::Duration;

use super::registry::{collect_registry_events, not_found_task, requeue_task, task_to_api};
use super::sse::{
    build_coordinator_sse_event, build_heartbeat_sse_event, pending_events_after,
    register_web_viewers, resolve_source_seq_cursor, web_client_id, EventsQuery,
};

const SSE_POLL_INTERVAL: Duration = Duration::from_millis(250);
const SSE_HEARTBEAT_INTERVAL: Duration = Duration::from_secs(5);

#[derive(Debug, Deserialize)]
pub(super) struct DiffQuery {
    format: Option<String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct ApiTaskDiff {
    pub task_id: String,
    pub format: String,
    pub diff: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct ApiTaskExplain {
    pub task_id: String,
    pub timeline: Vec<ApiTimelineEvent>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct ApiTimelineEvent {
    pub timestamp: String,
    pub severity: String,
    pub phase: String,
    pub event_type: String,
    pub message: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct ApiTaskLogs {
    pub task_id: String,
    pub stdout: Option<String>,
    pub stderr: Option<String>,
}

pub(super) async fn get_registry_task_handler(
    State(state): State<WebState>,
    Path(task_id): Path<String>,
) -> std::result::Result<Json<ApiRegistryTask>, ApiError> {
    let snapshot = state
        .engine
        .coordinator_state_snapshot(&state.paths.root, &BTreeMap::new())
        .map_err(ApiError::from)?;
    let task = snapshot
        .registry
        .find_task(&task_id)
        .ok_or_else(|| not_found_task(&task_id))?;
    let events_by_task = collect_registry_events(&snapshot.events);
    Ok(Json(task_to_api(
        task,
        events_by_task
            .get(task.id.as_str())
            .map(Vec::as_slice)
            .unwrap_or(&[]),
    )))
}

pub(super) async fn get_registry_task_events_handler(
    State(state): State<WebState>,
    Path(task_id): Path<String>,
) -> std::result::Result<Json<Vec<super::types::ApiRegistryEvent>>, ApiError> {
    let snapshot = state
        .engine
        .coordinator_state_snapshot(&state.paths.root, &BTreeMap::new())
        .map_err(ApiError::from)?;
    let task = snapshot
        .registry
        .find_task(&task_id)
        .ok_or_else(|| not_found_task(&task_id))?;
    let events_by_task = collect_registry_events(&snapshot.events);
    let events = events_by_task
        .get(task.id.as_str())
        .cloned()
        .unwrap_or_default();
    Ok(Json(events))
}

pub(super) async fn get_registry_task_logs_handler(
    State(state): State<WebState>,
    Path(task_id): Path<String>,
) -> std::result::Result<Json<ApiTaskLogs>, ApiError> {
    let snapshot = state
        .engine
        .coordinator_state_snapshot(&state.paths.root, &BTreeMap::new())
        .map_err(ApiError::from)?;
    let task = snapshot
        .registry
        .find_task(&task_id)
        .ok_or_else(|| not_found_task(&task_id))?;

    let rt = &task.task_runtime;
    let mut stdout = None;
    let mut stderr = None;

    if let Some(stdout_rel) = &rt.stdout_log {
        let path = state.paths.root.join(stdout_rel);
        if path.exists() {
            if let Ok(content) = std::fs::read_to_string(&path) {
                stdout = Some(content);
            }
        }
    }

    if let Some(stderr_rel) = &rt.stderr_log {
        let path = state.paths.root.join(stderr_rel);
        if path.exists() {
            if let Ok(content) = std::fs::read_to_string(&path) {
                stderr = Some(content);
            }
        }
    }

    Ok(Json(ApiTaskLogs {
        task_id,
        stdout,
        stderr,
    }))
}

pub(super) async fn get_registry_task_diff_handler(
    State(state): State<WebState>,
    Path(task_id): Path<String>,
    Query(query): Query<DiffQuery>,
) -> std::result::Result<Json<ApiTaskDiff>, ApiError> {
    let snapshot = state
        .engine
        .coordinator_state_snapshot(&state.paths.root, &BTreeMap::new())
        .map_err(ApiError::from)?;
    let task = snapshot
        .registry
        .find_task(&task_id)
        .ok_or_else(|| not_found_task(&task_id))?;

    let format = query.format.clone().unwrap_or_else(|| "patch".to_string());

    // Resolve worktree path from task_runtime or task.worktree
    let worktree_path = task
        .task_runtime
        .worktree
        .as_deref()
        .filter(|s| !s.is_empty())
        .or_else(|| {
            task.worktree
                .as_ref()
                .and_then(|w| w.worktree_path.as_deref())
                .filter(|s| !s.is_empty())
        })
        .map(|p| state.paths.root.join(p));

    // Resolve base branch
    let base_branch = task
        .worktree
        .as_ref()
        .and_then(|w| w.base_branch.clone())
        .filter(|s| !s.is_empty())
        .or_else(|| task.base_branch.clone().filter(|s| !s.is_empty()))
        .unwrap_or_else(|| "main".to_string());

    let mut diff_str = String::new();

    if let Some(ref wt) = worktree_path {
        if wt.exists() {
            let diff_target = format!("{}...HEAD", base_branch);
            let mut args = vec!["diff", &diff_target];
            if format == "stat" {
                args.push("--stat");
            }
            let output = macc_core::git::run_git_output_mapped(wt, &args, "git diff worktree")
                .map_err(ApiError::from)?;
            diff_str = String::from_utf8_lossy(&output.stdout).into_owned();
        }
    }

    // Fallback: commit-based diff if worktree is gone or doesn't have changes
    if diff_str.is_empty() {
        if let Some(commit) = task.worktree.as_ref().and_then(|w| w.last_commit.clone()) {
            let diff_target = format!("{}...{}", base_branch, commit);
            let mut args = vec!["diff", &diff_target];
            if format == "stat" {
                args.push("--stat");
            }
            let output =
                macc_core::git::run_git_output_mapped(&state.paths.root, &args, "git diff commit")
                    .map_err(ApiError::from)?;
            diff_str = String::from_utf8_lossy(&output.stdout).into_owned();
        }
    }

    Ok(Json(ApiTaskDiff {
        task_id,
        format,
        diff: diff_str,
    }))
}

pub(super) async fn get_registry_task_explain_handler(
    State(state): State<WebState>,
    Path(task_id): Path<String>,
) -> std::result::Result<Json<ApiTaskExplain>, ApiError> {
    let snapshot = state
        .engine
        .coordinator_state_snapshot(&state.paths.root, &BTreeMap::new())
        .map_err(ApiError::from)?;
    let task = snapshot
        .registry
        .find_task(&task_id)
        .ok_or_else(|| not_found_task(&task_id))?;

    let rt = &task.task_runtime;
    let events_log_path = rt.events_log.as_deref().map(|p| state.paths.root.join(p));

    let events_resolved_path = if let Some(ref path) = events_log_path {
        if path.exists() {
            Some(path.clone())
        } else {
            None
        }
    } else {
        let global_events = state.paths.root.join(".macc/log/events.jsonl");
        if global_events.exists() {
            Some(global_events)
        } else {
            None
        }
    };

    let mut timeline = Vec::new();

    if let Some(ref path) = events_resolved_path {
        use std::io::{BufRead, BufReader};
        if let Ok(file) = std::fs::File::open(path) {
            let reader = BufReader::new(file);
            for line in reader.lines() {
                let Ok(line) = line else { continue };
                let Ok(val) = serde_json::from_str::<serde_json::Value>(&line) else {
                    continue;
                };

                // Filter by task_id
                let event_task = val.get("task_id").and_then(|v| v.as_str()).unwrap_or("");
                if !event_task.eq_ignore_ascii_case(&task_id) {
                    continue;
                }

                let ts = val
                    .get("timestamp")
                    .and_then(|v| v.as_str())
                    .or_else(|| val.get("ts").and_then(|v| v.as_str()))
                    .unwrap_or("")
                    .to_string();
                let sev = val
                    .get("severity")
                    .and_then(|v| v.as_str())
                    .unwrap_or("info")
                    .to_string();
                let phase = val
                    .get("phase")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string();
                let event_type = val
                    .get("event_type")
                    .and_then(|v| v.as_str())
                    .or_else(|| val.get("type").and_then(|v| v.as_str()))
                    .unwrap_or("")
                    .to_string();
                let message = val
                    .get("message")
                    .and_then(|v| v.as_str())
                    .or_else(|| val.get("msg").and_then(|v| v.as_str()))
                    .or_else(|| {
                        val.get("payload")
                            .and_then(|p| p.get("message"))
                            .and_then(|v| v.as_str())
                    })
                    .unwrap_or("")
                    .to_string();

                timeline.push(ApiTimelineEvent {
                    timestamp: ts,
                    severity: sev,
                    phase,
                    event_type,
                    message,
                });
            }
        }
    }

    Ok(Json(ApiTaskExplain { task_id, timeline }))
}

pub(super) async fn task_stream_handler(
    State(state): State<WebState>,
    Path(task_id): Path<String>,
    Query(query): Query<EventsQuery>,
    headers: HeaderMap,
) -> std::result::Result<
    Sse<impl tokio_stream::Stream<Item = std::result::Result<Event, Infallible>>>,
    ApiError,
> {
    // Check if task exists in registry first
    {
        let snapshot = state
            .engine
            .coordinator_state_snapshot(&state.paths.root, &BTreeMap::new())
            .map_err(ApiError::from)?;
        let _task = snapshot
            .registry
            .find_task(&task_id)
            .ok_or_else(|| not_found_task(&task_id))?;
    }

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

    Ok(Sse::new(task_event_stream(
        state,
        task_id,
        initial_events,
        last_event_id,
        viewer_guards,
        SSE_POLL_INTERVAL,
        SSE_HEARTBEAT_INTERVAL,
    )))
}

fn task_event_stream(
    state: WebState,
    task_id: String,
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

                let is_heartbeat = event.event_type == "heartbeat";
                let is_task_match = event.task_id.as_deref() == Some(&task_id);
                if is_heartbeat || is_task_match {
                    yield Ok(build_coordinator_sse_event(&event));
                }
            }

            tokio::select! {
                _ = poll_tick.tick() => {
                    match state.engine.get_coordinator_events(&state.paths) {
                        Ok(events) => {
                            pending_events = pending_events_after(&events, source_seq_cursor);
                        }
                        Err(err) => {
                            tracing::warn!("failed to refresh task SSE events: {}", err);
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

pub(super) async fn task_retry_handler(
    State(state): State<WebState>,
    Path(task_id): Path<String>,
    headers: axum::http::HeaderMap,
) -> std::result::Result<Json<ApiRegistryTask>, ApiError> {
    crate::commands::web::mutation_gate::require_project_owner(&state, &headers)?;
    let mut snapshot = state
        .engine
        .coordinator_state_snapshot(&state.paths.root, &BTreeMap::new())
        .map_err(ApiError::from)?;
    let now = chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Secs, true);

    let updated_task = {
        let task = snapshot
            .registry
            .find_task_mut(&task_id)
            .ok_or_else(|| not_found_task(&task_id))?;
        requeue_task(task, &now, None)?;
        task.clone()
    };

    snapshot.registry.recompute_resource_locks(&now);
    snapshot.registry.set_updated_at(now);
    state
        .engine
        .coordinator_state_save_snapshot(&state.paths.root, &BTreeMap::new(), &snapshot)
        .map_err(ApiError::from)?;

    let events_by_task = collect_registry_events(&snapshot.events);
    Ok(Json(task_to_api(
        &updated_task,
        events_by_task
            .get(updated_task.id.as_str())
            .map(Vec::as_slice)
            .unwrap_or(&[]),
    )))
}

pub(super) async fn task_stop_handler(
    State(state): State<WebState>,
    Path(task_id): Path<String>,
    headers: axum::http::HeaderMap,
) -> std::result::Result<Json<ApiRegistryTask>, ApiError> {
    crate::commands::web::mutation_gate::require_project_owner(&state, &headers)?;
    let paths = state.paths.clone();
    let engine = state.engine.clone();

    // Check if task exists in registry first
    {
        let snapshot = state
            .engine
            .coordinator_state_snapshot(&state.paths.root, &BTreeMap::new())
            .map_err(ApiError::from)?;
        let _task = snapshot
            .registry
            .find_task(&task_id)
            .ok_or_else(|| not_found_task(&task_id))?;
    }

    let env_cfg = CoordinatorEnvConfig::default();
    let task_id_clone = task_id.clone();
    tokio::task::spawn_blocking(move || {
        engine.coordinator_execute_command(
            &paths,
            CoordinatorCommand::KillTask {
                task_id: task_id_clone,
            },
            CoordinatorCommandRequest {
                canonical: None,
                coordinator_cfg: None,
                env_cfg: &env_cfg,
                logger: None,
            },
        )
    })
    .await
    .map_err(|e| ApiError::validation(e.to_string()))??;

    let snapshot = state
        .engine
        .coordinator_state_snapshot(&state.paths.root, &BTreeMap::new())
        .map_err(ApiError::from)?;
    let events_by_task = collect_registry_events(&snapshot.events);
    let task = snapshot
        .registry
        .find_task(&task_id)
        .ok_or_else(|| not_found_task(&task_id))?;

    Ok(Json(task_to_api(
        task,
        events_by_task
            .get(task.id.as_str())
            .map(Vec::as_slice)
            .unwrap_or(&[]),
    )))
}

pub(super) async fn task_run_testing_handler(
    State(state): State<WebState>,
    Path(task_id): Path<String>,
    headers: axum::http::HeaderMap,
) -> std::result::Result<Json<ApiRegistryTask>, ApiError> {
    crate::commands::web::mutation_gate::require_project_owner(&state, &headers)?;
    let mut snapshot = state
        .engine
        .coordinator_state_snapshot(&state.paths.root, &BTreeMap::new())
        .map_err(ApiError::from)?;
    let now = chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Secs, true);

    let updated_task = {
        let task = snapshot
            .registry
            .find_task_mut(&task_id)
            .ok_or_else(|| not_found_task(&task_id))?;

        // Transition task workflow state to Testing, set phase to test and status to Idle
        task.state = "testing".to_string();
        let runtime = task.ensure_runtime();
        runtime.set_status(RuntimeStatus::Idle);
        runtime.current_phase = Some("test".to_string());
        runtime.pid = None;
        runtime.last_error = None;
        runtime.clear_last_error_details();
        runtime.last_heartbeat = None;
        runtime.started_at = None;
        runtime.phase_started_at = None;

        task.touch_state_changed(&now);
        task.clone()
    };

    snapshot.registry.recompute_resource_locks(&now);
    snapshot.registry.set_updated_at(now);
    state
        .engine
        .coordinator_state_save_snapshot(&state.paths.root, &BTreeMap::new(), &snapshot)
        .map_err(ApiError::from)?;

    let events_by_task = collect_registry_events(&snapshot.events);
    Ok(Json(task_to_api(
        &updated_task,
        events_by_task
            .get(updated_task.id.as_str())
            .map(Vec::as_slice)
            .unwrap_or(&[]),
    )))
}

pub(super) async fn task_run_review_handler(
    State(state): State<WebState>,
    Path(task_id): Path<String>,
    headers: axum::http::HeaderMap,
) -> std::result::Result<Json<ApiRegistryTask>, ApiError> {
    crate::commands::web::mutation_gate::require_project_owner(&state, &headers)?;
    let mut snapshot = state
        .engine
        .coordinator_state_snapshot(&state.paths.root, &BTreeMap::new())
        .map_err(ApiError::from)?;
    let now = chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Secs, true);

    let updated_task = {
        let task = snapshot
            .registry
            .find_task_mut(&task_id)
            .ok_or_else(|| not_found_task(&task_id))?;

        // Transition task workflow state to Reviewing, set phase to review and status to Idle
        task.state = "reviewing".to_string();
        let runtime = task.ensure_runtime();
        runtime.set_status(RuntimeStatus::Idle);
        runtime.current_phase = Some("review".to_string());
        runtime.pid = None;
        runtime.last_error = None;
        runtime.clear_last_error_details();
        runtime.last_heartbeat = None;
        runtime.started_at = None;
        runtime.phase_started_at = None;

        task.touch_state_changed(&now);
        task.clone()
    };

    snapshot.registry.recompute_resource_locks(&now);
    snapshot.registry.set_updated_at(now);
    state
        .engine
        .coordinator_state_save_snapshot(&state.paths.root, &BTreeMap::new(), &snapshot)
        .map_err(ApiError::from)?;

    let events_by_task = collect_registry_events(&snapshot.events);
    Ok(Json(task_to_api(
        &updated_task,
        events_by_task
            .get(updated_task.id.as_str())
            .map(Vec::as_slice)
            .unwrap_or(&[]),
    )))
}
