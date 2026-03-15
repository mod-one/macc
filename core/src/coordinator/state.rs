use crate::coordinator::model::TaskRegistry;
use crate::coordinator::{CoordinatorEventPayload, RuntimeStatus, WorkflowState};
use crate::coordinator_storage::{
    apply_transition_sqlite_with_event, coordinator_storage_export_sqlite_to_json,
    increment_retries_sqlite, set_merge_pending_sqlite, set_merge_processed_sqlite,
    set_runtime_sqlite_with_event, upsert_slo_warning_sqlite, CoordinatorSnapshot,
    CoordinatorStorage, CoordinatorStorageMode, CoordinatorStoragePaths, EventMutation,
    JsonStorage, MergePendingMutation, MergeProcessedMutation, RetryIncrementMutation,
    RuntimeMutation, SloWarningMutation, SqliteStorage, TransitionMutation,
};
use crate::{MaccError, ProjectPaths, Result};
use serde_json::{json, Value};
use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

pub fn coordinator_state_apply_transition(
    repo_root: &Path,
    args: &BTreeMap<String, String>,
) -> Result<()> {
    let task_id = required_arg(args, "task-id")?;
    let new_state = required_arg(args, "state")?;
    let pr_url = args.get("pr-url").cloned().unwrap_or_default();
    let reviewer = args.get("reviewer").cloned().unwrap_or_default();
    let reason = args.get("reason").cloned().unwrap_or_default();
    let now = now_iso();
    let storage_mode = resolve_storage_mode(args)?;
    if storage_mode != CoordinatorStorageMode::Json {
        let paths = ProjectPaths::from_root(repo_root);
        let event = parse_optional_event_mutation(args, &task_id, &new_state)?;
        let change = TransitionMutation {
            task_id: task_id.clone(),
            new_state: new_state.clone(),
            pr_url,
            reviewer,
            reason,
            now,
        };
        apply_transition_sqlite_with_event(&paths, &change, event.as_ref())?;
        maybe_mirror_json(&paths, args)?;
        return Ok(());
    }

    let registry_path = coordinator_registry_path(repo_root);
    let mut typed = TaskRegistry::from_value(&load_registry(&registry_path)?)?;
    let Some(task) = typed.find_task_mut(&task_id) else {
        return Err(MaccError::Validation(format!(
            "Task not found in registry: {}",
            task_id
        )));
    };
    task.state = new_state.clone();
    task.updated_at = Some(now.clone());
    task.state_changed_at = Some(now.clone());

    if new_state == WorkflowState::PrOpen.as_str() && !pr_url.is_empty() {
        task.pr_url = Some(pr_url.clone());
    }
    if new_state == WorkflowState::ChangesRequested.as_str() {
        let mut review = task.review.clone().unwrap_or_default();
        review.changed = Some(true);
        review.last_reviewed_at = Some(now.clone());
        if !reviewer.is_empty() {
            review.reviewer = Some(reviewer.clone());
        }
        if !reason.is_empty() {
            review.reason = Some(reason.clone());
        }
        task.review = Some(review);
    }
    if matches!(new_state.as_str(), "merged" | "abandoned" | "todo") {
        task.clear_assignment();
        reset_runtime_to_idle_typed(task);
    }

    typed.recompute_resource_locks(&now);
    typed.updated_at = Some(now);
    save_registry(&registry_path, &typed.to_value()?)?;
    Ok(())
}

