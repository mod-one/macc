use crate::config::CoordinatorConfigResolved;
use crate::coordinator::{engine as coordinator_engine, runtime as coordinator_runtime};
use crate::coordinator::helpers::{
    append_coordinator_event_with_severity, build_non_task_worker_slug, count_pool_worktrees,
    find_reusable_worktree_native, is_worktree_activity_recent, now_iso_coordinator,
    recompute_resource_locks_from_tasks, score_worktree_session_warmth, set_registry_updated_at,
    write_worktree_prd_for_task,
};
use crate::coordinator::runtime::{CoordinatorJob, CoordinatorRunState};
use crate::coordinator::types::CoordinatorEnvConfig;
use crate::{MaccError, Result};
use std::collections::{BTreeMap, HashMap};
use std::path::{Path, PathBuf};
use super::base::{
    mark_task_merged_from_merge_gate, resolve_rate_limit_fallback_enabled, retry_count_for_task,
    CoordinatorLog,
};
use super::merge_gate::{merge_gate_check, MergeGateResult};
use super::phase_runner::{
    append_task_lifecycle_event_with_session, ensure_tool_json_for_tool, read_session_id_from_state,
};
use super::sanitize::{maybe_rollback_new_worktree_on_sanitize_failure, sanitize_worktree_to_base};

fn ensure_expected_worktree_branch(worktree_path: &Path, expected_branch: &str) -> Result<bool> {
    let current_branch = crate::git::current_branch(worktree_path)?;
    Ok(current_branch == expected_branch)
}
pub(super) struct DispatchCandidate {
    pub(super) task: crate::coordinator::task_selector::SelectedTask,
    worktree_slot: WorktreeSlot,
}

#[derive(Debug, Clone)]
enum WorktreeSlot {
    Auto,
}

#[derive(Debug, Clone)]
struct AcquiredWorktree {
    path: PathBuf,
    branch: String,
    is_new: bool,
    last_commit: String,
    active_session_id: Option<String>,
}

#[derive(Debug, Clone)]
struct DispatchClaim {
    task_id: String,
    worktree_path: PathBuf,
    branch: String,
    session_id: String,
    tool: String,
    base_branch: String,
    last_commit: String,
    active_session_id: Option<String>,
}

fn build_task_selector_config(
    canonical: &crate::config::CanonicalConfig,
    env_cfg: &CoordinatorEnvConfig,
    cfg: &CoordinatorConfigResolved,
    coordinator: Option<&crate::config::CoordinatorConfig>,
    state: &CoordinatorRunState,
) -> crate::coordinator::task_selector::TaskSelectorConfig {
    crate::coordinator::task_selector::TaskSelectorConfig {
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
            .unwrap_or_else(|| cfg.tool_priority.clone()),
        max_parallel_per_tool: env_cfg
            .max_parallel_per_tool_json
            .clone()
            .and_then(|raw| serde_json::from_str::<HashMap<String, usize>>(&raw).ok())
            .unwrap_or_else(|| cfg.max_parallel_per_tool.clone().into_iter().collect()),
        tool_specializations: env_cfg
            .tool_specializations_json
            .clone()
            .and_then(|raw| serde_json::from_str::<HashMap<String, Vec<String>>>(&raw).ok())
            .unwrap_or_else(|| cfg.tool_specializations.clone().into_iter().collect()),
        max_parallel: state.effective_max_parallel,
        default_tool: canonical.tools.enabled.first().cloned().unwrap_or_default(),
        default_base_branch: env_cfg
            .reference_branch
            .clone()
            .unwrap_or_else(|| cfg.reference_branch.clone()),
        now: chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Secs, true),
        throttle_registry: state.throttle_registry.clone(),
        rate_limit_fallback_enabled: resolve_rate_limit_fallback_enabled(env_cfg, coordinator),
    }
}

pub(super) fn select_dispatch_candidate(
    registry: &serde_json::Value,
    config: &crate::coordinator::task_selector::TaskSelectorConfig,
) -> Option<DispatchCandidate> {
    let task = crate::coordinator::task_selector::select_next_ready_task(registry, config)?;
    Some(DispatchCandidate {
        task,
        worktree_slot: WorktreeSlot::Auto,
    })
}

