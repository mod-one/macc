use crate::coordinator::engine::ReviewVerdict;
use crate::coordinator::model::Task;
use crate::coordinator::{CoordinatorEventRecord, PerformerCompletionKind};
use crate::coordinator_storage::CoordinatorStorage;
use crate::git;
use crate::{MaccError, Result};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::io::Read;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone)]
pub struct CoordinatorJob {
    pub tool: String,
    pub worktree_path: PathBuf,
    pub attempt: usize,
    pub started_at: std::time::Instant,
    pub pid: Option<i64>,
}

#[derive(Debug, Clone)]
pub struct CoordinatorMergeJob {
    pub started_at: std::time::Instant,
}

#[derive(Debug, Clone)]
pub struct CoordinatorJobEvent {
    pub task_id: String,
    pub success: bool,
    pub status_text: String,
    pub timed_out: bool,
    pub completion_kind: Option<PerformerCompletionKind>,
    pub completion_details_source: Option<String>,
    pub error_code: Option<String>,
    pub error_origin: Option<String>,
    pub error_message: Option<String>,
}

#[derive(Debug, Clone)]
pub struct CoordinatorMergeEvent {
    pub task_id: String,
    pub success: bool,
    pub reason: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CoordinatorRuntimeEventKind {
    Heartbeat,
    Progress {
        status: String,
        phase: Option<String>,
        message: Option<String>,
    },
    PhaseResult {
        status: String,
        phase: Option<String>,
        message: Option<String>,
    },
    Failed {
        phase: Option<String>,
        message: Option<String>,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CoordinatorRuntimeEvent {
    pub task_id: String,
    pub ts: String,
    pub source: String,
    pub kind: CoordinatorRuntimeEventKind,
}

pub struct CoordinatorRunState {
    pub active_jobs: HashMap<String, CoordinatorJob>,
    pub join_set: tokio::task::JoinSet<()>,
    pub event_tx: tokio::sync::mpsc::UnboundedSender<CoordinatorJobEvent>,
    pub event_rx: tokio::sync::mpsc::UnboundedReceiver<CoordinatorJobEvent>,
    pub active_merge_jobs: HashMap<String, CoordinatorMergeJob>,
    pub merge_join_set: tokio::task::JoinSet<()>,
    pub merge_event_tx: tokio::sync::mpsc::UnboundedSender<CoordinatorMergeEvent>,
    pub merge_event_rx: tokio::sync::mpsc::UnboundedReceiver<CoordinatorMergeEvent>,
    pub runtime_event_bus_tx: tokio::sync::broadcast::Sender<CoordinatorRuntimeEvent>,
    pub runtime_event_bus_rx: tokio::sync::broadcast::Receiver<CoordinatorRuntimeEvent>,
    pub last_heartbeat_log_at: Option<std::time::Instant>,
    pub heartbeat_updates_since_log: usize,
    pub dispatch_retry_not_before: HashMap<String, std::time::Instant>,
    pub last_priority_zero_dispatch_block_task_id: Option<String>,
    pub dispatched_total_run: usize,
    pub dispatch_limit_event_emitted: bool,
    pub performer_ipc_addr: Option<String>,
    pub performer_ipc_listener_started: bool,
}

pub trait PhaseExecutor {
    fn run_phase(
        &self,
        task: &Task,
        mode: &str,
        coordinator_tool_override: Option<&str>,
        max_attempts: usize,
    ) -> Result<std::result::Result<String, String>>;
}

impl CoordinatorRunState {
    pub fn new() -> Self {
        let (event_tx, event_rx) = tokio::sync::mpsc::unbounded_channel();
        let (merge_event_tx, merge_event_rx) = tokio::sync::mpsc::unbounded_channel();
        let (runtime_event_bus_tx, runtime_event_bus_rx) = tokio::sync::broadcast::channel(4096);
        Self {
            active_jobs: HashMap::new(),
            join_set: tokio::task::JoinSet::new(),
            event_tx,
            event_rx,
            active_merge_jobs: HashMap::new(),
            merge_join_set: tokio::task::JoinSet::new(),
            merge_event_tx,
            merge_event_rx,
            runtime_event_bus_tx,
            runtime_event_bus_rx,
            last_heartbeat_log_at: None,
            heartbeat_updates_since_log: 0,
            dispatch_retry_not_before: HashMap::new(),
            last_priority_zero_dispatch_block_task_id: None,
            dispatched_total_run: 0,
            dispatch_limit_event_emitted: false,
            performer_ipc_addr: None,
            performer_ipc_listener_started: false,
        }
    }
}

impl Default for CoordinatorRunState {
    fn default() -> Self {
        Self::new()
    }
}

pub fn raw_event_identity(event: &CoordinatorEventRecord) -> Option<(String, String, String)> {
    let task_id = event.task_id.clone().unwrap_or_default();
    let ts = event.ts.clone();
    let source = event.source.clone();
    if task_id.is_empty() || ts.is_empty() {
        None
    } else {
        Some((task_id, ts, source))
    }
}

pub fn raw_event_to_runtime_event(
    event: &CoordinatorEventRecord,
) -> Option<CoordinatorRuntimeEvent> {
    let (task_id, ts, source) = raw_event_identity(event)?;
    let event_type = event.event_type.as_str();
    let event_status = event.status.as_str();
    let event_phase = event.phase.clone().filter(|value| !value.is_empty());
    let event_message = event.message().map(|value| value.to_string());
    let runtime_status = crate::coordinator::runtime_status_from_event(event_type, event_status)
        .as_str()
        .to_string();
    let kind = match event_type {
        "heartbeat" => CoordinatorRuntimeEventKind::Heartbeat,
        "progress" => CoordinatorRuntimeEventKind::Progress {
            status: runtime_status,
            phase: event_phase,
            message: event_message,
        },
        "phase_result" => CoordinatorRuntimeEventKind::PhaseResult {
            status: crate::coordinator::runtime_status_from_event(event_type, event_status)
                .as_str()
                .to_string(),
            phase: event_phase,
            message: event_message,
        },
        "failed" => CoordinatorRuntimeEventKind::Failed {
            phase: event_phase,
            message: event_message,
        },
        _ => return None,
    };
    Some(CoordinatorRuntimeEvent {
        task_id,
        ts,
        source,
        kind,
    })
}

pub fn parse_review_verdict(output: &str) -> Option<ReviewVerdict> {
    for line in output.lines().rev() {
        let trimmed = line.trim();
        if let Some(raw) = trimmed.strip_prefix("REVIEW_VERDICT:") {
            let verdict = raw.trim().to_ascii_uppercase();
            if verdict == "OK" {
                return Some(ReviewVerdict::Ok);
            }
            if verdict == "CHANGES_REQUESTED" {
                return Some(ReviewVerdict::ChangesRequested);
            }
            return None;
        }
    }
    None
}

fn git_status_clean(worktree: &Path) -> Result<bool> {
    Ok(git::status_porcelain(worktree)?.trim().is_empty())
}

fn git_head_commit(worktree: &Path) -> Result<String> {
    git::head_commit(worktree)
}

fn git_ahead_count(worktree: &Path, base: &str) -> Result<usize> {
    let range = format!("{}..HEAD", base);
    let output = git::run_git_output_mapped(
        worktree,
        &["rev-list", "--count", &range],
        "count ahead commits for review checks",
    )?;
    if !output.status.success() {
        return Ok(0);
    }
    let raw = String::from_utf8_lossy(&output.stdout);
    Ok(raw.trim().parse::<usize>().unwrap_or(0))
}

pub fn run_phase<E: PhaseExecutor>(
    executor: &E,
    task: &Task,
    mode: &str,
    coordinator_tool_override: Option<&str>,
    max_attempts: usize,
) -> Result<std::result::Result<String, String>> {
    executor.run_phase(task, mode, coordinator_tool_override, max_attempts)
}

pub fn run_review_phase<E: PhaseExecutor>(
    executor: &E,
    task: &Task,
    coordinator_tool_override: Option<&str>,
    max_attempts: usize,
) -> Result<std::result::Result<ReviewVerdict, String>> {
    let task_id = task.id.as_str();
    let worktree_path = task.worktree_path().unwrap_or_default();
    let base_branch = task.base_branch("master");
    if task_id.is_empty() || worktree_path.is_empty() {
        return Ok(Err(
            "review cannot run: missing task id or worktree path".to_string()
        ));
    }
    let worktree = PathBuf::from(worktree_path);
    let clean_before = git_status_clean(&worktree)?;
    if !clean_before {
        return Ok(Err(format!(
            "review precheck failed for task {}: worktree not clean before review",
            task_id
        )));
    }
    let ahead = git_ahead_count(&worktree, &base_branch)?;
    if ahead == 0 {
        return Ok(Err(format!(
            "review precheck failed for task {}: no committed diff to review against base '{}'",
            task_id, base_branch
        )));
    }
    let head_before = git_head_commit(&worktree)?;
    let phase = run_phase(
        executor,
        task,
        "review",
        coordinator_tool_override,
        max_attempts,
    )?;
    let output = match phase {
        Ok(out) => out,
        Err(reason) => return Ok(Err(reason)),
    };
    let clean_after = git_status_clean(&worktree)?;
    if !clean_after {
        return Ok(Err(format!(
            "review postcheck failed for task {}: worktree not clean after review",
            task_id
        )));
    }
    let head_after = git_head_commit(&worktree)?;
    if head_after != head_before {
        return Ok(Err(format!(
            "review postcheck failed for task {}: review changed commit {} -> {}",
            task_id, head_before, head_after
        )));
    }
    let Some(verdict) = parse_review_verdict(&output) else {
        return Ok(Err(format!(
            "review verdict parse failed for task {}: missing final REVIEW_VERDICT line",
            task_id
        )));
    };
    Ok(Ok(verdict))
}

pub fn resolve_phase_runner(
    repo_root: &Path,
    worktree_path: &Path,
    tool: &str,
) -> Result<Option<PathBuf>> {
    let explicit = worktree_path
        .join(".macc")
        .join("automation")
        .join("embedded")
        .join("adapters")
        .join(tool)
        .join(format!("{}.performer.sh", tool));
    if explicit.exists() {
        return Ok(Some(explicit));
    }
    let legacy = worktree_path
        .join(".macc")
        .join("automation")
        .join("runners")
        .join(format!("{}.performer.sh", tool));
    if legacy.exists() {
        return Ok(Some(legacy));
    }
    let tool_json_path = worktree_path.join(".macc").join("tool.json");
    if !tool_json_path.exists() {
        return Ok(None);
    }
    let raw = std::fs::read_to_string(&tool_json_path).map_err(|e| MaccError::Io {
        path: tool_json_path.to_string_lossy().into(),
        action: "read tool.json for phase runner".into(),
        source: e,
    })?;
    let value: serde_json::Value = serde_json::from_str(&raw).map_err(|e| {
        MaccError::Validation(format!(
            "Failed to parse tool.json for phase runner {}: {}",
            tool_json_path.display(),
            e
        ))
    })?;
    let runner = value
        .get("performer")
        .and_then(|v| v.get("runner"))
        .and_then(serde_json::Value::as_str)
        .unwrap_or_default();
    if runner.is_empty() {
        return Ok(None);
    }
    let path = if Path::new(runner).is_absolute() {
        PathBuf::from(runner)
    } else {
        repo_root.join(runner)
    };
    Ok(Some(path))
}

pub fn build_phase_prompt(mode: &str, task_id: &str, tool: &str, task: &Task) -> Result<String> {
    let task_payload = serde_json::to_string(task).map_err(|e| {
        MaccError::Validation(format!(
            "Failed to serialize task payload for '{}' phase prompt (task={}): {}",
            mode, task_id, e
        ))
    })?;
    if mode == "review" {
        return Ok(format!(
            "You are the assigned {} performer running inside a MACC worktree.\n\nMode: {}\nTask ID: {}\n\nTask registry entry (JSON):\n{}\n\nInstructions:\n1) Execute the review phase only.\n2) Review the already committed task changes and produce a verdict.\n3) Do not modify files, do not create commits, and do not modify task registry state.\n4) Return exactly one final verdict line at the end of your response:\n   - REVIEW_VERDICT: OK\n   - REVIEW_VERDICT: CHANGES_REQUESTED\n",
            tool, mode, task_id, task_payload
        ));
    }
    Ok(format!(
        "You are the assigned {} performer running inside a MACC worktree.\n\nMode: {}\nTask ID: {}\n\nTask registry entry (JSON):\n{}\n\nInstructions:\n1) Execute the {} phase only.\n2) Keep changes minimal and focused on this task.\n3) Update code/tests/docs as needed for this phase.\n4) Do not modify task registry state directly.\n",
        tool, mode, task_id, task_payload, mode
    ))
}

#[allow(clippy::too_many_arguments)]
pub fn spawn_performer_job(
    executable_path: &Path,
    repo_root: &Path,
    task_id: &str,
    worktree_path: &Path,
    event_tx: &tokio::sync::mpsc::UnboundedSender<CoordinatorJobEvent>,
    join_set: &mut tokio::task::JoinSet<()>,
    phase_timeout_seconds: usize,
    performer_ipc_addr: Option<&str>,
) -> Result<Option<i64>> {
    let effective_ipc_addr = performer_ipc_addr
        .filter(|value| !value.trim().is_empty())
        .map(|value| value.to_string());
    if effective_ipc_addr.is_none() {
        return Err(MaccError::Validation(
            "performer spawn refused: no coordinator IPC address".to_string(),
        ));
    }
    let mut run_cmd = tokio::process::Command::new(executable_path);
    let event_source = format!(
        "coordinator-worktree:{}:{}",
        task_id,
        chrono::Utc::now().timestamp_nanos_opt().unwrap_or_default()
    );
    run_cmd
        .current_dir(repo_root)
        .env(
            "COORDINATOR_RUN_ID",
            std::env::var("COORDINATOR_RUN_ID").unwrap_or_else(|_| {
                format!(
                    "run-{}-{}",
                    chrono::Utc::now().timestamp_nanos_opt().unwrap_or_default(),
                    std::process::id()
                )
            }),
        )
        .env("MACC_EVENT_SOURCE", event_source.clone())
        .env("MACC_EVENT_TASK_ID", task_id)
        .env_remove(crate::coordinator::ipc::COORDINATOR_IPC_ADDR_ENV)
        .arg("--cwd")
        .arg(repo_root)
        .arg("worktree")
        .arg("run")
        .arg(worktree_path.to_string_lossy().to_string());
    if let Some(ipc_addr) = effective_ipc_addr.as_deref() {
        run_cmd.env(crate::coordinator::ipc::COORDINATOR_IPC_ADDR_ENV, ipc_addr);
    }
    let mut child = run_cmd.spawn().map_err(|e| MaccError::Io {
        path: worktree_path.to_string_lossy().into(),
        action: "spawn performer process".into(),
        source: e,
    })?;
    let pid = child.id().map(|v| v as i64);
    let repo_root_owned = repo_root.to_path_buf();
    let task_id_owned = task_id.to_string();
    let worktree_path_owned = worktree_path.to_path_buf();
    let event_source_owned = event_source.clone();
    let tx = event_tx.clone();
    join_set.spawn(async move {
        let (success, status_text, timed_out) = if phase_timeout_seconds > 0 {
            match tokio::time::timeout(
                std::time::Duration::from_secs(phase_timeout_seconds as u64),
                child.wait(),
            )
            .await
            {
                Ok(Ok(status)) => (status.success(), status.to_string(), false),
                Ok(Err(err)) => (false, err.to_string(), false),
                Err(_) => {
                    let _ = child.kill().await;
                    (false, "timeout".to_string(), true)
                }
            }
        } else {
            match child.wait().await {
                Ok(status) => (status.success(), status.to_string(), false),
                Err(err) => (false, err.to_string(), false),
            }
        };
        let reported_success = success;
        let (completion_details, completion_details_source) = if reported_success {
            if let Some(details) =
                read_last_completion_details(&repo_root_owned, &task_id_owned, &event_source_owned)
            {
                (Some(details), Some("sqlite".to_string()))
            } else if compat_phase_result_log_fallback_enabled() {
                (
                    read_completion_details_from_worktree_log(&worktree_path_owned, &task_id_owned),
                    Some("log-fallback".to_string()),
                )
            } else {
                (None, None)
            }
        } else {
            (None, None)
        };
        let success = if reported_success {
            completion_details.is_some()
        } else {
            false
        };
        let status_text = if reported_success && completion_details.is_none() {
            "performer exited successfully but no phase_result event was persisted via coordinator IPC"
                .to_string()
        } else {
            status_text
        };
        let mut error_code = None;
        let mut error_origin = None;
        let mut error_message = None;
        if !success {
            if let Some(details) =
                read_last_error_details(&repo_root_owned, &task_id_owned, &event_source_owned)
            {
                error_code = details.error_code;
                error_origin = details.error_origin;
                error_message = details.error_message;
            } else if reported_success {
                error_code = Some("E901".to_string());
                error_origin = Some("coordinator".to_string());
                error_message = Some(
                    "performer exited without persisting terminal phase_result event".to_string(),
                );
            }
        }
        let _ = tx.send(CoordinatorJobEvent {
            task_id: task_id_owned,
            success,
            status_text: completion_details
                .as_ref()
                .and_then(|details| details.message.clone())
                .filter(|message| !message.trim().is_empty())
                .unwrap_or(status_text),
            timed_out,
            completion_kind: completion_details.and_then(|details| details.result_kind),
            completion_details_source,
            error_code,
            error_origin,
            error_message,
        });
    });
    Ok(pid)
}

#[derive(Debug, Clone)]
struct ErrorDetails {
    error_code: Option<String>,
    error_origin: Option<String>,
    error_message: Option<String>,
}

#[derive(Debug, Clone)]
struct CompletionDetails {
    result_kind: Option<PerformerCompletionKind>,
    message: Option<String>,
}

const COORDINATOR_COMPAT_PHASE_RESULT_LOG_FALLBACK: &str =
    "COORDINATOR_COMPAT_PHASE_RESULT_LOG_FALLBACK";

fn compat_phase_result_log_fallback_enabled() -> bool {
    std::env::var(COORDINATOR_COMPAT_PHASE_RESULT_LOG_FALLBACK)
        .ok()
        .map(|raw| {
            matches!(
                raw.trim().to_ascii_lowercase().as_str(),
                "1" | "true" | "yes" | "on"
            )
        })
        .unwrap_or(false)
}

fn read_last_error_details(
    repo_root: &Path,
    task_id: &str,
    event_source: &str,
) -> Option<ErrorDetails> {
    read_last_error_details_from_sqlite(repo_root, task_id, event_source)
}

fn read_last_completion_details(
    repo_root: &Path,
    task_id: &str,
    event_source: &str,
) -> Option<CompletionDetails> {
    let project_paths = crate::ProjectPaths::from_root(repo_root);
    let storage_paths =
        crate::coordinator_storage::CoordinatorStoragePaths::from_project_paths(&project_paths);
    let sqlite = crate::coordinator_storage::SqliteStorage::new(storage_paths);
    let snapshot = sqlite.load_snapshot().ok()?;
    for event in snapshot.events.iter().rev() {
        let Some(event_task_id) = event.task_id.as_deref() else {
            continue;
        };
        if event_task_id != task_id {
            continue;
        }
        if !event_source.is_empty() && event.source != event_source {
            continue;
        }
        if event.event_type != "phase_result" {
            continue;
        }
        return Some(CompletionDetails {
            result_kind: event.payload_result_kind(),
            message: event.message().map(|value| value.to_string()),
        });
    }
    None
}

fn read_completion_details_from_worktree_log(
    worktree_path: &Path,
    task_id: &str,
) -> Option<CompletionDetails> {
    let log_path = performer_task_log_path(worktree_path, task_id);
    let raw = std::fs::read_to_string(log_path).ok()?;
    let mut result_kind = None;
    let mut message = None;
    for line in raw.lines().rev() {
        let trimmed = line.trim();
        if result_kind.is_none() {
            if let Some(raw_kind) = trimmed.strip_prefix("- Result kind:") {
                result_kind = raw_kind.trim().parse::<PerformerCompletionKind>().ok();
                continue;
            }
            if let Some(raw_kind) = trimmed.strip_prefix("MACC_TASK_RESULT:") {
                result_kind = raw_kind.trim().parse::<PerformerCompletionKind>().ok();
                continue;
            }
        }
        if message.is_none() && !trimmed.is_empty() && !trimmed.starts_with('-') {
            message = Some(trimmed.to_string());
        }
        if result_kind.is_some() && message.is_some() {
            break;
        }
    }
    result_kind.map(|kind| CompletionDetails {
        result_kind: Some(kind),
        message,
    })
}

fn performer_task_log_path(worktree_path: &Path, task_id: &str) -> PathBuf {
    let safe: String = task_id
        .chars()
        .filter(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '.' | '-'))
        .collect();
    let file = if safe.is_empty() {
        "task".to_string()
    } else {
        safe
    };
    worktree_path
        .join(".macc/log/performer")
        .join(format!("{}.md", file))
}

fn read_last_error_details_from_sqlite(
    repo_root: &Path,
    task_id: &str,
    event_source: &str,
) -> Option<ErrorDetails> {
    let project_paths = crate::ProjectPaths::from_root(repo_root);
    let storage_paths =
        crate::coordinator_storage::CoordinatorStoragePaths::from_project_paths(&project_paths);
    let sqlite = crate::coordinator_storage::SqliteStorage::new(storage_paths);
    let snapshot = sqlite.load_snapshot().ok()?;
    let mut failed_candidate: Option<ErrorDetails> = None;
    let mut saw_terminal_success_before_failed = false;
    for event in snapshot.events.iter().rev() {
        let Some(event_task_id) = event.task_id.as_deref() else {
            continue;
        };
        if event_task_id != task_id {
            continue;
        }
        let source = event.source.as_str();
        if !event_source.is_empty() && source != event_source {
            continue;
        }
        let event_type = event.event_type.as_str();
        let status = event.status.as_str();
        let is_terminal_success = event.is_terminal_success();
        if is_terminal_success && failed_candidate.is_some() {
            saw_terminal_success_before_failed = true;
            continue;
        }
        if event_type != "failed" && !(event_type == "phase_result" && status == "failed") {
            continue;
        }
        let error_code = event.payload_error_code();
        let error_origin = event.payload_origin();
        let error_message = event.message().map(|v| v.to_string());
        if failed_candidate.is_none() {
            failed_candidate = Some(ErrorDetails {
                error_code,
                error_origin,
                error_message,
            });
        }
    }
    if saw_terminal_success_before_failed {
        return None;
    }
    failed_candidate.and_then(|details| {
        if details.error_code.is_some()
            || details.error_origin.is_some()
            || details.error_message.is_some()
        {
            Some(details)
        } else {
            None
        }
    })
}

pub async fn spawn_merge_job<F>(
    task_id: &str,
    event_tx: &tokio::sync::mpsc::UnboundedSender<CoordinatorMergeEvent>,
    join_set: &mut tokio::task::JoinSet<()>,
    merge_timeout_seconds: usize,
    merge_runner: F,
) -> Result<()>
where
    F: FnOnce() -> Result<std::result::Result<(), String>> + Send + 'static,
{
    let merge_timeout_seconds = merge_timeout_seconds as u64;
    let task_id_owned = task_id.to_string();
    let tx = event_tx.clone();
    join_set.spawn(async move {
        let worker = tokio::task::spawn_blocking(merge_runner);
        let outcome = if merge_timeout_seconds > 0 {
            match tokio::time::timeout(
                std::time::Duration::from_secs(merge_timeout_seconds),
                worker,
            )
            .await
            {
                Ok(joined) => joined,
                Err(_) => {
                    let _ = tx.send(CoordinatorMergeEvent {
                        task_id: task_id_owned,
                        success: false,
                        reason: format!(
                            "failure:local_merge step=timeout timeout_s={}",
                            merge_timeout_seconds
                        ),
                    });
                    return;
                }
            }
        } else {
            worker.await
        };
        let evt = match outcome {
            Ok(Ok(Ok(()))) => CoordinatorMergeEvent {
                task_id: task_id_owned,
                success: true,
                reason: "merge completed".to_string(),
            },
            Ok(Ok(Err(reason))) => CoordinatorMergeEvent {
                task_id: task_id_owned,
                success: false,
                reason,
            },
            Ok(Err(err)) => CoordinatorMergeEvent {
                task_id: task_id_owned,
                success: false,
                reason: err.to_string(),
            },
            Err(join_err) => CoordinatorMergeEvent {
                task_id: task_id_owned,
                success: false,
                reason: format!("merge worker join error: {}", join_err),
            },
        };
        let _ = tx.send(evt);
    });
    Ok(())
}

pub fn terminate_active_jobs(state: &CoordinatorRunState) -> Vec<(String, i64)> {
    let mut terminated = Vec::new();
    for (task_id, job) in &state.active_jobs {
        let Some(pid) = job.pid else {
            continue;
        };
        let _ = std::process::Command::new("kill")
            .arg("-TERM")
            .arg(pid.to_string())
            .status();
        let _ = std::process::Command::new("kill")
            .arg("-TERM")
            .arg(format!("-{}", pid))
            .status();
        terminated.push((task_id.clone(), pid));
    }
    terminated
}

pub fn summarize_output(text: &str) -> String {
    let normalized = text.replace(['\n', '\r'], " ");
    let collapsed = normalized.split_whitespace().collect::<Vec<_>>().join(" ");
    if collapsed.len() > 1000 {
        format!("{}...", &collapsed[..1000])
    } else {
        collapsed
    }
}

fn coordinator_log_dir(repo_root: &Path) -> PathBuf {
    repo_root.join(".macc").join("log").join("coordinator")
}

enum HookRunResult {
    Completed { output: String, timed_out: bool },
}

fn run_merge_hook_with_timeout(
    repo_root: &Path,
    hook: &Path,
    task_id: &str,
    branch: &str,
    base: &str,
    conflicts: &str,
    timeout_seconds: u64,
) -> Result<HookRunResult> {
    use std::process::{Command, Stdio};
    let mut child = Command::new(hook)
        .current_dir(repo_root)
        .arg("--repo")
        .arg(repo_root)
        .arg("--task-id")
        .arg(task_id)
        .arg("--branch")
        .arg(branch)
        .arg("--base-branch")
        .arg(base)
        .arg("--failure-step")
        .arg("merge")
        .arg("--failure-reason")
        .arg("git merge reported conflicts")
        .arg("--conflicts")
        .arg(conflicts)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| MaccError::Io {
            path: hook.to_string_lossy().into(),
            action: "spawn merge-fix hook".into(),
            source: e,
        })?;
    let started = std::time::Instant::now();
    let mut timed_out = false;
    loop {
        match child.try_wait() {
            Ok(Some(_)) => break,
            Ok(None) => {
                if timeout_seconds > 0 && started.elapsed().as_secs() >= timeout_seconds {
                    timed_out = true;
                    let _ = child.kill();
                    let _ = child.wait();
                    break;
                }
                std::thread::sleep(std::time::Duration::from_millis(100));
            }
            Err(e) => {
                return Err(MaccError::Validation(format!(
                    "merge-fix hook wait error: {}",
                    e
                )));
            }
        }
    }

    let mut out = String::new();
    if let Some(mut stdout) = child.stdout.take() {
        let _ = stdout.read_to_string(&mut out);
    }
    let mut err = String::new();
    if let Some(mut stderr) = child.stderr.take() {
        let _ = stderr.read_to_string(&mut err);
    }
    Ok(HookRunResult::Completed {
        output: format!("{}\n{}", out, err),
        timed_out,
    })
}

#[allow(clippy::too_many_arguments)]
pub fn merge_task_with_policy_native<FE>(
    repo_root: &Path,
    task_id: &str,
    branch: &str,
    base: &str,
    merge_ai_fix: bool,
    merge_hook_timeout: Option<u64>,
    mut emit_event: FE,
) -> Result<std::result::Result<(), String>>
where
    FE: FnMut(&str, &str, &str, &str, &str, &str),
{
    let log_dir = coordinator_log_dir(repo_root);
    std::fs::create_dir_all(&log_dir).map_err(|e| MaccError::Io {
        path: log_dir.to_string_lossy().into(),
        action: "create coordinator log dir".into(),
        source: e,
    })?;
    let suggestion = format!("git checkout {} && git merge {}", base, branch);

    if !git::rev_parse_verify(repo_root, branch).unwrap_or(false) {
        return Ok(Err(format!(
            "failure:local_merge step=verify_branch branch={} base={} suggestion=\"{}\"",
            branch, base, suggestion
        )));
    }
    if !git::rev_parse_verify(repo_root, base).unwrap_or(false) {
        return Ok(Err(format!(
            "failure:local_merge step=verify_base branch={} base={} suggestion=\"{}\"",
            branch, base, suggestion
        )));
    }

    if !git::status_porcelain(repo_root)?.trim().is_empty() {
        return Ok(Err(format!(
            "failure:local_merge step=precheck_clean branch={} base={} suggestion=\"{}\"",
            branch, base, suggestion
        )));
    }

    let _ = git::checkout(repo_root, base, false);
    let merge_msg = format!("macc: merge task {}", task_id);
    let merge = git::run_git_output_mapped(
        repo_root,
        &["merge", "--no-ff", "-m", &merge_msg, branch],
        "run local merge",
    )?;
    if merge.status.success() {
        return Ok(Ok(()));
    }

    let merge_output = format!(
        "{}\n{}",
        String::from_utf8_lossy(&merge.stdout),
        String::from_utf8_lossy(&merge.stderr)
    );
    let conflicts = git::run_git_output_mapped(
        repo_root,
        &["diff", "--name-only", "--diff-filter=U"],
        "list merge conflict files",
    )
    .ok()
    .map(|o| String::from_utf8_lossy(&o.stdout).trim().replace('\n', ","))
    .unwrap_or_default();

    let mut hook_output = String::new();
    if merge_ai_fix {
        let hook = crate::ProjectPaths::from_root(repo_root).automation_merge_fix_hook_path();
        let hook_timeout_seconds = merge_hook_timeout.unwrap_or(90);
        let hook_started = std::time::Instant::now();
        emit_event(
            "merge_hook",
            task_id,
            "integrate",
            "started",
            &format!(
                "merge-fix hook started task={} timeout_s={}",
                task_id, hook_timeout_seconds
            ),
            "info",
        );
        let hook_result = run_merge_hook_with_timeout(
            repo_root,
            &hook,
            task_id,
            branch,
            base,
            &conflicts,
            hook_timeout_seconds,
        );
        let hook_elapsed = hook_started.elapsed().as_secs();
        match hook_result {
            Ok(HookRunResult::Completed { output, timed_out }) => {
                hook_output = output;
                emit_event(
                    "merge_hook",
                    task_id,
                    "integrate",
                    if timed_out { "timeout" } else { "done" },
                    &format!(
                        "merge-fix hook completed task={} elapsed={}s timeout={}",
                        task_id, hook_elapsed, timed_out
                    ),
                    if timed_out { "warning" } else { "info" },
                );
            }
            Err(err) => {
                hook_output = format!("merge-fix hook execution error: {}", err);
                emit_event(
                    "merge_hook",
                    task_id,
                    "integrate",
                    "failed",
                    &format!(
                        "merge-fix hook failed task={} elapsed={}s error={}",
                        task_id, hook_elapsed, err
                    ),
                    "warning",
                );
            }
        }
        let unresolved = git::run_git_output_mapped(
            repo_root,
            &["diff", "--name-only", "--diff-filter=U"],
            "list unresolved merge conflict files",
        )
        .ok()
        .map(|o| !String::from_utf8_lossy(&o.stdout).trim().is_empty())
        .unwrap_or(true);
        let in_merge = git::rev_parse_verify(repo_root, "MERGE_HEAD").unwrap_or(false);
        if !unresolved && !in_merge {
            return Ok(Ok(()));
        }
    }

    let in_merge = git::rev_parse_verify(repo_root, "MERGE_HEAD").unwrap_or(false);
    if in_merge {
        let _ =
            git::run_git_output_mapped(repo_root, &["merge", "--abort"], "abort conflicted merge");
    }

    let report_file = log_dir.join(format!(
        "merge-fail-{}-{}.md",
        task_id,
        chrono::Utc::now().format("%Y%m%dT%H%M%SZ")
    ));
    let report = format!(
        "# Local merge failure report\n\n- Task: {}\n- Branch: {}\n- Base: {}\n- UTC: {}\n\n## Conflicts\n\n{}\n\n## Suggested manual command\n\n`cd \"{}\" && {}`\n\n## Merge stdout/stderr\n\n```text\n{}\n```\n\n## Merge-fix hook output\n\n```text\n{}\n```\n",
        task_id,
        branch,
        base,
        chrono::Utc::now().to_rfc3339(),
        if conflicts.is_empty() { "none" } else { &conflicts },
        repo_root.display(),
        suggestion,
        merge_output,
        hook_output
    );
    let _ = std::fs::write(&report_file, report);
    let err = format!(
        "failure:local_merge step=merge branch={} base={} conflicts=[{}] git_output=\"{}\" suggestion=\"{}\" report=\"{}\"",
        branch,
        base,
        if conflicts.is_empty() { "none" } else { &conflicts },
        summarize_output(&merge_output),
        suggestion,
        report_file.display()
    );
    Ok(Err(err))
}

#[derive(Debug, Clone)]
pub enum BranchCleanupResult {
    Deleted,
    Skipped { reason: String },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BranchCleanupQueueEntry {
    task_id: String,
    phase: String,
    branch: String,
    base: String,
    context: String,
    reason: String,
    attempts: usize,
}

fn delete_local_branch(repo_root: &Path, branch: &str) -> Result<()> {
    match git::delete_local_branch(repo_root, branch, false) {
        Ok(()) => Ok(()),
        Err(_) => git::delete_local_branch(repo_root, branch, true),
    }
}

pub fn cleanup_merged_local_branch(
    repo_root: &Path,
    branch: &str,
    base: &str,
) -> Result<BranchCleanupResult> {
    if branch.is_empty() || branch == base {
        return Ok(BranchCleanupResult::Skipped {
            reason: "empty_or_base_branch".to_string(),
        });
    }
    if let Some(in_use_path) = find_worktree_using_branch(repo_root, branch)? {
        return Ok(BranchCleanupResult::Skipped {
            reason: format!("branch_in_use_by_worktree:{}", in_use_path.display()),
        });
    }
    delete_local_branch(repo_root, branch)?;
    Ok(BranchCleanupResult::Deleted)
}

fn branch_cleanup_queue_path(repo_root: &Path) -> PathBuf {
    repo_root
        .join(".macc")
        .join("tmp")
        .join("branch-cleanup-queue.jsonl")
}

fn append_branch_cleanup_queue_entry(
    repo_root: &Path,
    entry: &BranchCleanupQueueEntry,
) -> Result<()> {
    let path = branch_cleanup_queue_path(repo_root);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| MaccError::Io {
            path: parent.to_string_lossy().into(),
            action: "create branch cleanup queue dir".into(),
            source: e,
        })?;
    }
    let line = serde_json::to_string(entry).map_err(|e| {
        MaccError::Validation(format!(
            "Failed to serialize branch cleanup queue entry: {}",
            e
        ))
    })?;
    use std::io::Write;
    let mut file = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)
        .map_err(|e| MaccError::Io {
            path: path.to_string_lossy().into(),
            action: "open branch cleanup queue".into(),
            source: e,
        })?;
    file.write_all(line.as_bytes()).map_err(|e| MaccError::Io {
        path: path.to_string_lossy().into(),
        action: "append branch cleanup queue entry".into(),
        source: e,
    })?;
    file.write_all(b"\n").map_err(|e| MaccError::Io {
        path: path.to_string_lossy().into(),
        action: "write branch cleanup queue newline".into(),
        source: e,
    })?;
    Ok(())
}