pub fn coordinator_state_set_runtime(
    repo_root: &Path,
    args: &BTreeMap<String, String>,
) -> Result<()> {
    let task_id = required_arg(args, "task-id")?;
    let runtime_status = required_arg(args, "runtime-status")?;
    let phase = args.get("phase").cloned().unwrap_or_default();
    let pid = args.get("pid").cloned().unwrap_or_default();
    let last_error = args.get("last-error").cloned().unwrap_or_default();
    let heartbeat_ts = args.get("heartbeat-ts").cloned().unwrap_or_default();
    let attempt = args.get("attempt").cloned().unwrap_or_default();
    let now = now_iso();
    let storage_mode = resolve_storage_mode(args)?;
    if storage_mode != CoordinatorStorageMode::Json {
        let paths = ProjectPaths::from_root(repo_root);
        let event = parse_optional_event_mutation(args, &task_id, &runtime_status)?;
        let change = RuntimeMutation {
            task_id: task_id.clone(),
            runtime_status: runtime_status.clone(),
            phase,
            pid: if pid.is_empty() {
                None
            } else {
                pid.parse::<i64>().ok()
            },
            last_error,
            heartbeat_ts,
            attempt: if attempt.is_empty() {
                None
            } else {
                attempt.parse::<i64>().ok()
            },
            now,
        };
        set_runtime_sqlite_with_event(&paths, &change, event.as_ref())?;
        maybe_mirror_json(&paths, args)?;
        return Ok(());
    }

    let registry_path = coordinator_registry_path(repo_root);
    let mut typed = TaskRegistry::from_value(&load_registry(&registry_path)?)?;
    let Some(task) = typed.find_task_mut(&task_id) else {
        return Err(MaccError::Validation(format!(
            "Task not found in registry: {}",
            task_id
        )));
    };
    let runtime = &mut task.task_runtime;
    let old_status = runtime.status.clone().unwrap_or_else(|| "idle".to_string());
    let old_phase = runtime.current_phase.clone().unwrap_or_default();

    runtime.status = Some(runtime_status.clone());
    if !phase.is_empty() {
        runtime.current_phase = Some(phase.clone());
    }
    if !pid.is_empty() {
        runtime.pid = pid.parse::<i64>().ok();
    } else if matches!(
        runtime_status.as_str(),
        "idle" | "phase_done" | "failed" | "stale"
    ) {
        runtime.pid = None;
    }
    if !last_error.is_empty() {
        runtime.last_error = Some(last_error.clone());
    }
    if !heartbeat_ts.is_empty() {
        runtime.last_heartbeat = Some(heartbeat_ts);
    }
    if !attempt.is_empty() {
        runtime.attempt = attempt.parse::<i64>().ok();
    }
    if runtime_status == RuntimeStatus::Running.as_str()
        && runtime.started_at.as_deref().unwrap_or_default().is_empty()
    {
        runtime.started_at = Some(now.clone());
    }
    let phase_changed = !phase.is_empty() && phase != old_phase;
    let status_became_running = old_status != RuntimeStatus::Running.as_str()
        && runtime_status == RuntimeStatus::Running.as_str();
    let missing_phase_started = runtime
        .phase_started_at
        .as_deref()
        .unwrap_or_default()
        .is_empty();
    if runtime_status == RuntimeStatus::Running.as_str()
        && (phase_changed || status_became_running || missing_phase_started)
    {
        runtime.phase_started_at = Some(now.clone());
    } else if matches!(
        runtime_status.as_str(),
        "idle" | "phase_done" | "failed" | "stale"
    ) {
        runtime.phase_started_at = None;
    }
    task.updated_at = Some(now.clone());
    typed.updated_at = Some(now);
    save_registry(&registry_path, &typed.to_value()?)?;
    Ok(())
}

pub fn coordinator_state_task_field(
    repo_root: &Path,
    args: &BTreeMap<String, String>,
) -> Result<()> {
    let task_id = required_arg(args, "task-id")?;
    let field_expr = required_arg(args, "field")?;
    let snapshot = load_snapshot_view(repo_root, args)?;
    if let Some(task) = snapshot.registry.find_task(&task_id) {
        if let Some(value) = extract_task_field_value_typed(task, &field_expr) {
            println!("{}", value);
        }
    }
    Ok(())
}

pub fn coordinator_state_task_exists(
    repo_root: &Path,
    args: &BTreeMap<String, String>,
) -> Result<()> {
    let task_id = required_arg(args, "task-id")?;
    let snapshot = load_snapshot_view(repo_root, args)?;
    let exists = snapshot.registry.find_task(&task_id).is_some();
    if exists {
        Ok(())
    } else {
        Err(MaccError::Validation(format!(
            "Task not found in registry: {}",
            task_id
        )))
    }
}

pub fn coordinator_state_counts(repo_root: &Path, args: &BTreeMap<String, String>) -> Result<()> {
    let snapshot = load_snapshot_view(repo_root, args)?;
    let (total, todo, active, blocked, merged) = snapshot.registry.counts();
    println!("{}\t{}\t{}\t{}\t{}", total, todo, active, blocked, merged);
    Ok(())
}