pub(super) async fn acquire_worktree_for_dispatch(
    repo_root: &Path,
    registry: &serde_json::Value,
    candidate: &DispatchCandidate,
    cfg: &CoordinatorConfigResolved,
    state: &mut CoordinatorRunState,
    logger: Option<&dyn CoordinatorLog>,
) -> Result<AcquiredWorktree> {
    let task = &candidate.task;
    let session_cache_ttl_seconds = cfg.session_cache_ttl_seconds;
    let (reusable, _reuse_prepare_error) = find_reusable_worktree_native(
        repo_root,
        registry,
        &task.tool,
        &task.base_branch,
        session_cache_ttl_seconds,
        &state.last_session_activity_at,
    )?;
    if let Some((path, branch, last_commit, skipped_reset, dirty_before)) = reusable {
        let _ = candidate.worktree_slot.clone();
        let warm_by_session = matches!(
            score_worktree_session_warmth(repo_root, &path, &task.tool, session_cache_ttl_seconds),
            crate::coordinator::helpers::SessionWarmth::Warm(_)
        );
        let warm_by_activity = is_worktree_activity_recent(
            &state.last_session_activity_at,
            &path,
            session_cache_ttl_seconds,
        );
        if warm_by_session || warm_by_activity {
            let warm_msg = format!(
                "warm_slot_reuse task={} tool={} path={} warm_session={} warm_recent_activity={}",
                task.id,
                task.tool,
                path.display(),
                warm_by_session,
                warm_by_activity
            );
            let _ = append_coordinator_event_with_severity(
                repo_root,
                "warm_slot_reuse",
                &task.id,
                "dev",
                "info",
                &warm_msg,
                "info",
            );
        }
        let _ = append_coordinator_event_with_severity(
            repo_root,
            "sanitize_done",
            &task.id,
            "dev",
            "success",
            &format!(
                "sanitize done task={} mode=reused path={} dirty_before={} skipped_reset={}",
                task.id,
                path.display(),
                dirty_before,
                skipped_reset
            ),
            "info",
        );
        if let Some(log) = logger {
            let _ = log.note(format!(
                "- Lifecycle task={} stage=sanitize path={} dirty_before={} skipped_reset={}",
                task.id,
                path.display(),
                dirty_before,
                skipped_reset
            ));
        }
        let active_session_id = read_session_id_from_state(repo_root, &task.tool, &path);
        return Ok(AcquiredWorktree {
            path,
            branch,
            is_new: false,
            last_commit,
            active_session_id,
        });
    }

    let pool_count = count_pool_worktrees(repo_root)?;
    let create_spec = crate::WorktreeCreateSpec {
        slug: build_non_task_worker_slug(pool_count),
        tool: task.tool.clone(),
        count: 1,
        base: task.base_branch.clone(),
        dir: std::path::PathBuf::from(".macc/worktree"),
        scope: None,
        feature: None,
    };
    let mut created = crate::create_worktrees(repo_root, &create_spec)?;
    let created = created
        .pop()
        .ok_or_else(|| MaccError::Validation("No worktree created".into()))?;
    if let Some(failed_step) = sanitize_worktree_to_base(&created.path, &task.base_branch).await? {
        let _ = maybe_rollback_new_worktree_on_sanitize_failure(
            repo_root,
            state,
            &task.id,
            failed_step,
            Some(&created.path),
            true,
            cfg.remove_worktree_on_sanitize_failure,
            logger,
        );
        return Err(MaccError::Coordinator {
            code: "sanitize_new_worktree_failed",
            message: format!("sanitize failed at step '{}'", failed_step),
        });
    }
    if !crate::git::checkout_async(&created.path, &created.branch, false).await? {
        return Err(MaccError::Coordinator {
            code: "restore_task_branch_failed",
            message: created.branch.clone(),
        });
    }
    let last_commit = crate::git::head_commit_async(&created.path)
        .await
        .unwrap_or_default();
    let active_session_id = read_session_id_from_state(repo_root, &task.tool, &created.path);
    Ok(AcquiredWorktree {
        path: created.path,
        branch: created.branch,
        is_new: true,
        last_commit,
        active_session_id,
    })
}

