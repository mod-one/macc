use crate::coordinator::helpers::now_iso_coordinator;
use crate::coordinator::model::TaskRegistry;
use crate::coordinator::{engine as coordinator_engine, RuntimeStatus};
use crate::coordinator_storage::CoordinatorStorage;
use crate::{MaccError, Result};
use std::collections::BTreeMap;
use std::path::Path;

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq, Eq)]
pub struct CoordinatorPauseFile {
    pub paused: bool,
    pub task_id: String,
    pub phase: String,
    pub reason: String,
    pub updated_at: String,
}

fn coordinator_registry_path(repo_root: &Path) -> std::path::PathBuf {
    repo_root.join(crate::coordinator::COORDINATOR_TASK_REGISTRY_REL_PATH)
}

pub fn coordinator_pause_file_path(repo_root: &Path) -> std::path::PathBuf {
    repo_root.join(crate::coordinator::COORDINATOR_PAUSE_FILE_REL_PATH)
}

pub fn write_coordinator_pause_file(
    repo_root: &Path,
    task_id: &str,
    phase: &str,
    reason: &str,
) -> Result<()> {
    let path = coordinator_pause_file_path(repo_root);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| MaccError::Io {
            path: parent.to_string_lossy().into(),
            action: "create coordinator pause file parent".into(),
            source: e,
        })?;
    }
    let payload = serde_json::json!({
        "paused": true,
        "task_id": task_id,
        "phase": phase,
        "reason": reason,
        "updated_at": now_iso_coordinator(),
    });
    let payload: CoordinatorPauseFile =
        serde_json::from_value(payload).map_err(|e| MaccError::Coordinator {
            code: "runtime_state",
            message: format!(
                "Failed to build coordinator pause file '{}': {}",
                path.display(),
                e
            ),
        })?;
    let body = serde_json::to_string_pretty(&payload).map_err(|e| MaccError::Coordinator {
        code: "runtime_state",
        message: format!(
            "Failed to serialize coordinator pause file '{}': {}",
            path.display(),
            e
        ),
    })?;
    std::fs::write(&path, body).map_err(|e| MaccError::Io {
        path: path.to_string_lossy().into(),
        action: "write coordinator pause file".into(),
        source: e,
    })
}

pub fn clear_coordinator_pause_file(repo_root: &Path) -> Result<bool> {
    let path = coordinator_pause_file_path(repo_root);
    if !path.exists() {
        return Ok(false);
    }
    std::fs::remove_file(&path).map_err(|e| MaccError::Io {
        path: path.to_string_lossy().into(),
        action: "remove coordinator pause file".into(),
        source: e,
    })?;
    Ok(true)
}

pub fn read_coordinator_pause_file(repo_root: &Path) -> Result<Option<CoordinatorPauseFile>> {
    let path = coordinator_pause_file_path(repo_root);
    if !path.exists() {
        return Ok(None);
    }
    let raw = std::fs::read_to_string(&path).map_err(|e| MaccError::Io {
        path: path.to_string_lossy().into(),
        action: "read coordinator pause file".into(),
        source: e,
    })?;
    let value: CoordinatorPauseFile =
        serde_json::from_str(&raw).map_err(|e| MaccError::Coordinator {
            code: "runtime_state",
            message: format!(
                "Failed to parse coordinator pause file '{}': {}",
                path.display(),
                e
            ),
        })?;
    Ok(Some(value))
}

pub fn set_task_paused_for_merge(repo_root: &Path, task_id: &str, reason: &str) -> Result<()> {
    let mut args = BTreeMap::new();
    args.insert("task-id".to_string(), task_id.to_string());
    args.insert("runtime-status".to_string(), "paused".to_string());
    args.insert("phase".to_string(), "merge".to_string());
    args.insert("last-error".to_string(), reason.to_string());
    args.insert("pid".to_string(), "".to_string());
    crate::coordinator::state::coordinator_state_set_runtime(repo_root, &args)
}

pub fn resume_paused_task_merge(repo_root: &Path, task_id: &str) -> Result<()> {
    let mut transition_args = BTreeMap::new();
    transition_args.insert("task-id".to_string(), task_id.to_string());
    transition_args.insert("state".to_string(), "queued".to_string());
    transition_args.insert("reason".to_string(), "resume:merge_pause".to_string());
    crate::coordinator::state::coordinator_state_apply_transition(repo_root, &transition_args)?;

    let mut runtime_args = BTreeMap::new();
    runtime_args.insert("task-id".to_string(), task_id.to_string());
    runtime_args.insert("runtime-status".to_string(), "phase_done".to_string());
    runtime_args.insert("phase".to_string(), "merge".to_string());
    runtime_args.insert("pid".to_string(), "".to_string());
    crate::coordinator::state::coordinator_state_set_runtime(repo_root, &runtime_args)
}

fn is_pid_running(pid: i64) -> bool {
    if pid <= 0 {
        return false;
    }
    #[cfg(unix)]
    {
        unsafe { libc::kill(pid as libc::pid_t, 0) == 0 }
    }
    #[cfg(not(unix))]
    {
        std::process::Command::new("kill")
            .arg("-0")
            .arg(pid.to_string())
            .status()
            .map(|s| s.success())
            .unwrap_or(false)
    }
}