pub fn coordinator_state_locks(repo_root: &Path, args: &BTreeMap<String, String>) -> Result<()> {
    let format = args
        .get("format")
        .cloned()
        .unwrap_or_else(|| "count".to_string());
    let snapshot = load_snapshot_view(repo_root, args)?;
    let locks = snapshot.registry.resource_locks;
    match format.as_str() {
        "count" => println!("{}", locks.len()),
        "lines" => {
            let mut rows: Vec<(String, String)> = locks
                .iter()
                .map(|(resource, value)| (resource.clone(), value.task_id.clone()))
                .collect();
            rows.sort_by(|a, b| a.0.cmp(&b.0));
            for (resource, task_id) in rows {
                println!("{} -> {}", resource, task_id);
            }
        }
        other => {
            return Err(MaccError::Validation(format!(
                "Unknown lock format '{}'; expected count|lines",
                other
            )));
        }
    }
    Ok(())
}

pub fn coordinator_state_snapshot(
    repo_root: &Path,
    args: &BTreeMap<String, String>,
) -> Result<CoordinatorSnapshot> {
    load_snapshot_view(repo_root, args)
}

pub fn coordinator_state_save_snapshot(
    repo_root: &Path,
    args: &BTreeMap<String, String>,
    snapshot: &CoordinatorSnapshot,
) -> Result<()> {
    let mode = resolve_storage_mode(args)?;
    let project_paths = ProjectPaths::from_root(repo_root);
    let store_paths = CoordinatorStoragePaths::from_project_paths(&project_paths);
    match mode {
        CoordinatorStorageMode::Json => JsonStorage::new(store_paths).save_snapshot(snapshot)?,
        CoordinatorStorageMode::DualWrite | CoordinatorStorageMode::Sqlite => {
            SqliteStorage::new(store_paths.clone()).save_snapshot(snapshot)?;
            maybe_mirror_json(&project_paths, args)?;
        }
    }
    Ok(())
}

pub fn coordinator_state_unlock_resource(
    repo_root: &Path,
    args: &BTreeMap<String, String>,
    resource: Option<&str>,
    clear_all: bool,
) -> Result<usize> {
    let mut snapshot = load_snapshot_view(repo_root, args)?;
    let removed = if clear_all {
        let count = snapshot.registry.resource_locks.len();
        snapshot.registry.resource_locks.clear();
        count
    } else if let Some(name) = resource {
        if snapshot.registry.resource_locks.remove(name).is_some() {
            1
        } else {
            0
        }
    } else {
        0
    };
    coordinator_state_save_snapshot(repo_root, args, &snapshot)?;
    Ok(removed)
}

pub fn coordinator_state_set_merge_pending(
    repo_root: &Path,
    args: &BTreeMap<String, String>,
) -> Result<()> {
    let task_id = required_arg(args, "task-id")?;
    let result_file = required_arg(args, "result-file")?;
    let pid = args.get("pid").and_then(|v| v.parse::<i64>().ok());
    let now = args.get("now").cloned().unwrap_or_else(now_iso);
    let paths = ProjectPaths::from_root(repo_root);
    let change = MergePendingMutation {
        task_id,
        result_file,
        pid,
        now,
    };
    set_merge_pending_sqlite(&paths, &change)?;
    maybe_mirror_json(&paths, args)?;
    Ok(())
}

pub fn coordinator_state_set_merge_processed(
    repo_root: &Path,
    args: &BTreeMap<String, String>,
) -> Result<()> {
    let task_id = required_arg(args, "task-id")?;
    let result_file = args.get("result-file").cloned().unwrap_or_default();
    let status = args.get("status").cloned().unwrap_or_default();
    let rc = args.get("rc").and_then(|v| v.parse::<i64>().ok());
    let now = args.get("now").cloned().unwrap_or_else(now_iso);
    let paths = ProjectPaths::from_root(repo_root);
    let change = MergeProcessedMutation {
        task_id,
        result_file,
        status,
        rc,
        now,
    };
    set_merge_processed_sqlite(&paths, &change)?;
    maybe_mirror_json(&paths, args)?;
    Ok(())
}

