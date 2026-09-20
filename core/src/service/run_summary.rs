use crate::coordinator::model::{Task, TaskRegistry};
use crate::coordinator::WorkflowState;
use crate::coordinator_storage::{CoordinatorRun, SqliteStorage};
use crate::Result;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};

const RECENT_RUN_LIMIT: usize = 5;

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct CoordinatorRunHistoryItem {
    pub run_id: String,
    pub status: String,
    pub started_at: String,
    pub stopped_at: Option<String>,
    pub stop_reason: Option<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct CoordinatorRunSummary {
    pub run_id: String,
    pub status: String,
    pub severity: String,
    pub headline: String,
    pub cause: String,
    pub task_id: Option<String>,
    pub error_code: Option<String>,
    pub occurred_at: String,
    pub next_action: String,
    pub dependent_task_ids: Vec<String>,
    pub repeated_count: usize,
    pub recent_runs: Vec<CoordinatorRunHistoryItem>,
}

pub fn load_latest_run_summary(
    sqlite: &SqliteStorage,
    registry: &TaskRegistry,
) -> Result<Option<CoordinatorRunSummary>> {
    let runs = sqlite.get_recent_coordinator_runs(RECENT_RUN_LIMIT)?;
    let Some(latest) = runs.first() else {
        return Ok(None);
    };
    let blocked_task = blocked_root_for_run(registry, latest);
    let dependent_task_ids = blocked_task
        .map(|task| dependent_tasks(registry, &task.id))
        .unwrap_or_default();
    let cause = blocked_task
        .and_then(task_error_message)
        .or_else(|| latest.stop_reason.clone())
        .unwrap_or_else(|| default_cause(&latest.status).to_string());
    let error_code = blocked_task.and_then(|task| task.task_runtime.last_error_code.clone());
    let task_id = blocked_task.map(|task| task.id.clone());
    let severity = severity_for(latest, blocked_task.is_some(), registry);
    let headline = headline_for(latest, blocked_task, dependent_task_ids.len());
    let next_action = next_action_for(latest, blocked_task, error_code.as_deref());
    let signature = outcome_signature(latest);
    let repeated_count = runs
        .iter()
        .filter(|run| outcome_signature(run) == signature)
        .count()
        .max(1);
    let occurred_at = latest
        .stopped_at
        .clone()
        .unwrap_or_else(|| latest.started_at.clone());

    Ok(Some(CoordinatorRunSummary {
        run_id: latest.run_id.clone(),
        status: latest.status.clone(),
        severity: severity.to_string(),
        headline,
        cause,
        task_id,
        error_code,
        occurred_at,
        next_action,
        dependent_task_ids,
        repeated_count,
        recent_runs: runs
            .into_iter()
            .map(CoordinatorRunHistoryItem::from)
            .collect(),
    }))
}

fn blocked_root_for_run<'a>(registry: &'a TaskRegistry, run: &CoordinatorRun) -> Option<&'a Task> {
    let blocked = |task: &&Task| task.workflow_state() == Some(WorkflowState::Blocked);
    let current_run_root = registry
        .tasks
        .iter()
        .filter(blocked)
        .find(|task| task.task_runtime.run_id.as_deref() == Some(run.run_id.as_str()));
    if current_run_root.is_some() {
        return current_run_root;
    }
    matches!(run.status.as_str(), "blocked" | "failed" | "crashed")
        .then(|| registry.tasks.iter().find(blocked))
        .flatten()
}

fn task_error_message(task: &Task) -> Option<String> {
    task.task_runtime
        .last_error_message
        .clone()
        .or_else(|| task.task_runtime.last_error.clone())
        .filter(|message| !message.trim().is_empty())
}

fn dependent_tasks(registry: &TaskRegistry, root_id: &str) -> Vec<String> {
    let tasks: HashMap<&str, &Task> = registry
        .tasks
        .iter()
        .map(|task| (task.id.as_str(), task))
        .collect();
    let mut dependents = registry
        .tasks
        .iter()
        .filter(|task| !task.is_merged() && task.id != root_id)
        .filter(|task| reaches_dependency(task, root_id, &tasks, &mut HashSet::new()))
        .map(|task| task.id.clone())
        .collect::<Vec<_>>();
    dependents.sort();
    dependents
}

fn reaches_dependency(
    task: &Task,
    target: &str,
    tasks: &HashMap<&str, &Task>,
    visiting: &mut HashSet<String>,
) -> bool {
    if !visiting.insert(task.id.clone()) {
        return false;
    }
    for dependency in task.dependency_ids() {
        if dependency == target {
            return true;
        }
        if let Some(dependency_task) = tasks.get(dependency.as_str()) {
            if reaches_dependency(dependency_task, target, tasks, visiting) {
                return true;
            }
        }
    }
    visiting.remove(&task.id);
    false
}

