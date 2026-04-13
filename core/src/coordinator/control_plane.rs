use crate::coordinator::helpers::{
    append_coordinator_event, append_coordinator_event_with_severity, build_non_task_worker_slug,
    count_pool_worktrees, find_reusable_worktree_native, now_iso_coordinator,
    recompute_resource_locks_from_tasks, set_registry_updated_at, write_worktree_prd_for_task,
};
use crate::coordinator::ipc::{ensure_performer_ipc_listener, read_performer_ipc_addr};
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
use std::time::{Duration, Instant};

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

fn resolve_dispatch_cooldown_seconds(
    env_cfg: &CoordinatorEnvConfig,
    coordinator: Option<&crate::config::CoordinatorConfig>,
) -> u64 {
    env_cfg
        .dispatch_cooldown_seconds
        .or_else(|| coordinator.and_then(|c| c.dispatch_cooldown_seconds))
        .unwrap_or(2)
}

fn resolve_merge_timeout_seconds(
    env_cfg: &CoordinatorEnvConfig,
    coordinator: Option<&crate::config::CoordinatorConfig>,
) -> usize {
    env_cfg
        .merge_job_timeout_seconds
        .or_else(|| coordinator.and_then(|c| c.merge_job_timeout_seconds))
        .unwrap_or(0)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum MergeGateResult {
    Merged,
    ConflictProceed,
    NoBranchProceed,
}

fn merge_gate_check(task_id: &str, base_branch: &str, repo_root: &Path) -> MergeGateResult {
    let mut branch_candidates = Vec::new();
    let prefixes = [
        format!("task/{}", task_id.to_ascii_lowercase()),
        format!("task/{}", task_id),
    ];
    for prefix in prefixes {
        if let Ok(branches) = crate::git::list_branches_by_prefix(repo_root, &prefix) {
            branch_candidates.extend(branches);
        }
    }
    branch_candidates.sort();
    branch_candidates.dedup();
    if branch_candidates.is_empty() {
        return MergeGateResult::NoBranchProceed;
    }

    let original_branch = crate::git::current_branch_name(repo_root).ok();
    let mut attempted_merge = false;
    let mut conflict_or_error = false;
    let mut merged = false;

    for branch in branch_candidates {
        if branch == base_branch {
            continue;
        }
        if !crate::git::checkout(repo_root, &branch, false).unwrap_or(false) {
            conflict_or_error = true;
            continue;
        }
        let commits_ahead = match crate::git::commits_ahead_of_base(repo_root, base_branch) {
            Ok(commits) => commits,
            Err(_) => {
                conflict_or_error = true;
                continue;
            }
        };
        if commits_ahead.is_empty() {
            continue;
        }
        attempted_merge = true;
        if !crate::git::checkout(repo_root, base_branch, false).unwrap_or(false) {
            conflict_or_error = true;
            continue;
        }
        if crate::git::merge_ff_only(repo_root, &branch).unwrap_or(false) {
            merged = true;
            break;
        }
        let merge_no_edit = crate::git::run_git_output_mapped(
            repo_root,
            &["merge", "--no-edit", &branch],
            "run git merge --no-edit",
        );
        if merge_no_edit
            .map(|out| out.status.success())
            .unwrap_or(false)
        {
            merged = true;
            break;
        }
        let _ = crate::git::run_git_output_mapped(
            repo_root,
            &["merge", "--abort"],
            "abort merge gate conflict",
        );
        conflict_or_error = true;
    }

    if let Some(branch) = original_branch {
        let _ = crate::git::checkout(repo_root, &branch, false);
    }

    if merged {
        return MergeGateResult::Merged;
    }
    if attempted_merge || conflict_or_error {
        MergeGateResult::ConflictProceed
    } else {
        MergeGateResult::NoBranchProceed
    }
}

fn retry_count_for_task(registry: &serde_json::Value, task_id: &str) -> usize {
    crate::coordinator::model::TaskRegistry::from_value(registry)
        .ok()
        .and_then(|typed| {
            typed
                .find_task(task_id)
                .map(|task| task.task_runtime.retries_count())
        })
        .unwrap_or(0)
}

fn mark_task_merged_from_merge_gate(
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

/// Sanitize a worktree back to the base branch.
/// Returns `Ok(None)` on success, `Ok(Some(step_name))` when a specific step
/// fails, allowing the caller to include the failed step in diagnostics.
async fn sanitize_worktree_to_base(
    worktree_path: &Path,
    base_branch: &str,
) -> Result<Option<&'static str>> {
    if !crate::git::reset_hard_async(worktree_path, "HEAD").await? {
        return Ok(Some("reset_hard_head"));
    }
    if !crate::git::clean_fd_async(worktree_path).await? {
        return Ok(Some("clean_fd"));
    }
    if !crate::git::checkout_async(worktree_path, base_branch, false).await?
        && !crate::git::checkout_reset_branch_async(worktree_path, base_branch, false).await?
    {
        // Base branch may be checked out in the main worktree; detach HEAD as fallback.
        if !crate::git::checkout_detach_async(worktree_path).await? {
            return Ok(Some("checkout_base_branch"));
        }
    }
    if !crate::git::fetch_async(worktree_path, "origin").await? {
        return Ok(Some("fetch_origin"));
    }
    if !crate::git::reset_hard_async(worktree_path, base_branch).await? {
        return Ok(Some("reset_hard_base_branch"));
    }
    if !crate::git::reset_hard_async(worktree_path, "HEAD").await? {
        return Ok(Some("reset_hard_head_final"));
    }
    if !crate::git::clean_fd_async(worktree_path).await? {
        return Ok(Some("clean_fd_final"));
    }
    Ok(None)
}

fn ensure_expected_worktree_branch(worktree_path: &Path, expected_branch: &str) -> Result<bool> {
    let current_branch = crate::git::current_branch(worktree_path)?;
    Ok(current_branch == expected_branch)
}

fn emit_dispatch_skipped(
    repo_root: &Path,
    logger: Option<&dyn CoordinatorLog>,
    task_id: &str,
    reason: &str,
    detail: &str,
) {
    let msg = format!(
        "dispatch skipped task={} reason={} detail={}",
        task_id, reason, detail
    );
    let _ = append_coordinator_event_with_severity(
        repo_root,
        "dispatch_skipped",
        task_id,
        "dev",
        "skipped",
        &msg,
        "warning",
    );
    if let Some(log) = logger {
        let _ = log.note(format!("- {}", msg));
    }
}

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

    // First action after merge success: force checkout base to release task branch immediately.
    let switched = if crate::git::checkout_async(wt, &base_branch, true).await? {
        true
    } else {
        crate::git::checkout_reset_branch_async(wt, &base_branch, true).await?
    };
    if !switched {
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
    // Continue with sanitization now that the worker branch is no longer checked out.
    let _ = crate::git::reset_hard_async(wt, "HEAD").await?;
    let _ = crate::git::clean_fd_async(wt).await?;
    // Stateless policy: fetch origin refs then hard reset to base.
    if !crate::git::fetch_async(wt, "origin").await? {
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
    if !crate::git::reset_hard_async(wt, &base_branch).await? {
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

struct NativePhaseExecutor<'a> {
    repo_root: &'a Path,
    logger: Option<&'a dyn CoordinatorLog>,
}

/// Append a line to the performer log file for this task.
/// Mirrors the format used by performer.sh so all phases appear in the same log.
/// Read the active session ID for a tool + worktree from tool-sessions.json.
/// Returns `None` if no session exists or the file is missing/unreadable.
/// Read the tool ID currently stored in a worktree's `.macc/tool.json`.
fn read_tool_id_from_tool_json(worktree: &std::path::Path) -> Option<String> {
    let tool_json = worktree.join(".macc").join("tool.json");
    let raw = std::fs::read_to_string(&tool_json).ok()?;
    let value: serde_json::Value = serde_json::from_str(&raw).ok()?;
    value
        .get("id")
        .and_then(serde_json::Value::as_str)
        .filter(|id| !id.is_empty())
        .map(|id| id.to_string())
}

/// Ensure the worktree's `.macc/tool.json` matches the desired tool.
/// If the current tool.json is missing or for a different tool, regenerate it.
fn ensure_tool_json_for_tool(
    repo_root: &std::path::Path,
    worktree: &std::path::Path,
    desired_tool: &str,
) -> Result<()> {
    let current_tool = read_tool_id_from_tool_json(worktree);
    if current_tool.as_deref() == Some(desired_tool) {
        return Ok(());
    }
    crate::worktree::write_tool_json(repo_root, worktree, desired_tool)?;
    Ok(())
}

fn read_session_id_from_state(
    repo_root: &Path,
    tool_id: &str,
    worktree_path: &Path,
) -> Option<String> {
    let path = repo_root.join(".macc/state/tool-sessions.json");
    let raw = std::fs::read_to_string(&path).ok()?;
    let root: serde_json::Value = serde_json::from_str(&raw).ok()?;
    let sessions = root
        .get("tools")?
        .get(tool_id)?
        .get("sessions")?
        .as_object()?;
    // Try both as-is and canonicalized worktree path as lookup keys.
    let key_plain = worktree_path.to_string_lossy().to_string();
    let key_canon = std::fs::canonicalize(worktree_path)
        .ok()
        .map(|p| p.to_string_lossy().to_string());
    for key in std::iter::once(&key_plain).chain(key_canon.iter()) {
        if let Some(entry) = sessions.get(key.as_str()) {
            let sid = entry.get("session_id")?.as_str().unwrap_or_default();
            if !sid.is_empty() {
                return Some(sid.to_string());
            }
        }
    }
    None
}

fn task_active_session_id_from_registry(
    registry: &serde_json::Value,
    task_id: &str,
) -> Option<String> {
    let typed = TaskRegistry::from_value(registry).ok()?;
    typed
        .tasks
        .into_iter()
        .find(|task| task.id == task_id)
        .and_then(|task| task.task_runtime.active_session_id)
        .filter(|sid| !sid.is_empty())
}

fn refresh_task_active_session_id_in_registry(
    registry: &mut serde_json::Value,
    repo_root: &Path,
    task_id: &str,
    tool_id: &str,
    worktree_path: &Path,
) -> Result<Option<String>> {
    let Some(session_id) = read_session_id_from_state(repo_root, tool_id, worktree_path) else {
        return Ok(None);
    };
    let mut typed = TaskRegistry::from_value(registry)?;
    if let Some(task) = typed.find_task_mut(task_id) {
        let runtime = task.ensure_runtime();
        runtime.active_session_id = Some(session_id.clone());
        runtime.last_session_id = Some(session_id.clone());
        runtime.last_session_tool = Some(tool_id.to_string());
    }
    *registry = typed.to_value()?;
    Ok(Some(session_id))
}

fn append_task_lifecycle_event_with_session(
    repo_root: &Path,
    event_type: &str,
    task_id: &str,
    phase: &str,
    status: &str,
    message: &str,
    session_id: Option<&str>,
) -> Result<()> {
    let run_id = crate::coordinator::helpers::ensure_coordinator_run_id();
    let now = now_iso_coordinator();
    let seq = chrono::Utc::now().timestamp_nanos_opt().unwrap_or_default() as u64;
    let payload = serde_json::json!({
        "schema_version":"1",
        "event_id": format!("evt-{}-{}-{}", event_type, task_id, seq),
        "run_id": run_id,
        "seq": seq,
        "ts": now,
        "source": "coordinator:native",
        "task_id": task_id,
        "type": event_type,
        "phase": phase,
        "status": status,
        "severity": if status.eq_ignore_ascii_case("failed") || status.eq_ignore_ascii_case("error") { "blocking" } else { "info" },
        "payload": {
            "message": message,
            "session_id": session_id
        }
    });
    let project_paths = crate::ProjectPaths::from_root(repo_root);
    let _ = crate::coordinator_storage::append_event_sqlite(&project_paths, &payload)?;
    Ok(())
}

fn append_performer_log(worktree: &Path, task_id: &str, line: &str) {
    let safe: String = task_id
        .chars()
        .filter(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '.' | '-'))
        .collect();
    let file = if safe.is_empty() {
        "task"
    } else {
        safe.as_str()
    };
    let log_dir = worktree.join(".macc/log/performer");
    let log_path = log_dir.join(format!("{}.md", file));
    let _ = std::fs::create_dir_all(&log_dir);
    let _ = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&log_path)
        .and_then(|mut f| {
            use std::io::Write;
            writeln!(f, "{}", line)
        });
}

impl coordinator_runtime::PhaseExecutor for NativePhaseExecutor<'_> {
    fn run_phase(
        &self,
        task: &crate::coordinator::model::Task,
        mode: &str,
        coordinator_tool_override: Option<&str>,
        max_attempts: usize,
    ) -> Result<std::result::Result<String, String>> {
        let task_id = task.id.as_str();
        let worktree_path = task.worktree_path().unwrap_or_default();
        if task_id.is_empty() || worktree_path.is_empty() {
            return Ok(Err(format!(
                "phase '{}' cannot run: missing task id or worktree path",
                mode
            )));
        }
        let phase_tool = coordinator_tool_override
            .filter(|v| !v.trim().is_empty())
            .or_else(|| task.coordinator_tool())
            .or_else(|| task.task_tool())
            .filter(|v| !v.trim().is_empty())
            .unwrap_or_default()
            .to_string();
        if phase_tool.is_empty() {
            return Ok(Err(format!(
                "phase '{}' cannot run for task {}: missing coordinator tool",
                mode, task_id
            )));
        }
        let worktree = std::path::PathBuf::from(worktree_path);
        let tool_json = worktree.join(".macc").join("tool.json");
        // Ensure tool.json exists and matches the phase tool.  When the
        // coordinator falls back to a different tool after quota exhaustion,
        // the worktree still carries the original tool's tool.json.
        // Regenerating it here guarantees the performer script invokes the
        // correct command and uses the correct session config.
        if let Err(err) = ensure_tool_json_for_tool(self.repo_root, &worktree, &phase_tool) {
            return Ok(Err(format!(
                "phase '{}' cannot run for task {}: failed to ensure tool.json for '{}': {}",
                mode, task_id, phase_tool, err
            )));
        }
        if !tool_json.exists() {
            return Ok(Err(format!(
                "phase '{}' cannot run for task {}: missing {}",
                mode,
                task_id,
                tool_json.display()
            )));
        }
        let Some(runner_path) =
            coordinator_runtime::resolve_phase_runner(self.repo_root, &worktree, &phase_tool)?
        else {
            return Ok(Err(format!(
                "phase '{}' cannot run for task {}: missing runner for tool '{}'",
                mode, task_id, phase_tool
            )));
        };
        if !runner_path.exists() {
            return Ok(Err(format!(
                "phase '{}' cannot run for task {}: runner path not found {}",
                mode,
                task_id,
                runner_path.display()
            )));
        }
        let prompt = coordinator_runtime::build_phase_prompt(mode, task_id, &phase_tool, task)?;
        let prompt_dir = worktree.join(".macc").join("tmp");
        std::fs::create_dir_all(&prompt_dir).map_err(|e| MaccError::Io {
            path: prompt_dir.to_string_lossy().into(),
            action: "create coordinator phase prompt directory".into(),
            source: e,
        })?;
        let prompt_path = prompt_dir.join(format!(
            "coordinator-phase-{}-{}.prompt.txt",
            mode,
            task_id.replace('/', "-")
        ));
        std::fs::write(&prompt_path, prompt).map_err(|e| MaccError::Io {
            path: prompt_path.to_string_lossy().into(),
            action: "write coordinator phase prompt".into(),
            source: e,
        })?;
        let performer_ipc_addr = read_performer_ipc_addr(self.repo_root);
        if performer_ipc_addr.is_none() {
            return Ok(Err(format!(
                "phase '{}' cannot run for task {}: coordinator IPC address is unavailable",
                mode, task_id
            )));
        }
        let attempts = max_attempts.max(1);
        let phase_started_at = chrono::Utc::now();
        if let Some(log) = self.logger {
            let _ = log.note(format!(
                "- Phase {} start task={} tool={} attempts={} at={}",
                mode,
                task_id,
                phase_tool,
                attempts,
                phase_started_at.format("%H:%M:%SZ"),
            ));
        }
        // Log phase start to performer log so all phases appear in one file.
        append_performer_log(
            &worktree,
            task_id,
            &format!(
                "## Phase: {} (tool={} attempts={})\n\n- Task ID: {}\n- Tool: {}\n- Started: {}\n",
                mode,
                phase_tool,
                attempts,
                task_id,
                phase_tool,
                chrono::Utc::now().format("%Y-%m-%dT%H:%M:%SZ"),
            ),
        );
        // Read existing session ID from tool-sessions.json so we can inject it
        // into the tool runner command, just like the performer.sh wrapper does.
        let mut session_id = read_session_id_from_state(self.repo_root, &phase_tool, &worktree);
        // Fallback: if no session in state file, use the preserved session from
        // the prior run (saved in task_runtime on error_with_changes / error_without_changes)
        // so retries resume with cached context rather than cold-starting.
        if session_id.is_none() {
            let rt = &task.task_runtime;
            if rt.last_session_tool.as_deref() == Some(phase_tool.as_str()) {
                if let Some(ref sid) = rt.last_session_id {
                    if !sid.is_empty() {
                        session_id = Some(sid.clone());
                    }
                }
            }
        }
        let mut last_reason = String::new();
        for attempt in 1..=attempts {
            append_performer_log(
                &worktree,
                task_id,
                &format!("### Attempt {}/{}\n", attempt, attempts),
            );
            let mut command = std::process::Command::new(&runner_path);
            command
                .current_dir(&worktree)
                .env_remove(crate::coordinator::ipc::COORDINATOR_IPC_ADDR_ENV)
                .env(
                    "MACC_EVENT_SOURCE",
                    format!(
                        "coordinator-phase:{}:{}:{}:{}",
                        mode,
                        phase_tool,
                        task_id,
                        chrono::Utc::now().timestamp_nanos_opt().unwrap_or_default()
                    ),
                )
                .env("MACC_EVENT_TASK_ID", task_id)
                .arg("--prompt-file")
                .arg(&prompt_path)
                .arg("--tool-json")
                .arg(&tool_json)
                .arg("--repo")
                .arg(self.repo_root)
                .arg("--worktree")
                .arg(&worktree)
                .arg("--task-id")
                .arg(task_id)
                .arg("--attempt")
                .arg(attempt.to_string())
                .arg("--max-attempts")
                .arg(attempts.to_string());
            if let Some(sid) = session_id.as_deref() {
                command.arg("--session-id").arg(sid);
            }
            if let Some(ipc_addr) = performer_ipc_addr
                .as_deref()
                .filter(|value| !value.trim().is_empty())
            {
                command.env(crate::coordinator::ipc::COORDINATOR_IPC_ADDR_ENV, ipc_addr);
            }
            let output = command.output();
            let Ok(out) = output else {
                last_reason = format!(
                    "phase '{}' failed to execute runner '{}'",
                    mode,
                    runner_path.display()
                );
                append_performer_log(
                    &worktree,
                    task_id,
                    "- Result: failed (runner could not be executed)\n",
                );
                continue;
            };
            let combined_output = format!(
                "{}\n{}",
                String::from_utf8_lossy(&out.stdout),
                String::from_utf8_lossy(&out.stderr)
            );
            // Log the tool output and exit status to the performer log.
            append_performer_log(
                &worktree,
                task_id,
                &format!(
                    "```text\n{}\n```\n\n- Exit status: {}\n",
                    combined_output.trim(),
                    out.status,
                ),
            );
            if out.status.success() {
                // Auto-commit any uncommitted changes produced by the phase runner.
                // The review phase explicitly must not commit; implement/dev commits
                // are managed by performer.sh. Fix phases may leave
                // uncommitted file changes that must be committed before merging.
                if mode != "review" && crate::git::is_dirty(&worktree).unwrap_or(false) {
                    let _ = crate::git::run_git_output_mapped(
                        &worktree,
                        &["add", "-A"],
                        "stage all changes after phase",
                    );
                    let commit_type = if mode == "fix" {
                        crate::commit_message::CommitType::Fix
                    } else {
                        crate::commit_message::CommitType::Feat
                    };
                    let commit_msg = crate::commit_message::task_commit(
                        commit_type,
                        task_id,
                        task.title.as_deref(),
                        Some(mode),
                    )
                    .with_tool(&phase_tool)
                    .format();
                    let commit_out = crate::git::run_git_output_mapped(
                        &worktree,
                        &["commit", "-m", &commit_msg],
                        "auto-commit phase changes",
                    );
                    if let Some(log) = self.logger {
                        match commit_out {
                            Ok(ref o) if o.status.success() => {
                                let _ = log.note(format!(
                                    "- Phase {} auto-committed changes task={}",
                                    mode, task_id
                                ));
                            }
                            _ => {
                                let _ = log.note(format!(
                                    "- Phase {} auto-commit failed task={} (continuing)",
                                    mode, task_id
                                ));
                            }
                        }
                    }
                }
                append_performer_log(
                    &worktree,
                    task_id,
                    &format!(
                        "- Result: **done** (phase={} attempt={}/{})\n",
                        mode, attempt, attempts
                    ),
                );
                let elapsed = chrono::Utc::now()
                    .signed_duration_since(phase_started_at)
                    .num_seconds();
                let _ = std::fs::remove_file(&prompt_path);
                if let Some(log) = self.logger {
                    let _ = log.note(format!(
                        "- Phase {} done task={} attempt={} elapsed={}s",
                        mode, task_id, attempt, elapsed
                    ));
                }
                return Ok(Ok(combined_output));
            }
            last_reason = format!(
                "phase '{}' failed for task {} on attempt {}/{}: status={} stdout=\"{}\" stderr=\"{}\"",
                mode,
                task_id,
                attempt,
                attempts,
                out.status,
                coordinator_runtime::summarize_output(&String::from_utf8_lossy(&out.stdout)),
                coordinator_runtime::summarize_output(&String::from_utf8_lossy(&out.stderr))
            );
            append_performer_log(
                &worktree,
                task_id,
                &format!(
                    "- Result: **failed** (phase={} attempt={}/{})\n",
                    mode, attempt, attempts
                ),
            );
            // Refresh session ID so the next attempt can resume.
            session_id = read_session_id_from_state(self.repo_root, &phase_tool, &worktree);
        }
        let elapsed = chrono::Utc::now()
            .signed_duration_since(phase_started_at)
            .num_seconds();
        let _ = std::fs::remove_file(&prompt_path);
        if let Some(log) = self.logger {
            let _ = log.note(format!(
                "- Phase {} failed task={} elapsed={}s reason={}",
                mode, task_id, elapsed, last_reason
            ));
        }
        append_performer_log(
            &worktree,
            task_id,
            &format!(
                "- Phase {} exhausted all {} attempt(s): {}\n",
                mode, attempts, last_reason
            ),
        );
        Ok(Err(last_reason))
    }
}

