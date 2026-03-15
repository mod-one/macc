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
    let payload: CoordinatorPauseFile = serde_json::from_value(payload).map_err(|e| {
        MaccError::Validation(format!(
            "Failed to build coordinator pause file '{}': {}",
            path.display(),
            e
        ))
    })?;
    let body = serde_json::to_string_pretty(&payload).map_err(|e| {
        MaccError::Validation(format!(
            "Failed to serialize coordinator pause file '{}': {}",
            path.display(),
            e
        ))
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
    let value: CoordinatorPauseFile = serde_json::from_str(&raw).map_err(|e| {
        MaccError::Validation(format!(
            "Failed to parse coordinator pause file '{}': {}",
            path.display(),
            e
        ))
    })?;
    Ok(Some(value))
}

pub fn set_task_paused_for_integrate(repo_root: &Path, task_id: &str, reason: &str) -> Result<()> {
    let mut args = BTreeMap::new();
    args.insert("task-id".to_string(), task_id.to_string());
    args.insert("runtime-status".to_string(), "paused".to_string());
    args.insert("phase".to_string(), "integrate".to_string());
    args.insert("last-error".to_string(), reason.to_string());
    args.insert("pid".to_string(), "".to_string());
    crate::coordinator::state::coordinator_state_set_runtime(repo_root, &args)
}

pub fn resume_paused_task_integrate(repo_root: &Path, task_id: &str) -> Result<()> {
    let mut transition_args = BTreeMap::new();
    transition_args.insert("task-id".to_string(), task_id.to_string());
    transition_args.insert("state".to_string(), "queued".to_string());
    transition_args.insert("reason".to_string(), "resume:integrate_pause".to_string());
    crate::coordinator::state::coordinator_state_apply_transition(repo_root, &transition_args)?;

    let mut runtime_args = BTreeMap::new();
    runtime_args.insert("task-id".to_string(), task_id.to_string());
    runtime_args.insert("runtime-status".to_string(), "phase_done".to_string());
    runtime_args.insert("phase".to_string(), "integrate".to_string());
    runtime_args.insert("pid".to_string(), "".to_string());
    crate::coordinator::state::coordinator_state_set_runtime(repo_root, &runtime_args)
}

fn is_pid_running(pid: i64) -> bool {
    if pid <= 0 {
        return false;
    }
    std::process::Command::new("kill")
        .arg("-0")
        .arg(pid.to_string())
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
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