pub fn cleanup_dead_runtime_tasks_in_registry(
    registry: &mut serde_json::Value,
    reason: &str,
    logger: Option<&dyn Fn(String)>,
    repo_root: Option<&Path>,
) -> Result<usize> {
    let mut typed = TaskRegistry::from_value(registry)?;
    let fixed = cleanup_dead_runtime_tasks_in_typed_registry(
        &mut typed, reason, 60, // Default for untyped registry cleanup
        logger, repo_root,
    )?;
    *registry = typed.to_value()?;
    Ok(fixed)
}

pub fn cleanup_dead_runtime_tasks_in_typed_registry(
    registry: &mut TaskRegistry,
    reason: &str,
    heartbeat_grace_seconds: i64,
    logger: Option<&dyn Fn(String)>,
    repo_root: Option<&Path>,
) -> Result<usize> {
    let now = now_iso_coordinator();
    if let Some(root) = repo_root {
        let refreshed = refresh_candidate_heartbeats_from_events_typed(
            registry,
            root,
            heartbeat_grace_seconds,
            logger,
        )?;
        if refreshed > 0 {
            registry.set_updated_at(now.clone());
        }
    }
    let mut registry_value = registry.to_value()?;
    let cleaned = coordinator_engine::cleanup_dead_runtime_tasks_in_registry_with(
        &mut registry_value,
        &now,
        heartbeat_grace_seconds,
        is_pid_running,
    )?;
    *registry = TaskRegistry::from_value(&registry_value)?;
    let fixed = cleaned.len();
    for entry in cleaned {
        if let Some(log) = logger {
            log(format!(
                "- Runtime ghost cleanup task={} state={} phase={} pid={} -> {} ({})",
                entry.task_id, entry.old_state, entry.phase, entry.pid, entry.new_state, reason
            ));
        }
    }
    if fixed > 0 {
        registry.recompute_resource_locks(&now);
        registry.set_updated_at(now.clone());
        if let (Some(log), Some(root)) = (logger, repo_root) {
            log(format!(
                "- Runtime ghost cleanup applied count={} registry={}",
                fixed,
                coordinator_registry_path(root).display()
            ));
        }
    }
    Ok(fixed)
}

fn refresh_candidate_heartbeats_from_events_typed(
    registry: &mut TaskRegistry,
    repo_root: &Path,
    heartbeat_grace_seconds: i64,
    logger: Option<&dyn Fn(String)>,
) -> Result<usize> {
    if heartbeat_grace_seconds <= 0 {
        return Ok(0);
    }
    let mut candidates: std::collections::HashSet<String> = std::collections::HashSet::new();
    for task in &registry.tasks {
        let id = task.id.as_str();
        let pid = task.runtime_pid();
        let runtime_status = task.runtime_status();
        if !id.is_empty()
            && pid.is_some()
            && runtime_status == RuntimeStatus::Running
            && !is_pid_running(pid.unwrap_or_default())
        {
            candidates.insert(id.to_string());
        }
    }
    if candidates.is_empty() {
        return Ok(0);
    }

    let run_id = std::env::var("COORDINATOR_RUN_ID").ok();
    let now_ts = chrono::DateTime::parse_from_rfc3339(&now_iso_coordinator())
        .ok()
        .map(|dt| dt.timestamp())
        .unwrap_or_default();
    let project_paths = crate::ProjectPaths::from_root(repo_root);
    let storage_paths =
        crate::coordinator_storage::CoordinatorStoragePaths::from_project_paths(&project_paths);
    let snapshot = crate::coordinator_storage::SqliteStorage::new(storage_paths)
        .load_snapshot()
        .unwrap_or_else(|_| crate::coordinator_storage::CoordinatorSnapshot::empty());
    let mut latest_by_task: std::collections::HashMap<String, String> =
        std::collections::HashMap::new();
    for event in &snapshot.events {
        if let Some(expected_run_id) = run_id.as_deref() {
            let event_run_id = event.run_id.as_deref().unwrap_or_default();
            if !event_run_id.is_empty() && event_run_id != expected_run_id {
                continue;
            }
        }
        let event_type = event.event_type.as_str();
        if event_type != "heartbeat" {
            continue;
        }
        let task_id = event.task_id.as_deref().unwrap_or_default();
        if !candidates.contains(task_id) {
            continue;
        }
        let ts = event.ts.as_str();
        let Some(parsed) = chrono::DateTime::parse_from_rfc3339(ts).ok() else {
            continue;
        };
        if now_ts.saturating_sub(parsed.timestamp()) > heartbeat_grace_seconds {
            continue;
        }
        let entry = latest_by_task
            .entry(task_id.to_string())
            .or_insert_with(|| ts.to_string());
        let existing_ts = chrono::DateTime::parse_from_rfc3339(entry)
            .ok()
            .map(|dt| dt.timestamp())
            .unwrap_or_default();
        if parsed.timestamp() > existing_ts {
            *entry = ts.to_string();
        }
    }
    if latest_by_task.is_empty() {
        return Ok(0);
    }
    let mut updated = 0usize;
    for task in &mut registry.tasks {
        let Some(ts) = latest_by_task.get(task.id.as_str()) else {
            continue;
        };
        task.ensure_runtime().last_heartbeat = Some(ts.clone());
        updated += 1;
    }
    if updated > 0 {
        if let Some(log) = logger {
            log(format!(
                "- Refreshed {} candidate heartbeat(s) from snapshot events before ghost cleanup",
                updated
            ));
        }
    }
    Ok(updated)
}