pub(super) fn claim_task_in_registry(
    repo_root: &Path,
    candidate: &DispatchCandidate,
    worktree: &AcquiredWorktree,
    registry: &mut serde_json::Value,
    logger: Option<&dyn CoordinatorLog>,
) -> Result<DispatchClaim> {
    let dispatch_now = now_iso_coordinator();
    let session_id = format!("coordinator-{}-{}", candidate.task.id, dispatch_now);
    let claim_update = coordinator_engine::DispatchClaimUpdate {
        task_id: candidate.task.id.clone(),
        tool: candidate.task.tool.clone(),
        worktree_path: worktree.path.to_string_lossy().to_string(),
        branch: worktree.branch.clone(),
        base_branch: candidate.task.base_branch.clone(),
        last_commit: worktree.last_commit.clone(),
        session_id: session_id.clone(),
        active_session_id: worktree.active_session_id.clone(),
        pid: None,
        phase: "dev".to_string(),
        now: dispatch_now,
    };
    coordinator_engine::apply_dispatch_claim_in_registry(registry, &claim_update)?;
    recompute_resource_locks_from_tasks(registry);
    set_registry_updated_at(registry);
    crate::coordinator::state::coordinator_state_registry_save(repo_root, &BTreeMap::new(), registry)?;
    if let Some(log) = logger {
        let _ = log.note(format!(
            "- Lifecycle task={} stage=claim persisted session_id={}",
            candidate.task.id, session_id
        ));
    }
    Ok(DispatchClaim {
        task_id: candidate.task.id.clone(),
        worktree_path: worktree.path.clone(),
        branch: worktree.branch.clone(),
        session_id,
        tool: candidate.task.tool.clone(),
        base_branch: candidate.task.base_branch.clone(),
        last_commit: worktree.last_commit.clone(),
        active_session_id: worktree.active_session_id.clone(),
    })
}

pub(super) async fn launch_performer(
    repo_root: &Path,
    prd_file: &Path,
    canonical: &crate::config::CanonicalConfig,
    coordinator: Option<&crate::config::CoordinatorConfig>,
    env_cfg: &CoordinatorEnvConfig,
    state: &mut CoordinatorRunState,
    logger: Option<&dyn CoordinatorLog>,
    claim: &DispatchClaim,
) -> Result<Option<i64>> {
    write_worktree_prd_for_task(prd_file, &claim.task_id, &claim.worktree_path)?;
    ensure_tool_json_for_tool(repo_root, &claim.worktree_path, &claim.tool)?;
    let worktree_paths = crate::ProjectPaths::from_root(&claim.worktree_path);
    crate::init(&worktree_paths, false)?;
    let canonical_yaml = canonical.to_yaml().map_err(|e| {
        MaccError::Validation(format!(
            "Failed to serialize canonical config for worktree dispatch apply: {}",
            e
        ))
    })?;
    crate::atomic_write(
        &worktree_paths,
        &worktree_paths.config_path,
        canonical_yaml.as_bytes(),
    )?;
    let mut apply_cmd = tokio::process::Command::new(std::env::current_exe().map_err(|e| {
        MaccError::Validation(format!("Failed to resolve current executable path: {}", e))
    })?);
    apply_cmd
        .current_dir(repo_root)
        .arg("--cwd")
        .arg(repo_root)
        .arg("worktree")
        .arg("apply")
        .arg(claim.worktree_path.to_string_lossy().to_string())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped());
    let apply_output = apply_cmd.output().await.map_err(|e| MaccError::Io {
        path: claim.worktree_path.to_string_lossy().into(),
        action: "run worktree apply for coordinator dispatch".into(),
        source: e,
    })?;
    if !apply_output.status.success() {
        return Err(MaccError::Coordinator {
            code: "worktree_apply_failed",
            message: format!("status={}", apply_output.status),
        });
    }
    // Use stale_in_progress_seconds as the per-phase hard kill timeout, matching
    // the FSM path (fsm.rs). Default is 0 (disabled) — consistent with
    // CoordinatorConfigResolved::resolve returning 0 when not configured.
    let phase_timeout_seconds = env_cfg.stale_in_progress_seconds.unwrap_or_else(|| {
        let cfg = CoordinatorConfigResolved::resolve(coordinator);
        cfg.stale_in_progress_seconds
    });
    if !ensure_expected_worktree_branch(&claim.worktree_path, &claim.branch)? {
        return Err(MaccError::Coordinator {
            code: "worktree_head_mismatch",
            message: claim.branch.clone(),
        });
    }
    let current_exe = std::env::current_exe().map_err(|e| {
        MaccError::Validation(format!("Failed to resolve current executable path: {}", e))
    })?;
    let pid = coordinator_runtime::spawn_performer_job(
        &current_exe,
        repo_root,
        &claim.task_id,
        &claim.base_branch,
        &claim.worktree_path,
        &state.event_tx,
        &mut state.join_set,
        phase_timeout_seconds,
        state.performer_ipc_addr.as_deref(),
    )?;
    let mut registry =
        crate::coordinator::state::coordinator_state_registry_load(repo_root, &BTreeMap::new())?;
    coordinator_engine::apply_dispatch_pid_in_registry(&mut registry, &claim.task_id, pid)?;
    set_registry_updated_at(&mut registry);
    crate::coordinator::state::coordinator_state_registry_save(repo_root, &BTreeMap::new(), &registry)?;
    if let Some(log) = logger {
        let _ = log.note(format!("- Lifecycle task={} stage=run", claim.task_id));
    }
    Ok(pid)
}

