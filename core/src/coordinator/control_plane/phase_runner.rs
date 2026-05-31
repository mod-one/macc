use super::base::CoordinatorLog;
use crate::coordinator::helpers::now_iso_coordinator;
use crate::coordinator::ipc::read_performer_ipc_addr;
use crate::coordinator::model::TaskRegistry;
use crate::coordinator::{engine as coordinator_engine, runtime as coordinator_runtime};
use crate::{MaccError, Result};
use std::path::Path;

pub(super) struct NativePhaseExecutor<'a> {
    pub(super) repo_root: &'a Path,
    pub(super) logger: Option<&'a dyn CoordinatorLog>,
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
pub(super) fn ensure_tool_json_for_tool(
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

pub(super) fn read_session_id_from_state(
    repo_root: &Path,
    tool_id: &str,
    _worktree_path: &Path,
) -> Option<String> {
    let path = repo_root.join(".macc/state/tool-sessions.json");
    let raw = std::fs::read_to_string(&path).ok()?;
    let root: serde_json::Value = serde_json::from_str(&raw).ok()?;
    let sessions = root
        .get("tools")?
        .get(tool_id)?
        .get("sessions")?
        .as_object()?;
    // Pool model: sessions are keyed by session_id; find the first available one.
    // Old-format entries (keyed by worktree path) carry a nested "session_id"
    // sub-field and are skipped — they belong to the previous per-worktree scheme.
    for (session_id, entry) in sessions {
        if entry.get("session_id").is_some() {
            continue;
        }
        let status = entry
            .get("status")
            .and_then(serde_json::Value::as_str)
            .unwrap_or("available");
        if status != "active" {
            return Some(session_id.clone());
        }
    }
    None
}

pub(super) fn task_active_session_id_from_registry(
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

pub(super) fn refresh_task_active_session_id_in_registry(
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

pub(super) fn append_task_lifecycle_event_with_session(
    repo_root: &Path,
    event_type: &str,
    task_id: &str,
    phase: &str,
    status: &str,
    message: &str,
    session_id: Option<&str>,
) -> Result<()> {
    let run_id = crate::coordinator::helpers::ensure_coordinator_run_id();
    let epoch = std::env::var("COORDINATOR_EPOCH")
        .ok()
        .and_then(|s| s.parse::<i64>().ok())
        .unwrap_or(0);
    let now = now_iso_coordinator();
    let seq = chrono::Utc::now().timestamp_nanos_opt().unwrap_or_default() as u64;
    let payload = serde_json::json!({
        "schema_version":"1",
        "event_id": format!("evt-{}-{}-{}", event_type, task_id, seq),
        "run_id": run_id,
        "coordinator_epoch": epoch,
        "claim_id": session_id,
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

    let event_record = crate::coordinator::CoordinatorEventRecord {
        schema_version: "1".to_string(),
        event_id: format!("evt-{}-{}-{}", event_type, task_id, seq),
        run_id: Some(run_id),
        coordinator_epoch: Some(epoch),
        claim_id: session_id.map(|s| s.to_string()),
        seq: seq as i64,
        ts: now,
        source: "coordinator:native".to_string(),
        task_id: if task_id.is_empty() { None } else { Some(task_id.to_string()) },
        event_type: event_type.to_string(),
        phase: if phase.is_empty() { None } else { Some(phase.to_string()) },
        status: status.to_string(),
        detail: Some(message.to_string()),
        msg: None,
        payload: serde_json::json!({ "session_id": session_id }),
        extra: std::collections::BTreeMap::new(),
    };
    let _ = crate::coordinator::helpers::append_structured_event_record(repo_root, &event_record);

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
        let _ = crate::coordinator::helpers::write_structured_event_jsonl(
            self.repo_root,
            "phase_started",
            task_id,
            mode,
            &format!("Phase {} started (tool={})", mode, phase_tool),
            "info",
        );
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
                let _ = crate::coordinator::helpers::write_structured_event_jsonl(
                    self.repo_root,
                    "phase_completed",
                    task_id,
                    mode,
                    &format!("Phase {} completed successfully (tool={})", mode, phase_tool),
                    "info",
                );
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
        let _ = crate::coordinator::helpers::write_structured_event_jsonl(
            self.repo_root,
            "phase_completed",
            task_id,
            mode,
            &format!("Phase {} failed: {}", mode, last_reason),
            "error",
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