fn load_branch_cleanup_queue(repo_root: &Path) -> Result<Vec<BranchCleanupQueueEntry>> {
    let path = branch_cleanup_queue_path(repo_root);
    if !path.exists() {
        return Ok(Vec::new());
    }
    let raw = std::fs::read_to_string(&path).map_err(|e| MaccError::Io {
        path: path.to_string_lossy().into(),
        action: "read branch cleanup queue".into(),
        source: e,
    })?;
    let mut entries = Vec::new();
    for line in raw.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        if let Ok(entry) = serde_json::from_str::<BranchCleanupQueueEntry>(trimmed) {
            entries.push(entry);
        }
    }
    Ok(entries)
}

fn save_branch_cleanup_queue(repo_root: &Path, entries: &[BranchCleanupQueueEntry]) -> Result<()> {
    let path = branch_cleanup_queue_path(repo_root);
    if entries.is_empty() {
        if path.exists() {
            let _ = std::fs::remove_file(&path);
        }
        return Ok(());
    }
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| MaccError::Io {
            path: parent.to_string_lossy().into(),
            action: "create branch cleanup queue dir".into(),
            source: e,
        })?;
    }
    let mut out = String::new();
    for entry in entries {
        let line = serde_json::to_string(entry).map_err(|e| {
            MaccError::Validation(format!(
                "Failed to serialize branch cleanup queue entry: {}",
                e
            ))
        })?;
        out.push_str(&line);
        out.push('\n');
    }
    std::fs::write(&path, out).map_err(|e| MaccError::Io {
        path: path.to_string_lossy().into(),
        action: "write branch cleanup queue".into(),
        source: e,
    })?;
    Ok(())
}