pub(super) fn dispatch_limit_reached(
    repo_root: &Path,
    state: &mut CoordinatorRunState,
    max_dispatch_total: usize,
    logger: Option<&dyn CoordinatorLog>,
) -> bool {
    if max_dispatch_total == 0 || state.dispatched_total_run < max_dispatch_total {
        return false;
    }
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
    true
}

pub(super) async fn run_dispatch_pipeline(
    repo_root: &Path,
    canonical: &crate::config::CanonicalConfig,
    coordinator: Option<&crate::config::CoordinatorConfig>,
    env_cfg: &CoordinatorEnvConfig,
    prd_file: &Path,
    state: &mut CoordinatorRunState,
    logger: Option<&dyn CoordinatorLog>,
    cfg: &CoordinatorConfigResolved,
    remaining_budget: usize,
) -> Result<usize> {
    let mut dispatched = 0usize;
    while dispatched < remaining_budget {
        if state.effective_max_parallel > 0 && state.active_jobs.len() >= state.effective_max_parallel {
            break;
        }
        let mut registry = crate::coordinator::state::coordinator_state_registry_load(repo_root, &BTreeMap::new())?;
        let config = build_task_selector_config(canonical, env_cfg, cfg, coordinator, state);
        let Some(candidate) = select_dispatch_candidate(&registry, &config) else {
            break;
        };
        // Merge-gate: for retry tasks, check if the task branch is already
        // cleanly merged — if so, mark merged and skip dispatch.
        if cfg.merge_gate_on_dispatch && retry_count_for_task(&registry, &candidate.task.id) > 0 {
            let attempt_msg = format!(
                "merge-gate check started task={} base={}",
                candidate.task.id, candidate.task.base_branch
            );
            let _ = append_coordinator_event_with_severity(
                repo_root,
                "merge_gate_attempt",
                &candidate.task.id,
                "dev",
                "started",
                &attempt_msg,
                "info",
            );
            if let Some(log) = logger {
                let _ = log.note(format!("- {}", attempt_msg));
            }
            match merge_gate_check(&candidate.task.id, &candidate.task.base_branch, repo_root) {
                MergeGateResult::Merged => {
                    let now = now_iso_coordinator();
                    mark_task_merged_from_merge_gate(&mut registry, &candidate.task.id, &now)?;
                    crate::coordinator::state::coordinator_state_registry_save(
                        repo_root,
                        &BTreeMap::new(),
                        &registry,
                    )?;
                    let msg = format!(
                        "merge-gate merged task={} base={}; dispatch canceled",
                        candidate.task.id, candidate.task.base_branch
                    );
                    let _ = append_coordinator_event_with_severity(
                        repo_root,
                        "merge_gate_merged",
                        &candidate.task.id,
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
                        candidate.task.id, candidate.task.base_branch
                    );
                    let _ = append_coordinator_event_with_severity(
                        repo_root,
                        "merge_gate_conflict",
                        &candidate.task.id,
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
                        candidate.task.id
                    );
                    let _ = append_coordinator_event_with_severity(
                        repo_root,
                        "merge_gate_no_branch",
                        &candidate.task.id,
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
        let worktree = acquire_worktree_for_dispatch(repo_root, &registry, &candidate, cfg, state, logger).await?;
        let claim = claim_task_in_registry(repo_root, &candidate, &worktree, &mut registry, logger)?;
        let pid = launch_performer(repo_root, prd_file, canonical, coordinator, env_cfg, state, logger, &claim).await?;
        let _ = append_task_lifecycle_event_with_session(
            repo_root,
            "task_dispatched",
            &claim.task_id,
            "dev",
            "started",
            &format!("task {} dispatched tool={} worktree={} pid={}", claim.task_id, claim.tool, claim.worktree_path.display(), pid.map(|v| v.to_string()).unwrap_or_else(|| "unknown".to_string())),
            claim.active_session_id.as_deref(),
        );
        state.active_jobs.insert(
            claim.task_id.clone(),
            CoordinatorJob {
                tool: claim.tool,
                base_branch: claim.base_branch,
                worktree_path: claim.worktree_path,
                attempt: 1,
                started_at: std::time::Instant::now(),
                pid,
                failure_signaled_at: None,
            },
        );
        state.dispatch_retry_count.remove(&claim.task_id);
        state.dispatch_retry_not_before.remove(&claim.task_id);
        dispatched += 1;
        state.dispatched_total_run += 1;
        if dispatch_limit_reached(repo_root, state, env_cfg.max_dispatch.unwrap_or(cfg.max_dispatch), logger) {
            break;
        }
    }
    Ok(dispatched)
}
