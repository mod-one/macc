use crate::config::CoordinatorConfigResolved;
use crate::coordinator::helpers::{
    append_coordinator_event, append_coordinator_event_with_severity, now_iso_coordinator,
    recompute_resource_locks_from_tasks, set_registry_updated_at,
};
use crate::coordinator::ipc::ensure_performer_ipc_listener;
use crate::coordinator::model::{PrdInput, Task, TaskRegistry};
use crate::coordinator::rate_limit::{RateLimitInfo, ToolThrottleState, E602_QUOTA_EXHAUSTED};
use crate::coordinator::runtime::{
    CoordinatorJob, CoordinatorMergeJob, CoordinatorRunState, CoordinatorRuntimeEventKind,
};
use crate::coordinator::types::CoordinatorEnvConfig;
use crate::coordinator::{engine as coordinator_engine, runtime as coordinator_runtime};
use crate::{MaccError, Result};
use std::collections::{BTreeMap, HashMap, HashSet};
use std::path::Path;
#[cfg(test)]
use std::time::Duration;
use std::time::Instant;

use super::dispatch::{dispatch_limit_reached, run_dispatch_pipeline, DispatchPipelineContext};
use super::phase_runner::{
    append_task_lifecycle_event_with_session, refresh_task_active_session_id_in_registry,
    task_active_session_id_from_registry, NativePhaseExecutor,
};
use super::sanitize::{prepare_clean_worktree, SanitizeOptions};
#[cfg(test)]
use super::{
    dispatch::select_dispatch_candidate,
    merge_gate::{merge_gate_check, MergeGateResult},
    sanitize::maybe_rollback_new_worktree_on_sanitize_failure,
};

pub trait CoordinatorLog: Sync {
    fn note(&self, line: String) -> Result<()>;
}

fn aggregate_performer_logs_after_completion(
    repo_root: &Path,
    task_id: &str,
    logger: Option<&dyn CoordinatorLog>,
) {
    match crate::coordinator::logs::aggregate_performer_logs(repo_root) {
        Ok(copied) => {
            if copied > 0 {
                let msg = format!(
                    "performer log aggregation updated task={} copied={}",
                    task_id, copied
                );
                let _ = append_coordinator_event_with_severity(
                    repo_root,
                    "performer_logs_aggregated",
                    task_id,
                    "dev",
                    "success",
                    &msg,
                    "info",
                );
                if let Some(log) = logger {
                    let _ = log.note(format!("- {}", msg));
                }
            }
        }
        Err(err) => {
            let msg = format!(
                "performer log aggregation failed task={} error={}",
                task_id, err
            );
            let _ = append_coordinator_event_with_severity(
                repo_root,
                "performer_logs_aggregation_failed",
                task_id,
                "dev",
                "failed",
                &msg,
                "warning",
            );
            if let Some(log) = logger {
                let _ = log.note(format!("- {}", msg));
            }
        }
    }
}

#[cfg(test)]
fn record_dispatch_retry_or_block(
    repo_root: &Path,
    state: &mut CoordinatorRunState,
    task_id: &str,
    cooldown_seconds: u64,
    max_dispatch_retries: u32,
    logger: Option<&dyn CoordinatorLog>,
) -> Result<bool> {
    let retry_count = {
        let entry = state
            .dispatch_retry_count
            .entry(task_id.to_string())
            .or_insert(0);
        *entry += 1;
        *entry
    };
    if retry_count >= max_dispatch_retries {
        let registry_value = crate::coordinator::state::coordinator_state_registry_load(
            repo_root,
            &BTreeMap::new(),
        )?;
        let mut registry = TaskRegistry::from_value(&registry_value)?;
        let mut blocked = false;
        if let Some(task) = registry.find_task_mut(task_id) {
            task.state = "blocked".to_string();
            let runtime = task.ensure_runtime();
            runtime.status = Some("failed".to_string());
            runtime.last_error = Some("dispatch_retry_limit_exceeded".to_string());
            runtime.set_last_error_details(
                "E901",
                "coordinator",
                "dispatch_retry_limit_exceeded".to_string(),
            );
            let now = now_iso_coordinator();
            task.updated_at = Some(now.clone());
            task.state_changed_at = Some(now.clone());
            registry.recompute_resource_locks(&now);
            registry.set_updated_at(now);
            crate::coordinator::state::coordinator_state_registry_save(
                repo_root,
                &BTreeMap::new(),
                &registry.to_value()?,
            )?;
            blocked = true;
        }
        state.dispatch_retry_count.remove(task_id);
        state.dispatch_retry_not_before.remove(task_id);
        let msg = format!(
            "dispatch retry limit reached task={} retry_count={} max_dispatch_retries={}",
            task_id, retry_count, max_dispatch_retries
        );
        let _ = append_coordinator_event_with_severity(
            repo_root,
            "dispatch_retry_limit_reached",
            task_id,
            "dev",
            "failed",
            &msg,
            "warning",
        );
        if let Some(log) = logger {
            let _ = log.note(format!("- {}", msg));
        }
        return Ok(blocked);
    }

    if cooldown_seconds > 0 {
        state.dispatch_retry_not_before.insert(
            task_id.to_string(),
            Instant::now() + Duration::from_secs(cooldown_seconds),
        );
    }
    Ok(false)
}

fn resolve_merge_timeout_seconds(
    env_cfg: &CoordinatorEnvConfig,
    coordinator: Option<&crate::config::CoordinatorConfig>,
) -> usize {
    let cfg = CoordinatorConfigResolved::resolve(coordinator);
    env_cfg
        .merge_job_timeout_seconds
        .unwrap_or(cfg.merge_job_timeout_seconds)
}

pub(super) fn retry_count_for_task(registry: &serde_json::Value, task_id: &str) -> usize {
    crate::coordinator::model::TaskRegistry::from_value(registry)
        .ok()
        .and_then(|typed| {
            typed
                .find_task(task_id)
                .map(|task| task.task_runtime.retries_count())
        })
        .unwrap_or(0)
}

pub(super) fn mark_task_merged_from_merge_gate(
    registry: &mut serde_json::Value,
    task_id: &str,
    now: &str,
) -> Result<()> {
    let mut typed = TaskRegistry::from_value(registry)?;
    let task = typed
        .find_task_mut(task_id)
        .ok_or_else(|| MaccError::Coordinator {
            code: "task_not_found",
            message: format!("Task '{}' not found in registry", task_id),
        })?;
    task.set_workflow_state(crate::coordinator::WorkflowState::Merged);
    task.clear_assignment();
    let runtime = task.ensure_runtime();
    runtime.status = Some("idle".to_string());
    runtime.pid = None;
    runtime.started_at = None;
    runtime.current_phase = None;
    runtime.merge_result_pending = Some(false);
    task.touch_state_changed(now);
    typed.recompute_resource_locks(now);
    typed.set_updated_at(now.to_string());
    *registry = typed.to_value()?;
    Ok(())
}

/// Persist the volatile throttle registry to SQLite so cooldowns survive restart.
fn persist_throttle_registry(
    repo_root: &Path,
    registry: &crate::coordinator::rate_limit::ToolThrottleRegistry,
) {
    let paths = crate::ProjectPaths::from_root(repo_root);
    let storage_paths =
        crate::coordinator_storage::CoordinatorStoragePaths::from_project_paths(&paths);
    let sqlite = crate::coordinator_storage::SqliteStorage::new(storage_paths);
    let _ = sqlite.save_throttle_registry(registry);
}

/// Detect transient tool unavailability in a phase failure reason string.
///
/// The phase executor returns a free-form error string. We look for known
/// markers that indicate the tool is temporarily unavailable:
/// - E602 quota exhaustion (hard quota limit)
/// - E601 rate-limit / overloaded
/// - E603 session conflict
/// - E101 timeout / network errors
/// - Provider-specific patterns (TerminalQuotaError, 429, 529, etc.)
fn is_tool_unavailability_error(reason: &str) -> bool {
    // Canonical error codes
    if reason.contains(E602_QUOTA_EXHAUSTED) {
        return true;
    }
    for code in &["E601", "E603", "E101"] {
        if reason.contains(code) {
            return true;
        }
    }
    let lower = reason.to_ascii_lowercase();
    // Quota exhaustion patterns
    if lower.contains("terminalquotaerror") {
        return true;
    }
    if lower.contains("quota") && (lower.contains("exhaust") || lower.contains("exceeded")) {
        return true;
    }
    if lower.contains("exhausted your capacity") {
        return true;
    }
    // Session daily-limit message: "You're out of extra usage · resets <time>"
    if lower.contains("out of extra usage") {
        return true;
    }
    // Rate-limit / overloaded patterns
    if lower.contains("rate limit") || lower.contains("rate_limit") || lower.contains("ratelimit") {
        return true;
    }
    if lower.contains("too many requests") || lower.contains("429") {
        return true;
    }
    if lower.contains("overloaded") || lower.contains("529") {
        return true;
    }
    // Session conflict patterns
    if lower.contains("session conflict") || lower.contains("already in use") {
        return true;
    }
    // Timeout / network patterns
    if lower.contains("timed out") || lower.contains("timeout") {
        return true;
    }
    if lower.contains("connection refused")
        || lower.contains("connection reset")
        || lower.contains("network error")
    {
        return true;
    }
    false
}

/// Default cooldown (seconds) per error code when the provider doesn't specify
/// a `retry_after_seconds` value.
fn default_cooldown_for_code(error_code: &str) -> u64 {
    match error_code {
        "E602" => 3600, // Quota exhaustion: 1 hour
        "E601" => 120,  // Rate-limit / overloaded: 2 minutes
        "E603" => 60,   // Session conflict: 1 minute
        "E101" => 30,   // Timeout / network: 30 seconds
        _ => 120,       // Unknown transient: 2 minutes
    }
}

/// Transient error codes that should trigger the tool-unavailability handler.
const TRANSIENT_ERROR_CODES: &[&str] = &["E601", "E602", "E603", "E101"];

/// Try to classify a phase failure reason via the per-adapter error normalizers.
/// Returns `(error_code, cooldown_seconds)` if the error is a transient
/// tool-unavailability error (E601, E602, E603, E101).
fn extract_cooldown_from_reason(
    reason: &str,
    tool_id: &str,
    normalizer_registry: &crate::coordinator::error_normalizer::NormalizerRegistry,
) -> Option<(String, u64)> {
    let normalizer = normalizer_registry.get(tool_id)?;
    let te = normalizer.normalize(1, reason, reason)?;
    if TRANSIENT_ERROR_CODES.contains(&te.error_code.as_str()) {
        let cooldown = te
            .retry_after_seconds
            .unwrap_or_else(|| default_cooldown_for_code(&te.error_code));
        Some((te.error_code, cooldown))
    } else {
        None
    }
}

/// Reset a worktree to a known-good commit, discarding any uncommitted or
/// partially committed changes made during a failed phase attempt.
fn rollback_worktree_to_sha(worktree_path: &Path, target_sha: &str) -> bool {
    let reset = crate::git::run_git_output_mapped(
        worktree_path,
        &["reset", "--hard", target_sha],
        "rollback worktree to pre-phase SHA",
    );
    let clean = crate::git::run_git_output_mapped(
        worktree_path,
        &["clean", "-fd"],
        "clean worktree after rollback",
    );
    reset.map(|o| o.status.success()).unwrap_or(false)
        && clean.map(|o| o.status.success()).unwrap_or(false)
}

