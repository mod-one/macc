//! Ordered PRD inputs shared by the coordinator and its terminal clients.
use crate::{MaccError, Result};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::{
    collections::HashSet,
    path::{Path, PathBuf},
};

fn invalid(message: impl Into<String>) -> MaccError {
    MaccError::Validation(message.into())
}

pub fn read(path: &Path) -> Result<Vec<u8>> {
    std::fs::read(path).map_err(|source| MaccError::Io {
        path: path.display().to_string(),
        action: "read PRD queue input".into(),
        source,
    })
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct Entry {
    pub path: String,
    pub fingerprint: String,
    pub task_ids: Vec<String>,
}

/// Import immediate JSON children only; the saved list never expands at run time.
pub fn import(root: &Path, input: &str, current: &[String]) -> Result<Vec<String>> {
    if input.trim().is_empty() {
        return Err(invalid("Enter a PRD file or directory path."));
    }
    let path = root.join(input);
    let mut candidates = if path.is_dir() {
        let mut files = Vec::new();
        for entry in std::fs::read_dir(&path).map_err(|e| invalid(e.to_string()))? {
            let entry = entry.map_err(|e| invalid(e.to_string()))?;
            if entry
                .file_type()
                .map_err(|e| invalid(e.to_string()))?
                .is_file()
                && entry.path().extension().is_some_and(|ext| ext == "json")
            {
                files.push(entry.path());
            }
        }
        files.sort();
        files
    } else {
        vec![path]
    };
    if candidates.is_empty() {
        return Err(invalid("No JSON files found in this directory."));
    }
    let canonical_root = root.canonicalize().map_err(|e| invalid(e.to_string()))?;
    let mut result = current.to_vec();
    let mut seen: HashSet<PathBuf> = current
        .iter()
        .map(|p| root.join(p).canonicalize())
        .collect::<std::io::Result<_>>()
        .map_err(|e| invalid(e.to_string()))?;
    for path in candidates.drain(..) {
        let path = path.canonicalize().map_err(|e| invalid(e.to_string()))?;
        inspect(&path)?;
        if seen.insert(path.clone()) {
            result.push(
                path.strip_prefix(&canonical_root)
                    .unwrap_or(&path)
                    .display()
                    .to_string(),
            );
        }
    }
    Ok(result)
}

fn inspect(path: &Path) -> Result<(Entry, Vec<Vec<String>>)> {
    let bytes = read(path)?;
    let _: crate::coordinator::model::PrdInput =
        serde_json::from_slice(&bytes).map_err(|e| invalid(format!("{}: {e}", path.display())))?;
    let value: Value =
        serde_json::from_slice(&bytes).map_err(|e| invalid(format!("{}: {e}", path.display())))?;
    let tasks = value
        .get("tasks")
        .and_then(Value::as_array)
        .filter(|tasks| !tasks.is_empty())
        .ok_or_else(|| {
            invalid(format!(
                "{}: non-empty tasks array required",
                path.display()
            ))
        })?;
    let mut ids = Vec::new();
    let mut deps = Vec::new();
    for task in tasks {
        let id = task
            .get("id")
            .and_then(Value::as_str)
            .filter(|id| !id.trim().is_empty())
            .ok_or_else(|| invalid(format!("{}: task ID missing", path.display())))?;
        ids.push(id.to_owned());
        let dependencies = match task.get("dependencies") {
            None => Vec::new(),
            Some(Value::Array(items)) => items
                .iter()
                .map(|item| {
                    item.as_str()
                        .map(str::to_owned)
                        .ok_or_else(|| invalid(format!("{id}: dependency must be a task ID")))
                })
                .collect::<Result<_>>()?,
            Some(_) => return Err(invalid(format!("{id}: dependencies must be an array"))),
        };
        deps.push(dependencies);
    }
    Ok((
        Entry {
            path: path.display().to_string(),
            fingerprint: format!("{:x}", Sha256::digest(bytes)),
            task_ids: ids,
        },
        deps,
    ))
}

pub fn validate(root: &Path, files: &[String]) -> Result<Vec<Entry>> {
    let mut entries = Vec::new();
    let mut dependencies = Vec::new();
    let mut seen = HashSet::new();
    for file in files {
        let (mut entry, deps) = inspect(&root.join(file))?;
        entry.path = file.clone();
        for id in &entry.task_ids {
            if !seen.insert(id.clone()) {
                return Err(invalid(format!("Duplicate task ID in PRD queue: {id}")));
            }
        }
        entries.push(entry);
        dependencies.push(deps);
    }
    let mut available = HashSet::new();
    for (entry, deps) in entries.iter().zip(dependencies) {
        available.extend(entry.task_ids.iter().cloned());
        for dependency in deps.into_iter().flatten() {
            if seen.contains(&dependency) && !available.contains(&dependency) {
                return Err(invalid(format!(
                    "{} depends on later PRD task {dependency}; reorder the queue",
                    entry.path
                )));
            }
        }
    }
    Ok(entries)
}

#[derive(Default, Debug, Serialize, Deserialize)]
pub struct Progress {
    pub entries: Vec<Entry>,
    pub next: usize,
    pub status: String,
    pub reason: Option<String>,
}

pub fn state_path(root: &Path) -> PathBuf {
    root.join(".macc/state/prd-queue.json")
}

pub fn active_path(
    root: &Path,
    config: Option<&crate::config::CoordinatorConfig>,
    explicit: Option<&str>,
) -> Result<PathBuf> {
    if let Some(path) = explicit {
        return Ok(root.join(path));
    }
    if let Some(config) = config.filter(|c| !c.prd_files.is_empty()) {
        let progress = load(root)?;
        let entries = validate(root, &config.prd_files)?;
        if !progress.entries.is_empty() && progress.entries != entries {
            return Err(invalid(
                "PRD queue changed; review and reset progress before continuing.",
            ));
        }
        return config
            .prd_files
            .get(progress.next)
            .map(|p| root.join(p))
            .ok_or_else(|| {
                invalid(
                    "PRD queue completed. Reset progress explicitly to start another execution.",
                )
            });
    }
    Ok(root.join(
        config
            .and_then(|c| c.prd_file.as_deref())
            .unwrap_or("prd.json"),
    ))
}

pub fn load(root: &Path) -> Result<Progress> {
    let path = state_path(root);
    if !path.exists() {
        return Ok(Progress::default());
    }
    serde_json::from_slice(&read(&path)?)
        .map_err(|e| invalid(format!("Invalid queue progress: {e}")))
}

pub fn save(root: &Path, progress: &Progress) -> Result<()> {
    let path = state_path(root);
    let bytes = serde_json::to_vec_pretty(progress).map_err(|e| invalid(e.to_string()))?;
    crate::atomic_write(&crate::ProjectPaths::from_root(root), &path, &bytes)
}

fn incomplete_tasks(
    entry: &Entry,
    registry: &crate::coordinator::model::TaskRegistry,
) -> Vec<String> {
    entry
        .task_ids
        .iter()
        .filter(|id| {
            registry
                .find_task(id)
                .is_none_or(|task| !task.is_merged() || !task.gate_verdict_satisfies_dependencies())
        })
        .cloned()
        .collect()
}

pub async fn run(
    root: &Path,
    canonical: &crate::config::CanonicalConfig,
    config: &crate::config::CoordinatorConfig,
    env: &crate::coordinator::types::CoordinatorEnvConfig,
    logger: Option<&dyn crate::coordinator::control_plane::CoordinatorLog>,
) -> Result<()> {
    use crate::coordinator_storage::CoordinatorStorage;
    let entries = validate(root, &config.prd_files)?;
    let mut progress = load(root)?;
    if !progress.entries.is_empty() && progress.entries != entries {
        return Err(invalid("PRD queue or file contents changed. Inspect `macc coordinator prds status`, then explicitly reset queue progress."));
    }
    progress.entries = entries;
    if progress.next > progress.entries.len() {
        return Err(invalid("Invalid PRD queue cursor"));
    }
    let paths = crate::ProjectPaths::from_root(root);
    let storage = crate::coordinator_storage::SqliteStorage::new(
        crate::coordinator_storage::CoordinatorStoragePaths::from_project_paths(&paths),
    );
    while progress.next < progress.entries.len() {
        let entry = progress.entries[progress.next].clone();
        if inspect(&root.join(&entry.path))?.0.fingerprint != entry.fingerprint {
            let reason = format!("PRD changed during execution: {}", entry.path);
            progress.status = "blocked".into();
            progress.reason = Some(reason.clone());
            save(root, &progress)?;
            return Err(invalid(reason));
        }
        // Preserve completed registry tasks for diagnostics before the next PRD sync replaces them.
        if let Ok(mut snapshot) = storage.load_snapshot() {
            let unsafe_tasks: Vec<_> = snapshot
                .registry
                .tasks
                .iter()
                .filter(|task| {
                    !entry.task_ids.contains(&task.id)
                        && !task.is_merged()
                        && (task.state != "todo" || task.worktree.is_some())
                })
                .map(|task| task.id.clone())
                .collect();
            if !unsafe_tasks.is_empty() {
                progress.status = "blocked".into();
                progress.reason = Some(format!("Cannot replace registry with {}: unfinished work outside this PRD: {}. Resolve it before switching inputs.", entry.path, unsafe_tasks.join(", ")));
                save(root, &progress)?;
                return Err(invalid(progress.reason.clone().unwrap()));
            }
            let archive = root.join(format!(
                ".macc/state/prd-queue-registry-{}.json",
                progress.next
            ));
            crate::atomic_write(
                &paths,
                &archive,
                &serde_json::to_vec_pretty(&snapshot.registry)
                    .map_err(|e| invalid(e.to_string()))?,
            )?;
            for task in &snapshot.registry.tasks {
                if task.is_merged() && task.gate_verdict_satisfies_dependencies() {
                    snapshot
                        .registry
                        .external_merged_task_ids
                        .insert(task.id.clone());
                }
            }
            storage.save_snapshot(&snapshot)?;
        }
        progress.status = "running".into();
        progress.reason = None;
        save(root, &progress)?;
        let message = format!(
            "PRD {}/{}: {}",
            progress.next + 1,
            progress.entries.len(),
            entry.path
        );
        if let Some(log) = logger {
            log.note(message.clone())?;
        }
        crate::coordinator::helpers::append_coordinator_event_with_severity(
            root,
            "prd_queue_started",
            "-",
            "run",
            "started",
            &message,
            "info",
        )?;
        let mut current_env = env.clone();
        current_env.prd = Some(root.join(&entry.path).display().to_string());
        let run_id = format!(
            "queue-{}-{}",
            chrono::Utc::now().timestamp_nanos_opt().unwrap_or_default(),
            progress.next
        );
        std::env::set_var("COORDINATOR_RUN_ID", &run_id);
        let result = crate::coordinator::engine::fsm::run_single_prd_control_plane(
            root,
            canonical,
            Some(config),
            &current_env,
            logger,
        )
        .await;
        let outcome = result.and_then(|_| {
            if inspect(&root.join(&entry.path))?.0.fingerprint != entry.fingerprint {
                return Err(invalid(format!("PRD changed during execution: {}; review and reset queue progress before continuing", entry.path)));
            }
            if storage
                .get_coordinator_run(&run_id)?
                .is_none_or(|run| run.status != "success")
            {
                return Err(invalid(
                    "PRD run did not finish successfully; the remaining queue is preserved.",
                ));
            }
            if storage
                .get_coordinator_control()?
                .is_some_and(|control| control.mode != "running")
            {
                return Err(invalid(
                    "PRD queue interrupted by coordinator stop; next PRD was not started.",
                ));
            }
            let snapshot = storage.load_snapshot()?;
            let incomplete = incomplete_tasks(&entry, &snapshot.registry);
            if incomplete.is_empty() {
                Ok(())
            } else {
                let reasons: Vec<_> = incomplete.iter().map(|id| {
                    match snapshot.registry.find_task(id) {
                        Some(task) if task.is_merged() => format!(
                            "{id}: merged but gate requires {:?}, recorded {:?}",
                            task.gate.as_ref().map(|gate| gate.required_verdict),
                            task.task_runtime.gate_verdict,
                        ),
                        Some(task) => format!("{id}: {}{}", task.state,
                            task.task_runtime.last_error_message.as_ref().map(|reason| format!(" - {reason}")).unwrap_or_default()),
                        None => format!("{id}: missing from registry"),
                    }
                }).collect();
                Err(invalid(format!(
                    "PRD {} is not fully delivered; queue stopped. Remaining: {}",
                    entry.path,
                    reasons.join("; ")
                )))
            }
        });
        if let Err(error) = outcome {
            progress.status = "blocked".into();
            if let Some(mut run) = storage.get_coordinator_run(&run_id)? {
                if matches!(run.status.as_str(), "paused" | "stopped_by_user" | "failed") {
                    progress.status = run.status.clone();
                } else if run.status == "success" {
                    // A successful tool run is not necessarily a delivered PRD.
                    // Keep the durable result consumed by clients consistent with the queue.
                    run.status = "blocked".into();
                    run.stop_reason = Some(error.to_string());
                    storage.upsert_coordinator_run(&run)?;
                }
            }
            progress.reason = Some(error.to_string());
            save(root, &progress)?;
            crate::coordinator::helpers::append_coordinator_event_with_severity(
                root,
                "prd_queue_stopped",
                "-",
                "run",
                &progress.status,
                &error.to_string(),
                "blocking",
            )?;
            return Err(error);
        }
        let completed = storage.load_snapshot()?;
        crate::atomic_write(
            &paths,
            &root.join(format!(
                ".macc/state/prd-queue-completed-{}.json",
                progress.next
            )),
            &serde_json::to_vec_pretty(&completed.registry).map_err(|e| invalid(e.to_string()))?,
        )?;
        progress.next += 1;
        progress.status = if progress.next == progress.entries.len() {
            "completed"
        } else {
            "ready"
        }
        .into();
        save(root, &progress)?;
        crate::coordinator::helpers::append_coordinator_event_with_severity(
            root,
            "prd_queue_completed_entry",
            "-",
            "run",
            "success",
            &message,
            "info",
        )?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn cursor_survives_reload_and_explicit_prd_bypasses_queue() {
        let root = tempfile::tempdir().unwrap();
        std::fs::write(root.path().join("a.json"), r#"{"tasks":[{"id":"A"}]}"#).unwrap();
        std::fs::write(root.path().join("b.json"), r#"{"tasks":[{"id":"B"}]}"#).unwrap();
        let config = crate::config::CoordinatorConfig {
            prd_files: vec!["a.json".into(), "b.json".into()],
            ..Default::default()
        };
        let mut progress = Progress {
            entries: validate(root.path(), &config.prd_files).unwrap(),
            next: 1,
            status: "blocked".into(),
            reason: Some("gate pending".into()),
        };
        save(root.path(), &progress).unwrap();
        assert_eq!(
            load(root.path()).unwrap().reason.as_deref(),
            Some("gate pending")
        );
        assert_eq!(
            active_path(root.path(), Some(&config), None).unwrap(),
            root.path().join("b.json")
        );
        assert_eq!(
            active_path(root.path(), Some(&config), Some("override.json")).unwrap(),
            root.path().join("override.json")
        );
        progress.next = 2;
        save(root.path(), &progress).unwrap();
        assert!(active_path(root.path(), Some(&config), None)
            .unwrap_err()
            .to_string()
            .contains("completed"));
        progress.next = 1;
        save(root.path(), &progress).unwrap();
        std::fs::write(
            root.path().join("b.json"),
            r#"{"tasks":[{"id":"B","title":"changed"}]}"#,
        )
        .unwrap();
        assert!(active_path(root.path(), Some(&config), None)
            .unwrap_err()
            .to_string()
            .contains("changed"));
        save(root.path(), &Progress::default()).unwrap();
        assert_eq!(
            active_path(root.path(), Some(&config), None).unwrap(),
            root.path().join("a.json")
        );
    }

    #[test]
    fn refuses_missing_unmerged_and_negative_gate_deliveries() {
        use crate::coordinator::model::{GateVerdict, TaskRegistry};
        let entry = Entry {
            path: "gate.json".into(),
            fingerprint: String::new(),
            task_ids: vec!["G".into()],
        };
        let mut registry: TaskRegistry = serde_json::from_str(r#"{"tasks":[{"id":"G","state":"merged","gate":{},"task_runtime":{"gate_verdict":"rejected"}}]}"#).unwrap();
        assert_eq!(incomplete_tasks(&entry, &registry), vec!["G"]);
        registry.tasks[0].task_runtime.gate_verdict = Some(GateVerdict::Accepted);
        assert!(incomplete_tasks(&entry, &registry).is_empty());
        registry.tasks[0].state = "blocked".into();
        assert_eq!(incomplete_tasks(&entry, &registry), vec!["G"]);
        registry.tasks.clear();
        assert_eq!(incomplete_tasks(&entry, &registry), vec!["G"]);
    }

    #[test]
    fn directory_import_rejects_invalid_inputs_atomically() {
        let root = tempfile::tempdir().unwrap();
        std::fs::write(root.path().join("a.json"), r#"{"tasks":[{"id":"A"}]}"#).unwrap();
        std::fs::write(root.path().join("b.json"), "{}").unwrap();
        let files = vec!["a.json".to_string()];
        assert!(import(root.path(), ".", &files).is_err());
        assert_eq!(files, vec!["a.json"]);
    }

    #[test]
    fn configured_queue_roundtrips_without_changing_legacy_input() {
        let config: crate::config::CoordinatorConfig =
            serde_json::from_str(r#"{"prd_file":"legacy.json","prd_files":["b.json","a.json"]}"#)
                .unwrap();
        let copy: crate::config::CoordinatorConfig =
            serde_json::from_value(serde_json::to_value(config).unwrap()).unwrap();
        assert_eq!(copy.prd_files, vec!["b.json", "a.json"]);
        assert_eq!(copy.prd_file.as_deref(), Some("legacy.json"));
        assert!(crate::config::CoordinatorConfig::default()
            .prd_files
            .is_empty());
    }

    #[test]
    fn syncing_next_prd_preserves_completed_dependency_evidence() {
        use crate::coordinator::model::TaskRegistry;
        use std::collections::BTreeMap;
        let root = tempfile::tempdir().unwrap();
        std::fs::write(root.path().join("a.json"), r#"{"tasks":[{"id":"A"}]}"#).unwrap();
        std::fs::write(
            root.path().join("b.json"),
            r#"{"tasks":[{"id":"B","dependencies":["A"]}]}"#,
        )
        .unwrap();
        let progress = Progress {
            entries: validate(root.path(), &["a.json".into(), "b.json".into()]).unwrap(),
            next: 1,
            status: "running".into(),
            reason: None,
        };
        save(root.path(), &progress).unwrap();
        crate::coordinator::state::coordinator_state_registry_save(
            root.path(),
            &BTreeMap::new(),
            &serde_json::json!({"tasks":[]}),
        )
        .unwrap();
        crate::coordinator::control_plane::sync_registry_from_prd_native(
            root.path(),
            &root.path().join("b.json"),
            None,
        )
        .unwrap();
        let value = crate::coordinator::state::coordinator_state_registry_load(
            root.path(),
            &BTreeMap::new(),
        )
        .unwrap();
        let registry = TaskRegistry::from_value(&value).unwrap();
        assert!(registry.external_merged_task_ids.contains("A"));
        assert_eq!(registry.tasks.len(), 1);
        assert_eq!(registry.tasks[0].id, "B");
        assert_eq!(registry.tasks[0].state, "todo");
    }

    #[test]
    fn validates_order_and_rejects_duplicate_ids() {
        let root = tempfile::tempdir().unwrap();
        std::fs::write(root.path().join("a.json"), r#"{"tasks":[{"id":"A"}]}"#).unwrap();
        std::fs::write(
            root.path().join("b.json"),
            r#"{"tasks":[{"id":"B","dependencies":["A"]}]}"#,
        )
        .unwrap();
        assert!(validate(root.path(), &["a.json".into(), "b.json".into()]).is_ok());
        assert!(validate(root.path(), &["b.json".into(), "a.json".into()]).is_err());
        assert!(validate(root.path(), &["a.json".into(), "a.json".into()]).is_err());
        assert_eq!(
            import(root.path(), ".", &["a.json".into()]).unwrap(),
            vec!["a.json", "b.json"]
        );
    }
}