fn severity_for(
    run: &CoordinatorRun,
    has_blocked_root: bool,
    registry: &TaskRegistry,
) -> &'static str {
    match run.status.as_str() {
        "running" | "draining" => "info",
        "blocked" | "failed" | "crashed" => "error",
        "paused" | "stopped_by_user" | "force_stopping" => "warning",
        "success"
            if has_blocked_root
                || registry
                    .tasks
                    .iter()
                    .any(|task| task.workflow_state() == Some(WorkflowState::Blocked)) =>
        {
            "warning"
        }
        "success" => "success",
        _ if has_blocked_root => "error",
        _ => "warning",
    }
}

fn headline_for(run: &CoordinatorRun, blocked: Option<&Task>, dependent_count: usize) -> String {
    if let Some(task) = blocked {
        let code = task
            .task_runtime
            .last_error_code
            .as_deref()
            .unwrap_or("unknown error");
        return format!(
            "Cannot continue: {} is blocked ({}) and {} remaining task(s) depend on it.",
            task.id, code, dependent_count
        );
    }
    match run.status.as_str() {
        "running" | "draining" => "Coordinator run is active.".to_string(),
        "success" => "Coordinator completed successfully.".to_string(),
        "paused" => "Coordinator paused and is waiting for user action.".to_string(),
        "stopped_by_user" | "force_stopping" => "Coordinator was stopped by the user.".to_string(),
        "crashed" => "Coordinator process crashed unexpectedly.".to_string(),
        "failed" => "Coordinator stopped after an execution failure.".to_string(),
        "blocked" => "Coordinator cannot continue because work is blocked.".to_string(),
        status => format!("Coordinator stopped with status '{}'.", status),
    }
}

fn next_action_for(run: &CoordinatorRun, blocked: Option<&Task>, code: Option<&str>) -> String {
    if let Some(task) = blocked {
        if matches!(code, Some("E903" | "E904" | "E906" | "E907")) {
            return format!(
                "Resolve the recorded condition, then run `macc coordinator unblock-task --task {} --evidence \"<evidence>\"`.",
                task.id
            );
        }
        if code == Some("E905") {
            return "Resolve the blocked root task named in the dependency chain; dependants will unblock automatically.".to_string();
        }
        return format!(
            "Resolve {} ({}) or explicitly abandon it, then start a new coordinator run.",
            task.id,
            code.unwrap_or("unknown error")
        );
    }
    match run.status.as_str() {
        "running" | "draining" => "Monitor the active tasks and logs.".to_string(),
        "success" => "No action required.".to_string(),
        "paused" => "Resolve the recorded pause cause, then resume the coordinator.".to_string(),
        "stopped_by_user" | "force_stopping" => {
            "Review remaining tasks, then start the coordinator when ready.".to_string()
        }
        "crashed" => {
            "Inspect the run cause and logs before restarting the coordinator.".to_string()
        }
        _ => "Fix the recorded cause before starting another coordinator run.".to_string(),
    }
}

fn default_cause(status: &str) -> &'static str {
    match status {
        "success" => "All coordinator work completed.",
        "paused" => "The coordinator is paused and waiting for user action.",
        "stopped_by_user" | "force_stopping" => "The run was stopped by an operator.",
        "running" | "draining" => "The run is still active.",
        _ => "No detailed cause was recorded for this run.",
    }
}

fn outcome_signature(run: &CoordinatorRun) -> String {
    let status = match run.status.as_str() {
        "blocked" | "failed" | "crashed" => "unsuccessful",
        other => other,
    };
    let reason = run
        .stop_reason
        .as_deref()
        .unwrap_or_default()
        .trim()
        .replace("Err(Validation(\"", "")
        .replace("Validation error: ", "")
        .replace("\\n", "\n")
        .trim_end_matches("\"))")
        .trim()
        .to_string();
    format!("{status}:{reason}")
}