/// Handle transient tool unavailability detected during a phase
/// (review/fix).
///
/// Covers E601 (rate-limit), E602 (quota exhaustion), E603 (session conflict),
/// E101 (timeout/network), and any other error that imposes a waiting period or
/// partial/total tool unavailability.
///
/// **Worktree recycling**: when committed work exists on the worktree branch,
/// it is preserved (it cost tokens) and the task is handed off to a fallback
/// tool or kept waiting until the throttle expires.
///
/// Three cases:
///
/// | Committed work? | Fallback tool? | Action                                    |
/// |-----------------|----------------|-------------------------------------------|
/// | Yes             | Yes            | Recycle worktree → fallback tool, no delay |
/// | Yes             | No             | Keep worktree, delay until throttle expires |
/// | No              | —              | Detach worktree, re-queue as Todo          |
#[allow(clippy::too_many_arguments)]
fn handle_phase_tool_unavailability(
    repo_root: &Path,
    registry: &mut serde_json::Value,
    state: &mut CoordinatorRunState,
    task_snapshot: &Task,
    task_id: &str,
    phase: &str,
    reason: &str,
    pre_phase_sha: Option<&str>,
    worktree_path_str: Option<&str>,
    now: &str,
    enabled_tools: &[String],
    logger: Option<&dyn CoordinatorLog>,
) -> Result<()> {
    let tool_id = task_snapshot.tool.as_deref().unwrap_or("unknown");
    let base_branch = task_snapshot.base_branch("master");

    // ── Step 1: roll back worktree to pre-phase HEAD ───────────────────
    let rolled_back = match (worktree_path_str, pre_phase_sha) {
        (Some(wp), Some(sha)) => {
            let ok = rollback_worktree_to_sha(Path::new(wp), sha);
            if let Some(log) = logger {
                let _ = log.note(format!(
                    "- tool-unavail rollback task={} worktree={} sha={} ok={}",
                    task_id, wp, sha, ok
                ));
            }
            ok
        }
        _ => {
            if let Some(log) = logger {
                let _ = log.note(format!(
                    "- tool-unavail rollback skipped task={} (no worktree/sha)",
                    task_id
                ));
            }
            false
        }
    };

    // ── Step 2: compute cooldown and register tool throttle ────────────
    let (detected_error_code, cooldown) =
        extract_cooldown_from_reason(reason, tool_id, &state.normalizer_registry)
            .unwrap_or_else(|| ("E602".to_string(), 3600));
    let now_epoch = chrono::DateTime::parse_from_rfc3339(now)
        .map(|dt| dt.timestamp() as u64)
        .unwrap_or(0);
    let throttled_until = now_epoch + cooldown;

    let ts = ToolThrottleState {
        tool_id: tool_id.to_string(),
        throttled_until,
        consecutive_429_count: 1,
        backoff_seconds: cooldown,
        last_rate_limit_info: Some(RateLimitInfo {
            tool_id: tool_id.to_string(),
            error_code: detected_error_code,
            retry_after_seconds: Some(cooldown),
            detected_at: now_epoch,
            source_header: None,
        }),
    };
    state.throttle_registry.insert(tool_id.to_string(), ts);
    persist_throttle_registry(repo_root, &state.throttle_registry);

    // ── Step 3: check for committed work on the worktree ───────────────
    let has_committed_work = worktree_path_str
        .map(|wp| crate::git::has_commits_ahead(Path::new(wp), &base_branch))
        .unwrap_or(false);

    let has_fallback_tool = enabled_tools.iter().any(|t| {
        t != tool_id
            && !crate::coordinator::rate_limit::is_tool_throttled(&state.throttle_registry, t, now)
    });

    // ── Step 4: apply the appropriate recycling strategy ───────────────
    let delayed_until_str = chrono::DateTime::parse_from_rfc3339(now)
        .ok()
        .and_then(|dt| dt.checked_add_signed(chrono::Duration::seconds(cooldown as i64)))
        .map(|dt| dt.to_rfc3339_opts(chrono::SecondsFormat::Secs, true))
        .unwrap_or_default();

    let strategy: &str;

    let mut typed = TaskRegistry::from_value(registry)?;
    if let Some(task) = typed.find_task_mut(task_id) {
        // First pass: write all runtime fields while borrow is active.
        {
            let runtime = task.ensure_runtime();
            runtime.pid = None;
            runtime.completion_kind = None;
            runtime.last_error = Some(format!(
                "Tool unavailable during {} phase; rolled_back={}; cooldown {}s",
                phase, rolled_back, cooldown
            ));

            if has_committed_work && has_fallback_tool {
                runtime.delayed_until = None;
                runtime.set_status(crate::coordinator::RuntimeStatus::PhaseDone);
            } else if has_committed_work {
                runtime.delayed_until = Some(delayed_until_str.clone());
            } else {
                runtime.delayed_until = if has_fallback_tool {
                    None
                } else {
                    Some(delayed_until_str.clone())
                };
                runtime.set_status(crate::coordinator::RuntimeStatus::Idle);
                runtime.current_phase = None;
            }
        }
        // Second pass: write direct task fields (borrow on runtime dropped).
        if has_committed_work && has_fallback_tool {
            // ── Case 1: recycle worktree → fallback tool ────────────
            strategy = "recycle_to_fallback";
            let fallback = enabled_tools
                .iter()
                .find(|t| {
                    t.as_str() != tool_id
                        && !crate::coordinator::rate_limit::is_tool_throttled(
                            &state.throttle_registry,
                            t,
                            now,
                        )
                })
                .cloned()
                .unwrap_or_default();
            task.tool = Some(fallback);
            task.touch_state_changed(now);
        } else if has_committed_work {
            // ── Case 2: wait for throttle to expire ────────────────
            strategy = "wait_for_throttle";
            task.touch_state_changed(now);
        } else {
            // ── Case 3: no committed work → re-queue ───────────────
            strategy = "requeue_fresh";
            task.worktree = None;
            task.assignee = None;
            task.tool = None;
            task.set_workflow_state(crate::coordinator::WorkflowState::Todo);
            task.touch_state_changed(now);
        }
    } else {
        strategy = "task_not_found";
    }
    *registry = typed.to_value()?;

    // ── Log event ──────────────────────────────────────────────────────
    let msg = format!(
        "phase_tool_unavailability task={} phase={} tool={} cooldown={}s rolled_back={} \
         has_committed_work={} strategy={}",
        task_id, phase, tool_id, cooldown, rolled_back, has_committed_work, strategy,
    );
    let _ = append_coordinator_event_with_severity(
        repo_root,
        "phase_tool_unavailability",
        task_id,
        phase,
        match strategy {
            "recycle_to_fallback" => "recycled",
            "wait_for_throttle" => "waiting",
            _ => "requeued",
        },
        &msg,
        "warning",
    );
    if let Some(log) = logger {
        let _ = log.note(format!("- {}", msg));
    }

    Ok(())
}

#[cfg(test)]
fn should_emit_priority_zero_dispatch_skip(state: &mut CoordinatorRunState, task_id: &str) -> bool {
    if state.last_priority_zero_dispatch_block_task_id.as_deref() == Some(task_id) {
        return false;
    }
    state.last_priority_zero_dispatch_block_task_id = Some(task_id.to_string());
    true
}

async fn switch_worktree_to_base_after_merge(
    repo_root: &Path,
    task: &Task,
    logger: Option<&dyn CoordinatorLog>,
) -> Result<()> {
    let task_id = task.id.as_str();
    let worktree_path = task.worktree_path().unwrap_or_default();
    if task_id.is_empty() || worktree_path.is_empty() {
        return Ok(());
    }
    let base_branch = task.base_branch("master");

    let wt = Path::new(worktree_path);
    let failed_step = prepare_clean_worktree(
        wt,
        &base_branch,
        SanitizeOptions {
            fetch_remote: true,
            fail_on_fetch_error: false,
            tag_abandoned: true,
        },
    )
    .await?;

    if let Some(step) = failed_step {
        if step == "fetch_origin" {
            let msg = format!(
                "worktree switch warning task={} path={} base={} reason=fetch_failed",
                task_id, worktree_path, base_branch
            );
            let _ = append_coordinator_event_with_severity(
                repo_root,
                "worktree_switch",
                task_id,
                "merge",
                "warning",
                &msg,
                "warning",
            );
            if let Some(log) = logger {
                let _ = log.note(format!("- {}", msg));
            }
            return Ok(());
        }
        if step == "reset_hard_base_branch" {
            let msg = format!(
                "worktree switch warning task={} path={} base={} reason=reset_hard_failed",
                task_id, worktree_path, base_branch
            );
            let _ = append_coordinator_event_with_severity(
                repo_root,
                "worktree_switch",
                task_id,
                "merge",
                "warning",
                &msg,
                "warning",
            );
            if let Some(log) = logger {
                let _ = log.note(format!("- {}", msg));
            }
            return Ok(());
        }
        let msg = format!(
            "worktree switch skipped task={} path={} base={} reason=checkout_failed",
            task_id, worktree_path, base_branch
        );
        let _ = append_coordinator_event_with_severity(
            repo_root,
            "worktree_switch",
            task_id,
            "merge",
            "failed",
            &msg,
            "warning",
        );
        if let Some(log) = logger {
            let _ = log.note(format!("- {}", msg));
        }
        return Ok(());
    }
    let msg = format!(
        "worktree switched to base task={} path={} base={}",
        task_id, worktree_path, base_branch
    );
    let _ = append_coordinator_event_with_severity(
        repo_root,
        "worktree_switch",
        task_id,
        "merge",
        "success",
        &msg,
        "info",
    );
    if let Some(log) = logger {
        let _ = log.note(format!("- {}", msg));
    }
    Ok(())
}

pub fn sync_registry_from_prd_native(
    repo_root: &Path,
    prd_file: &Path,
    logger: Option<&dyn CoordinatorLog>,
) -> Result<()> {
    let registry_value =
        crate::coordinator::state::coordinator_state_registry_load(repo_root, &BTreeMap::new())?;
    let mut registry = TaskRegistry::from_value(&registry_value)?;
    let raw_prd = std::fs::read_to_string(prd_file).map_err(|e| MaccError::Io {
        path: prd_file.to_string_lossy().into(),
        action: "read coordinator prd".into(),
        source: e,
    })?;
    let prd: PrdInput = serde_json::from_str(&raw_prd).map_err(|e| {
        MaccError::Validation(format!("Failed to parse PRD {}: {}", prd_file.display(), e))
    })?;
    let mut by_id: HashMap<String, Task> = registry
        .tasks
        .iter()
        .cloned()
        .map(|task| (task.id.clone(), task))
        .collect();

    let mut merged = Vec::new();
    for prd_task in prd.tasks {
        let id = prd_task.id.clone();
        if id.is_empty() {
            continue;
        }
        let mut task = by_id.remove(&id).unwrap_or_else(|| Task {
            id: id.clone(),
            state: "todo".to_string(),
            ..Task::default()
        });
        task.id = id;
        task.title = prd_task.title.clone();
        task.priority = prd_task.priority.clone();
        task.category = prd_task.category.clone();
        task.scope = prd_task.scope.clone();
        task.base_branch = prd_task.base_branch.clone();
        task.coordinator_tool = prd_task.coordinator_tool.clone();
        task.dependencies = prd_task.dependencies.clone();
        task.exclusive_resources = prd_task.exclusive_resources.clone();
        task.extra.retain(|key, _| {
            !matches!(
                key.as_str(),
                "description"
                    | "objective"
                    | "result"
                    | "steps"
                    | "notes"
                    | "category"
                    | "dependencies"
                    | "base_branch"
                    | "coordinator_tool"
                    | "scope"
            )
        });
        for (key, value) in prd_task.extra {
            task.extra.insert(key, value);
        }
        let runtime = task.ensure_runtime();
        if runtime.status.is_none() {
            runtime.status = Some("idle".to_string());
        }
        if runtime.merge_result_pending.is_none() {
            runtime.merge_result_pending = Some(false);
        }
        if runtime.merge_result_file.is_none() {
            runtime.merge_result_file = None;
        }
        task.updated_at = Some(now_iso_coordinator());
        merged.push(task);
    }

    let tasks_changed = registry.tasks != merged;
    registry.tasks = merged;
    registry.recompute_resource_locks(&now_iso_coordinator());
    registry.set_updated_at(now_iso_coordinator());
    crate::coordinator::state::coordinator_state_registry_save(
        repo_root,
        &BTreeMap::new(),
        &registry.to_value()?,
    )?;

    if let Some(log) = logger {
        use std::sync::atomic::{AtomicU64, Ordering};
        static LAST_LOG_TS: AtomicU64 = AtomicU64::new(0);
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
        let last = LAST_LOG_TS.load(Ordering::Relaxed);

        if tasks_changed || now.saturating_sub(last) >= 300 {
            let _ = log.note(format!(
                "Registry synced from PRD (tasks={})",
                registry.tasks.len()
            ));
            LAST_LOG_TS.store(now, Ordering::Relaxed);
        }
    }
    Ok(())
}