pub fn process_branch_cleanup_queue<FE, FL>(
    repo_root: &Path,
    mut emit_event: FE,
    mut log_note: Option<FL>,
) -> Result<usize>
where
    FE: FnMut(&str, &str, &str, &str, &str, &str),
    FL: FnMut(String),
{
    let mut queue = load_branch_cleanup_queue(repo_root)?;
    if queue.is_empty() {
        return Ok(0);
    }
    let mut remaining = Vec::new();
    let mut processed = 0usize;
    for mut item in queue.drain(..) {
        match cleanup_merged_local_branch(repo_root, &item.branch, &item.base) {
            Ok(BranchCleanupResult::Deleted) => {
                processed += 1;
                let msg = format!(
                    "branch cleanup success context={} task={} branch={} base={} mode=maintenance",
                    item.context, item.task_id, item.branch, item.base
                );
                emit_event(
                    "branch_cleanup",
                    &item.task_id,
                    &item.phase,
                    "success",
                    &msg,
                    "info",
                );
            }
            Ok(BranchCleanupResult::Skipped { reason }) => {
                processed += 1;
                let msg = format!(
                    "branch cleanup skipped context={} task={} branch={} base={} reason={} mode=maintenance",
                    item.context, item.task_id, item.branch, item.base, reason
                );
                emit_event(
                    "branch_cleanup",
                    &item.task_id,
                    &item.phase,
                    "skipped",
                    &msg,
                    "warning",
                );
            }
            Err(err) => {
                item.attempts += 1;
                if item.attempts >= 5 {
                    let msg = format!(
                        "branch cleanup dropped after retries context={} task={} branch={} base={} error={}",
                        item.context, item.task_id, item.branch, item.base, err
                    );
                    emit_event(
                        "branch_cleanup",
                        &item.task_id,
                        &item.phase,
                        "failed",
                        &msg,
                        "warning",
                    );
                } else {
                    remaining.push(item);
                }
            }
        }
    }
    save_branch_cleanup_queue(repo_root, &remaining)?;
    if let Some(note) = log_note.as_mut() {
        if processed > 0 {
            note(format!(
                "- Maintenance branch-cleanup processed={} remaining={}",
                processed,
                remaining.len()
            ));
        }
    }
    Ok(processed)
}