impl From<CoordinatorRun> for CoordinatorRunHistoryItem {
    fn from(run: CoordinatorRun) -> Self {
        Self {
            run_id: run.run_id,
            status: run.status,
            started_at: run.started_at,
            stopped_at: run.stopped_at,
            stop_reason: run.stop_reason,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::load_latest_run_summary;
    use crate::coordinator::model::TaskRegistry;
    use crate::coordinator_storage::{CoordinatorRun, CoordinatorStoragePaths, SqliteStorage};
    use crate::ProjectPaths;
    use serde_json::json;

    fn run(run_id: &str, started_at: &str, status: &str, reason: &str) -> CoordinatorRun {
        CoordinatorRun {
            run_id: run_id.to_string(),
            pid: 1,
            hostname: "test".to_string(),
            started_at: started_at.to_string(),
            last_tick_at: None,
            stopped_at: Some(started_at.to_string()),
            status: status.to_string(),
            epoch: 1,
            version: "test".to_string(),
            stop_reason: Some(reason.to_string()),
        }
    }

    #[test]
    fn blocked_summary_leads_with_root_cause_dependents_and_repetition() {
        let root = tempfile::tempdir().expect("temp project");
        let paths = ProjectPaths::from_root(root.path());
        let sqlite = SqliteStorage::new(CoordinatorStoragePaths::from_project_paths(&paths));
        let reason = "Coordinator made no progress for 5 cycles";
        sqlite
            .upsert_coordinator_run(&run("run-1", "2026-08-29T16:28:00Z", "crashed", reason))
            .expect("first run");
        sqlite
            .upsert_coordinator_run(&run("run-2", "2026-08-29T18:00:00Z", "crashed", reason))
            .expect("second run");
        let registry = TaskRegistry::from_value(&json!({
            "tasks": [
                {
                    "id": "L4K-ROLLOUT-001",
                    "state": "blocked",
                    "task_runtime": {
                        "run_id": "run-2",
                        "last_error_code": "E902",
                        "last_error_message": "V-001 accessibility parity is unresolved"
                    }
                },
                {"id":"L4K-ROLLOUT-ACCEPTANCE-001","state":"todo","dependencies":["L4K-ROLLOUT-001"]},
                {"id":"L4K-REACTFLOW-CLEANUP-001","state":"todo","dependencies":["L4K-ROLLOUT-ACCEPTANCE-001"]},
                {"id":"L4K-VERIFY-002","state":"todo","dependencies":["L4K-REACTFLOW-CLEANUP-001"]}
            ]
        }))
        .expect("registry");

        let summary = load_latest_run_summary(&sqlite, &registry)
            .expect("summary")
            .expect("latest run");

        assert_eq!(summary.severity, "error");
        assert_eq!(summary.task_id.as_deref(), Some("L4K-ROLLOUT-001"));
        assert_eq!(summary.error_code.as_deref(), Some("E902"));
        assert!(summary.cause.contains("V-001"));
        assert_eq!(summary.dependent_task_ids.len(), 3);
        assert_eq!(summary.repeated_count, 2);
        assert!(!summary.next_action.contains("unlock"));
        assert!(summary.next_action.contains("L4K-ROLLOUT-001"));
    }

    #[test]
    fn successful_run_is_a_positive_result_with_no_recovery_action() {
        let root = tempfile::tempdir().expect("temp project");
        let paths = ProjectPaths::from_root(root.path());
        let sqlite = SqliteStorage::new(CoordinatorStoragePaths::from_project_paths(&paths));
        sqlite
            .upsert_coordinator_run(&run(
                "run-ok",
                "2026-08-30T09:00:00Z",
                "success",
                "all tasks completed",
            ))
            .expect("successful run");

        let summary = load_latest_run_summary(&sqlite, &TaskRegistry::default())
            .expect("summary")
            .expect("latest run");
        assert_eq!(summary.severity, "success");
        assert_eq!(summary.next_action, "No action required.");
    }

    #[test]
    fn successful_run_is_not_hijacked_by_an_old_blocked_task() {
        let root = tempfile::tempdir().expect("temp project");
        let paths = ProjectPaths::from_root(root.path());
        let sqlite = SqliteStorage::new(CoordinatorStoragePaths::from_project_paths(&paths));
        sqlite
            .upsert_coordinator_run(&run(
                "run-ok",
                "2026-08-30T09:00:00Z",
                "success",
                "all tasks completed",
            ))
            .expect("successful run");
        let registry = TaskRegistry::from_value(&json!({
            "tasks": [{
                "id": "OLD-BLOCKER",
                "state": "blocked",
                "task_runtime": {
                    "run_id": "old-run",
                    "last_error_code": "E902",
                    "last_error_message": "old failure"
                }
            }]
        }))
        .expect("registry");

        let summary = load_latest_run_summary(&sqlite, &registry)
            .expect("summary")
            .expect("latest run");
        assert_eq!(summary.headline, "Coordinator completed successfully.");
        assert_eq!(summary.task_id, None);
    }

    #[test]
    fn paused_run_is_not_reported_as_successful() {
        let root = tempfile::tempdir().expect("temp project");
        let paths = ProjectPaths::from_root(root.path());
        let sqlite = SqliteStorage::new(CoordinatorStoragePaths::from_project_paths(&paths));
        sqlite
            .upsert_coordinator_run(&run(
                "run-paused",
                "2026-08-30T09:00:00Z",
                "paused",
                "review requires operator approval",
            ))
            .expect("paused run");

        let summary = load_latest_run_summary(&sqlite, &TaskRegistry::default())
            .expect("summary")
            .expect("latest run");
        assert_eq!(summary.severity, "warning");
        assert!(summary.headline.contains("paused"));
        assert!(summary.next_action.contains("resume"));
    }
}
