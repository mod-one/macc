//! Preserve interrupted phase work while a provider is unavailable.
use super::base::{extract_cooldown_from_reason, persist_throttle_registry};
use super::phase_runner::phase_tool_for_task;
use crate::coordinator::control_plane::CoordinatorLog;
use crate::coordinator::helpers::append_coordinator_event_with_severity;
use crate::coordinator::model::{Task, TaskRegistry};
use crate::coordinator::rate_limit::{is_tool_throttled, RateLimitInfo, ToolThrottleState};
use crate::coordinator::runtime::CoordinatorRunState;
use crate::{MaccError, Result};
use std::path::Path;

#[allow(clippy::too_many_arguments)]
pub(super) fn handle_phase_tool_unavailability(
    repo_root: &Path,
    registry: &mut serde_json::Value,
    state: &mut CoordinatorRunState,
    task_snapshot: &Task,
    task_id: &str,
    phase: &str,
    reason: &str,
    configured_tool: Option<&str>,
    now: &str,
    enabled_tools: &[String],
    logger: Option<&dyn CoordinatorLog>,
) -> Result<()> {
    let tool_id = phase_tool_for_task(task_snapshot, configured_tool).unwrap_or("unknown");
    let (code, cooldown) =
        extract_cooldown_from_reason(reason, tool_id, &state.normalizer_registry)
            .unwrap_or_else(|| ("E602".into(), 3600));
    let now_epoch = chrono::DateTime::parse_from_rfc3339(now)
        .map(|dt| dt.timestamp() as u64)
        .unwrap_or(0);
    state.throttle_registry.insert(
        tool_id.into(),
        ToolThrottleState {
            tool_id: tool_id.into(),
            throttled_until: now_epoch.saturating_add(cooldown),
            backoff_seconds: cooldown,
            consecutive_429_count: 1,
            last_rate_limit_info: Some(RateLimitInfo {
                tool_id: tool_id.into(),
                error_code: code.clone(),
                retry_after_seconds: Some(cooldown),
                detected_at: now_epoch,
                source_header: None,
            }),
        },
    );
    persist_throttle_registry(repo_root, &state.throttle_registry, tool_id)?;
    let fallback = enabled_tools
        .iter()
        .find(|tool| !is_tool_throttled(&state.throttle_registry, tool, now));
    let deferred = enabled_tools
        .iter()
        .filter_map(|tool| {
            state
                .throttle_registry
                .get(tool)
                .map(|state| (state.throttled_until, tool))
        })
        .min_by_key(|(until, _)| *until);
    let next_tool = fallback.or_else(|| deferred.as_ref().map(|(_, tool)| *tool));
    let delayed_until = if fallback.is_none() {
        let until = deferred
            .map(|(until, _)| until)
            .unwrap_or(now_epoch.saturating_add(cooldown));
        chrono::DateTime::from_timestamp(until as i64, 0)
            .map(|dt| dt.to_rfc3339_opts(chrono::SecondsFormat::Secs, true))
    } else {
        None
    };
    let mut typed = TaskRegistry::from_value(registry)?;
    let task = typed.find_task_mut(task_id).ok_or_else(|| {
        MaccError::Validation(format!("Task {task_id} missing during phase handoff"))
    })?;
    if !task.has_worktree_attached() {
        return Err(MaccError::Coordinator {
            code: "resume_worktree_unavailable",
            message: format!(
                "Task {task_id} has no attached worktree for phase handoff; refusing a fresh slot"
            ),
        });
    }
    let runtime = task.ensure_runtime();
    runtime.pid = None;
    runtime.completion_kind = None;
    runtime.active_session_id = None;
    runtime.current_phase = Some(phase.into());
    runtime.set_status(crate::coordinator::RuntimeStatus::PhaseDone);
    runtime.delayed_until = delayed_until;
    runtime.set_last_error_details(
        code,
        "tool",
        format!("Tool {tool_id} unavailable during {phase}; work preserved; cooldown {cooldown}s"),
    );
    runtime
        .extra
        .insert("phase_tool_unavailable".into(), true.into());
    if let Some(next_tool) = next_tool {
        runtime
            .extra
            .insert("phase_tool_override".into(), next_tool.clone().into());
    }
    task.touch_state_changed(now);
    *registry = typed.to_value()?;
    let message = format!("phase_tool_unavailability task={task_id} phase={phase} tool={tool_id} cooldown={cooldown}s work_preserved=true strategy={}", if fallback.is_some() { "resume_with_fallback" } else { "wait_for_throttle" });
    append_coordinator_event_with_severity(
        repo_root,
        "phase_tool_unavailability",
        task_id,
        phase,
        if fallback.is_some() {
            "recycled"
        } else {
            "waiting"
        },
        &message,
        "warning",
    )?;
    if let Some(log) = logger {
        let _ = log.note(format!("- {message}"));
    }
    Ok(())
}
