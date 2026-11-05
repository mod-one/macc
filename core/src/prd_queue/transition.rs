//! Safe series transitions, shared by native commands and read-only client previews.
use super::*;
use crate::coordinator::model::TaskRegistry;
use crate::coordinator_storage::{
    CoordinatorStorage, CoordinatorStoragePaths, JsonStorage, SqliteStorage,
};
use crate::{config::CoordinatorConfig, ProjectPaths};
use std::time::Duration;

pub const CODE: &str = "MACC-PRD-QUEUE-TRANSITION";

fn blocked(reason: impl std::fmt::Display) -> MaccError {
    invalid(format!("{CODE}: {reason}. Review `macc coordinator prds check` and `macc coordinator prds status`; explicitly accept an intended change with `macc coordinator prds reset`. Existing work is preserved; application source repair cannot resolve a queue transition."))
}
pub(super) fn registry(root: &Path, config: &CoordinatorConfig) -> Result<TaskRegistry> {
    let paths = ProjectPaths::from_root(root);
    let mut storage = CoordinatorStoragePaths::from_project_paths(&paths);
    if let Some(file) = &config.task_registry_file {
        storage.registry_json_path = root.join(file);
    }
    if config.storage_mode.as_deref() != Some("json") && storage.sqlite_path.exists() {
        SqliteStorage::new(storage).load_registry()
    } else {
        Ok(JsonStorage::new(storage).load_snapshot()?.registry)
    }
}
fn quiescent(root: &Path, config: &CoordinatorConfig) -> Result<TaskRegistry> {
    let paths = ProjectPaths::from_root(root);
    if crate::supervisor::live::load(&paths).is_some_and(|s| s.active && s.running) {
        return Err(blocked("a supervisor intervention is active"));
    }
    if let Ok(pid) = std::fs::read_to_string(root.join(".macc/state/coordinator.pid")) {
        if pid.trim().parse::<i64>().ok().is_some_and(|p| {
            p != i64::from(std::process::id()) && crate::coordinator::helpers::is_pid_running(p)
        }) {
            return Err(blocked("the coordinator is still running"));
        }
    }
    let registry = registry(root, config)?;
    if registry.tasks.iter().any(|t| {
        t.task_runtime
            .pid
            .is_some_and(crate::coordinator::helpers::is_pid_running)
            || matches!(t.state.as_str(), "claimed" | "in_progress")
    }) {
        return Err(blocked("workers or pending claimed work are still active"));
    }
    super::transition_safety::check(root, config, &registry)?;
    Ok(registry)
}

/// Preview only: the persisted queue/cursor and history are never changed here.
pub fn preview(root: &Path, config: &CoordinatorConfig, entries: &[Entry]) -> Result<Progress> {
    let previous = load(root)?;
    if previous.entries == entries {
        return Ok(previous);
    }
    if previous.entries.is_empty() {
        super::identity::check(root, config, &previous, entries)?;
        return Ok(previous);
    }
    if previous.next > previous.entries.len() {
        return Err(blocked("invalid recorded queue cursor"));
    }
    let extension = entries.starts_with(&previous.entries);
    super::identity::check(root, config, &previous, entries)?;
    if entries.iter().any(|entry| {
        previous
            .entries
            .iter()
            .any(|old| old.path == entry.path && old.fingerprint != entry.fingerprint)
    }) {
        return Err(changed_error(&previous.entries, entries));
    }
    if !extension && (previous.status != "completed" || previous.next != previous.entries.len()) {
        return Err(changed_error(&previous.entries, entries));
    }
    let current = quiescent(root, config)?;
    if !extension || previous.status == "completed" {
        for (i, entry) in previous.entries.iter().enumerate() {
            let file = root.join(format!(".macc/state/prd-queue-completed-{i}.json"));
            let delivered: TaskRegistry =
                serde_json::from_slice(&read(&file).map_err(|_| {
                    blocked(format!("missing delivery evidence for {}", entry.path))
                })?)
                .map_err(|e| blocked(format!("invalid delivery evidence: {e}")))?;
            if !incomplete_tasks(entry, &delivered).is_empty() {
                return Err(blocked(format!(
                    "{} still has incomplete delivery or gates",
                    entry.path
                )));
            }
        }
        if current.tasks.iter().any(|t| !t.satisfies_dependants()) {
            return Err(blocked(
                "the current registry still contains unfinished work",
            ));
        }
    }
    Ok(Progress {
        entries: entries.to_vec(),
        next: if extension { previous.next } else { 0 },
        status: if extension && previous.status != "completed" {
            previous.status
        } else {
            "ready".into()
        },
        reason: if extension { previous.reason } else { None },
    })
}

/// Archive before changing a cursor. UUID generations prevent index collisions.
/// The queue lock must be held by the caller throughout reconciliation/sync.
pub(crate) fn reconcile_locked(root: &Path, config: &CoordinatorConfig) -> Result<Progress> {
    let entries = validate(root, &config.prd_files)?;
    let previous = load(root)?;
    let next = preview(root, config, &entries)?;
    if previous.entries.is_empty() || previous.entries == entries {
        return Ok(next);
    }
    archive(root, &previous, config, "automatic transition")?;
    preserve_deliveries(root, config)?;
    save(root, &next)?;
    tracing::info!(
        code = CODE,
        completed = previous.next,
        previous_prds = previous.entries.len(),
        new_prds = entries.len(),
        next = next.next,
        "PRD queue transition accepted; previous series archived"
    );
    Ok(next)
}