pub fn cleanup_dead_runtime_tasks(
    repo_root: &Path,
    reason: &str,
    heartbeat_grace_seconds: i64,
    logger: Option<&dyn Fn(String)>,
) -> Result<usize> {
    let registry_value =
        crate::coordinator::state::coordinator_state_registry_load(repo_root, &BTreeMap::new())?;
    let mut registry = TaskRegistry::from_value(&registry_value)?;
    let fixed = cleanup_dead_runtime_tasks_in_typed_registry(
        &mut registry,
        reason,
        heartbeat_grace_seconds,
        logger,
        Some(repo_root),
    )?;
    if fixed > 0 {
        crate::coordinator::state::coordinator_state_registry_save(
            repo_root,
            &BTreeMap::new(),
            &registry.to_value()?,
        )?;
    }
    Ok(fixed)
}

pub fn reconcile_registry_native(repo_root: &Path, heartbeat_grace_seconds: i64) -> Result<()> {
    let registry_value =
        crate::coordinator::state::coordinator_state_registry_load(repo_root, &BTreeMap::new())?;
    let mut registry = TaskRegistry::from_value(&registry_value)?;
    let _ = cleanup_dead_runtime_tasks_in_typed_registry(
        &mut registry,
        "reconcile",
        heartbeat_grace_seconds,
        None,
        Some(repo_root),
    )?;
    registry.recompute_resource_locks(&now_iso_coordinator());
    registry.set_updated_at(now_iso_coordinator());
    crate::coordinator::state::coordinator_state_registry_save(
        repo_root,
        &BTreeMap::new(),
        &registry.to_value()?,
    )
}

/// One entry produced by the startup recovery sweep for a single task.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct StartupRecoveryEntry {
    pub task_id: String,
    /// Human-readable description of the situation observed.
    pub situation: String,
    /// §11.2 classification label.
    pub classification: String,
    /// Default action taken (or recommended if dry_run).
    pub action: String,
    /// Whether the registry was mutated for this task.
    pub mutated: bool,
}

/// §11.1 Steps 7-10: Startup Recovery Sweep — canonical §11.2 classification
///
/// Inspects worktree state (step 7), Git branch and merge state (step 8),
/// runs deterministic PRD reconciliation from commit history (step 9), and
/// persists recovery decisions back to SQLite storage (step 10).
///
/// When `dry_run` is `true` the classification is performed and every entry is
/// returned, but no task is mutated and nothing is written to storage.
/// This is the mode used by `macc coordinator recover --dry-run`.
///
/// Must be called after dead-process cleanup and event replay, but before
/// any new task dispatch.
///
/// Returns the list of classification entries produced, one per active task.
#[derive(Debug, Clone, Copy)]
enum MutationAction {
    Merge,
    Stale,
    Blocked,
    PhaseDone,
    Requeue,
}

/// Classify a `todo` task that still holds a worktree.
///
/// Returns `None` when the task needs no repair -- either it carries no
/// worktree at all, or it is parked with attempts remaining and the dispatcher
/// will resume it normally.
///
/// When repair is needed the choice is deliberate:
/// * the branch holds unmerged commits -> **block**, naming the branch. Work is
///   at stake, so an operator must decide whether to recover or discard it.
/// * the branch holds nothing -> **requeue**, clearing the stale attachment so
///   the task can be dispatched fresh. Nothing is lost.
///
/// Never silently discard a branch with commits on it.
#[allow(clippy::type_complexity)]
fn classify_parked_todo_task(
    task: &crate::coordinator::model::Task,
    repo_root: &Path,
    reference_branch: &str,
    same_worktree_budget: usize,
    logger: Option<&dyn Fn(String)>,
) -> Option<(String, String, String, MutationAction)> {
    if !task.has_worktree_attached() {
        return None;
    }
    // Parked for a same-worktree retry with attempts left: dispatch will pick
    // it up on this run, so leave it alone.
    let resumable = task.is_awaiting_same_worktree_retry()
        && task.task_runtime.retries_count() <= same_worktree_budget;
    if resumable {
        return None;
    }

    let branch = task.branch().unwrap_or_default().to_string();
    let has_commits = !branch.is_empty()
        && crate::git::commits_between(repo_root, reference_branch, &branch)
            .map(|commits| !commits.is_empty())
            .unwrap_or(false);

    let (situation, classification, action, act) = if has_commits {
        (
            format!(
                "Task is todo but holds worktree with unmerged commits on {}",
                branch
            ),
            "parked_unschedulable_with_commits".to_string(),
            "Block for operator review; committed work is unmerged".to_string(),
            MutationAction::Blocked,
        )
    } else {
        (
            "Task is todo but holds a worktree it can no longer be dispatched into".to_string(),
            "parked_unschedulable".to_string(),
            "Release the stale worktree attachment and requeue".to_string(),
            MutationAction::Requeue,
        )
    };

    if let Some(log) = logger {
        log(format!(
            "- Recovery sweep task={} classification={} action=\"{}\"",
            task.id, classification, action
        ));
    }
    Some((situation, classification, action, act))
}