pub fn coordinator_state_increment_retries(
    repo_root: &Path,
    args: &BTreeMap<String, String>,
) -> Result<()> {
    let task_id = required_arg(args, "task-id")?;
    let now = args.get("now").cloned().unwrap_or_else(now_iso);
    let paths = ProjectPaths::from_root(repo_root);
    let change = RetryIncrementMutation { task_id, now };
    increment_retries_sqlite(&paths, &change)?;
    maybe_mirror_json(&paths, args)?;
    Ok(())
}

pub fn coordinator_state_upsert_slo_warning(
    repo_root: &Path,
    args: &BTreeMap<String, String>,
) -> Result<()> {
    let task_id = required_arg(args, "task-id")?;
    let metric = required_arg(args, "metric")?;
    let threshold = required_arg(args, "threshold")?
        .parse::<i64>()
        .map_err(|e| MaccError::Validation(format!("Invalid --threshold: {}", e)))?;
    let value = required_arg(args, "value")?
        .parse::<i64>()
        .map_err(|e| MaccError::Validation(format!("Invalid --value: {}", e)))?;
    let suggestion = args.get("suggestion").cloned().unwrap_or_default();
    let now = args.get("now").cloned().unwrap_or_else(now_iso);
    let paths = ProjectPaths::from_root(repo_root);
    let change = SloWarningMutation {
        task_id,
        metric,
        threshold,
        value,
        suggestion,
        now,
    };
    upsert_slo_warning_sqlite(&paths, &change)?;
    maybe_mirror_json(&paths, args)?;
    Ok(())
}

pub fn coordinator_state_slo_metric(
    repo_root: &Path,
    args: &BTreeMap<String, String>,
) -> Result<()> {
    let task_id = required_arg(args, "task-id")?;
    let metric = required_arg(args, "metric")?;
    let snapshot = load_snapshot_view(repo_root, args)?;
    let task = snapshot.registry.find_task(&task_id);
    if let Some(task) = task {
        let value = task.task_runtime.metric_i64(&metric).unwrap_or(0);
        let warned = task.task_runtime.has_slo_warning(&metric);
        println!("{}\t{}", value, if warned { "true" } else { "false" });
    } else {
        println!("0\tfalse");
    }
    Ok(())
}

pub fn coordinator_state_registry_load(
    repo_root: &Path,
    args: &BTreeMap<String, String>,
) -> Result<Value> {
    load_snapshot_view(repo_root, args)?.registry.to_value()
}

pub fn coordinator_state_registry_save(
    repo_root: &Path,
    args: &BTreeMap<String, String>,
    registry: &Value,
) -> Result<()> {
    let mode = resolve_storage_mode(args)?;
    let project_paths = ProjectPaths::from_root(repo_root);
    let store_paths = CoordinatorStoragePaths::from_project_paths(&project_paths);
    let mut snapshot = match mode {
        CoordinatorStorageMode::Json => JsonStorage::new(store_paths.clone()).load_snapshot()?,
        CoordinatorStorageMode::DualWrite | CoordinatorStorageMode::Sqlite => {
            if SqliteStorage::new(store_paths.clone()).has_snapshot_data()? {
                SqliteStorage::new(store_paths.clone()).load_snapshot()?
            } else if store_paths.registry_json_path.exists() {
                JsonStorage::new(store_paths.clone()).load_snapshot()?
            } else {
                CoordinatorSnapshot::empty()
            }
        }
    };
    snapshot.registry = TaskRegistry::from_value(registry)?;
    match mode {
        CoordinatorStorageMode::Json => {
            JsonStorage::new(store_paths).save_snapshot(&snapshot)?;
        }
        CoordinatorStorageMode::DualWrite | CoordinatorStorageMode::Sqlite => {
            SqliteStorage::new(store_paths.clone()).save_snapshot(&snapshot)?;
            if should_mirror_json(args) {
                let _ = JsonStorage::new(store_paths).save_snapshot(&snapshot);
            }
        }
    }
    Ok(())
}

fn required_arg(args: &BTreeMap<String, String>, key: &str) -> Result<String> {
    let value = args
        .get(key)
        .cloned()
        .unwrap_or_default()
        .trim()
        .to_string();
    if value.is_empty() {
        return Err(MaccError::Validation(format!("Missing --{}", key)));
    }
    Ok(value)
}