fn find_worktree_using_branch(repo_root: &Path, branch: &str) -> Result<Option<PathBuf>> {
    let output = match git::worktree_list_porcelain(repo_root) {
        Ok(raw) => raw,
        Err(_) => return Ok(None),
    };
    let mut current_path: Option<PathBuf> = None;
    for line in output.lines() {
        let trimmed = line.trim();
        if let Some(path) = trimmed.strip_prefix("worktree ") {
            current_path = Some(PathBuf::from(path));
            continue;
        }
        if let Some(found_branch) = trimmed.strip_prefix("branch ") {
            let normalized = found_branch
                .strip_prefix("refs/heads/")
                .unwrap_or(found_branch);
            if normalized == branch {
                return Ok(current_path);
            }
        }
    }
    Ok(None)
}

#[allow(clippy::too_many_arguments)]
pub fn report_branch_cleanup_outcome<FE, FW>(
    repo_root: &Path,
    task_id: Option<&str>,
    phase: &str,
    branch: &str,
    base: &str,
    context: &str,
    cleanup_result: std::result::Result<BranchCleanupResult, MaccError>,
    mut emit_event: FE,
    mut warn: FW,
) where
    FE: FnMut(&str, &str, &str, &str, &str, &str),
    FW: FnMut(String),
{
    let task_ref = task_id.unwrap_or("unknown");
    match cleanup_result {
        Ok(BranchCleanupResult::Deleted) => {
            let msg = format!(
                "branch cleanup success context={} task={} branch={} base={}",
                context, task_ref, branch, base
            );
            emit_event("branch_cleanup", task_ref, phase, "success", &msg, "info");
        }
        Ok(BranchCleanupResult::Skipped { reason }) => {
            let msg = format!(
                "branch cleanup skipped context={} task={} branch={} base={} reason={}",
                context, task_ref, branch, base, reason
            );
            warn(format!("warning: {}", msg));
            emit_event(
                "branch_cleanup",
                task_ref,
                phase,
                "skipped",
                &msg,
                "warning",
            );
        }
        Err(err) => {
            let msg = format!(
                "branch cleanup deferred context={} task={} branch={} base={} error={}",
                context, task_ref, branch, base, err
            );
            warn(format!("warning: {}", msg));
            emit_event(
                "branch_cleanup",
                task_ref,
                phase,
                "deferred",
                &msg,
                "warning",
            );
            let _ = append_branch_cleanup_queue_entry(
                repo_root,
                &BranchCleanupQueueEntry {
                    task_id: task_ref.to_string(),
                    phase: phase.to_string(),
                    branch: branch.to_string(),
                    base: base.to_string(),
                    context: context.to_string(),
                    reason: err.to_string(),
                    attempts: 0,
                },
            );
        }
    }
}