pub fn execute_startup_recovery_sweep(
    repo_root: &Path,
    reference_branch: &str,
    dry_run: bool,
    logger: Option<&dyn Fn(String)>,
) -> Result<Vec<StartupRecoveryEntry>> {
    let project_paths = crate::ProjectPaths::from_root(repo_root);
    let storage_paths =
        crate::coordinator_storage::CoordinatorStoragePaths::from_project_paths(&project_paths);
    let sqlite = crate::coordinator_storage::SqliteStorage::new(storage_paths);

    let mut snapshot = sqlite
        .load_snapshot()
        .unwrap_or_else(|_| crate::coordinator_storage::CoordinatorSnapshot::empty());

    // ── Step 9 — deterministic PRD reconciliation from commit history ──────
    let commits =
        crate::coordinator::commit_reconciler::read_commit_range(repo_root, None, reference_branch)
            .unwrap_or_default();
    let reconcile_report =
        crate::coordinator::commit_reconciler::reconcile(&snapshot.registry, &commits);

    let mut entries = Vec::new();
    let mut changed = false;

    let mut proposed_mutations = Vec::new();

    // Same-worktree retry budget, so the sweep can tell a task that is parked
    // and still dispatchable from one that has run out of attempts.
    let same_worktree_budget = crate::config::load_canonical_config(&project_paths.config_path)
        .ok()
        .map(|canonical| {
            crate::config::CoordinatorConfigResolved::resolve(
                canonical.automation.coordinator.as_ref(),
            )
            .phase_runner_max_attempts
            .max(1)
        })
        .unwrap_or(1);

    for task in &snapshot.registry.tasks {
        // `todo` tasks are normally none of the sweep's business, with one
        // exception: a task parked by the same-worktree retry path keeps its
        // worktree on purpose. If it has also run out of attempts it is
        // unschedulable -- neither active nor blocked, so nothing else reclaims
        // it -- and it sits looking healthy while committed work goes unmerged.
        // That is exactly the state that must not stay invisible.
        if task.state == "todo" {
            if let Some(entry) = classify_parked_todo_task(
                task,
                repo_root,
                reference_branch,
                same_worktree_budget,
                logger,
            ) {
                let (situation, classification, action, act) = entry;
                entries.push(StartupRecoveryEntry {
                    task_id: task.id.clone(),
                    situation,
                    classification: classification.clone(),
                    action,
                    mutated: !dry_run,
                });
                proposed_mutations.push((task.id.clone(), act, classification));
            }
            continue;
        }
        if !task.is_active() && task.state != "blocked" {
            continue;
        }

        #[allow(unused_assignments)]
        let mut situation = "Process not spawned".to_string();
        #[allow(unused_assignments)]
        let mut classification = "dispatched_without_process".to_string();
        #[allow(unused_assignments)]
        let mut action = "Requeue safely".to_string();
        #[allow(unused_assignments)]
        let mut proposed_action = Some(MutationAction::Requeue);

        // ── Step 9 check: commit already on base branch? ──────────────────
        let matched_commit = reconcile_report
            .reconciled
            .iter()
            .find(|r| r.task_id == task.id);

        if let Some(m) = matched_commit {
            situation = format!(
                "Commit {} matches task on base branch",
                &m.matched_commit_sha[..7.min(m.matched_commit_sha.len())]
            );
            classification = "merged".to_string();
            action = "Close runtime claim".to_string();
            proposed_action = Some(MutationAction::Merge);
        } else if let Some(pid) = task.runtime_pid() {
            let is_alive = crate::coordinator::helpers::is_pid_running(pid);
            let mut is_stale = false;
            let mut elapsed_sec = 0i64;

            if let Some(ref lh) = task.task_runtime.last_heartbeat {
                if let Ok(parsed) = chrono::DateTime::parse_from_rfc3339(lh) {
                    elapsed_sec = chrono::Utc::now().timestamp()
                        - parsed.with_timezone(&chrono::Utc).timestamp();
                    if elapsed_sec > 180 {
                        is_stale = true;
                    }
                }
            }

            if is_alive {
                // ── Step 6 (verified alive): adopted or heartbeat_stale ────
                if is_stale {
                    situation = format!("Performer alive but heartbeat stale for {}s", elapsed_sec);
                    classification = "heartbeat_stale".to_string();
                    action = "Wait grace period, then block/requeue".to_string();
                    proposed_action = Some(MutationAction::Stale);
                } else {
                    situation = "Performer alive and heartbeat fresh".to_string();
                    classification = "adopted".to_string();
                    action = "Continue monitoring".to_string();
                    proposed_action = None;
                }
            } else {
                // ── Steps 7-8: worktree and Git branch/merge inspection ────
                let wt_path_opt = task.worktree_path();

                let has_commits = if let Some(wt) = wt_path_opt {
                    let wt_path = repo_root.join(wt);
                    branch_has_commits_ahead(
                        repo_root,
                        task.branch().unwrap_or(""),
                        reference_branch,
                    ) || crate::git::has_commits_ahead(&wt_path, reference_branch)
                } else {
                    false
                };

                // Step 8: inspect Git merge state
                let is_merge = if let Some(wt) = wt_path_opt {
                    is_merge_in_progress(&repo_root.join(wt))
                } else {
                    false
                };

                // Step 7: inspect worktree cleanliness
                let is_dirty = if let Some(wt) = wt_path_opt {
                    crate::git::is_dirty(&repo_root.join(wt)).unwrap_or(false)
                } else {
                    false
                };

                if is_merge {
                    situation = "Git merge in progress".to_string();
                    classification = "blocked_merge_recovery".to_string();
                    action = "Manual intervention required".to_string();
                    proposed_action = Some(MutationAction::Blocked);
                } else if has_commits {
                    situation = "Performer dead but commit exists".to_string();
                    classification = "phase_done".to_string();
                    action = "Continue FSM advancement".to_string();
                    proposed_action = Some(MutationAction::PhaseDone);
                } else if is_dirty {
                    situation = "Worktree dirty, no phase result".to_string();
                    classification = "blocked_dirty_worktree".to_string();
                    action = "Require operator review".to_string();
                    proposed_action = Some(MutationAction::Blocked);
                } else {
                    situation = "Performer dead, no changes".to_string();
                    classification = "process_dead".to_string();
                    action = "Requeue task".to_string();
                    proposed_action = Some(MutationAction::Requeue);
                }
            }
        } else {
            // No PID: claim without process
            situation = "Claim exists but no process spawned".to_string();
            classification = "dispatched_without_process".to_string();
            action = "Requeue safely".to_string();
            proposed_action = Some(MutationAction::Requeue);
        }

        let mutated = proposed_action.is_some();

        if let Some(log) = logger {
            log(format!(
                "- Recovery sweep task={} classification={} action=\"{}\"",
                task.id, classification, action
            ));
        }

        entries.push(StartupRecoveryEntry {
            task_id: task.id.clone(),
            situation,
            classification: classification.clone(),
            action,
            mutated: mutated && !dry_run,
        });

        if let Some(act) = proposed_action {
            proposed_mutations.push((task.id.clone(), act, classification.clone()));
        }
    }

    // 2. Application Phase (Only runs if !dry_run, applying all mutations to snapshot)
    if !dry_run && !proposed_mutations.is_empty() {
        changed = true;
        for (task_id, act, classification) in proposed_mutations {
            if let Some(task) = snapshot.registry.find_task_mut(&task_id) {
                let branch = task.branch().unwrap_or_default().to_string();
                let err_code = match classification.as_str() {
                    "heartbeat_stale" => Some("E413".to_string()),
                    "process_dead" => Some("E414".to_string()),
                    "blocked_dirty_worktree" => Some("E417".to_string()),
                    // Same code the live path uses when a same-worktree retry
                    // budget runs out, so both routes to this state read alike.
                    "parked_unschedulable_with_commits" => Some("E902".to_string()),
                    _ => None,
                };
                if let Some(code) = err_code {
                    let runtime = task.ensure_runtime();
                    runtime.last_error_code = Some(code.clone());
                    runtime.last_error = Some(match classification.as_str() {
                        "heartbeat_stale" => "Performer heartbeat stale".to_string(),
                        "process_dead" => "Performer process dead".to_string(),
                        "blocked_dirty_worktree" => "Dirty worktree blocks recovery".to_string(),
                        "parked_unschedulable_with_commits" => format!(
                            "Task could no longer be dispatched into its own worktree; committed work is unmerged on branch {}",
                            branch
                        ),
                        _ => "Recovery classification".to_string(),
                    });
                }
                match act {
                    MutationAction::Merge => {
                        task.state = "merged".to_string();
                        task.task_runtime.status = Some("idle".to_string());
                        task.clear_assignment();
                    }
                    MutationAction::Stale => {
                        task.task_runtime.status = Some("stale".to_string());
                    }
                    MutationAction::Blocked => {
                        task.state = "blocked".to_string();
                    }
                    MutationAction::PhaseDone => {
                        task.task_runtime.status = Some("phase_done".to_string());
                    }
                    MutationAction::Requeue => {
                        task.state = "todo".to_string();
                        task.task_runtime.status = Some("idle".to_string());
                        task.clear_assignment();
                    }
                }
            }
        }
    }

    // ── Step 9b: orphaned processes (no active claim) ─────────────────────
    let db_pids: Vec<i64> = snapshot
        .registry
        .tasks
        .iter()
        .filter_map(|t| t.runtime_pid())
        .collect();
    let orphaned_pids = find_orphaned_pids_local(repo_root, &db_pids).unwrap_or_default();
    for pid in orphaned_pids {
        if let Some(log) = logger {
            log(format!(
                "- Recovery sweep pid={} classification=orphaned action=\"Surface and optionally force terminate\" [E415]",
                pid
            ));
        }
        entries.push(StartupRecoveryEntry {
            task_id: "-".to_string(),
            situation: "Process exists but no active task claim".to_string(),
            classification: "orphaned".to_string(),
            action: "Surface and optionally force terminate".to_string(),
            mutated: false,
        });
    }

    // ── Step 10: persist recovery decisions (skipped when dry_run=true) ─────
    if changed && !dry_run {
        let now = now_iso_coordinator();
        snapshot.registry.recompute_resource_locks(&now);
        snapshot.registry.set_updated_at(now);
        if let Err(e) = sqlite.save_snapshot(&snapshot) {
            return Err(MaccError::Coordinator {
                code: crate::coordinator::error_normalizer::E412_RECOVERY_CLASSIFICATION_FAILED,
                message: format!("Failed to persist recovery decisions: {}", e),
            });
        } else if let Some(log) = logger {
            let mutated = entries.iter().filter(|e| e.mutated).count();
            log(format!(
                "- Recovery sweep complete: {} task(s) classified, {} mutated",
                entries.len(),
                mutated
            ));
        }
    } else if let Some(log) = logger {
        let label = if dry_run { "(dry-run)" } else { "0 mutated" };
        log(format!(
            "- Recovery sweep complete: {} task(s) classified, {}",
            entries.len(),
            label
        ));
    }

    Ok(entries)
}