pub async fn advance_tasks_native(
    repo_root: &Path,
    env_cfg: &CoordinatorEnvConfig,
    coordinator: Option<&crate::config::CoordinatorConfig>,
    coordinator_tool_override: Option<&str>,
    phase_runner_max_attempts: usize,
    state: &mut CoordinatorRunState,
    logger: Option<&dyn CoordinatorLog>,
) -> Result<coordinator_engine::AdvanceResult> {
    let cfg = CoordinatorConfigResolved::resolve(coordinator);
    let mut registry =
        crate::coordinator::state::coordinator_state_registry_load(repo_root, &BTreeMap::new())?;
    let registry_snapshot = TaskRegistry::from_value(&registry)?;
    let mut progressed = false;
    let blocked_merge: Option<(String, String)> = None;
    let now = now_iso_coordinator();
    let merge_timeout = resolve_merge_timeout_seconds(env_cfg, coordinator);
    // Derive enabled tools for fallback routing in E602 handling.
    let enabled_tools: Vec<String> = coordinator
        .map(|c| c.tool_priority.clone())
        .unwrap_or_default();
    let active_merge_ids = state
        .active_merge_jobs
        .keys()
        .cloned()
        .collect::<HashSet<_>>();
    let max_review_cycles = env_cfg.max_review_cycles.or(cfg.max_review_cycles);
    let actions = coordinator_engine::build_advance_actions(
        &registry,
        &active_merge_ids,
        &now,
        max_review_cycles,
    )?;
    if !actions.is_empty() {
        if let Some(log) = logger {
            let _ = log.note(format!("- Advance started (actions={})", actions.len()));
        }
    }
    for action in actions {
        match action {
            coordinator_engine::AdvanceTaskAction::RunPhase {
                task_id,
                mode,
                transition,
            } => {
                let task_snapshot =
                    registry_snapshot
                        .find_task(&task_id)
                        .cloned()
                        .ok_or_else(|| {
                            MaccError::Validation(format!(
                                "Task '{}' not found while advancing phase",
                                task_id
                            ))
                        })?;

                // ── Part A: snapshot HEAD before the phase so we can roll back
                // if the tool hits quota exhaustion mid-phase. ──────────────────
                let worktree_path_str = task_snapshot.worktree_path().map(|s| s.to_string());
                let pre_phase_sha = worktree_path_str
                    .as_deref()
                    .and_then(|wp| crate::git::head_commit(Path::new(wp)).ok());

                let executor = NativePhaseExecutor { repo_root, logger };
                if mode == "review" {
                    // block_in_place: the phase runner is synchronous blocking I/O (spawns
                    // an external process and waits). Running it directly in the async
                    // executor would seize the tokio thread for minutes, preventing heartbeat
                    // monitoring, merge detection, and rate-limit timers from firing.
                    match tokio::task::block_in_place(|| {
                        coordinator_runtime::run_review_phase(
                            &executor,
                            &task_snapshot,
                            coordinator_tool_override,
                            phase_runner_max_attempts,
                        )
                    })? {
                        Ok(verdict) => {
                            let verdict_status = match verdict {
                                coordinator_engine::ReviewVerdict::Ok => "ok",
                                coordinator_engine::ReviewVerdict::ChangesRequested => {
                                    "changes_requested"
                                }
                            };
                            append_coordinator_event(
                                repo_root,
                                "review_done",
                                &task_id,
                                "review",
                                verdict_status,
                                &format!("Review verdict for task {}: {}", task_id, verdict_status),
                            )?;
                            coordinator_engine::apply_phase_outcome_in_registry(
                                &mut registry,
                                &task_id,
                                mode,
                                transition,
                                Some(verdict),
                                None,
                                &now,
                            )?
                        }
                        Err(reason) => {
                            if is_tool_unavailability_error(&reason) {
                                handle_phase_tool_unavailability(
                                    repo_root,
                                    &mut registry,
                                    state,
                                    &task_snapshot,
                                    &task_id,
                                    mode,
                                    &reason,
                                    pre_phase_sha.as_deref(),
                                    worktree_path_str.as_deref(),
                                    &now,
                                    &enabled_tools,
                                    logger,
                                )?;
                            } else {
                                coordinator_engine::apply_phase_outcome_in_registry(
                                    &mut registry,
                                    &task_id,
                                    mode,
                                    transition,
                                    None,
                                    Some(&reason),
                                    &now,
                                )?;
                            }
                        }
                    }
                } else {
                    match tokio::task::block_in_place(|| {
                        coordinator_runtime::run_phase(
                            &executor,
                            &task_snapshot,
                            mode,
                            coordinator_tool_override,
                            phase_runner_max_attempts,
                        )
                    })? {
                        Ok(_) => coordinator_engine::apply_phase_outcome_in_registry(
                            &mut registry,
                            &task_id,
                            mode,
                            transition,
                            None,
                            None,
                            &now,
                        )?,
                        Err(reason) => {
                            if is_tool_unavailability_error(&reason) {
                                handle_phase_tool_unavailability(
                                    repo_root,
                                    &mut registry,
                                    state,
                                    &task_snapshot,
                                    &task_id,
                                    mode,
                                    &reason,
                                    pre_phase_sha.as_deref(),
                                    worktree_path_str.as_deref(),
                                    &now,
                                    &enabled_tools,
                                    logger,
                                )?;
                            } else {
                                coordinator_engine::apply_phase_outcome_in_registry(
                                    &mut registry,
                                    &task_id,
                                    mode,
                                    transition,
                                    None,
                                    Some(&reason),
                                    &now,
                                )?;
                            }
                        }
                    }
                }
                progressed = true;
            }
            coordinator_engine::AdvanceTaskAction::QueueMerge {
                task_id,
                branch,
                base,
                merge_context,
            } => {
                // Only one merge at a time — all merges operate on the same
                // repo_root so concurrent merges cause races (dirty worktree,
                // git lock conflicts, lost merge results).  Remaining merges
                // will be picked up in the next advance cycle.
                if !state.active_merge_jobs.is_empty() {
                    if let Some(log) = logger {
                        let _ = log.note(format!(
                            "- Merge deferred task={} reason=another_merge_active",
                            task_id
                        ));
                    }
                    continue;
                }
                if let Some(log) = logger {
                    let _ = log.note(format!(
                        "- Merge start task={} branch={} base={}",
                        task_id, branch, base
                    ));
                }
                let repo = repo_root.to_path_buf();
                let task_for_worker = task_id.clone();
                let branch_for_worker = branch.clone();
                let base_for_worker = base.clone();

                let merge_ai_fix = env_cfg.merge_ai_fix.unwrap_or(cfg.merge_ai_fix);
                let merge_hook_timeout = env_cfg
                    .merge_hook_timeout_seconds
                    .or(Some(cfg.merge_hook_timeout_seconds));

                coordinator_runtime::spawn_merge_job(
                    &task_id,
                    &state.merge_event_tx,
                    &mut state.merge_join_set,
                    merge_timeout,
                    move || {
                        coordinator_runtime::merge_task_with_policy_native(
                            &repo,
                            &task_for_worker,
                            &branch_for_worker,
                            &base_for_worker,
                            merge_ai_fix,
                            merge_hook_timeout,
                            &merge_context,
                            |event_type, task_id, phase, status, message, severity| {
                                let _ = append_coordinator_event_with_severity(
                                    &repo, event_type, task_id, phase, status, message, severity,
                                );
                            },
                        )
                    },
                )
                .await?;
                state.active_merge_jobs.insert(
                    task_id.clone(),
                    CoordinatorMergeJob {
                        started_at: std::time::Instant::now(),
                    },
                );
                if let Some(log) = logger {
                    let _ = log.note(format!("- Merge queued task={}", task_id));
                }
                progressed = true;
            }
            coordinator_engine::AdvanceTaskAction::BlockNoBranch { task_id } => {
                // Task reached merge-ready state but has no branch (worktree
                // cleared by ghost cleanup before the completion was applied).
                // Transition to blocked so the coordinator can make progress
                // instead of spinning with active > 0 forever.
                if let Some(log) = logger {
                    let _ = log.note(format!(
                        "- Blocking task={} reason=no_branch_after_completion",
                        task_id
                    ));
                }
                let _ = coordinator_engine::apply_merge_result_in_registry(
                    &mut registry,
                    &task_id,
                    false,
                    "no branch recorded after completion; worktree was cleared by ghost cleanup",
                    &now,
                );
                progressed = true;
            }
        }
    }
    if progressed {
        if let Some(log) = logger {
            let _ = log.note("- Advance done".to_string());
        }
    }
    recompute_resource_locks_from_tasks(&mut registry);
    set_registry_updated_at(&mut registry);
    crate::coordinator::state::coordinator_state_registry_save(
        repo_root,
        &BTreeMap::new(),
        &registry,
    )?;
    Ok(coordinator_engine::AdvanceResult {
        progressed,
        blocked_merge,
    })
}