fn preserve_deliveries(root: &Path, config: &CoordinatorConfig) -> Result<()> {
    // Snapshot and all completed registries are archived before any overwrite.
    // Existing sync preserves this dependency evidence when importing the new PRD.
    let mut current = registry(root, config)?;
    for task in &current.tasks {
        if task.satisfies_dependants() {
            current.external_merged_task_ids.insert(task.id.clone());
        }
    }
    let mut paths = CoordinatorStoragePaths::from_project_paths(&ProjectPaths::from_root(root));
    if let Some(file) = &config.task_registry_file {
        paths.registry_json_path = root.join(file);
    }
    if config.storage_mode.as_deref() == Some("json") || !paths.sqlite_path.exists() {
        let store = JsonStorage::new(paths);
        let mut snapshot = store.load_snapshot()?;
        snapshot.registry = current;
        store.save_snapshot(&snapshot)?;
    } else {
        let store = SqliteStorage::new(paths.clone());
        let mut snapshot = store.load_snapshot()?;
        snapshot.registry = current;
        store.save_snapshot(&snapshot)?;
        // Manual Sync imports the compatibility snapshot before PRD import.
        // Keep its baseline aligned so earlier approvals/dependencies survive.
        JsonStorage::new(paths).save_snapshot(&snapshot)?;
    }
    Ok(())
}

fn archive(
    root: &Path,
    previous: &Progress,
    config: &CoordinatorConfig,
    reason: &str,
) -> Result<()> {
    let paths = ProjectPaths::from_root(root);
    let dir = root
        .join(".macc/state/prd-queue-history")
        .join(uuid::Uuid::new_v4().to_string());
    let write = |name: &str, bytes: &[u8]| crate::atomic_write(&paths, &dir.join(name), bytes);
    write(
        "progress.json",
        &serde_json::to_vec_pretty(previous).map_err(|e| invalid(e.to_string()))?,
    )?;
    write(
        "registry.json",
        &serde_json::to_vec_pretty(&registry(root, config)?).map_err(|e| invalid(e.to_string()))?,
    )?;
    for i in 0..previous.entries.len() {
        for kind in ["completed", "registry"] {
            let name = format!("prd-queue-{kind}-{i}.json");
            let source = root.join(".macc/state").join(&name);
            if source.exists() {
                write(&name, &read(&source)?)?;
            }
        }
    }
    write(
        "transition.json",
        &serde_json::to_vec_pretty(
            &serde_json::json!({"code":CODE,"reason":reason,"proposed_files":config.prd_files,"at":chrono::Utc::now().to_rfc3339()}),
        )
        .map_err(|e| invalid(e.to_string()))?,
    )
}

/// Explicit reset uses the same archive; caller owns the existing queue lock.
pub fn reset_locked(root: &Path, config: &CoordinatorConfig) -> Result<()> {
    quiescent(root, config)?;
    let previous = load(root)?;
    if !config.prd_files.is_empty() {
        super::identity::check(root, config, &previous, &validate(root, &config.prd_files)?)?;
    }
    if !previous.entries.is_empty() {
        archive(root, &previous, config, "explicit reset")?;
    }
    preserve_deliveries(root, config)?;
    // Pin the accepted file fingerprints now. A later edit requires a fresh
    // review instead of silently changing what this acceptance authorized.
    save(
        root,
        &Progress {
            entries: validate(root, &config.prd_files)?,
            next: 0,
            status: "ready".into(),
            reason: None,
        },
    )
}

pub fn reconcile(root: &Path, config: &CoordinatorConfig) -> Result<()> {
    let _lock = crate::fs_lock::AdvisoryLock::acquire(
        &root.join(".macc/state/prd-queue.lock"),
        Duration::ZERO,
        "PRD queue transition",
    )?;
    reconcile_locked(root, config)?;
    Ok(())
}

/// Called under the queue lock before a manual import. The native run has the
/// equivalent per-entry guard in prepare_registry_transition.
pub(crate) fn prepare_sync_locked(root: &Path, config: &CoordinatorConfig) -> Result<()> {
    let progress = reconcile_locked(root, config)?;
    let entries = validate(root, &config.prd_files)?;
    let Some(entry) = entries.get(progress.next) else {
        return Ok(());
    };
    let current = registry(root, config)?;
    super::transition_safety::target(&current, entry)?;
    Ok(())
}

/// Shared command fence; the returned guard covers the caller's entire import.
pub(crate) fn begin_sync(
    root: &Path,
    config: Option<&CoordinatorConfig>,
    explicit: Option<&str>,
) -> Result<Option<crate::fs_lock::AdvisoryLock>> {
    if explicit.is_some() {
        return Ok(None);
    }
    let Some(config) = config.filter(|c| !c.prd_files.is_empty()) else {
        return Ok(None);
    };
    let lock = crate::fs_lock::AdvisoryLock::acquire(
        &root.join(".macc/state/prd-queue.lock"),
        Duration::ZERO,
        "PRD queue sync",
    )?;
    prepare_sync_locked(root, config)?;
    Ok(Some(lock))
}