/// Returns true if `branch` has commits that are not yet on `reference_branch`.
fn branch_has_commits_ahead(repo_root: &Path, branch: &str, reference_branch: &str) -> bool {
    if branch.is_empty() || reference_branch.is_empty() {
        return false;
    }
    if let Ok(out) = crate::git::run_git_output_mapped(
        repo_root,
        &[
            "rev-list",
            "--count",
            &format!("{}..{}", reference_branch, branch),
        ],
        "check branch commits ahead",
    ) {
        if out.status.success() {
            let count_str = String::from_utf8_lossy(&out.stdout).trim().to_string();
            return count_str.parse::<usize>().unwrap_or(0) > 0;
        }
    }
    false
}

/// Returns true if a merge is in progress in the given repo or worktree directory.
fn is_merge_in_progress(repo_or_worktree: &Path) -> bool {
    if let Ok(out) = crate::git::run_git_output_mapped(
        repo_or_worktree,
        &["rev-parse", "--git-path", "MERGE_HEAD"],
        "check MERGE_HEAD path",
    ) {
        if out.status.success() {
            let path_str = String::from_utf8_lossy(&out.stdout).trim().to_string();
            if !path_str.is_empty() {
                let path = std::path::Path::new(&path_str);
                let target_path = if path.is_absolute() {
                    path.to_path_buf()
                } else {
                    repo_or_worktree.join(path)
                };
                return target_path.exists();
            }
        }
    }
    false
}