pub async fn monitor_active_jobs_native(
    repo_root: &Path,
    env_cfg: &CoordinatorEnvConfig,
    coordinator: Option<&crate::config::CoordinatorConfig>,
    state: &mut CoordinatorRunState,
    max_attempts: usize,
    phase_timeout_seconds: usize,
    logger: Option<&dyn CoordinatorLog>,
) -> Result<()> {
    let cfg = CoordinatorConfigResolved::resolve(coordinator);
    ensure_performer_ipc_listener(repo_root, state, logger).await?;
    consume_runtime_events(repo_root, state, logger)?;
    apply_runtime_event_bus_updates(repo_root, env_cfg, coordinator, state, logger)?;
    apply_stale_heartbeat_policy(repo_root, env_cfg, coordinator, logger)?;
    force_kill_stale_failures(repo_root, env_cfg, coordinator, state, logger);
    let retry_codes = resolve_error_code_retry_list(env_cfg, coordinator);
    let retry_max = resolve_error_code_retry_max(env_cfg, coordinator);
    loop {
        match state.event_rx.try_recv() {
            Ok(evt) => {
                if let Some(log) = logger {
                    let _ = log.note(format!(
                        "- Lifecycle task={} stage=monitor status=job_exit_received success={} detail={}",
                        evt.task_id, evt.success, evt.status_text
                    ));
                }
                let maybe_job = state.active_jobs.remove(&evt.task_id);
                let Some(job) = maybe_job else {
                    continue;
                };
                state.last_session_activity_at.insert(
                    job.worktree_path.to_string_lossy().to_string(),
                    chrono::Utc::now().timestamp(),
                );
                let mut registry = crate::coordinator::state::coordinator_state_registry_load(
                    repo_root,
                    &BTreeMap::new(),
                )?;
                // On failure, read the performer task log from the worktree
                // and feed it to the per-adapter error normalizer.  This
                // populates normalizer_input so that the canonical error
                // classification pipeline actually runs in production.
                let normalizer_input = if !evt.success {
                    crate::coordinator::runtime::read_performer_log_tail(
                        &job.worktree_path,
                        &evt.task_id,
                        8192,
                    )
                    .map(|log_content| coordinator_engine::NormalizerInput {
                        exit_code: 1,
                        stderr: log_content.clone(),
                        stdout: log_content,
                    })
                } else {
                    None
                };
                let completion = coordinator_engine::apply_job_completion_in_registry(
                    &mut registry,
                    &evt.task_id,
                    &coordinator_engine::JobCompletionInput {
                        success: evt.success,
                        attempt: job.attempt,
                        max_attempts: max_attempts.max(1),
                        timed_out: evt.timed_out,
                        phase_timeout_seconds,
                        elapsed_seconds: job.started_at.elapsed().as_secs(),
                        status_text: evt.status_text.clone(),
                        completion_kind: evt.completion_kind,
                        error_code: evt.error_code.clone(),
                        error_origin: evt.error_origin.clone(),
                        error_message: evt.error_message.clone(),
                        auto_retry_error_codes: retry_codes.clone(),
                        auto_retry_max: retry_max,
                        backoff_base_seconds: resolve_rate_limit_backoff_base_seconds(
                            env_cfg,
                            coordinator,
                        ),
                        backoff_max_seconds: resolve_rate_limit_backoff_max_seconds(
                            env_cfg,
                            coordinator,
                        ),
                        normalizer_input,
                    },
                    &state.normalizer_registry,
                    &now_iso_coordinator(),
                )?;
                if let Some(log) = logger {
                    if let Some(source) = evt.completion_details_source.as_deref() {
                        let _ = log.note(format!(
                            "- completion details source={} task={} kind={}",
                            source,
                            evt.task_id,
                            evt.completion_kind
                                .map(|kind| kind.as_str())
                                .unwrap_or("unknown")
                        ));
                    }
                    let _ = log.note(format!(
                        "- Lifecycle task={} stage=monitor status=completion_applied new_state={} should_retry={}",
                        evt.task_id, completion.status_label, completion.should_retry
                    ));
                }
                let refreshed_session_id = refresh_task_active_session_id_in_registry(
                    &mut registry,
                    repo_root,
                    &evt.task_id,
                    &job.tool,
                    &job.worktree_path,
                )?;
                let active_session_id = refreshed_session_id
                    .or_else(|| task_active_session_id_from_registry(&registry, &evt.task_id));
                recompute_resource_locks_from_tasks(&mut registry);
                set_registry_updated_at(&mut registry);
                crate::coordinator::state::coordinator_state_registry_save(
                    repo_root,
                    &BTreeMap::new(),
                    &registry,
                )?;
                let completion_event_message = format!(
                    "task {} completed status={} attempt={} detail={}",
                    evt.task_id, completion.status_label, job.attempt, evt.status_text
                );
                let _ = append_task_lifecycle_event_with_session(
                    repo_root,
                    "task_completed",
                    &evt.task_id,
                    "dev",
                    completion.status_label,
                    &completion_event_message,
                    active_session_id.as_deref(),
                );
                aggregate_performer_logs_after_completion(repo_root, &evt.task_id, logger);
                // RL-ROUTE-005 / RL-THROTTLE-006: maintain throttle registry and
                // adjust effective concurrency based on rate-limit signals.
                if completion.status_label == "rate_limit_backoff" {
                    // Extract the throttle state written by the engine and
                    // cache it in the volatile registry so `pick_tool()` can
                    // skip this tool on the next dispatch cycle.
                    let task_typed = crate::coordinator::model::TaskRegistry::from_value(&registry)
                        .ok()
                        .and_then(|reg| reg.tasks.into_iter().find(|t| t.id == evt.task_id));
                    if let Some(task) = task_typed {
                        if let Some(ts_val) = task.task_runtime.extra.get("throttle_state") {
                            if let Ok(ts) = serde_json::from_value::<
                                crate::coordinator::rate_limit::ToolThrottleState,
                            >(ts_val.clone())
                            {
                                state.throttle_registry.insert(job.tool.clone(), ts);
                                persist_throttle_registry(repo_root, &state.throttle_registry);
                            }
                        }
                    }
                    if resolve_rate_limit_throttle_parallel(env_cfg, coordinator) {
                        let new_val = state.reduce_parallel();
                        let msg = format!(
                            "concurrency_adjusted task={} tool={} reason=rate_limit_backoff effective_max_parallel={}",
                            evt.task_id, job.tool, new_val
                        );
                        let _ = append_coordinator_event_with_severity(
                            repo_root,
                            "concurrency_adjusted",
                            &evt.task_id,
                            "dev",
                            "info",
                            &msg,
                            "info",
                        );
                        if let Some(log) = logger {
                            let _ = log.note(format!("- {}", msg));
                        }
                    }
                } else if evt.success
                    || matches!(
                        completion.completion_kind,
                        Some(
                            crate::coordinator::PerformerCompletionKind::SuccessWithChanges
                                | crate::coordinator::PerformerCompletionKind::SuccessWithoutChanges
                                | crate::coordinator::PerformerCompletionKind::AlreadySatisfied
                        )
                    )
                {
                    // RL-ROUTE-005: clear throttle on success so the tool is
                    // re-enabled for future tasks.
                    if state.throttle_registry.contains_key(&job.tool) {
                        state.throttle_registry.remove(&job.tool);
                        persist_throttle_registry(repo_root, &state.throttle_registry);
                        if resolve_rate_limit_throttle_parallel(env_cfg, coordinator) {
                            let new_val = state.restore_parallel();
                            let msg = format!(
                                "concurrency_adjusted task={} tool={} reason=rate_limit_cleared effective_max_parallel={}",
                                evt.task_id, job.tool, new_val
                            );
                            let _ = append_coordinator_event_with_severity(
                                repo_root,
                                "concurrency_adjusted",
                                &evt.task_id,
                                "dev",
                                "info",
                                &msg,
                                "info",
                            );
                            if let Some(log) = logger {
                                let _ = log.note(format!("- {}", msg));
                            }
                        }
                    }
                }
                if completion.status_label == "auto_retry" {
                    if let Some(log) = logger {
                        let _ = log.note(format!(
                            "- Task {} auto-retry queued detail={}",
                            evt.task_id, completion.detail
                        ));
                    }
                } else if completion.should_retry {
                    let salvage_attempt_msg = format!(
                        "pre-retry salvage check started task={} attempt={} status={}",
                        evt.task_id, job.attempt, completion.status_label
                    );
                    let _ = append_coordinator_event_with_severity(
                        repo_root,
                        "salvage_attempt",
                        &evt.task_id,
                        "dev",
                        "started",
                        &salvage_attempt_msg,
                        "info",
                    );
                    let salvage_result = coordinator_engine::salvage_check_in_registry(
                        &mut registry,
                        &evt.task_id,
                        repo_root,
                        env_cfg.merge_ai_fix.unwrap_or(cfg.merge_ai_fix),
                        env_cfg
                            .merge_hook_timeout_seconds
                            .or(Some(cfg.merge_hook_timeout_seconds)),
                        &now_iso_coordinator(),
                        |event_type, task_id, phase, status, detail, severity| {
                            let _ = append_coordinator_event_with_severity(
                                repo_root, event_type, task_id, phase, status, detail, severity,
                            );
                        },
                    )?;
                    match salvage_result {
                        coordinator_engine::SalvageResult::Merged => {
                            let msg = format!(
                                "pre-retry salvage merged committed work task={} base={}",
                                evt.task_id, job.base_branch
                            );
                            let _ = append_coordinator_event_with_severity(
                                repo_root,
                                "salvage_success",
                                &evt.task_id,
                                "dev",
                                "done",
                                &msg,
                                "info",
                            );
                            recompute_resource_locks_from_tasks(&mut registry);
                            set_registry_updated_at(&mut registry);
                            crate::coordinator::state::coordinator_state_registry_save(
                                repo_root,
                                &BTreeMap::new(),
                                &registry,
                            )?;
                            if let Some(log) = logger {
                                let _ = log.note(format!("- {}", msg));
                            }
                            continue;
                        }
                        coordinator_engine::SalvageResult::ConflictProceedRetry => {
                            let msg = format!(
                                "pre-retry salvage found commits but merge conflicted task={}; proceeding with retry",
                                evt.task_id
                            );
                            let _ = append_coordinator_event_with_severity(
                                repo_root,
                                "salvage_conflict",
                                &evt.task_id,
                                "dev",
                                "warning",
                                &msg,
                                "warning",
                            );
                            if let Some(log) = logger {
                                let _ = log.note(format!("- {}", msg));
                            }
                        }
                        coordinator_engine::SalvageResult::NoCommitsProceedRetry => {
                            let msg = format!(
                                "pre-retry salvage found no task commits task={}; proceeding with retry",
                                evt.task_id
                            );
                            let _ = append_coordinator_event_with_severity(
                                repo_root,
                                "salvage_no_commits",
                                &evt.task_id,
                                "dev",
                                "done",
                                &msg,
                                "info",
                            );
                            if let Some(log) = logger {
                                let _ = log.note(format!("- {}", msg));
                            }
                        }
                    }
                    let task_id = evt.task_id.clone();
                    let current_exe = std::env::current_exe().map_err(|e| {
                        MaccError::Validation(format!(
                            "Failed to resolve current executable path: {}",
                            e
                        ))
                    })?;
                    let typed_registry = TaskRegistry::from_value(&registry).ok();
                    let (claim_id, epoch) = if let Some(ref typed) = typed_registry {
                        if let Some(task) = typed.find_task(&task_id) {
                            (
                                task.task_runtime.claim_id.clone().unwrap_or_default(),
                                task.task_runtime.coordinator_epoch.unwrap_or(0),
                            )
                        } else {
                            (String::new(), 0)
                        }
                    } else {
                        (String::new(), 0)
                    };
                    let retry_pid = coordinator_runtime::spawn_performer_job(
                        &current_exe,
                        repo_root,
                        &task_id,
                        &job.base_branch,
                        &job.worktree_path,
                        &state.event_tx,
                        &mut state.join_set,
                        phase_timeout_seconds,
                        state.performer_ipc_addr.as_deref(),
                        &claim_id,
                        epoch,
                    )?;
                    state.active_jobs.insert(
                        task_id,
                        CoordinatorJob {
                            tool: job.tool,
                            base_branch: job.base_branch,
                            worktree_path: job.worktree_path,
                            attempt: job.attempt + 1,
                            started_at: std::time::Instant::now(),
                            pid: retry_pid,
                            failure_signaled_at: None,
                        },
                    );
                    if let Some(log) = logger {
                        let _ = log.note(format!(
                            "- Task {} retry scheduled attempt={}",
                            evt.task_id,
                            job.attempt + 1
                        ));
                    }
                } else if let Some(log) = logger {
                    let _ = log.note(format!(
                        "- Task {} completion status={} attempt={} detail={}",
                        evt.task_id, completion.status_label, job.attempt, evt.status_text
                    ));
                }
            }
            Err(tokio::sync::mpsc::error::TryRecvError::Empty) => break,
            Err(tokio::sync::mpsc::error::TryRecvError::Disconnected) => break,
        }
    }
    while let Some(joined) = state.join_set.try_join_next() {
        let _ = joined;
    }
    Ok(())
}