pub fn run_phase_for_task_native(
    repo_root: &Path,
    task: &crate::coordinator::model::Task,
    mode: &str,
    coordinator_tool_override: Option<&str>,
    max_attempts: usize,
    logger: Option<&dyn CoordinatorLog>,
) -> Result<std::result::Result<String, String>> {
    let executor = NativePhaseExecutor { repo_root, logger };
    coordinator_runtime::run_phase(
        &executor,
        task,
        mode,
        coordinator_tool_override,
        max_attempts,
    )
}

pub fn run_review_phase_for_task_native(
    repo_root: &Path,
    task: &crate::coordinator::model::Task,
    coordinator_tool_override: Option<&str>,
    max_attempts: usize,
    logger: Option<&dyn CoordinatorLog>,
) -> Result<std::result::Result<coordinator_engine::ReviewVerdict, String>> {
    let executor = NativePhaseExecutor { repo_root, logger };
    coordinator_runtime::run_review_phase(&executor, task, coordinator_tool_override, max_attempts)
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
    let max_review_cycles = env_cfg
        .max_review_cycles
        .or_else(|| coordinator.and_then(|c| c.max_review_cycles));
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

                let merge_ai_fix = env_cfg
                    .merge_ai_fix
                    .or_else(|| coordinator.and_then(|c| c.merge_ai_fix))
                    .unwrap_or(false);
                let merge_hook_timeout = env_cfg
                    .merge_hook_timeout_seconds
                    .or_else(|| coordinator.and_then(|c| c.merge_hook_timeout_seconds));

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
                if !completion.should_retry
                    && (evt.success
                        || matches!(
                            completion.completion_kind,
                            Some(
                                crate::coordinator::PerformerCompletionKind::SuccessWithChanges
                                    | crate::coordinator::PerformerCompletionKind::SuccessWithoutChanges
                                    | crate::coordinator::PerformerCompletionKind::AlreadySatisfied
                            )
                        ))
                {
                    let sealed = crate::coordinator::session_manager::seal_worktree_scoped_session(
                        repo_root,
                        &job.tool,
                        &job.worktree_path,
                        &evt.task_id,
                        &now_iso_coordinator(),
                    )?;
                    if sealed.sealed {
                        if let Some(log) = logger {
                            let sid = sealed.session_id.as_deref().unwrap_or("unknown");
                            let _ = log.note(format!(
                                "- Session sealed task={} tool={} session_id={}",
                                evt.task_id, job.tool, sid
                            ));
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
                        env_cfg
                            .merge_ai_fix
                            .or_else(|| coordinator.and_then(|c| c.merge_ai_fix))
                            .unwrap_or(false),
                        env_cfg
                            .merge_hook_timeout_seconds
                            .or_else(|| coordinator.and_then(|c| c.merge_hook_timeout_seconds)),
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

fn apply_runtime_event_bus_updates(
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
                    | CoordinatorRuntimeEventKind::WorktreeHealthCheckFailed { .. } => {}
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
            super::runtime::kill_process_group_sync(pid);
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
    _repo_root: &Path,
    _state: &mut CoordinatorRunState,
    _logger: Option<&dyn CoordinatorLog>,
) -> Result<usize> {
    Ok(0)
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
    env_cfg
        .stale_in_progress_seconds
        .or_else(|| coordinator.and_then(|c| c.stale_in_progress_seconds))
        .unwrap_or(0)
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
    env_cfg
        .error_code_retry_max
        .or_else(|| coordinator.and_then(|c| c.error_code_retry_max))
        .unwrap_or(2)
}

fn resolve_rate_limit_backoff_base_seconds(
    env_cfg: &CoordinatorEnvConfig,
    coordinator: Option<&crate::config::CoordinatorConfig>,
) -> u64 {
    env_cfg
        .rate_limit_backoff_base_seconds
        .or_else(|| coordinator.and_then(|c| c.rate_limit_backoff_base_seconds))
        .unwrap_or(30)
}

fn resolve_rate_limit_backoff_max_seconds(
    env_cfg: &CoordinatorEnvConfig,
    coordinator: Option<&crate::config::CoordinatorConfig>,
) -> u64 {
    env_cfg
        .rate_limit_backoff_max_seconds
        .or_else(|| coordinator.and_then(|c| c.rate_limit_backoff_max_seconds))
        .unwrap_or(300)
}

fn resolve_rate_limit_fallback_enabled(
    env_cfg: &CoordinatorEnvConfig,
    coordinator: Option<&crate::config::CoordinatorConfig>,
) -> bool {
    env_cfg
        .rate_limit_fallback_enabled
        .or_else(|| coordinator.and_then(|c| c.rate_limit_fallback_enabled))
        .unwrap_or(true)
}

fn resolve_rate_limit_throttle_parallel(
    env_cfg: &CoordinatorEnvConfig,
    coordinator: Option<&crate::config::CoordinatorConfig>,
) -> bool {
    env_cfg
        .rate_limit_throttle_parallel
        .or_else(|| coordinator.and_then(|c| c.rate_limit_throttle_parallel))
        .unwrap_or(true)
}

fn resolve_force_kill_grace_seconds(
    env_cfg: &CoordinatorEnvConfig,
    coordinator: Option<&crate::config::CoordinatorConfig>,
) -> u64 {
    env_cfg
        .force_kill_grace_seconds
        .or_else(|| coordinator.and_then(|c| c.force_kill_grace_seconds))
        .unwrap_or(crate::coordinator::runtime::FORCE_KILL_GRACE_SECONDS)
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
    ensure_performer_ipc_listener(repo_root, state, logger).await?;
    let mut dispatched = 0usize;
    let mut dispatch_failed_this_cycle: HashSet<String> = HashSet::new();
    let cooldown_seconds = resolve_dispatch_cooldown_seconds(env_cfg, coordinator);
    state
        .dispatch_retry_not_before
        .retain(|_, until| *until > Instant::now());
    let max_dispatch_total = env_cfg
        .max_dispatch
        .or_else(|| coordinator.and_then(|c| c.max_dispatch))
        .unwrap_or(10);
    let max_parallel = env_cfg
        .max_parallel
        .or_else(|| coordinator.and_then(|c| c.max_parallel))
        .unwrap_or(3);

    // RL-THROTTLE-006: lazy-initialize effective/original concurrency.
    if state.original_max_parallel == 0 {
        state.original_max_parallel = max_parallel;
        state.effective_max_parallel = max_parallel;
    }

    if max_dispatch_total > 0 && state.dispatched_total_run >= max_dispatch_total {
        if !state.dispatch_limit_event_emitted {
            let msg = format!(
                "dispatch limit reached run_total={} max_dispatch={}",
                state.dispatched_total_run, max_dispatch_total
            );
            let _ = append_coordinator_event_with_severity(
                repo_root,
                "dispatch_limit_reached",
                "-",
                "dev",
                "done",
                &msg,
                "info",
            );
            if let Some(log) = logger {
                let _ = log.note(format!("- {}", msg));
            }
            state.dispatch_limit_event_emitted = true;
        }
        return Ok(0);
    }
    let remaining_budget = if max_dispatch_total == 0 {
        usize::MAX
    } else {
        max_dispatch_total.saturating_sub(state.dispatched_total_run)
    };

    while dispatched < remaining_budget {
        if state.effective_max_parallel > 0
            && state.active_jobs.len() >= state.effective_max_parallel
        {
            break;
        }

        let mut registry = crate::coordinator::state::coordinator_state_registry_load(
            repo_root,
            &BTreeMap::new(),
        )?;
        let config = crate::coordinator::task_selector::TaskSelectorConfig {
            enabled_tools: canonical.tools.enabled.clone(),
            tool_priority: env_cfg
                .tool_priority
                .clone()
                .map(|csv| {
                    csv.split(',')
                        .map(|v| v.trim().to_string())
                        .filter(|v| !v.is_empty())
                        .collect::<Vec<_>>()
                })
                .or_else(|| coordinator.map(|c| c.tool_priority.clone()))
                .unwrap_or_default(),
            max_parallel_per_tool: env_cfg
                .max_parallel_per_tool_json
                .clone()
                .and_then(|raw| serde_json::from_str::<HashMap<String, usize>>(&raw).ok())
                .or_else(|| {
                    coordinator.map(|c| {
                        c.max_parallel_per_tool
                            .clone()
                            .into_iter()
                            .collect::<HashMap<_, _>>()
                    })
                })
                .unwrap_or_default(),
            tool_specializations: env_cfg
                .tool_specializations_json
                .clone()
                .and_then(|raw| serde_json::from_str::<HashMap<String, Vec<String>>>(&raw).ok())
                .or_else(|| {
                    coordinator.map(|c| {
                        c.tool_specializations
                            .clone()
                            .into_iter()
                            .collect::<HashMap<_, _>>()
                    })
                })
                .unwrap_or_default(),
            max_parallel: state.effective_max_parallel,
            default_tool: canonical.tools.enabled.first().cloned().unwrap_or_default(),
            default_base_branch: env_cfg
                .reference_branch
                .clone()
                .or_else(|| coordinator.and_then(|c| c.reference_branch.clone()))
                .unwrap_or_else(|| "master".to_string()),
            now: chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Secs, true),
            throttle_registry: state.throttle_registry.clone(),
            rate_limit_fallback_enabled: resolve_rate_limit_fallback_enabled(env_cfg, coordinator),
        };

        if let Some(reason) =
            crate::coordinator::task_selector::dispatch_block_reason(&registry, &config)
        {
            match reason {
                crate::coordinator::task_selector::DispatchBlockReason::ActivePriorityZero {
                    task_id,
                } => {
                    if should_emit_priority_zero_dispatch_skip(state, &task_id) {
                        emit_dispatch_skipped(
                            repo_root,
                            logger,
                            &task_id,
                            "priority_zero_exclusive",
                            "an active priority=0 task blocks parallel dispatch",
                        );
                    }
                }
                crate::coordinator::task_selector::DispatchBlockReason::ReadyPriorityZeroBlocked {
                    task_id,
                } => {
                    if should_emit_priority_zero_dispatch_skip(state, &task_id) {
                        emit_dispatch_skipped(
                            repo_root,
                            logger,
                            &task_id,
                            "priority_zero_exclusive",
                            "a ready priority=0 task must run alone before lower-priority dispatch",
                        );
                    }
                }
            }
            break;
        }
        state.last_priority_zero_dispatch_block_task_id = None;
        let Some(selected) =
            crate::coordinator::task_selector::select_next_ready_task(&registry, &config)
        else {
            break;
        };
        // RL-ROUTE-005: emit tool_fallback event when primary tool is throttled.
        if selected.is_fallback {
            let msg = format!(
                "tool_fallback task={} selected_tool={} reason=rate_limit_throttled",
                selected.id, selected.tool
            );
            let _ = append_coordinator_event_with_severity(
                repo_root,
                "tool_fallback",
                &selected.id,
                "dev",
                "info",
                &msg,
                "info",
            );
            if let Some(log) = logger {
                let _ = log.note(format!("- {}", msg));
            }
        }
        if let Some(until) = state.dispatch_retry_not_before.get(&selected.id) {
            let now = Instant::now();
            if *until > now {
                let remaining = until.duration_since(now).as_secs();
                emit_dispatch_skipped(
                    repo_root,
                    logger,
                    &selected.id,
                    "cooldown_active",
                    &format!("retry in {}s", remaining),
                );
                break;
            }
        }
        if dispatch_failed_this_cycle.contains(&selected.id) {
            if let Some(log) = logger {
                let _ = log.note(format!(
                    "- Dispatch stop: task {} already failed worktree preparation in this cycle",
                    selected.id
                ));
            }
            break;
        }
        if let Some(log) = logger {
            let _ = log.note(format!("- Lifecycle task={} stage=claim", selected.id));
        }
        if let Some(log) = logger {
            let _ = log.note(format!(
                "- Dispatch candidate task={} tool={} base={}",
                selected.id, selected.tool, selected.base_branch
            ));
        }
        let merge_gate_enabled = coordinator
            .map(|cfg| cfg.merge_gate_on_dispatch)
            .unwrap_or(true);
        if merge_gate_enabled && retry_count_for_task(&registry, &selected.id) > 0 {
            let attempt_msg = format!(
                "merge-gate check started task={} base={}",
                selected.id, selected.base_branch
            );
            let _ = append_coordinator_event_with_severity(
                repo_root,
                "merge_gate_attempt",
                &selected.id,
                "dev",
                "started",
                &attempt_msg,
                "info",
            );
            if let Some(log) = logger {
                let _ = log.note(format!("- {}", attempt_msg));
            }
            match merge_gate_check(&selected.id, &selected.base_branch, repo_root) {
                MergeGateResult::Merged => {
                    let now = now_iso_coordinator();
                    mark_task_merged_from_merge_gate(&mut registry, &selected.id, &now)?;
                    crate::coordinator::state::coordinator_state_registry_save(
                        repo_root,
                        &BTreeMap::new(),
                        &registry,
                    )?;
                    let msg = format!(
                        "merge-gate merged task={} base={}; dispatch canceled",
                        selected.id, selected.base_branch
                    );
                    let _ = append_coordinator_event_with_severity(
                        repo_root,
                        "merge_gate_merged",
                        &selected.id,
                        "merge",
                        "done",
                        &msg,
                        "info",
                    );
                    if let Some(log) = logger {
                        let _ = log.note(format!("- {}", msg));
                    }
                    continue;
                }
                MergeGateResult::ConflictProceed => {
                    let msg = format!(
                        "merge-gate could not merge task={} base={}; proceeding with dispatch",
                        selected.id, selected.base_branch
                    );
                    let _ = append_coordinator_event_with_severity(
                        repo_root,
                        "merge_gate_conflict",
                        &selected.id,
                        "merge",
                        "warning",
                        &msg,
                        "warning",
                    );
                    if let Some(log) = logger {
                        let _ = log.note(format!("- {}", msg));
                    }
                }
                MergeGateResult::NoBranchProceed => {
                    let msg = format!(
                        "merge-gate found no mergeable retry branch task={}; proceeding with dispatch",
                        selected.id
                    );
                    let _ = append_coordinator_event_with_severity(
                        repo_root,
                        "merge_gate_no_branch",
                        &selected.id,
                        "merge",
                        "done",
                        &msg,
                        "info",
                    );
                    if let Some(log) = logger {
                        let _ = log.note(format!("- {}", msg));
                    }
                }
            }
        }

        let reuse_scan_started = Instant::now();
        let (reusable, reuse_prepare_error) = find_reusable_worktree_native(
            repo_root,
            &registry,
            &selected.tool,
            &selected.base_branch,
        )?;
        let reuse_scan_elapsed_ms = reuse_scan_started.elapsed().as_millis();

        let (worktree_path, branch, last_commit) = if let Some(reused) = reusable {
            let (path, branch, last_commit, skipped_reset, dirty_before) = reused;
            let sanitize_msg = format!(
                "sanitize done task={} mode=reused path={} duration_ms={} dirty_before={} skipped_reset={}",
                selected.id,
                path.display(),
                reuse_scan_elapsed_ms,
                dirty_before,
                skipped_reset
            );
            let _ = append_coordinator_event_with_severity(
                repo_root,
                "sanitize_done",
                &selected.id,
                "dev",
                "success",
                &sanitize_msg,
                "info",
            );
            if let Some(log) = logger {
                let _ = log.note(format!(
                    "- Lifecycle task={} stage=sanitize path={} dirty_before={} skipped_reset={}",
                    selected.id,
                    path.display(),
                    dirty_before,
                    skipped_reset
                ));
            }
            (path, branch, last_commit)
        } else {
            let pool_count = count_pool_worktrees(repo_root)?;
            if state.effective_max_parallel > 0 && pool_count >= state.effective_max_parallel {
                if let Some((reason, detail)) = reuse_prepare_error {
                    emit_dispatch_skipped(repo_root, logger, &selected.id, &reason, &detail);
                    if cooldown_seconds > 0 {
                        state.dispatch_retry_not_before.insert(
                            selected.id.clone(),
                            Instant::now() + Duration::from_secs(cooldown_seconds),
                        );
                    }
                    dispatch_failed_this_cycle.insert(selected.id.clone());
                }
                break;
            }
            let create_spec = crate::WorktreeCreateSpec {
                slug: build_non_task_worker_slug(pool_count),
                tool: selected.tool.clone(),
                count: 1,
                base: selected.base_branch.clone(),
                dir: std::path::PathBuf::from(".macc/worktree"),
                scope: None,
                feature: None,
            };
            let mut created = match crate::create_worktrees(repo_root, &create_spec) {
                Ok(v) => v,
                Err(e) => {
                    let msg = format!(
                        "dispatch failed for task {}: create worktree failed ({})",
                        selected.id, e
                    );
                    let _ = append_coordinator_event_with_severity(
                        repo_root,
                        "dispatch_failed",
                        &selected.id,
                        "dev",
                        "failed",
                        &msg,
                        "warning",
                    );
                    if let Some(log) = logger {
                        let _ = log.note(format!("- {}", msg));
                    }
                    emit_dispatch_skipped(
                        repo_root,
                        logger,
                        &selected.id,
                        "create_worktree_failed",
                        &e.to_string(),
                    );
                    if cooldown_seconds > 0 {
                        state.dispatch_retry_not_before.insert(
                            selected.id.clone(),
                            Instant::now() + Duration::from_secs(cooldown_seconds),
                        );
                    }
                    dispatch_failed_this_cycle.insert(selected.id.clone());
                    break;
                }
            };
            let created = created
                .pop()
                .ok_or_else(|| MaccError::Validation("No worktree created".into()))?;
            let sanitize_started = Instant::now();
            if let Some(failed_step) =
                sanitize_worktree_to_base(&created.path, &selected.base_branch).await?
            {
                let msg =
                    format!(
                    "dispatch failed for task {}: sanitize new worktree failed at step '{}' ({})",
                    selected.id, failed_step, created.path.display()
                );
                let _ = append_coordinator_event_with_severity(
                    repo_root,
                    "dispatch_failed",
                    &selected.id,
                    "dev",
                    "failed",
                    &msg,
                    "error",
                );
                if let Some(log) = logger {
                    let _ = log.note(format!("- {}", msg));
                }
                emit_dispatch_skipped(
                    repo_root,
                    logger,
                    &selected.id,
                    "sanitize_new_worktree_failed",
                    &created.path.to_string_lossy(),
                );
                if cooldown_seconds > 0 {
                    state.dispatch_retry_not_before.insert(
                        selected.id.clone(),
                        Instant::now() + Duration::from_secs(cooldown_seconds),
                    );
                }
                dispatch_failed_this_cycle.insert(selected.id.clone());
                break;
            }
            if !crate::git::checkout_async(&created.path, &created.branch, false).await? {
                let msg = format!(
                    "dispatch failed for task {}: restore task branch failed path={} branch={}",
                    selected.id,
                    created.path.display(),
                    created.branch
                );
                let _ = append_coordinator_event_with_severity(
                    repo_root,
                    "dispatch_failed",
                    &selected.id,
                    "dev",
                    "failed",
                    &msg,
                    "warning",
                );
                if let Some(log) = logger {
                    let _ = log.note(format!("- {}", msg));
                }
                emit_dispatch_skipped(
                    repo_root,
                    logger,
                    &selected.id,
                    "restore_task_branch_failed",
                    &created.branch,
                );
                if cooldown_seconds > 0 {
                    state.dispatch_retry_not_before.insert(
                        selected.id.clone(),
                        Instant::now() + Duration::from_secs(cooldown_seconds),
                    );
                }
                dispatch_failed_this_cycle.insert(selected.id.clone());
                break;
            }
            let sanitize_elapsed_ms = sanitize_started.elapsed().as_millis();
            let sanitize_msg = format!(
                "sanitize done task={} mode=new path={} duration_ms={} dirty_before=false skipped_reset=false",
                selected.id,
                created.path.display(),
                sanitize_elapsed_ms
            );
            let _ = append_coordinator_event_with_severity(
                repo_root,
                "sanitize_done",
                &selected.id,
                "dev",
                "success",
                &sanitize_msg,
                "info",
            );
            let last_commit = crate::git::head_commit_async(&created.path)
                .await
                .unwrap_or_default();
            if let Some(log) = logger {
                let _ = log.note(format!(
                    "- Lifecycle task={} stage=sanitize path={} dirty_before=false skipped_reset=false",
                    selected.id,
                    created.path.display()
                ));
            }
            (created.path, created.branch, last_commit)
        };
        let active_session_id =
            read_session_id_from_state(repo_root, &selected.tool, &worktree_path);
        let dispatch_now = now_iso_coordinator();
        let dispatch_session_id = format!("coordinator-{}-{}", selected.id, dispatch_now);
        let claim_update = coordinator_engine::DispatchClaimUpdate {
            task_id: selected.id.clone(),
            tool: selected.tool.clone(),
            worktree_path: worktree_path.to_string_lossy().to_string(),
            branch: branch.clone(),
            base_branch: selected.base_branch.clone(),
            last_commit: last_commit.clone(),
            session_id: dispatch_session_id.clone(),
            active_session_id: active_session_id.clone(),
            pid: None,
            phase: "dev".to_string(),
            now: dispatch_now.clone(),
        };
        coordinator_engine::apply_dispatch_claim_in_registry(&mut registry, &claim_update)?;
        recompute_resource_locks_from_tasks(&mut registry);
        set_registry_updated_at(&mut registry);
        crate::coordinator::state::coordinator_state_registry_save(
            repo_root,
            &BTreeMap::new(),
            &registry,
        )?;
        if let Some(log) = logger {
            let _ = log.note(format!(
                "- Lifecycle task={} stage=claim persisted session_id={}",
                selected.id, dispatch_session_id
            ));
        }

        let rollback_claim = |detail: &str| -> Result<()> {
            let rollback_registry_value =
                crate::coordinator::state::coordinator_state_registry_load(
                    repo_root,
                    &BTreeMap::new(),
                )?;
            let mut rollback_registry = TaskRegistry::from_value(&rollback_registry_value)?;
            if let Some(task) = rollback_registry.find_task_mut(selected.id.as_str()) {
                let now = now_iso_coordinator();
                task.state = "todo".to_string();
                task.assignee = None;
                task.claimed_at = None;
                task.worktree = None;
                let runtime = task.ensure_runtime();
                runtime.status = Some("idle".to_string());
                runtime.pid = None;
                runtime.current_phase = None;
                runtime.last_error = Some(detail.to_string());
                task.updated_at = Some(now.clone());
                task.state_changed_at = Some(now);
            }
            rollback_registry.recompute_resource_locks(&now_iso_coordinator());
            rollback_registry.set_updated_at(now_iso_coordinator());
            crate::coordinator::state::coordinator_state_registry_save(
                repo_root,
                &BTreeMap::new(),
                &rollback_registry.to_value()?,
            )
        };

        if let Some(log) = logger {
            let _ = log.note(format!("- Lifecycle task={} stage=setup", selected.id));
        }
        if let Err(err) = write_worktree_prd_for_task(prd_file, &selected.id, &worktree_path) {
            let msg = format!(
                "dispatch failed for task {}: write worktree.prd.json failed ({})",
                selected.id, err
            );
            let _ = append_coordinator_event_with_severity(
                repo_root,
                "dispatch_failed",
                &selected.id,
                "dev",
                "failed",
                &msg,
                "warning",
            );
            emit_dispatch_skipped(
                repo_root,
                logger,
                &selected.id,
                "write_worktree_prd_failed",
                &err.to_string(),
            );
            let _ = rollback_claim(&msg);
            if let Some(log) = logger {
                let _ = log.note(format!("- {}", msg));
            }
            if cooldown_seconds > 0 {
                state.dispatch_retry_not_before.insert(
                    selected.id.clone(),
                    Instant::now() + Duration::from_secs(cooldown_seconds),
                );
            }
            dispatch_failed_this_cycle.insert(selected.id.clone());
            break;
        }
        // Always ensure tool.json matches the selected tool.  When a worktree
        // is recycled from a previous task that used a different tool, the old
        // tool.json would otherwise persist and cause the performer to invoke
        // the wrong command.
        if let Err(err) = ensure_tool_json_for_tool(repo_root, &worktree_path, &selected.tool) {
            let msg = format!(
                "dispatch failed for task {}: ensure tool.json failed ({})",
                selected.id, err
            );
            let _ = append_coordinator_event_with_severity(
                repo_root,
                "dispatch_failed",
                &selected.id,
                "dev",
                "failed",
                &msg,
                "warning",
            );
            emit_dispatch_skipped(
                repo_root,
                logger,
                &selected.id,
                "ensure_tool_json_failed",
                &err.to_string(),
            );
            let _ = rollback_claim(&msg);
            if let Some(log) = logger {
                let _ = log.note(format!("- {}", msg));
            }
            if cooldown_seconds > 0 {
                state.dispatch_retry_not_before.insert(
                    selected.id.clone(),
                    Instant::now() + Duration::from_secs(cooldown_seconds),
                );
            }
            dispatch_failed_this_cycle.insert(selected.id.clone());
            break;
        }
        let worktree_paths = crate::ProjectPaths::from_root(&worktree_path);
        if let Err(err) = crate::init(&worktree_paths, false) {
            let msg = format!(
                "dispatch failed for task {}: initialize worktree failed ({})",
                selected.id, err
            );
            let _ = append_coordinator_event_with_severity(
                repo_root,
                "dispatch_failed",
                &selected.id,
                "dev",
                "failed",
                &msg,
                "warning",
            );
            emit_dispatch_skipped(
                repo_root,
                logger,
                &selected.id,
                "worktree_init_failed",
                &err.to_string(),
            );
            let _ = rollback_claim(&msg);
            if let Some(log) = logger {
                let _ = log.note(format!("- {}", msg));
            }
            if cooldown_seconds > 0 {
                state.dispatch_retry_not_before.insert(
                    selected.id.clone(),
                    Instant::now() + Duration::from_secs(cooldown_seconds),
                );
            }
            dispatch_failed_this_cycle.insert(selected.id.clone());
            break;
        }
        let canonical_yaml = canonical.to_yaml().map_err(|e| {
            MaccError::Validation(format!(
                "Failed to serialize canonical config for worktree dispatch apply: {}",
                e
            ))
        })?;
        if let Err(err) = crate::atomic_write(
            &worktree_paths,
            &worktree_paths.config_path,
            canonical_yaml.as_bytes(),
        ) {
            let msg = format!(
                "dispatch failed for task {}: write canonical config failed ({})",
                selected.id, err
            );
            let _ = append_coordinator_event_with_severity(
                repo_root,
                "dispatch_failed",
                &selected.id,
                "dev",
                "failed",
                &msg,
                "warning",
            );
            emit_dispatch_skipped(
                repo_root,
                logger,
                &selected.id,
                "write_canonical_config_failed",
                &err.to_string(),
            );
            let _ = rollback_claim(&msg);
            if let Some(log) = logger {
                let _ = log.note(format!("- {}", msg));
            }
            if cooldown_seconds > 0 {
                state.dispatch_retry_not_before.insert(
                    selected.id.clone(),
                    Instant::now() + Duration::from_secs(cooldown_seconds),
                );
            }
            dispatch_failed_this_cycle.insert(selected.id.clone());
            break;
        }

        let mut apply_cmd = tokio::process::Command::new(std::env::current_exe().map_err(|e| {
            MaccError::Validation(format!("Failed to resolve current executable path: {}", e))
        })?);
        apply_cmd
            .current_dir(repo_root)
            .arg("--cwd")
            .arg(repo_root)
            .arg("worktree")
            .arg("apply")
            .arg(worktree_path.to_string_lossy().to_string())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped());
        let apply_output = apply_cmd.output().await.map_err(|e| MaccError::Io {
            path: worktree_path.to_string_lossy().into(),
            action: "run worktree apply for coordinator dispatch".into(),
            source: e,
        })?;
        if !apply_output.status.success() {
            let detail = format!(
                "stdout=\"{}\" stderr=\"{}\"",
                coordinator_runtime::summarize_output(&String::from_utf8_lossy(
                    &apply_output.stdout
                )),
                coordinator_runtime::summarize_output(&String::from_utf8_lossy(
                    &apply_output.stderr
                ))
            );
            let msg = format!(
                "dispatch failed for task {}: worktree apply failed status={} {}",
                selected.id, apply_output.status, detail
            );
            let _ = append_coordinator_event_with_severity(
                repo_root,
                "dispatch_failed",
                &selected.id,
                "dev",
                "failed",
                &msg,
                "warning",
            );
            if let Some(log) = logger {
                let _ = log.note(format!("- {}", msg));
            }
            emit_dispatch_skipped(
                repo_root,
                logger,
                &selected.id,
                "worktree_apply_failed",
                &detail,
            );
            let _ = rollback_claim(&msg);
            if cooldown_seconds > 0 {
                state.dispatch_retry_not_before.insert(
                    selected.id.clone(),
                    Instant::now() + Duration::from_secs(cooldown_seconds),
                );
            }
            dispatch_failed_this_cycle.insert(selected.id.clone());
            break;
        }
        if let Some(log) = logger {
            let _ = log.note(format!(
                "- Worktree ready task={} path={}",
                selected.id,
                worktree_path.display()
            ));
        }

        let phase_timeout_seconds = env_cfg
            .stale_in_progress_seconds
            .or_else(|| coordinator.and_then(|c| c.stale_in_progress_seconds))
            .unwrap_or(600);
        let current_exe = std::env::current_exe().map_err(|e| {
            MaccError::Validation(format!("Failed to resolve current executable path: {}", e))
        })?;
        let branch_matches = match ensure_expected_worktree_branch(&worktree_path, &branch) {
            Ok(matches) => matches,
            Err(err) => {
                let msg = format!(
                    "dispatch failed for task {}: verify worktree branch failed ({})",
                    selected.id, err
                );
                let _ = append_coordinator_event_with_severity(
                    repo_root,
                    "dispatch_failed",
                    &selected.id,
                    "dev",
                    "failed",
                    &msg,
                    "warning",
                );
                let _ = rollback_claim(&msg);
                if let Some(log) = logger {
                    let _ = log.note(format!("- {}", msg));
                }
                emit_dispatch_skipped(
                    repo_root,
                    logger,
                    &selected.id,
                    "verify_worktree_branch_failed",
                    &err.to_string(),
                );
                dispatch_failed_this_cycle.insert(selected.id.clone());
                if cooldown_seconds > 0 {
                    state.dispatch_retry_not_before.insert(
                        selected.id.clone(),
                        Instant::now() + Duration::from_secs(cooldown_seconds),
                    );
                }
                break;
            }
        };
        if !branch_matches {
            let current_branch = crate::git::current_branch(&worktree_path)
                .unwrap_or_else(|_| "unknown".to_string());
            let msg = format!(
                "dispatch failed for task {}: worktree HEAD mismatch expected={} actual={}",
                selected.id, branch, current_branch
            );
            let _ = append_coordinator_event_with_severity(
                repo_root,
                "dispatch_failed",
                &selected.id,
                "dev",
                "failed",
                &msg,
                "warning",
            );
            let _ = rollback_claim(&msg);
            if let Some(log) = logger {
                let _ = log.note(format!("- {}", msg));
            }
            emit_dispatch_skipped(
                repo_root,
                logger,
                &selected.id,
                "worktree_head_mismatch",
                &format!("expected={} actual={}", branch, current_branch),
            );
            dispatch_failed_this_cycle.insert(selected.id.clone());
            if cooldown_seconds > 0 {
                state.dispatch_retry_not_before.insert(
                    selected.id.clone(),
                    Instant::now() + Duration::from_secs(cooldown_seconds),
                );
            }
            break;
        }
        let pid = match coordinator_runtime::spawn_performer_job(
            &current_exe,
            repo_root,
            &selected.id,
            &selected.base_branch,
            &worktree_path,
            &state.event_tx,
            &mut state.join_set,
            phase_timeout_seconds,
            state.performer_ipc_addr.as_deref(),
        ) {
            Ok(pid) => pid,
            Err(err) => {
                let msg = format!(
                    "dispatch failed for task {}: performer spawn failed ({})",
                    selected.id, err
                );
                let _ = append_coordinator_event_with_severity(
                    repo_root,
                    "dispatch_failed",
                    &selected.id,
                    "dev",
                    "failed",
                    &msg,
                    "warning",
                );
                let _ = rollback_claim(&msg);
                if let Some(log) = logger {
                    let _ = log.note(format!("- {}", msg));
                }
                emit_dispatch_skipped(
                    repo_root,
                    logger,
                    &selected.id,
                    "spawn_performer_failed",
                    &err.to_string(),
                );
                dispatch_failed_this_cycle.insert(selected.id.clone());
                if cooldown_seconds > 0 {
                    state.dispatch_retry_not_before.insert(
                        selected.id.clone(),
                        Instant::now() + Duration::from_secs(cooldown_seconds),
                    );
                }
                break;
            }
        };
        let mut registry = crate::coordinator::state::coordinator_state_registry_load(
            repo_root,
            &BTreeMap::new(),
        )?;
        coordinator_engine::apply_dispatch_pid_in_registry(&mut registry, &selected.id, pid)?;
        set_registry_updated_at(&mut registry);
        crate::coordinator::state::coordinator_state_registry_save(
            repo_root,
            &BTreeMap::new(),
            &registry,
        )?;
        if let Some(log) = logger {
            let _ = log.note(format!(
                "- Lifecycle task={} stage=run pid_persisted={}",
                selected.id,
                pid.map(|v| v.to_string())
                    .unwrap_or_else(|| "unknown".to_string())
            ));
        }
        let dispatch_event_message = format!(
            "task {} dispatched tool={} worktree={} pid={}",
            selected.id,
            selected.tool,
            worktree_path.display(),
            pid.map(|v| v.to_string())
                .unwrap_or_else(|| "unknown".to_string())
        );
        let _ = append_task_lifecycle_event_with_session(
            repo_root,
            "task_dispatched",
            &selected.id,
            "dev",
            "started",
            &dispatch_event_message,
            active_session_id.as_deref(),
        );

        state.active_jobs.insert(
            selected.id.clone(),
            CoordinatorJob {
                tool: selected.tool,
                base_branch: selected.base_branch,
                worktree_path,
                attempt: 1,
                started_at: std::time::Instant::now(),
                pid,
                failure_signaled_at: None,
            },
        );
        if let Some(log) = logger {
            let _ = log.note(format!("- Lifecycle task={} stage=run", selected.id));
            let _ = log.note(format!(
                "- Task dispatched task={} pid={}",
                selected.id,
                pid.map(|v| v.to_string())
                    .unwrap_or_else(|| "unknown".to_string())
            ));
        }
        dispatched += 1;
        state.dispatched_total_run += 1;
        if max_dispatch_total > 0 && state.dispatched_total_run >= max_dispatch_total {
            if !state.dispatch_limit_event_emitted {
                let msg = format!(
                    "dispatch limit reached run_total={} max_dispatch={}",
                    state.dispatched_total_run, max_dispatch_total
                );
                let _ = append_coordinator_event_with_severity(
                    repo_root,
                    "dispatch_limit_reached",
                    "-",
                    "dev",
                    "done",
                    &msg,
                    "info",
                );
                if let Some(log) = logger {
                    let _ = log.note(format!("- {}", msg));
                }
                state.dispatch_limit_event_emitted = true;
            }
            break;
        }
    }
    if !dispatch_failed_this_cycle.is_empty() {
        let failed_ids: Vec<&String> = dispatch_failed_this_cycle.iter().take(3).collect();
        state.last_dispatch_failure = Some(format!(
            "dispatch failed for {} task(s): {:?}. Check coordinator logs for details.",
            dispatch_failed_this_cycle.len(),
            failed_ids,
        ));
    }
    Ok(dispatched)
}

#[cfg(test)]
mod tests {
    use super::{
        merge_gate_check, refresh_task_active_session_id_in_registry,
        should_emit_priority_zero_dispatch_skip, MergeGateResult,
    };
    use crate::coordinator::runtime::CoordinatorRunState;
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

        let mut sessions = serde_json::Map::new();
        sessions.insert(
            worktree.to_string_lossy().to_string(),
            serde_json::json!({ "session_id": "codex-session-new" }),
        );
        let state_payload = serde_json::json!({
            "tools": {
                "codex": {
                    "sessions": sessions
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
}