/// Finds PIDs of performer/macc processes that are running in `repo_root`
/// but are not recorded in `db_pids`.
fn find_orphaned_pids_local(repo_root: &Path, db_pids: &[i64]) -> Result<Vec<i64>> {
    let mut candidate_pids = std::collections::HashSet::new();
    if let Ok(pids) = crate::coordinator::helpers::pgrep_pids("performer") {
        for pid in pids {
            candidate_pids.insert(pid);
        }
    }
    if let Ok(pids) = crate::coordinator::helpers::pgrep_pids("macc") {
        for pid in pids {
            candidate_pids.insert(pid);
        }
    }

    let mut orphaned = Vec::new();
    let current_pid = std::process::id() as i32;

    for pid in candidate_pids {
        if pid == current_pid {
            continue;
        }
        if db_pids.contains(&(pid as i64)) {
            continue;
        }
        if crate::coordinator::helpers::pid_in_repo(pid, repo_root) {
            orphaned.push(pid as i64);
        }
    }
    Ok(orphaned)
}

pub fn cleanup_registry_native(repo_root: &Path) -> Result<()> {
    let registry_value =
        crate::coordinator::state::coordinator_state_registry_load(repo_root, &BTreeMap::new())?;
    let mut registry = TaskRegistry::from_value(&registry_value)?;
    let mut changed = false;
    for task in registry.tasks.iter_mut() {
        match task.state.as_str() {
            "abandoned" | "todo" => {
                if task.worktree.is_some() {
                    task.worktree = None;
                    changed = true;
                }
                if task.assignee.is_some() {
                    task.assignee = None;
                    changed = true;
                }
                if task.task_runtime.pid.is_some() {
                    task.task_runtime.pid = None;
                    changed = true;
                }
            }
            "merged" => {
                if task.assignee.is_some() {
                    task.assignee = None;
                    changed = true;
                }
                if task.task_runtime.pid.is_some() {
                    task.task_runtime.pid = None;
                    changed = true;
                }
            }
            _ => {}
        }
    }
    if changed {
        registry.recompute_resource_locks(&now_iso_coordinator());
        registry.set_updated_at(now_iso_coordinator());
        crate::coordinator::state::coordinator_state_registry_save(
            repo_root,
            &BTreeMap::new(),
            &registry.to_value()?,
        )?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn make_test_git_repo() -> std::path::PathBuf {
        use std::time::SystemTime;
        let nanos = SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("time")
            .as_nanos();
        let repo = std::env::temp_dir().join(format!(
            "macc-state-runtime-tests-{}-{}",
            std::process::id(),
            nanos
        ));
        fs::create_dir_all(&repo).expect("create temp repo");
        std::process::Command::new("git")
            .args(&["init"])
            .current_dir(&repo)
            .output()
            .expect("git init");
        std::process::Command::new("git")
            .args(&["checkout", "-b", "main"])
            .current_dir(&repo)
            .output()
            .ok();
        std::process::Command::new("git")
            .args(&["config", "user.email", "test@example.com"])
            .current_dir(&repo)
            .output()
            .ok();
        std::process::Command::new("git")
            .args(&["config", "user.name", "Test"])
            .current_dir(&repo)
            .output()
            .ok();
        fs::write(repo.join("readme.txt"), "init\n").expect("write readme");
        std::process::Command::new("git")
            .args(&["add", "readme.txt"])
            .current_dir(&repo)
            .output()
            .expect("git add");
        std::process::Command::new("git")
            .args(&["commit", "-m", "initial commit"])
            .current_dir(&repo)
            .output()
            .expect("git commit");
        repo
    }

    #[test]
    fn test_recovery_sweep_classifies_no_pid_as_dispatched_without_process() {
        let repo = make_test_git_repo();
        let project_paths = crate::ProjectPaths::from_root(&repo);
        let storage_paths =
            crate::coordinator_storage::CoordinatorStoragePaths::from_project_paths(&project_paths);
        let sqlite = crate::coordinator_storage::SqliteStorage::new(storage_paths);

        // Build a snapshot with one "in_progress" task that has no PID
        let mut snapshot = crate::coordinator_storage::CoordinatorSnapshot::empty();
        let task_json = serde_json::json!({
            "id": "task-no-pid",
            "title": "Task without spawned process",
            "state": "in_progress",
            "dependencies": [],
            "exclusive_resources": [],
            "task_runtime": {}
        });
        let task: crate::coordinator::model::Task =
            serde_json::from_value(task_json).expect("parse task");
        snapshot.registry.tasks.push(task);
        sqlite.save_snapshot(&snapshot).expect("save snapshot");

        // Run the recovery sweep
        let entries = execute_startup_recovery_sweep(&repo, "main", false, None)
            .expect("recovery sweep should succeed");

        assert_eq!(entries.len(), 1, "expected exactly one task classified");
        let entry = &entries[0];
        assert_eq!(entry.task_id, "task-no-pid");
        assert_eq!(
            entry.classification, "dispatched_without_process",
            "no-PID task should be classified as dispatched_without_process"
        );
        assert!(entry.mutated, "task should be mutated (requeued to todo)");

        // Verify the mutation was persisted to SQLite
        let reloaded = sqlite.load_snapshot().expect("reload snapshot");
        let reloaded_task = reloaded
            .registry
            .tasks
            .iter()
            .find(|t| t.id == "task-no-pid")
            .expect("task should still exist");
        assert_eq!(
            reloaded_task.state, "todo",
            "task should have been requeued to todo"
        );
    }

    #[test]
    fn test_recovery_sweep_adopts_alive_fresh_task() {
        let repo = make_test_git_repo();
        let project_paths = crate::ProjectPaths::from_root(&repo);
        let storage_paths =
            crate::coordinator_storage::CoordinatorStoragePaths::from_project_paths(&project_paths);
        let sqlite = crate::coordinator_storage::SqliteStorage::new(storage_paths);

        // Use the current process PID to simulate an "alive" performer
        let current_pid = std::process::id() as i64;
        let now_ts = chrono::Utc::now().to_rfc3339();

        let mut snapshot = crate::coordinator_storage::CoordinatorSnapshot::empty();
        let task_json = serde_json::json!({
            "id": "task-alive",
            "title": "Task with alive process",
            "state": "in_progress",
            "dependencies": [],
            "exclusive_resources": [],
            "task_runtime": {
                "pid": current_pid,
                "last_heartbeat": now_ts
            }
        });
        let task: crate::coordinator::model::Task =
            serde_json::from_value(task_json).expect("parse task");
        snapshot.registry.tasks.push(task);
        sqlite.save_snapshot(&snapshot).expect("save snapshot");

        let entries = execute_startup_recovery_sweep(&repo, "main", false, None)
            .expect("recovery sweep should succeed");

        assert_eq!(entries.len(), 1);
        let entry = &entries[0];
        assert_eq!(entry.task_id, "task-alive");
        assert_eq!(
            entry.classification, "adopted",
            "alive task with fresh heartbeat should be adopted"
        );
        assert!(!entry.mutated, "adopted task should not be mutated");
    }

    // ── Parked `todo` tasks holding a worktree ─────────────────────────────
    //
    // These are the tasks the sweep used to skip entirely: `is_active()`
    // excludes `todo`, so a task parked by the same-worktree retry path with
    // its budget spent was neither repaired nor reported. It sat looking
    // healthy while committed work went unmerged, and a restart did not help.

    fn run_git(repo: &Path, args: &[&str]) {
        let out = std::process::Command::new("git")
            .args(args)
            .current_dir(repo)
            .output()
            .expect("run git");
        assert!(
            out.status.success(),
            "git {:?} failed: {}",
            args,
            String::from_utf8_lossy(&out.stderr)
        );
    }

    /// Build a snapshot with one parked `todo` task and run the sweep.
    fn sweep_parked_task(
        repo: &Path,
        branch: &str,
        retries: i64,
        runtime_status: &str,
    ) -> (Vec<StartupRecoveryEntry>, crate::coordinator::model::Task) {
        let project_paths = crate::ProjectPaths::from_root(repo);
        let storage_paths =
            crate::coordinator_storage::CoordinatorStoragePaths::from_project_paths(&project_paths);
        let sqlite = crate::coordinator_storage::SqliteStorage::new(storage_paths);

        let mut snapshot = crate::coordinator_storage::CoordinatorSnapshot::empty();
        let task_json = serde_json::json!({
            "id": "task-parked",
            "title": "Parked task holding a worktree",
            "state": "todo",
            "dependencies": [],
            "exclusive_resources": [],
            "worktree": {
                "worktree_path": repo.join(".macc/worktree/worker-01").to_string_lossy(),
                "branch": branch,
                "base_branch": "main"
            },
            "task_runtime": { "status": runtime_status, "retries": retries }
        });
        let task: crate::coordinator::model::Task =
            serde_json::from_value(task_json).expect("parse task");
        snapshot.registry.tasks.push(task);
        sqlite.save_snapshot(&snapshot).expect("save snapshot");

        let entries = execute_startup_recovery_sweep(repo, "main", false, None)
            .expect("recovery sweep should succeed");
        let reloaded = sqlite.load_snapshot().expect("reload snapshot");
        let task = reloaded
            .registry
            .tasks
            .into_iter()
            .find(|t| t.id == "task-parked")
            .expect("task should still exist");
        (entries, task)
    }

    #[test]
    fn sweep_blocks_a_parked_task_whose_branch_holds_unmerged_commits() {
        let repo = make_test_git_repo();
        run_git(&repo, &["checkout", "-q", "-b", "ai/codex/worker-01"]);
        fs::write(repo.join("work.txt"), "work\n").expect("write");
        run_git(&repo, &["add", "work.txt"]);
        run_git(&repo, &["commit", "-qm", "task work"]);
        run_git(&repo, &["checkout", "-q", "main"]);

        // retries (5) exceeds the default budget, so the task is unschedulable.
        let (entries, task) = sweep_parked_task(&repo, "ai/codex/worker-01", 5, "failed");

        assert_eq!(
            entries.len(),
            1,
            "the parked task must be reported: {entries:?}"
        );
        assert_eq!(
            entries[0].classification,
            "parked_unschedulable_with_commits"
        );
        assert!(entries[0].mutated);
        assert_eq!(
            task.state, "blocked",
            "work is at stake, so an operator must decide"
        );
        assert_eq!(task.task_runtime.last_error_code.as_deref(), Some("E902"));
        assert!(
            task.task_runtime
                .last_error
                .as_deref()
                .unwrap_or_default()
                .contains("ai/codex/worker-01"),
            "the branch holding the work must be named: {:?}",
            task.task_runtime.last_error
        );
        assert!(
            task.worktree.is_some(),
            "the worktree pointer must survive so the work can be found"
        );
        let _ = fs::remove_dir_all(&repo);
    }

    #[test]
    fn sweep_requeues_a_parked_task_with_nothing_at_stake() {
        let repo = make_test_git_repo();
        run_git(&repo, &["branch", "ai/codex/worker-01"]); // no commits ahead

        let (entries, task) = sweep_parked_task(&repo, "ai/codex/worker-01", 5, "failed");

        assert_eq!(entries.len(), 1, "got: {entries:?}");
        assert_eq!(entries[0].classification, "parked_unschedulable");
        assert_eq!(task.state, "todo");
        assert!(
            task.worktree.is_none(),
            "the stale attachment must be released so the task can dispatch again"
        );
        let _ = fs::remove_dir_all(&repo);
    }

    #[test]
    fn sweep_leaves_a_resumable_parked_task_alone() {
        // Budget remains, so dispatch will resume it this run. Touching it
        // would throw away the commits the retry is meant to build on.
        let repo = make_test_git_repo();
        run_git(&repo, &["checkout", "-q", "-b", "ai/codex/worker-01"]);
        fs::write(repo.join("work.txt"), "work\n").expect("write");
        run_git(&repo, &["add", "work.txt"]);
        run_git(&repo, &["commit", "-qm", "task work"]);
        run_git(&repo, &["checkout", "-q", "main"]);

        let (entries, task) = sweep_parked_task(&repo, "ai/codex/worker-01", 1, "failed");

        assert!(
            entries.is_empty(),
            "a resumable task needs no repair: {entries:?}"
        );
        assert_eq!(task.state, "todo");
        assert!(task.worktree.is_some());
        let _ = fs::remove_dir_all(&repo);
    }

    #[test]
    fn sweep_ignores_a_clean_todo_task() {
        let repo = make_test_git_repo();
        let project_paths = crate::ProjectPaths::from_root(&repo);
        let storage_paths =
            crate::coordinator_storage::CoordinatorStoragePaths::from_project_paths(&project_paths);
        let sqlite = crate::coordinator_storage::SqliteStorage::new(storage_paths);

        let mut snapshot = crate::coordinator_storage::CoordinatorSnapshot::empty();
        let task: crate::coordinator::model::Task = serde_json::from_value(serde_json::json!({
            "id": "task-fresh",
            "state": "todo",
            "dependencies": [],
            "exclusive_resources": [],
            "task_runtime": {}
        }))
        .expect("parse task");
        snapshot.registry.tasks.push(task);
        sqlite.save_snapshot(&snapshot).expect("save snapshot");

        let entries = execute_startup_recovery_sweep(&repo, "main", false, None)
            .expect("recovery sweep should succeed");
        assert!(
            entries.is_empty(),
            "an ordinary todo task must not be touched: {entries:?}"
        );
        let _ = fs::remove_dir_all(&repo);
    }
}