pub fn apply_runtime_event_bus_updates(
    repo_root: &Path,
    env_cfg: &CoordinatorEnvConfig,
    coordinator: Option<&crate::config::CoordinatorConfig>,
    state: &mut CoordinatorRunState,
    logger: Option<&dyn CoordinatorLog>,
) -> Result<usize> {
    #[derive(Default)]
    struct PendingRuntimeUpdate {
        last_heartbeat: Option<String>,
        status: Option<String>,
        phase: Option<String>,
        last_error: Option<String>,
        /// True when a terminal failure IPC event was received for this task.
        failure_signaled: bool,
    }

    let mut runtime_updates: HashMap<String, PendingRuntimeUpdate> = HashMap::new();
    loop {
        match state.runtime_event_bus_rx.try_recv() {
            Ok(event) => {
                if let Some(log) = logger {
                    let event_type = match &event.kind {
                        CoordinatorRuntimeEventKind::Heartbeat => "heartbeat",
                        CoordinatorRuntimeEventKind::TaskDispatched { .. } => "task_dispatched",
                        CoordinatorRuntimeEventKind::TaskCompleted { .. } => "task_completed",
                        CoordinatorRuntimeEventKind::Progress { .. } => "progress",
                        CoordinatorRuntimeEventKind::PhaseResult { .. } => "phase_result",
                        CoordinatorRuntimeEventKind::Failed { .. } => "failed",
                        // L4-EVENTS-001: reliability observability events are
                        // informational; no runtime state update is needed.
                        CoordinatorRuntimeEventKind::SalvageAttempted { .. } => "salvage_attempted",
                        CoordinatorRuntimeEventKind::SalvageMerged { .. } => "salvage_merged",
                        CoordinatorRuntimeEventKind::SalvageFailed { .. } => "salvage_failed",
                        CoordinatorRuntimeEventKind::MergeGateChecked { .. } => {
                            "merge_gate_checked"
                        }
                        CoordinatorRuntimeEventKind::MergeGateMerged { .. } => "merge_gate_merged",
                        CoordinatorRuntimeEventKind::BranchTaggedAbandoned { .. } => {
                            "branch_tagged_abandoned"
                        }
                        CoordinatorRuntimeEventKind::SyncUnmergedBranchFound { .. } => {
                            "sync_unmerged_branch_found"
                        }
                        CoordinatorRuntimeEventKind::SyncUnmergedBranchMerged { .. } => {
                            "sync_unmerged_branch_merged"
                        }
                        CoordinatorRuntimeEventKind::WorktreeHealthCheckFailed { .. } => {
                            "worktree_health_check_failed"
                        }
                        CoordinatorRuntimeEventKind::WorktreeOrphanCleaned { .. } => {
                            "worktree_orphan_cleaned"
                        }
                        CoordinatorRuntimeEventKind::DispatchRetryLimitReached { .. } => {
                            "dispatch_retry_limit_reached"
                        }
                    };
                    let _ = log.note(format!(
                        "- performer event received task={} type={} source={}",
                        event.task_id, event_type, event.source
                    ));
                }
                let update = runtime_updates.entry(event.task_id.clone()).or_default();
                update.last_heartbeat = Some(event.ts.clone());
                match event.kind {
                    CoordinatorRuntimeEventKind::Heartbeat => {}
                    CoordinatorRuntimeEventKind::TaskDispatched { .. }
                    | CoordinatorRuntimeEventKind::TaskCompleted { .. } => {}
                    CoordinatorRuntimeEventKind::Progress {
                        status,
                        phase,
                        message,
                    } => {
                        update.status = Some(status);
                        if let Some(phase) = phase {
                            update.phase = Some(phase);
                        }
                        if let Some(message) = message {
                            update.last_error = Some(message);
                        }
                    }
                    CoordinatorRuntimeEventKind::PhaseResult {
                        status,
                        phase,
                        message,
                    } => {
                        if let Some(log) = logger {
                            let _ = log.note(format!(
                                "- performer phase_result persisted task={} source={} status={}",
                                event.task_id, event.source, status
                            ));
                        }
                        if status == "failed" {
                            update.failure_signaled = true;
                        }
                        update.status = Some(status);
                        if let Some(phase) = phase {
                            update.phase = Some(phase);
                        }
                        if let Some(message) = message {
                            update.last_error = Some(message);
                        }
                    }
                    CoordinatorRuntimeEventKind::Failed { phase, message } => {
                        update.failure_signaled = true;
                        update.status = Some(
                            crate::coordinator::RuntimeStatus::Failed
                                .as_str()
                                .to_string(),
                        );
                        if let Some(phase) = phase {
                            update.phase = Some(phase);
                        }
                        if let Some(message) = message {
                            update.last_error = Some(message);
                        }
                    }
                    // L4-EVENTS-001: reliability observability events — no
                    // runtime state update required; they are informational.
                    CoordinatorRuntimeEventKind::SalvageAttempted { .. }
                    | CoordinatorRuntimeEventKind::SalvageMerged { .. }
                    | CoordinatorRuntimeEventKind::SalvageFailed { .. }
                    | CoordinatorRuntimeEventKind::MergeGateChecked { .. }
                    | CoordinatorRuntimeEventKind::MergeGateMerged { .. }
                    | CoordinatorRuntimeEventKind::BranchTaggedAbandoned { .. }
                    | CoordinatorRuntimeEventKind::SyncUnmergedBranchFound { .. }
                    | CoordinatorRuntimeEventKind::SyncUnmergedBranchMerged { .. }
                    | CoordinatorRuntimeEventKind::WorktreeHealthCheckFailed { .. }
                    | CoordinatorRuntimeEventKind::WorktreeOrphanCleaned { .. }
                    | CoordinatorRuntimeEventKind::DispatchRetryLimitReached { .. } => {}
                }
            }
            Err(tokio::sync::broadcast::error::TryRecvError::Empty) => break,
            Err(tokio::sync::broadcast::error::TryRecvError::Closed) => break,
            Err(tokio::sync::broadcast::error::TryRecvError::Lagged(skipped)) => {
                if let Some(log) = logger {
                    let _ = log.note(format!(
                        "- Runtime event bus lagged skipped={} events; continuing with newest",
                        skipped
                    ));
                }
                continue;
            }
        }
    }

    if runtime_updates.is_empty() {
        return Ok(0);
    }

    let registry_value =
        crate::coordinator::state::coordinator_state_registry_load(repo_root, &BTreeMap::new())?;
    let mut registry = TaskRegistry::from_value(&registry_value)?;
    let mut updated = 0usize;
    for task in &mut registry.tasks {
        let Some(update) = runtime_updates.get(task.id.as_str()) else {
            continue;
        };
        let runtime = task.ensure_runtime();
        if let Some(ts) = &update.last_heartbeat {
            runtime.last_heartbeat = Some(ts.clone());
        }
        if let Some(status) = &update.status {
            runtime.status = Some(status.clone());
            if matches!(status.as_str(), "phase_done" | "failed" | "stale" | "idle") {
                runtime.pid = None;
            }
        }
        if let Some(phase) = &update.phase {
            runtime.current_phase = Some(phase.clone());
        }
        if let Some(last_error) = &update.last_error {
            runtime.last_error = Some(last_error.clone());
        }
        updated += 1;
    }
    if updated == 0 {
        return Ok(0);
    }

    registry.set_updated_at(now_iso_coordinator());
    crate::coordinator::state::coordinator_state_registry_save(
        repo_root,
        &BTreeMap::new(),
        &registry.to_value()?,
    )?;

    // Mark active jobs that received a terminal failure IPC signal so the
    // force-kill grace period timer starts.
    let grace_seconds = resolve_force_kill_grace_seconds(env_cfg, coordinator);
    for (task_id, update) in &runtime_updates {
        if update.failure_signaled {
            if let Some(job) = state.active_jobs.get_mut(task_id.as_str()) {
                if job.failure_signaled_at.is_none() {
                    job.failure_signaled_at = Some(std::time::Instant::now());
                    if let Some(log) = logger {
                        let _ = log.note(format!(
                            "- Force-kill grace timer started task={} pid={:?} grace={}s",
                            task_id, job.pid, grace_seconds,
                        ));
                    }
                }
            }
        }
    }

    if let Some(log) = logger {
        state.heartbeat_updates_since_log += updated;
        let should_log = state
            .last_heartbeat_log_at
            .map(|last| last.elapsed() >= std::time::Duration::from_secs(30))
            .unwrap_or(true);
        if should_log {
            let _ = log.note(format!(
                "- Runtime event bus updates applied count={} (30s window)",
                state.heartbeat_updates_since_log
            ));
            state.last_heartbeat_log_at = Some(std::time::Instant::now());
            state.heartbeat_updates_since_log = 0;
        }
    }

    Ok(updated)
}

/// Force-kill performer processes that signaled failure via IPC but did not
/// exit within the grace period ([`FORCE_KILL_GRACE_SECONDS`]).  Sends
/// SIGKILL via `kill(2)` using the stored PID.  The async wrapper in
/// `spawn_performer_job` will observe the child exit and emit the normal
/// `CoordinatorJobEvent`, so the regular state-transition path still fires.
fn force_kill_stale_failures(
    repo_root: &Path,
    env_cfg: &CoordinatorEnvConfig,
    coordinator: Option<&crate::config::CoordinatorConfig>,
    state: &mut CoordinatorRunState,
    logger: Option<&dyn CoordinatorLog>,
) {
    let grace_seconds = resolve_force_kill_grace_seconds(env_cfg, coordinator);
    let grace = std::time::Duration::from_secs(grace_seconds);
    let mut killed: Vec<String> = Vec::new();

    for (task_id, job) in &state.active_jobs {
        let Some(signaled_at) = job.failure_signaled_at else {
            continue;
        };
        if signaled_at.elapsed() < grace {
            continue;
        }
        if let Some(pid) = job.pid {
            // Kill the entire process group (PGID == PID because we spawn
            // with process_group(0)).  This ensures orphaned tool subprocesses
            // are also killed, not just the shell wrapper.
            crate::coordinator::runtime::kill_process_group_sync(pid);
            if let Some(log) = logger {
                let _ = log.note(format!(
                    "- Force-killed performer task={} pid={} reason=failure_signaled_grace_expired elapsed={:.1}s",
                    task_id,
                    pid,
                    signaled_at.elapsed().as_secs_f64(),
                ));
            }
            let _ = append_coordinator_event_with_severity(
                repo_root,
                "force_kill",
                task_id,
                "dev",
                "failed",
                &format!(
                    "Force-killed performer pid={} after failure IPC grace period ({}s) expired",
                    pid, grace_seconds
                ),
                "warning",
            );
            killed.push(task_id.clone());
        } else if let Some(log) = logger {
            let _ = log.note(format!(
                "- Force-kill requested but no PID for task={} (already exited?)",
                task_id,
            ));
        }
    }

    // Clear the failure signal for killed processes so we don't re-kill.
    for task_id in &killed {
        if let Some(job) = state.active_jobs.get_mut(task_id.as_str()) {
            job.failure_signaled_at = None;
        }
    }
}

pub fn consume_heartbeat_events(
    repo_root: &Path,
    state: &mut CoordinatorRunState,
    logger: Option<&dyn CoordinatorLog>,
) -> Result<usize> {
    consume_runtime_events(repo_root, state, logger)
}

