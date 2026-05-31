use super::snapshot::*;
use crate::coordinator_storage::{
    CoordinatorStorage, CoordinatorStoragePaths, JsonStorage, SqliteStorage,
};
use crate::{ProjectPaths, Result};
use chrono::Utc;

pub struct RuntimeSnapshotBuilder;

impl RuntimeSnapshotBuilder {
    pub fn build(paths: &ProjectPaths) -> Result<RuntimeSnapshot> {
        let storage_paths = CoordinatorStoragePaths::from_project_paths(paths);
        let sqlite = SqliteStorage::new(storage_paths.clone());
        let snapshot = if sqlite.has_snapshot_data().unwrap_or(false) {
            sqlite.load_snapshot()
        } else {
            JsonStorage::new(storage_paths).load_snapshot()
        };

        let (queue, workers, tasks, throttled_tools, coordinator) = match snapshot {
            Ok(s) => build_from_snapshot(&s),
            Err(_) => (
                QueueSummary::default(),
                Vec::new(),
                Vec::new(),
                Vec::new(),
                CoordinatorStatus::default(),
            ),
        };

        let recent_events = load_recent_events(paths);
        let git = build_git_summary(paths);
        let skill_runs = load_skill_runs(paths);

        let project_name = paths
            .root
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_else(|| "project".to_string());

        Ok(RuntimeSnapshot {
            generated_at: Utc::now().to_rfc3339(),
            project: ProjectSummary {
                name: project_name,
                root: paths.root.clone(),
                config_version: None,
            },
            coordinator,
            queue,
            workers,
            tasks,
            active_runs: skill_runs,
            throttled_tools,
            recent_events,
            git,
            diagnostics: RuntimeDiagnostics::default(),
        })
    }
}

fn build_from_snapshot(
    snapshot: &crate::coordinator_storage::CoordinatorSnapshot,
) -> (
    QueueSummary,
    Vec<WorkerRuntime>,
    Vec<TaskRuntimeSummary>,
    Vec<ToolThrottleStatus>,
    CoordinatorStatus,
) {
    let mut queue = QueueSummary::default();
    let mut workers = Vec::new();
    let mut tasks = Vec::new();
    let mut throttle_map: std::collections::BTreeMap<String, ToolThrottleStatus> =
        std::collections::BTreeMap::new();

    let now_iso = Utc::now().to_rfc3339();

    for task in &snapshot.registry.tasks {
        let state = task.state.to_ascii_lowercase();
        queue.total += 1;
        match state.as_str() {
            "todo" => queue.todo += 1,
            "claimed" => queue.claimed += 1,
            "in_progress" => {
                queue.in_progress += 1;
                queue.ready += 1;
            }
            "testing" => queue.testing += 1,
            "reviewing" => queue.reviewing += 1,
            "changes_requested" => queue.changes_requested += 1,
            "blocked" => queue.blocked += 1,
            "merged" => queue.merged += 1,
            "abandoned" => queue.failed += 1,
            _ => {}
        }

        let runtime = &task.task_runtime;
        let runtime_status = runtime
            .status
            .as_deref()
            .unwrap_or("idle")
            .to_string();

        let is_active = matches!(
            runtime_status.as_str(),
            "dispatched" | "running" | "phase_done" | "stale"
        );

        let task_summary = TaskRuntimeSummary {
            task_id: task.id.clone(),
            title: task.title.clone().unwrap_or_default(),
            workflow_state: state.clone(),
            runtime_status: runtime_status.clone(),
            tool: runtime.tool.clone(),
            phase: runtime.current_phase.clone(),
            worker_id: runtime.worker_id.clone(),
            worktree: runtime.worktree.clone(),
            branch: runtime.branch.clone(),
            message: runtime.message.clone(),
            last_heartbeat: runtime.last_heartbeat.clone(),
            started_at: runtime.started_at.clone(),
            last_error: runtime.last_error.clone(),
            last_error_code: runtime.last_error_code.clone(),
        };
        tasks.push(task_summary);

        if is_active {
            if let Some(wt_path) = runtime.worktree.as_deref() {
                let worker = WorkerRuntime {
                    id: runtime
                        .worker_id
                        .clone()
                        .unwrap_or_else(|| task.id.clone()),
                    worktree_path: std::path::PathBuf::from(wt_path),
                    tool: runtime.tool.clone().unwrap_or_default(),
                    task_id: Some(task.id.clone()),
                    branch: runtime.branch.clone(),
                    base_branch: None,
                    phase: runtime.current_phase.clone(),
                    runtime_status: runtime_status.clone(),
                    last_heartbeat: runtime.last_heartbeat.clone(),
                    git_status: GitStatusSummary::default(),
                    retry_count: runtime.attempt.unwrap_or(0) as u32,
                    delayed_until: runtime.delayed_until.clone(),
                };
                workers.push(worker);
            }
        }

        if let (Some(delayed_until), Some(tool_id)) =
            (runtime.delayed_until.as_deref(), task.tool.as_deref())
        {
            if !tool_id.is_empty() && delayed_until > now_iso.as_str() {
                let backoff = runtime
                    .extra
                    .get("throttle_state")
                    .and_then(|v| v.get("backoff_seconds"))
                    .and_then(|x| x.as_u64())
                    .unwrap_or(0);
                let entry = throttle_map
                    .entry(tool_id.to_string())
                    .or_insert_with(|| ToolThrottleStatus {
                        tool: tool_id.to_string(),
                        reason: "rate_limited".to_string(),
                        error_code: "E601".to_string(),
                        retryable: true,
                        delayed_until: Some(delayed_until.to_string()),
                        backoff_seconds: backoff,
                        effective_parallelism_delta: -1,
                    });
                if delayed_until > entry.delayed_until.as_deref().unwrap_or("") {
                    entry.delayed_until = Some(delayed_until.to_string());
                }
            }
        }
    }

    let throttled_tools = throttle_map.into_values().collect();
    let coordinator = CoordinatorStatus::default();

    (queue, workers, tasks, throttled_tools, coordinator)
}

