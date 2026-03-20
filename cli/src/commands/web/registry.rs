use super::errors::ApiError;
use super::types::{ApiRegistryEvent, ApiRegistryTask, ApiRegistryTaskWorktree};
use super::WebState;
use axum::extract::{Path, State};
use axum::Json;
use macc_core::coordinator::model::{Task, TaskWorktree};
use macc_core::coordinator::{CoordinatorEventRecord, RuntimeStatus, WorkflowState};
use serde::Deserialize;
use serde_json::{json, Value};
use std::collections::BTreeMap;

#[derive(Debug, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub(super) struct RegistryTaskActionBody {
    kind: Option<String>,
    tool: Option<String>,
    justification: Option<String>,
}

pub(super) async fn list_registry_tasks_handler(
    State(state): State<WebState>,
) -> std::result::Result<Json<Vec<ApiRegistryTask>>, ApiError> {
    let snapshot = state
        .engine
        .coordinator_state_snapshot(&state.paths.root, &BTreeMap::new())
        .map_err(ApiError::from)?;
    let events_by_task = collect_registry_events(&snapshot.events);
    let tasks = snapshot
        .registry
        .tasks
        .iter()
        .map(|task| {
            task_to_api(
                task,
                events_by_task
                    .get(task.id.as_str())
                    .map(Vec::as_slice)
                    .unwrap_or(&[]),
            )
        })
        .collect();
    Ok(Json(tasks))
}

pub(super) async fn task_action_handler(
    State(state): State<WebState>,
    Path((task_id, action)): Path<(String, String)>,
    body: Option<Json<RegistryTaskActionBody>>,
) -> std::result::Result<Json<ApiRegistryTask>, ApiError> {
    let action = action.trim().to_ascii_lowercase();
    let body = body.map(|payload| payload.0).unwrap_or_default();
    if let Some(kind) = body.kind.as_deref() {
        if !kind.trim().is_empty() && kind.trim().to_ascii_lowercase() != action {
            return Err(ApiError::validation(format!(
                "registry action body kind '{}' does not match path action '{}'",
                kind, action
            )));
        }
    }

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
        match action.as_str() {
            "requeue" => requeue_task(task, &now, body.justification.as_deref())?,
            "reassign" => reassign_task(task, &now, &body)?,
            other => {
                return Err(ApiError::validation(format!(
                    "unsupported registry action '{}'",
                    other
                )));
            }
        }
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

fn requeue_task(
    task: &mut Task,
    now: &str,
    _justification: Option<&str>,
) -> std::result::Result<(), ApiError> {
    let workflow_state = task.workflow_state();
    let runtime_status = task.runtime_status();
    let can_requeue = matches!(workflow_state, Some(WorkflowState::Blocked))
        || matches!(
            runtime_status,
            RuntimeStatus::Failed | RuntimeStatus::Stale | RuntimeStatus::Paused
        );
    if !can_requeue {
        return Err(ApiError::conflict(
            format!(
                "task '{}' cannot be requeued from state '{}' with runtime '{}'",
                task.id,
                task.state,
                runtime_status.as_str()
            ),
            Some(json!({
                "taskId": task.id,
                "state": task.state,
                "runtimeStatus": runtime_status.as_str(),
            })),
        ));
    }

    task.set_workflow_state(WorkflowState::Todo);
    task.clear_assignment();
    reset_task_runtime(task);
    task.touch_state_changed(now);
    Ok(())
}

fn reassign_task(
    task: &mut Task,
    now: &str,
    body: &RegistryTaskActionBody,
) -> std::result::Result<(), ApiError> {
    if task.is_active() || task.is_merged() {
        return Err(ApiError::conflict(
            format!(
                "task '{}' cannot be reassigned while in state '{}'",
                task.id, task.state
            ),
            Some(json!({
                "taskId": task.id,
                "state": task.state,
            })),
        ));
    }

    let tool = body
        .tool
        .as_deref()
        .map(str::trim)
        .filter(|tool| !tool.is_empty())
        .ok_or_else(|| ApiError::validation("reassign action requires a non-empty tool"))?;

    // Reassignment changes the dispatch tool. Any stale assignee/worktree
    // attachment from a previous tool run must be dropped to keep registry
    // state coherent for operators and later dispatch decisions.
    task.clear_assignment();
    task.tool = Some(tool.to_string());
    task.updated_at = Some(now.to_string());
    Ok(())
}

fn reset_task_runtime(task: &mut Task) {
    let runtime = task.ensure_runtime();
    runtime.set_status(RuntimeStatus::Idle);
    runtime.pid = None;
    runtime.current_phase = None;
    runtime.last_error = None;
    runtime.clear_last_error_details();
    runtime.last_heartbeat = None;
    runtime.started_at = None;
    runtime.phase_started_at = None;
    runtime.attempt = None;
    runtime.completion_kind = None;
    runtime.merge_result_pending = Some(false);
    runtime.merge_result_file = None;
    runtime.merge_worker_pid = None;
    runtime.merge_result_started_at = None;
    runtime.delayed_until = None;
}

fn task_to_api(task: &Task, events: &[ApiRegistryEvent]) -> ApiRegistryTask {
    ApiRegistryTask {
        id: task.id.clone(),
        title: task.title.clone(),
        priority: task.priority.clone(),
        state: task.state.clone(),
        tool: task.tool.clone(),
        attempts: task.task_runtime.attempt,
        heartbeat: task.task_runtime.last_heartbeat.clone(),
        delayed_until: task.task_runtime.delayed_until.clone(),
        current_phase: task.task_runtime.current_phase.clone(),
        last_error: task.task_runtime.last_error.clone(),
        last_error_code: task.task_runtime.last_error_code.clone(),
        assignee: task.assignee.clone(),
        worktree: task.worktree.as_ref().map(worktree_to_api),
        events: events.to_vec(),
        updated_at: task.updated_at.clone(),
    }
}

fn worktree_to_api(worktree: &TaskWorktree) -> ApiRegistryTaskWorktree {
    ApiRegistryTaskWorktree {
        worktree_path: worktree.worktree_path.clone(),
        branch: worktree.branch.clone(),
        base_branch: worktree.base_branch.clone(),
        last_commit: worktree.last_commit.clone(),
        session_id: worktree.session_id.clone(),
    }
}

fn collect_registry_events(
    events: &[CoordinatorEventRecord],
) -> BTreeMap<&str, Vec<ApiRegistryEvent>> {
    let mut out = BTreeMap::new();
    for event in events {
        let Some(task_id) = event
            .task_id
            .as_deref()
            .filter(|task_id| !task_id.is_empty())
        else {
            continue;
        };
        out.entry(task_id)
            .or_insert_with(Vec::new)
            .push(ApiRegistryEvent {
                event_id: Some(event.event_id.clone()).filter(|event_id| !event_id.is_empty()),
                event_type: event.event_type.clone(),
                ts: Some(event.ts.clone()).filter(|ts| !ts.is_empty()),
                status: Some(event.status.clone()).filter(|status| !status.is_empty()),
                severity: event.severity().map(ToString::to_string),
                message: event.message().map(ToString::to_string),
            });
    }
    out
}

fn not_found_task(task_id: &str) -> ApiError {
    ApiError::not_found(
        format!("task '{}' was not found in the registry", task_id),
        Some(Value::Object(
            [("taskId".to_string(), Value::String(task_id.to_string()))]
                .into_iter()
                .collect(),
        )),
    )
}