pub fn consume_runtime_events(
    repo_root: &Path,
    state: &mut CoordinatorRunState,
    logger: Option<&dyn CoordinatorLog>,
) -> Result<usize> {
    let project_paths = crate::ProjectPaths::from_root(repo_root);
    let storage_paths = crate::coordinator_storage::CoordinatorStoragePaths::from_project_paths(&project_paths);
    let sqlite = crate::coordinator_storage::SqliteStorage::new(storage_paths);
    let conn = sqlite.open()?;
    sqlite.init_schema(&conn)?;

    let sql_err = |e: rusqlite::Error| MaccError::Storage {
        backend: "sqlite",
        message: e.to_string(),
    };

    let last_event_id: Option<String> = match conn.query_row(
        "SELECT last_event_id FROM event_cursor WHERE stream = 'coordinator'",
        [],
        |row| row.get::<_, String>(0),
    ) {
        Ok(id) => Some(id),
        Err(rusqlite::Error::QueryReturnedNoRows) => None,
        Err(e) => return Err(sql_err(e)),
    };

    let mut last_seq: Option<i64> = None;
    if let Some(ref id) = last_event_id {
        last_seq = match conn.query_row(
            "SELECT seq FROM events WHERE event_id = ?1",
            [id],
            |row| row.get::<_, i64>(0),
        ) {
            Ok(seq) => Some(seq),
            Err(rusqlite::Error::QueryReturnedNoRows) => None,
            Err(e) => return Err(sql_err(e)),
        };
    }

    let mut stmt = if let (Some(seq), Some(id)) = (last_seq, &last_event_id) {
        conn.prepare("SELECT raw_json, event_id FROM events WHERE seq > ?1 OR (seq = ?1 AND event_id > ?2) ORDER BY seq ASC, event_id ASC")
            .map_err(sql_err)?
    } else {
        conn.prepare("SELECT raw_json, event_id FROM events ORDER BY seq ASC, event_id ASC")
            .map_err(sql_err)?
    };

    let mut mapped_rows = Vec::new();
    if let (Some(seq), Some(id)) = (last_seq, &last_event_id) {
        let mut rows = stmt.query(rusqlite::params![seq, id]).map_err(sql_err)?;
        while let Some(row) = rows.next().map_err(sql_err)? {
            let raw: String = row.get(0).map_err(sql_err)?;
            let event_id: String = row.get(1).map_err(sql_err)?;
            mapped_rows.push((raw, event_id));
        }
    } else {
        let mut rows = stmt.query([]).map_err(sql_err)?;
        while let Some(row) = rows.next().map_err(sql_err)? {
            let raw: String = row.get(0).map_err(sql_err)?;
            let event_id: String = row.get(1).map_err(sql_err)?;
            mapped_rows.push((raw, event_id));
        }
    }

    let mut count = 0;
    let mut latest_event_id = None;
    for (raw, id) in mapped_rows {
        if let Ok(v) = serde_json::from_str::<crate::coordinator::CoordinatorEventRecord>(&raw) {
            if let Some(runtime_event) = coordinator_runtime::raw_event_to_runtime_event(&v) {
                let _ = state.runtime_event_bus_tx.send(runtime_event);
                count += 1;
            }
        }
        latest_event_id = Some(id);
    }

    if let Some(ref id) = latest_event_id {
        let now = chrono::Utc::now().to_rfc3339();
        conn.execute(
            "INSERT INTO event_cursor (stream, last_event_id, last_read_at)
             VALUES ('coordinator', ?1, ?2)
             ON CONFLICT(stream) DO UPDATE SET last_event_id = ?1, last_read_at = ?2",
            [id, &now],
        )
        .map_err(sql_err)?;

        if let Some(log) = logger {
            let _ = log.note(format!(
                "- Recovery: replayed {} events; updated event_cursor to {}",
                count, id
            ));
        }
    }

    Ok(count)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum StaleHeartbeatAction {
    Retry,
    Block,
    Requeue,
}

pub fn apply_stale_heartbeat_policy(
    repo_root: &Path,
    env_cfg: &CoordinatorEnvConfig,
    coordinator: Option<&crate::config::CoordinatorConfig>,
    logger: Option<&dyn CoordinatorLog>,
) -> Result<usize> {
    let stale_seconds = resolve_stale_heartbeat_seconds(env_cfg, coordinator);
    if stale_seconds == 0 {
        return Ok(0);
    }
    let action = resolve_stale_heartbeat_action(env_cfg, coordinator, logger);
    let now = chrono::Utc::now();
    let now_ts = now.timestamp();
    let now_iso = now.to_rfc3339_opts(chrono::SecondsFormat::Secs, true);

    let registry_value =
        crate::coordinator::state::coordinator_state_registry_load(repo_root, &BTreeMap::new())?;
    let mut registry = TaskRegistry::from_value(&registry_value)?;

    let mut stale_ids = Vec::new();
    for task in &mut registry.tasks {
        if task.runtime_status() != crate::coordinator::RuntimeStatus::Running {
            continue;
        }
        let phase = task.current_phase().to_string();
        let last_ts = task
            .task_runtime
            .last_heartbeat
            .as_deref()
            .filter(|v| !v.is_empty())
            .or_else(|| {
                task.task_runtime
                    .started_at
                    .as_deref()
                    .filter(|v| !v.is_empty())
            })
            .or(task.updated_at.as_deref());
        let Some(last_ts) = last_ts else {
            continue;
        };
        let Ok(parsed) = chrono::DateTime::parse_from_rfc3339(last_ts) else {
            continue;
        };
        let age = now_ts.saturating_sub(parsed.timestamp());
        if age <= stale_seconds as i64 {
            continue;
        }

        let task_id = task.id.clone();
        if task_id.is_empty() {
            continue;
        }

        let detail = format!(
            "stale heartbeat: last={} age={}s threshold={}s action={}",
            last_ts,
            age,
            stale_seconds,
            match action {
                StaleHeartbeatAction::Retry => "retry",
                StaleHeartbeatAction::Block => "block",
                StaleHeartbeatAction::Requeue => "requeue",
            }
        );

        match action {
            StaleHeartbeatAction::Block => {
                let runtime = task.ensure_runtime();
                runtime.status = Some("stale".to_string());
                runtime.pid = None;
                runtime.last_error = Some(detail.clone());
                task.state = "blocked".to_string();
            }
            StaleHeartbeatAction::Requeue => {
                let runtime = task.ensure_runtime();
                runtime.status = Some("idle".to_string());
                runtime.pid = None;
                runtime.current_phase = None;
                runtime.last_error = Some(detail.clone());
                task.state = "todo".to_string();
                task.assignee = None;
                task.claimed_at = None;
                task.worktree = None;
            }
            StaleHeartbeatAction::Retry => {
                let runtime = task.ensure_runtime();
                runtime.increment_retries();
                runtime.status = Some("idle".to_string());
                runtime.pid = None;
                runtime.current_phase = None;
                runtime.last_error = Some(detail.clone());
                task.state = "todo".to_string();
                task.assignee = None;
                task.claimed_at = None;
                task.worktree = None;
            }
        }

        task.updated_at = Some(now_iso.clone());
        task.state_changed_at = Some(now_iso.clone());
        stale_ids.push((task_id, phase));
    }

    if stale_ids.is_empty() {
        return Ok(0);
    }

    registry.recompute_resource_locks(&now_iso);
    registry.set_updated_at(now_iso.clone());
    crate::coordinator::state::coordinator_state_registry_save(
        repo_root,
        &BTreeMap::new(),
        &registry.to_value()?,
    )?;

    for (task_id, phase) in &stale_ids {
        let _ = append_coordinator_event(
            repo_root,
            "task_runtime_stale",
            task_id,
            phase,
            "stale",
            "stale heartbeat detected",
        );
        if action == StaleHeartbeatAction::Retry {
            let _ = append_coordinator_event(
                repo_root,
                "task_runtime_retry",
                task_id,
                phase,
                "queued",
                "stale heartbeat retry queued",
            );
        } else if action == StaleHeartbeatAction::Requeue {
            let _ = append_coordinator_event(
                repo_root,
                "task_runtime_requeue",
                task_id,
                phase,
                "queued",
                "stale heartbeat requeue queued",
            );
        }
    }

    if let Some(log) = logger {
        let _ = log.note(format!(
            "- Stale heartbeat policy applied count={} action={:?}",
            stale_ids.len(),
            action
        ));
    }

    Ok(stale_ids.len())
}

fn resolve_stale_heartbeat_seconds(
    env_cfg: &CoordinatorEnvConfig,
    coordinator: Option<&crate::config::CoordinatorConfig>,
) -> usize {
    let cfg = CoordinatorConfigResolved::resolve(coordinator);
    env_cfg
        .stale_in_progress_seconds
        .unwrap_or(cfg.stale_in_progress_seconds)
}

fn resolve_stale_heartbeat_action(
    env_cfg: &CoordinatorEnvConfig,
    coordinator: Option<&crate::config::CoordinatorConfig>,
    logger: Option<&dyn CoordinatorLog>,
) -> StaleHeartbeatAction {
    let raw = env_cfg
        .stale_action
        .clone()
        .or_else(|| coordinator.and_then(|c| c.stale_action.clone()))
        .unwrap_or_else(|| "block".to_string())
        .trim()
        .to_ascii_lowercase();
    match raw.as_str() {
        "retry" => StaleHeartbeatAction::Retry,
        "requeue" => StaleHeartbeatAction::Requeue,
        "block" => StaleHeartbeatAction::Block,
        other => {
            if let Some(log) = logger {
                let _ = log.note(format!(
                    "- Unknown stale heartbeat action '{}', defaulting to block",
                    other
                ));
            }
            StaleHeartbeatAction::Block
        }
    }
}

fn resolve_error_code_retry_list(
    env_cfg: &CoordinatorEnvConfig,
    coordinator: Option<&crate::config::CoordinatorConfig>,
) -> Vec<String> {
    let raw = env_cfg
        .error_code_retry_list
        .clone()
        .or_else(|| coordinator.and_then(|c| c.error_code_retry_list.clone()))
        .unwrap_or_else(|| "E101,E102,E103,E301,E302,E303,E601,E603".to_string());
    raw.split(',')
        .map(|v| v.trim().to_string())
        .filter(|v| !v.is_empty())
        .collect()
}

fn resolve_error_code_retry_max(
    env_cfg: &CoordinatorEnvConfig,
    coordinator: Option<&crate::config::CoordinatorConfig>,
) -> usize {
    let cfg = CoordinatorConfigResolved::resolve(coordinator);
    env_cfg
        .error_code_retry_max
        .unwrap_or(cfg.error_code_retry_max)
}

fn resolve_rate_limit_backoff_base_seconds(
    env_cfg: &CoordinatorEnvConfig,
    coordinator: Option<&crate::config::CoordinatorConfig>,
) -> u64 {
    let cfg = CoordinatorConfigResolved::resolve(coordinator);
    env_cfg
        .rate_limit_backoff_base_seconds
        .unwrap_or(cfg.rate_limit_backoff_base_seconds)
}

fn resolve_rate_limit_backoff_max_seconds(
    env_cfg: &CoordinatorEnvConfig,
    coordinator: Option<&crate::config::CoordinatorConfig>,
) -> u64 {
    let cfg = CoordinatorConfigResolved::resolve(coordinator);
    env_cfg
        .rate_limit_backoff_max_seconds
        .unwrap_or(cfg.rate_limit_backoff_max_seconds)
}

pub(super) fn resolve_rate_limit_fallback_enabled(
    env_cfg: &CoordinatorEnvConfig,
    coordinator: Option<&crate::config::CoordinatorConfig>,
) -> bool {
    let cfg = CoordinatorConfigResolved::resolve(coordinator);
    env_cfg
        .rate_limit_fallback_enabled
        .unwrap_or(cfg.rate_limit_fallback_enabled)
}

fn resolve_rate_limit_throttle_parallel(
    env_cfg: &CoordinatorEnvConfig,
    coordinator: Option<&crate::config::CoordinatorConfig>,
) -> bool {
    let cfg = CoordinatorConfigResolved::resolve(coordinator);
    env_cfg
        .rate_limit_throttle_parallel
        .unwrap_or(cfg.rate_limit_throttle_parallel)
}

fn resolve_force_kill_grace_seconds(
    env_cfg: &CoordinatorEnvConfig,
    coordinator: Option<&crate::config::CoordinatorConfig>,
) -> u64 {
    let cfg = CoordinatorConfigResolved::resolve(coordinator);
    env_cfg
        .force_kill_grace_seconds
        .unwrap_or(cfg.force_kill_grace_seconds)
}

pub async fn monitor_merge_jobs_native(
    repo_root: &Path,
    _env_cfg: &CoordinatorEnvConfig,
    _coordinator: Option<&crate::config::CoordinatorConfig>,
    state: &mut CoordinatorRunState,
    logger: Option<&dyn CoordinatorLog>,
) -> Result<Option<(String, String)>> {
    let mut blocked_merge: Option<(String, String)> = None;
    loop {
        match state.merge_event_rx.try_recv() {
            Ok(evt) => {
                let maybe_job = state.active_merge_jobs.remove(&evt.task_id);
                let elapsed = maybe_job
                    .as_ref()
                    .map(|j| j.started_at.elapsed().as_secs())
                    .unwrap_or(0);
                let mut registry = crate::coordinator::state::coordinator_state_registry_load(
                    repo_root,
                    &BTreeMap::new(),
                )?;
                let now = now_iso_coordinator();
                coordinator_engine::apply_merge_result_in_registry(
                    &mut registry,
                    &evt.task_id,
                    evt.success,
                    &evt.reason,
                    &now,
                )?;
                if evt.success {
                    if let Ok(registry_snapshot) = TaskRegistry::from_value(&registry) {
                        if let Some(task_snapshot) = registry_snapshot.find_task(&evt.task_id) {
                            // Post-merge order is strict:
                            // 1) switch worktree to base (release task branch)
                            // 2) cleanup merged task branch
                            let _ = switch_worktree_to_base_after_merge(
                                repo_root,
                                task_snapshot,
                                logger,
                            )
                            .await;
                            let branch = task_snapshot.branch().unwrap_or_default();
                            let base = task_snapshot.base_branch("master");
                            if !branch.is_empty() && branch != base {
                                coordinator_runtime::report_branch_cleanup_outcome(
                                    repo_root,
                                    Some(&evt.task_id),
                                    "merge",
                                    branch,
                                    &base,
                                    "merge_success_post_switch",
                                    coordinator_runtime::cleanup_merged_local_branch(
                                        repo_root, branch, &base,
                                    ),
                                    |event_type, task_id, phase, status, message, severity| {
                                        let _ = append_coordinator_event_with_severity(
                                            repo_root, event_type, task_id, phase, status, message,
                                            severity,
                                        );
                                    },
                                    |msg| tracing::warn!("{}", msg),
                                );
                            }
                        }
                    }
                    if let Some(log) = logger {
                        let _ = log.note(format!(
                            "- Merge done task={} elapsed={}s",
                            evt.task_id, elapsed
                        ));
                    }
                } else {
                    blocked_merge = Some((evt.task_id.clone(), evt.reason.clone()));
                    if let Some(log) = logger {
                        let _ = log.note(format!(
                            "- Merge failed task={} elapsed={}s reason={}",
                            evt.task_id, elapsed, evt.reason
                        ));
                    }
                }
                recompute_resource_locks_from_tasks(&mut registry);
                set_registry_updated_at(&mut registry);
                crate::coordinator::state::coordinator_state_registry_save(
                    repo_root,
                    &BTreeMap::new(),
                    &registry,
                )?;
            }
            Err(tokio::sync::mpsc::error::TryRecvError::Empty) => break,
            Err(tokio::sync::mpsc::error::TryRecvError::Disconnected) => break,
        }
    }
    while let Some(joined) = state.merge_join_set.try_join_next() {
        let _ = joined;
    }
    Ok(blocked_merge)
}

pub async fn dispatch_ready_tasks_native(
    repo_root: &Path,
    canonical: &crate::config::CanonicalConfig,
    coordinator: Option<&crate::config::CoordinatorConfig>,
    env_cfg: &CoordinatorEnvConfig,
    prd_file: &Path,
    state: &mut CoordinatorRunState,
    logger: Option<&dyn CoordinatorLog>,
) -> Result<usize> {
    let storage_paths = crate::coordinator_storage::CoordinatorStoragePaths::from_project_paths(
        &crate::ProjectPaths::from_root(repo_root),
    );
    let sqlite = crate::coordinator_storage::SqliteStorage::new(storage_paths);
    if let Ok(Some(ctrl)) = sqlite.get_coordinator_control() {
        if ctrl.mode != "running" {
            if let Some(log) = logger {
                let _ = log.note(format!("- Dispatch skipped: coordinator control mode is {}", ctrl.mode));
            }
            return Ok(0);
        }
    }

    let cfg = CoordinatorConfigResolved::resolve(coordinator);
    ensure_performer_ipc_listener(repo_root, state, logger).await?;
    state
        .dispatch_retry_not_before
        .retain(|_, until| *until > Instant::now());
    let max_dispatch_total = env_cfg.max_dispatch.unwrap_or(cfg.max_dispatch);
    let max_parallel = env_cfg.max_parallel.unwrap_or(cfg.max_parallel);

    // RL-THROTTLE-006: lazy-initialize effective/original concurrency.
    if state.original_max_parallel == 0 {
        state.original_max_parallel = max_parallel;
        state.effective_max_parallel = max_parallel;
    }
    if dispatch_limit_reached(repo_root, state, max_dispatch_total, logger) {
        return Ok(0);
    }
    let remaining_budget = if max_dispatch_total == 0 {
        usize::MAX
    } else {
        max_dispatch_total.saturating_sub(state.dispatched_total_run)
    };
    run_dispatch_pipeline(
        DispatchPipelineContext {
            repo_root,
            canonical,
            coordinator,
            env_cfg,
            prd_file,
            state,
            logger,
            cfg: &cfg,
        },
        remaining_budget,
    )
    .await
}

#[cfg(test)]
mod tests {
    use super::{
        maybe_rollback_new_worktree_on_sanitize_failure, merge_gate_check, prepare_clean_worktree,
        record_dispatch_retry_or_block, refresh_task_active_session_id_in_registry,
        select_dispatch_candidate, should_emit_priority_zero_dispatch_skip, MergeGateResult,
        SanitizeOptions,
    };
    use crate::coordinator::control_plane::sanitize::RollbackWorktreeOptions;
    use crate::coordinator::model::TaskRegistry;
    use crate::coordinator::runtime::CoordinatorRunState;
    use rusqlite::Connection;
    use serde_json::json;
    use std::collections::{BTreeMap, HashMap};
    use std::future::Future;
    use std::path::{Path, PathBuf};
    use std::process::Command;
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::{fs, time::SystemTime};

    static TEST_COUNTER: AtomicU64 = AtomicU64::new(0);

    fn run_git(repo: &Path, args: &[&str]) {
        let output = Command::new("git")
            .args(args)
            .current_dir(repo)
            .output()
            .expect("run git command");
        assert!(
            output.status.success(),
            "git {:?} failed: {}",
            args,
            String::from_utf8_lossy(&output.stderr)
        );
    }

    fn make_test_repo() -> PathBuf {
        let suffix = TEST_COUNTER.fetch_add(1, Ordering::Relaxed);
        let nanos = SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("time")
            .as_nanos();
        let repo = std::env::temp_dir().join(format!(
            "macc-control-plane-tests-{}-{}-{}",
            std::process::id(),
            nanos,
            suffix
        ));
        fs::create_dir_all(&repo).expect("create temp repo");
        run_git(&repo, &["init"]);
        run_git(&repo, &["checkout", "-b", "main"]);
        run_git(&repo, &["config", "user.email", "tests@example.com"]);
        run_git(&repo, &["config", "user.name", "MACC Tests"]);
        fs::write(repo.join("base.txt"), "base\n").expect("write base");
        run_git(&repo, &["add", "base.txt"]);
        run_git(&repo, &["commit", "-m", "base"]);
        repo
    }

    fn run_async_test<F>(future: F) -> F::Output
    where
        F: Future,
    {
        tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("create tokio runtime")
            .block_on(future)
    }

    fn create_pool_worktree(repo: &Path) -> PathBuf {
        let mut created = crate::create_worktrees(
            repo,
            &crate::WorktreeCreateSpec {
                slug: "worker".to_string(),
                tool: "codex".to_string(),
                count: 1,
                base: "main".to_string(),
                dir: PathBuf::from(".macc/worktree"),
                scope: None,
                feature: None,
            },
        )
        .expect("create worktree");
        created.pop().expect("one worktree created").path
    }

    fn has_worktree_orphan_cleaned_event(
        repo: &Path,
        expected_path: &Path,
        expected_step: &str,
    ) -> bool {
        let db_path = repo.join(".macc").join("state").join("coordinator.sqlite");
        if !db_path.exists() {
            return false;
        }
        let Ok(conn) = Connection::open(db_path) else {
            return false;
        };
        let mut stmt = conn
            .prepare(
                "SELECT payload_json FROM events WHERE event_type = 'worktree_orphan_cleaned' ORDER BY seq DESC LIMIT 1",
            )
            .expect("prepare query");
        let mut rows = stmt.query([]).expect("query events");
        let Some(row) = rows.next().expect("iterate rows") else {
            return false;
        };
        let payload_raw: String = row.get(0).expect("payload json");
        let Ok(payload_json) = serde_json::from_str::<serde_json::Value>(&payload_raw) else {
            return false;
        };
        payload_json["worktree_path"]
            .as_str()
            .map(|value| value == expected_path.to_string_lossy())
            .unwrap_or(false)
            && payload_json["sanitize_step"]
                .as_str()
                .map(|value| value == expected_step)
                .unwrap_or(false)
    }

    fn has_dispatch_retry_limit_event(repo: &Path, task_id: &str) -> bool {
        let db_path = repo.join(".macc").join("state").join("coordinator.sqlite");
        if !db_path.exists() {
            return false;
        }
        let Ok(conn) = Connection::open(db_path) else {
            return false;
        };
        let mut stmt = conn
            .prepare(
                "SELECT task_id FROM events WHERE event_type = 'dispatch_retry_limit_reached' AND task_id = ?1 ORDER BY seq DESC LIMIT 1",
            )
            .expect("prepare query");
        let mut rows = stmt.query([task_id]).expect("query events");
        let Some(row) = rows.next().expect("iterate rows") else {
            return false;
        };
        let event_task_id: String = row.get(0).expect("task_id");
        event_task_id == task_id
    }

    #[test]
    fn priority_zero_dispatch_skip_logs_only_once_for_same_task() {
        let mut state = CoordinatorRunState::new();
        assert!(should_emit_priority_zero_dispatch_skip(
            &mut state, "TASK-1"
        ));
        assert!(!should_emit_priority_zero_dispatch_skip(
            &mut state, "TASK-1"
        ));
        assert!(should_emit_priority_zero_dispatch_skip(
            &mut state, "TASK-2"
        ));
    }

    #[test]
    fn merge_gate_check_returns_no_branch_when_task_branch_missing() {
        let repo = make_test_repo();
        assert_eq!(
            merge_gate_check("TASK-MISSING-001", "main", &repo),
            MergeGateResult::NoBranchProceed
        );
        let _ = fs::remove_dir_all(&repo);
    }

    #[test]
    fn merge_gate_check_returns_merged_for_clean_retry_branch() {
        let repo = make_test_repo();
        run_git(&repo, &["checkout", "-b", "task/task-merge-001"]);
        fs::write(repo.join("task.txt"), "task work\n").expect("write task file");
        run_git(&repo, &["add", "task.txt"]);
        run_git(&repo, &["commit", "-m", "task commit"]);
        run_git(&repo, &["checkout", "main"]);

        assert_eq!(
            merge_gate_check("TASK-MERGE-001", "main", &repo),
            MergeGateResult::Merged
        );
        let output = Command::new("git")
            .args(["log", "--oneline", "main", "--", "task.txt"])
            .current_dir(&repo)
            .output()
            .expect("git log");
        assert!(
            output.status.success(),
            "git log failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        assert!(
            !String::from_utf8_lossy(&output.stdout).trim().is_empty(),
            "expected merged task commit to be reachable from main"
        );
        let _ = fs::remove_dir_all(&repo);
    }

    #[test]
    fn merge_gate_check_returns_conflict_on_conflicting_retry_branch() {
        let repo = make_test_repo();
        run_git(&repo, &["checkout", "-b", "task/task-conflict-001"]);
        fs::write(repo.join("base.txt"), "task-side\n").expect("write task-side change");
        run_git(&repo, &["add", "base.txt"]);
        run_git(&repo, &["commit", "-m", "task conflicting commit"]);
        run_git(&repo, &["checkout", "main"]);
        fs::write(repo.join("base.txt"), "main-side\n").expect("write main-side change");
        run_git(&repo, &["add", "base.txt"]);
        run_git(&repo, &["commit", "-m", "main conflicting commit"]);

        assert_eq!(
            merge_gate_check("TASK-CONFLICT-001", "main", &repo),
            MergeGateResult::ConflictProceed
        );
        let status = Command::new("git")
            .args(["status", "--porcelain"])
            .current_dir(&repo)
            .output()
            .expect("git status");
        assert!(
            status.status.success(),
            "git status failed: {}",
            String::from_utf8_lossy(&status.stderr)
        );
        assert!(
            String::from_utf8_lossy(&status.stdout).trim().is_empty(),
            "merge gate should abort conflict and leave clean tree"
        );
        let _ = fs::remove_dir_all(&repo);
    }

    #[test]
    fn refresh_task_active_session_id_updates_runtime_from_state_file() {
        let repo = make_test_repo();
        let worktree = repo.join(".macc/worktree/worker-01");
        fs::create_dir_all(&worktree).expect("create worktree directory");
        let state_dir = repo.join(".macc/state");
        fs::create_dir_all(&state_dir).expect("create state directory");

        let state_payload = serde_json::json!({
            "tools": {
                "codex": {
                    "sessions": {
                        "codex-session-new": {
                            "status": "available",
                            "created_at": "2026-04-01T00:00:00Z",
                            "updated_at": "2026-04-01T00:00:00Z"
                        }
                    }
                }
            }
        });
        fs::write(
            state_dir.join("tool-sessions.json"),
            serde_json::to_string_pretty(&state_payload).expect("serialize state payload"),
        )
        .expect("write tool-sessions.json");

        let mut registry = serde_json::json!({
            "tasks": [{
                "id": "L4-SES-002",
                "state": "claimed",
                "task_runtime": {
                    "active_session_id": "codex-session-old",
                    "last_session_id": "codex-session-old",
                    "last_session_tool": "codex"
                }
            }]
        });
        let refreshed = refresh_task_active_session_id_in_registry(
            &mut registry,
            &repo,
            "L4-SES-002",
            "codex",
            &worktree,
        )
        .expect("refresh session id");
        assert_eq!(refreshed.as_deref(), Some("codex-session-new"));
        assert_eq!(
            registry["tasks"][0]["task_runtime"]["active_session_id"],
            "codex-session-new"
        );
        assert_eq!(
            registry["tasks"][0]["task_runtime"]["last_session_id"],
            "codex-session-new"
        );
        assert_eq!(
            registry["tasks"][0]["task_runtime"]["last_session_tool"],
            "codex"
        );
        let _ = fs::remove_dir_all(&repo);
    }

    #[test]
    fn prepare_clean_worktree_skips_fetch_when_origin_missing() {
        let repo = make_test_repo();
        let result = run_async_test(prepare_clean_worktree(
            &repo,
            "main",
            SanitizeOptions {
                fetch_remote: true,
                fail_on_fetch_error: false,
                tag_abandoned: false,
            },
        ))
        .expect("prepare clean worktree succeeds");
        assert_eq!(result, None);
        let _ = fs::remove_dir_all(&repo);
    }

    #[test]
    fn prepare_clean_worktree_continues_when_origin_fetch_fails() {
        let repo = make_test_repo();
        run_git(
            &repo,
            &["remote", "add", "origin", "/tmp/macc-missing-origin-repo"],
        );
        let result = run_async_test(prepare_clean_worktree(
            &repo,
            "main",
            SanitizeOptions {
                fetch_remote: true,
                fail_on_fetch_error: false,
                tag_abandoned: false,
            },
        ))
        .expect("prepare clean worktree should continue after fetch failure");
        assert_eq!(result, None);
        let _ = fs::remove_dir_all(&repo);
    }

    #[test]
    fn prepare_clean_worktree_still_fails_when_reset_to_base_fails() {
        let repo = make_test_repo();
        run_git(
            &repo,
            &["remote", "add", "origin", "/tmp/macc-missing-origin-repo"],
        );
        let result = run_async_test(prepare_clean_worktree(
            &repo,
            "missing-base-branch",
            SanitizeOptions {
                fetch_remote: true,
                fail_on_fetch_error: false,
                tag_abandoned: false,
            },
        ))
        .expect("prepare clean worktree call should not error");
        assert_eq!(result, Some("reset_hard_base_branch"));
        let _ = fs::remove_dir_all(&repo);
    }

    #[test]
    fn sanitize_failure_rolls_back_new_worktree_and_emits_event() {
        let repo = make_test_repo();
        let worktree_path = create_pool_worktree(&repo);
        assert!(worktree_path.exists());

        let mut state = CoordinatorRunState::new();
        state.last_session_activity_at.insert(
            worktree_path.to_string_lossy().to_string(),
            chrono::Utc::now().timestamp(),
        );

        let rolled_back = maybe_rollback_new_worktree_on_sanitize_failure(
            &repo,
            &mut state,
            "L5-CTRL-002",
            "reset_hard_base_branch",
            RollbackWorktreeOptions {
                path: Some(&worktree_path),
                was_newly_created: true,
                enabled: true,
            },
            None,
        );

        assert!(rolled_back);
        assert!(!worktree_path.exists());
        assert!(!state
            .last_session_activity_at
            .contains_key(&worktree_path.to_string_lossy().to_string()));
        assert!(has_worktree_orphan_cleaned_event(
            &repo,
            &worktree_path,
            "reset_hard_base_branch"
        ));
        let _ = fs::remove_dir_all(&repo);
    }

    #[test]
    fn sanitize_failure_rollback_disabled_keeps_new_worktree() {
        let repo = make_test_repo();
        let worktree_path = create_pool_worktree(&repo);
        assert!(worktree_path.exists());

        let mut state = CoordinatorRunState::new();
        state.last_session_activity_at.insert(
            worktree_path.to_string_lossy().to_string(),
            chrono::Utc::now().timestamp(),
        );

        let rolled_back = maybe_rollback_new_worktree_on_sanitize_failure(
            &repo,
            &mut state,
            "L5-CTRL-002",
            "reset_hard_base_branch",
            RollbackWorktreeOptions {
                path: Some(&worktree_path),
                was_newly_created: true,
                enabled: false,
            },
            None,
        );

        assert!(!rolled_back);
        assert!(worktree_path.exists());
        assert!(state
            .last_session_activity_at
            .contains_key(&worktree_path.to_string_lossy().to_string()));
        assert!(!has_worktree_orphan_cleaned_event(
            &repo,
            &worktree_path,
            "reset_hard_base_branch"
        ));
        let _ = crate::remove_worktree(&repo, &worktree_path, true);
        let _ = fs::remove_dir_all(&repo);
    }

    #[test]
    fn sanitize_failure_on_reused_worktree_does_not_rollback() {
        let repo = make_test_repo();
        let worktree_path = create_pool_worktree(&repo);
        assert!(worktree_path.exists());

        let mut state = CoordinatorRunState::new();
        state.last_session_activity_at.insert(
            worktree_path.to_string_lossy().to_string(),
            chrono::Utc::now().timestamp(),
        );

        let rolled_back = maybe_rollback_new_worktree_on_sanitize_failure(
            &repo,
            &mut state,
            "L5-CTRL-002",
            "reset_hard_base_branch",
            RollbackWorktreeOptions {
                path: Some(&worktree_path),
                was_newly_created: false,
                enabled: true,
            },
            None,
        );

        assert!(!rolled_back);
        assert!(worktree_path.exists());
        assert!(state
            .last_session_activity_at
            .contains_key(&worktree_path.to_string_lossy().to_string()));
        assert!(!has_worktree_orphan_cleaned_event(
            &repo,
            &worktree_path,
            "reset_hard_base_branch"
        ));
        let _ = crate::remove_worktree(&repo, &worktree_path, true);
        let _ = fs::remove_dir_all(&repo);
    }

    #[test]
    fn dispatch_retry_limit_blocks_task_and_emits_event() {
        let repo = make_test_repo();
        crate::init(&crate::ProjectPaths::from_root(&repo), false).expect("init repo");
        crate::coordinator::state::coordinator_state_registry_save(
            &repo,
            &BTreeMap::new(),
            &json!({
                "tasks": [{
                    "id": "L5-CTRL-004",
                    "state": "todo",
                    "task_runtime": {}
                }]
            }),
        )
        .expect("seed registry");

        let mut state = CoordinatorRunState::new();
        for _ in 0..4 {
            let blocked =
                record_dispatch_retry_or_block(&repo, &mut state, "L5-CTRL-004", 2, 5, None)
                    .expect("record retry");
            assert!(!blocked);
        }
        let blocked = record_dispatch_retry_or_block(&repo, &mut state, "L5-CTRL-004", 2, 5, None)
            .expect("record retry");
        assert!(blocked);
        assert!(!state.dispatch_retry_count.contains_key("L5-CTRL-004"));
        assert!(!state.dispatch_retry_not_before.contains_key("L5-CTRL-004"));

        let registry =
            crate::coordinator::state::coordinator_state_registry_load(&repo, &BTreeMap::new())
                .expect("load registry");
        let registry = TaskRegistry::from_value(&registry).expect("typed registry");
        let task = registry.find_task("L5-CTRL-004").expect("task exists");
        assert_eq!(task.state, "blocked");
        assert_eq!(
            task.task_runtime.last_error.as_deref(),
            Some("dispatch_retry_limit_exceeded")
        );
        assert!(has_dispatch_retry_limit_event(&repo, "L5-CTRL-004"));
        let _ = fs::remove_dir_all(&repo);
    }

    #[test]
    fn manual_unlock_resets_dispatch_retry_count_for_future_dispatch() {
        let repo = make_test_repo();
        crate::init(&crate::ProjectPaths::from_root(&repo), false).expect("init repo");
        crate::coordinator::state::coordinator_state_registry_save(
            &repo,
            &BTreeMap::new(),
            &json!({
                "tasks": [{
                    "id": "L5-CTRL-004",
                    "state": "todo",
                    "task_runtime": {}
                }]
            }),
        )
        .expect("seed registry");

        let mut state = CoordinatorRunState::new();
        for _ in 0..5 {
            let _ = record_dispatch_retry_or_block(&repo, &mut state, "L5-CTRL-004", 2, 5, None)
                .expect("record retry");
        }

        let mut args = BTreeMap::new();
        args.insert("task-id".to_string(), "L5-CTRL-004".to_string());
        args.insert("state".to_string(), "todo".to_string());
        args.insert("reason".to_string(), "manual_unlock".to_string());
        crate::coordinator::state::coordinator_state_apply_transition(&repo, &args)
            .expect("manual unlock transition");

        let blocked = record_dispatch_retry_or_block(&repo, &mut state, "L5-CTRL-004", 2, 5, None)
            .expect("record retry after unlock");
        assert!(!blocked);
        assert_eq!(state.dispatch_retry_count.get("L5-CTRL-004"), Some(&1));
        let _ = fs::remove_dir_all(&repo);
    }

    #[test]
    fn select_dispatch_candidate_prefers_highest_priority_ready_task() {
        let registry = json!({
            "tasks": [
                {
                    "id": "T-LOW",
                    "title": "lower priority",
                    "state": "todo",
                    "priority": "5",
                    "dependencies": [],
                    "exclusive_resources": []
                },
                {
                    "id": "T-HIGH",
                    "title": "higher priority",
                    "state": "todo",
                    "priority": "1",
                    "dependencies": [],
                    "exclusive_resources": []
                }
            ],
            "resource_locks": {}
        });
        let cfg = crate::coordinator::task_selector::TaskSelectorConfig {
            enabled_tools: vec!["codex".to_string()],
            tool_priority: vec!["codex".to_string()],
            max_parallel_per_tool: HashMap::new(),
            tool_specializations: HashMap::new(),
            max_parallel: 2,
            default_tool: "codex".to_string(),
            default_base_branch: "main".to_string(),
            now: chrono::Utc::now().to_rfc3339(),
            throttle_registry: BTreeMap::new(),
            rate_limit_fallback_enabled: false,
            external_merged_ids: std::collections::HashSet::new(),
        };
        let candidate = select_dispatch_candidate(&registry, &cfg).expect("candidate selected");
        assert_eq!(candidate.task.id, "T-HIGH");
    }

    #[test]
    fn test_consume_runtime_events_replays_properly() {
        let repo = make_test_repo();
        let project_paths = crate::ProjectPaths::from_root(&repo);
        let storage_paths = crate::coordinator_storage::CoordinatorStoragePaths::from_project_paths(&project_paths);
        let sqlite = crate::coordinator_storage::SqliteStorage::new(storage_paths);
        let conn = sqlite.open().unwrap();
        sqlite.init_schema(&conn).unwrap();

        let e1 = crate::coordinator::CoordinatorEventRecord {
            schema_version: "1".to_string(),
            event_id: "evt-1".to_string(),
            run_id: Some("run-1".to_string()),
            coordinator_epoch: Some(1),
            claim_id: Some("claim-1".to_string()),
            seq: 10,
            ts: "2026-03-20T12:00:00Z".to_string(),
            source: "performer".to_string(),
            task_id: Some("T1".to_string()),
            event_type: "heartbeat".to_string(),
            phase: Some("dev".to_string()),
            status: "ok".to_string(),
            ..Default::default()
        };
        let e2 = crate::coordinator::CoordinatorEventRecord {
            schema_version: "1".to_string(),
            event_id: "evt-2".to_string(),
            run_id: Some("run-1".to_string()),
            coordinator_epoch: Some(1),
            claim_id: Some("claim-1".to_string()),
            seq: 20,
            ts: "2026-03-20T12:05:00Z".to_string(),
            source: "performer".to_string(),
            task_id: Some("T1".to_string()),
            event_type: "heartbeat".to_string(),
            phase: Some("dev".to_string()),
            status: "ok".to_string(),
            ..Default::default()
        };

        sqlite.append_event_record(&e1).unwrap();
        sqlite.append_event_record(&e2).unwrap();

        let mut run_state = CoordinatorRunState::new();

        let replayed = super::consume_runtime_events(&repo, &mut run_state, None).unwrap();
        assert_eq!(replayed, 2);

        let last_event_id: String = conn.query_row(
            "SELECT last_event_id FROM event_cursor WHERE stream = 'coordinator'",
            [],
            |row| row.get(0),
        ).unwrap();
        assert_eq!(last_event_id, "evt-2");

        let replayed_again = super::consume_runtime_events(&repo, &mut run_state, None).unwrap();
        assert_eq!(replayed_again, 0);

        let e3 = crate::coordinator::CoordinatorEventRecord {
            schema_version: "1".to_string(),
            event_id: "evt-3".to_string(),
            run_id: Some("run-1".to_string()),
            coordinator_epoch: Some(1),
            claim_id: Some("claim-1".to_string()),
            seq: 30,
            ts: "2026-03-20T12:10:00Z".to_string(),
            source: "performer".to_string(),
            task_id: Some("T1".to_string()),
            event_type: "heartbeat".to_string(),
            phase: Some("dev".to_string()),
            status: "ok".to_string(),
            ..Default::default()
        };
        sqlite.append_event_record(&e3).unwrap();

        let replayed_third = super::consume_runtime_events(&repo, &mut run_state, None).unwrap();
        assert_eq!(replayed_third, 1);

        let last_event_id: String = conn.query_row(
            "SELECT last_event_id FROM event_cursor WHERE stream = 'coordinator'",
            [],
            |row| row.get(0),
        ).unwrap();
        assert_eq!(last_event_id, "evt-3");

        let _ = fs::remove_dir_all(repo);
    }
}