fn coordinator_registry_path(repo_root: &Path) -> PathBuf {
    repo_root
        .join(".macc")
        .join("automation")
        .join("task")
        .join("task_registry.json")
}

fn load_registry(path: &Path) -> Result<Value> {
    let raw = fs::read_to_string(path).map_err(|e| MaccError::Io {
        path: path.to_string_lossy().into(),
        action: "read coordinator registry".into(),
        source: e,
    })?;
    serde_json::from_str::<Value>(&raw).map_err(|e| {
        MaccError::Validation(format!(
            "Failed to parse coordinator registry '{}': {}",
            path.display(),
            e
        ))
    })
}

fn save_registry(path: &Path, value: &Value) -> Result<()> {
    let body = serde_json::to_vec_pretty(value).map_err(|e| {
        MaccError::Validation(format!(
            "Failed to serialize coordinator registry '{}': {}",
            path.display(),
            e
        ))
    })?;
    let tmp = path.with_extension("tmp");
    fs::write(&tmp, body).map_err(|e| MaccError::Io {
        path: tmp.to_string_lossy().into(),
        action: "write coordinator registry temp file".into(),
        source: e,
    })?;
    fs::rename(&tmp, path).map_err(|e| MaccError::Io {
        path: path.to_string_lossy().into(),
        action: "persist coordinator registry".into(),
        source: e,
    })?;
    Ok(())
}

fn reset_runtime_to_idle_typed(task: &mut crate::coordinator::model::Task) {
    let runtime = &mut task.task_runtime;
    runtime.status = Some(RuntimeStatus::Idle.as_str().to_string());
    runtime.pid = None;
    runtime.started_at = None;
    runtime.current_phase = None;
    runtime.clear_last_error_details();
    runtime.merge_result_pending = Some(false);
    runtime.merge_result_file = None;
}

fn now_iso() -> String {
    chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Secs, true)
}

fn resolve_storage_mode(args: &BTreeMap<String, String>) -> Result<CoordinatorStorageMode> {
    let raw = args
        .get("storage-mode")
        .cloned()
        .unwrap_or_else(|| "sqlite".to_string());
    raw.parse::<CoordinatorStorageMode>()
        .map_err(MaccError::Validation)
}

fn should_mirror_json(args: &BTreeMap<String, String>) -> bool {
    let raw = args
        .get("mirror-json")
        .cloned()
        .unwrap_or_else(|| "0".to_string());
    !matches!(
        raw.trim().to_ascii_lowercase().as_str(),
        "0" | "false" | "no" | "off"
    )
}

fn mirror_json_debounce_ms(args: &BTreeMap<String, String>) -> u64 {
    args.get("mirror-json-debounce-ms")
        .cloned()
        .and_then(|raw| raw.trim().parse::<u64>().ok())
        .unwrap_or(0)
}

fn mirror_export_guard() -> &'static Mutex<std::collections::HashMap<std::path::PathBuf, Instant>> {
    static LAST_EXPORT: OnceLock<Mutex<std::collections::HashMap<std::path::PathBuf, Instant>>> =
        OnceLock::new();
    LAST_EXPORT.get_or_init(|| Mutex::new(std::collections::HashMap::new()))
}

fn maybe_mirror_json(project_paths: &ProjectPaths, args: &BTreeMap<String, String>) -> Result<()> {
    if !should_mirror_json(args) {
        return Ok(());
    }
    let debounce_ms = mirror_json_debounce_ms(args);
    if debounce_ms == 0 {
        return coordinator_storage_export_sqlite_to_json(project_paths);
    }

    let now = Instant::now();
    let threshold = Duration::from_millis(debounce_ms);
    let mut guard = mirror_export_guard()
        .lock()
        .map_err(|_| MaccError::Validation("mirror export guard lock poisoned".to_string()))?;
    let key = project_paths.root.clone();
    let should_export = guard
        .get(&key)
        .map(|last| now.saturating_duration_since(*last) >= threshold)
        .unwrap_or(true);
    if should_export {
        coordinator_storage_export_sqlite_to_json(project_paths)?;
        guard.insert(key, now);
    }
    Ok(())
}