fn load_recent_events(paths: &ProjectPaths) -> Vec<RuntimeEvent> {
    let events_path = paths
        .macc_dir
        .join("log")
        .join("coordinator")
        .join("events.jsonl");

    if !events_path.exists() {
        return Vec::new();
    }

    let Ok(content) = std::fs::read_to_string(&events_path) else {
        return Vec::new();
    };

    let mut events: Vec<RuntimeEvent> = content
        .lines()
        .rev()
        .take(50)
        .filter_map(|line| {
            let v: serde_json::Value = serde_json::from_str(line).ok()?;
            Some(RuntimeEvent {
                ts: v.get("ts").and_then(|x| x.as_str()).map(|s| s.to_string()),
                event_type: v
                    .get("event_type")
                    .or_else(|| v.get("type"))
                    .and_then(|x| x.as_str())
                    .unwrap_or("unknown")
                    .to_string(),
                task_id: v
                    .get("task_id")
                    .and_then(|x| x.as_str())
                    .map(|s| s.to_string()),
                phase: v
                    .get("phase")
                    .and_then(|x| x.as_str())
                    .map(|s| s.to_string()),
                status: v
                    .get("status")
                    .and_then(|x| x.as_str())
                    .map(|s| s.to_string()),
                message: v
                    .get("message")
                    .and_then(|x| x.as_str())
                    .map(|s| s.to_string()),
            })
        })
        .collect();
    events.reverse();
    events
}

fn load_skill_runs(paths: &ProjectPaths) -> Vec<SkillRunSummary> {
    let run_dir = paths.macc_dir.join("log").join("run");
    if !run_dir.exists() {
        return Vec::new();
    }

    let Ok(entries) = std::fs::read_dir(&run_dir) else {
        return Vec::new();
    };

    let mut runs: Vec<SkillRunSummary> = entries
        .flatten()
        .filter(|e| {
            e.path()
                .extension()
                .map(|ext| ext == "jsonl")
                .unwrap_or(false)
        })
        .filter_map(|e| {
            let name = e.file_name().to_string_lossy().to_string();
            let stem = name.trim_end_matches(".jsonl");
            let parts: Vec<&str> = stem.splitn(2, '-').collect();
            if parts.len() < 2 {
                return None;
            }
            let skill_id = parts[1].to_string();
            let content = std::fs::read_to_string(e.path()).ok()?;
            let last_line = content.lines().last()?;
            let v: serde_json::Value = serde_json::from_str(last_line).ok()?;
            let status = v
                .get("status")
                .and_then(|x| x.as_str())
                .unwrap_or("unknown")
                .to_string();
            let started_at = parts[0].to_string();
            Some(SkillRunSummary {
                id: stem.to_string(),
                skill_id,
                tool: v
                    .get("tool")
                    .and_then(|x| x.as_str())
                    .map(|s| s.to_string()),
                status,
                started_at,
                duration_ms: v.get("duration_ms").and_then(|x| x.as_u64()),
            })
        })
        .collect();

    runs.sort_by(|a, b| b.started_at.cmp(&a.started_at));
    runs.truncate(10);
    runs
}

fn build_git_summary(paths: &ProjectPaths) -> GitRuntimeSummary {
    let branch = crate::git::run_git_output_mapped(
        &paths.root,
        &["branch", "--show-current"],
        "get current branch",
    )
    .ok()
    .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
    .filter(|s| !s.is_empty());

    let worktrees_count = crate::list_worktrees(&paths.root)
        .map(|wts| wts.len())
        .unwrap_or(0);

    let clean = crate::git::run_git_output_mapped(
        &paths.root,
        &["status", "--porcelain"],
        "get git status",
    )
    .ok()
    .map(|o| o.stdout.is_empty())
    .unwrap_or(true);

    GitRuntimeSummary {
        repo_root: Some(paths.root.clone()),
        current_branch: branch,
        clean,
        worktrees_count,
    }
}