fn load_snapshot_view(
    repo_root: &Path,
    args: &BTreeMap<String, String>,
) -> Result<CoordinatorSnapshot> {
    let mode = resolve_storage_mode(args)?;
    let project_paths = ProjectPaths::from_root(repo_root);
    let store_paths = CoordinatorStoragePaths::from_project_paths(&project_paths);
    match mode {
        CoordinatorStorageMode::Json => JsonStorage::new(store_paths).load_snapshot(),
        CoordinatorStorageMode::DualWrite | CoordinatorStorageMode::Sqlite => {
            let sqlite = SqliteStorage::new(store_paths.clone()).load_snapshot();
            match sqlite {
                Ok(snapshot) => Ok(snapshot),
                Err(sql_err) if allow_legacy_json_fallback(args) => JsonStorage::new(store_paths)
                    .load_snapshot()
                    .or(Err(sql_err)),
                Err(sql_err) => Err(sql_err),
            }
        }
    }
}

fn allow_legacy_json_fallback(args: &BTreeMap<String, String>) -> bool {
    let raw = args
        .get("legacy-json-fallback")
        .cloned()
        .unwrap_or_else(|| "0".to_string());
    !matches!(
        raw.trim().to_ascii_lowercase().as_str(),
        "0" | "false" | "no" | "off"
    )
}

fn extract_task_field_value_typed(
    task: &crate::coordinator::model::Task,
    field_expr: &str,
) -> Option<String> {
    let expr = field_expr.trim();
    match expr {
        ".state" => Some(task.state.clone()),
        ".scope // \"\"" => Some(task.scope().unwrap_or_default().to_string()),
        ".task_runtime.status // \"idle\"" => Some(
            task.task_runtime
                .status
                .clone()
                .unwrap_or_else(|| RuntimeStatus::Idle.as_str().to_string()),
        ),
        ".task_runtime.current_phase // \"\"" => {
            Some(task.task_runtime.current_phase.clone().unwrap_or_default())
        }
        ".task_runtime.last_error // \"\"" => {
            Some(task.task_runtime.last_error.clone().unwrap_or_default())
        }
        ".task_runtime.retries // .task_runtime.metrics.retries // 0" => {
            Some(task.task_runtime.retries_count().to_string())
        }
        _ => {
            if let Some(metric_name) = expr
                .strip_prefix(".task_runtime.metrics.")
                .and_then(|v| v.strip_suffix(" // 0"))
            {
                let value = task.task_runtime.metric_i64(metric_name).unwrap_or(0);
                return Some(value.to_string());
            }
            None
        }
    }
}

fn parse_optional_event_mutation(
    args: &BTreeMap<String, String>,
    default_task_id: &str,
    default_status: &str,
) -> Result<Option<EventMutation>> {
    let event_type = args.get("event-type").cloned().unwrap_or_default();
    if event_type.trim().is_empty() {
        return Ok(None);
    }
    let event_task_id = args
        .get("event-task-id")
        .cloned()
        .filter(|v| !v.trim().is_empty())
        .unwrap_or_else(|| default_task_id.to_string());
    let event_status = args
        .get("event-status")
        .cloned()
        .filter(|v| !v.trim().is_empty())
        .unwrap_or_else(|| default_status.to_string());
    let event_phase = args.get("event-phase").cloned().unwrap_or_default();
    let event_source = args
        .get("event-source")
        .cloned()
        .filter(|v| !v.trim().is_empty())
        .unwrap_or_else(|| "coordinator:native:state".to_string());
    let payload = match args.get("event-payload-json").cloned() {
        Some(raw) if !raw.trim().is_empty() => serde_json::from_str::<Value>(&raw)
            .map_err(|e| MaccError::Validation(format!("Invalid --event-payload-json: {}", e)))?,
        _ => {
            let msg = args.get("event-message").cloned().unwrap_or_default();
            if msg.is_empty() {
                json!({})
            } else {
                json!({ "message": msg })
            }
        }
    };
    let seq = args.get("event-seq").and_then(|v| v.parse::<i64>().ok());
    Ok(Some(EventMutation {
        event_id: args.get("event-id").cloned(),
        run_id: args.get("event-run-id").cloned(),
        seq,
        ts: args.get("event-ts").cloned(),
        source: event_source,
        task_id: event_task_id,
        event_type,
        phase: event_phase,
        status: event_status,
        payload: CoordinatorEventPayload::from(payload),
    }))
}
