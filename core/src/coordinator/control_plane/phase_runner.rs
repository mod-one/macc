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
    pub(super) cancelled: Option<&'a std::sync::atomic::AtomicBool>,
    pub(super) prepared_context: Option<&'a PreparedPhaseContext>,
}

pub(super) struct PreparedPhaseContext {
    pub attempt: String,
    pub context: Option<(
        crate::coordinator::decisions::ContextPackage,
        std::path::PathBuf,
    )>,
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

/// Read model and effort currently stored in a worktree's `.macc/tool.json`.
fn read_tool_meta_from_tool_json(worktree: &std::path::Path) -> (Option<String>, Option<String>) {
    let tool_json = worktree.join(".macc").join("tool.json");
    let raw = match std::fs::read_to_string(&tool_json) {
        Ok(r) => r,
        Err(_) => return (None, None),
    };
    let value: serde_json::Value = match serde_json::from_str(&raw) {
        Ok(v) => v,
        Err(_) => return (None, None),
    };
    let model = value
        .get("model")
        .and_then(serde_json::Value::as_str)
        .map(|s| s.to_string());
    let effort = value
        .get("effort")
        .and_then(serde_json::Value::as_str)
        .map(|s| s.to_string());
    (model, effort)
}

/// A phase fallback is local to its task; configured coordinator tools remain defaults.
pub(super) fn phase_tool_for_task<'a>(
    task: &'a crate::coordinator::model::Task,
    configured: Option<&'a str>,
) -> Option<&'a str> {
    task.task_runtime
        .extra
        .get("phase_tool_override")
        .and_then(serde_json::Value::as_str)
        .filter(|tool| !tool.trim().is_empty())
        .or_else(|| configured.filter(|tool| !tool.trim().is_empty()))
        .or_else(|| task.coordinator_tool())
        .or_else(|| task.task_tool())
}

/// Ensure the worktree's `.macc/tool.json` matches the desired tool.
/// If the current tool.json is missing or for a different tool, regenerate it.
pub(super) fn ensure_tool_json_for_tool(
    repo_root: &std::path::Path,
    worktree: &std::path::Path,
    desired_tool: &str,
) -> Result<()> {
    let current_tool = read_tool_id_from_tool_json(worktree);
    if current_tool.as_deref() != Some(desired_tool) {
        crate::worktree::write_tool_json(repo_root, worktree, desired_tool)?;
    }
    // worktree apply reads this tool ID, while the runner reads tool.json.
    if let Some(mut metadata) = crate::read_worktree_metadata(worktree)? {
        if metadata.tool != desired_tool {
            metadata.tool = desired_tool.to_string();
            let paths = crate::ProjectPaths::from_root(worktree);
            let content = serde_json::to_vec_pretty(&metadata).map_err(|error| {
                crate::MaccError::Validation(format!(
                    "Failed to serialize handoff metadata: {error}"
                ))
            })?;
            crate::atomic_write(&paths, &worktree.join(".macc/worktree.json"), &content)?;
        }
    }
    Ok(())
}

/// Returns `false` when the tool's `tool.json` explicitly sets
/// `performer.session.enabled` to `false`. Defaults to `true` when the field
/// is absent or unreadable so existing tools are unaffected.
fn tool_session_enabled(tool_json_path: &Path) -> bool {
    let raw = match std::fs::read_to_string(tool_json_path) {
        Ok(r) => r,
        Err(_) => return true,
    };
    let value: serde_json::Value = match serde_json::from_str(&raw) {
        Ok(v) => v,
        Err(_) => return true,
    };
    value
        .get("performer")
        .and_then(|p| p.get("session"))
        .and_then(|s| s.get("enabled"))
        .and_then(serde_json::Value::as_bool)
        .unwrap_or(true)
}

pub(super) use crate::coordinator::session_identity::read_session_id as read_session_id_from_state;

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
    let Some(session_id) =
        read_session_id_from_state(repo_root, tool_id, worktree_path, task_id, "performer")
    else {
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
        task_id: if task_id.is_empty() {
            None
        } else {
            Some(task_id.to_string())
        },
        event_type: event_type.to_string(),
        phase: if phase.is_empty() {
            None
        } else {
            Some(phase.to_string())
        },
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

/// Drain both pipes while waiting, so large tool output cannot stall exit.
/// Cancellation is owned by the phase job; never kill a PID loaded from state.
fn phase_output(
    command: &mut std::process::Command,
    cancelled: Option<&std::sync::atomic::AtomicBool>,
) -> std::io::Result<std::process::Output> {
    use std::io::Read;
    use std::process::Stdio;
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        command.process_group(0);
    }
    command
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .stdin(Stdio::null());
    let mut child = command.spawn()?;
    std::thread::scope(|scope| {
        let mut stdout = child.stdout.take().expect("piped stdout");
        let mut stderr = child.stderr.take().expect("piped stderr");
        let out = scope.spawn(move || {
            let mut bytes = Vec::new();
            stdout.read_to_end(&mut bytes).map(|_| bytes)
        });
        let err = scope.spawn(move || {
            let mut bytes = Vec::new();
            stderr.read_to_end(&mut bytes).map(|_| bytes)
        });
        let status = loop {
            if cancelled.is_some_and(|flag| flag.load(std::sync::atomic::Ordering::Acquire)) {
                #[cfg(unix)]
                unsafe {
                    libc::killpg(child.id() as libc::pid_t, libc::SIGKILL);
                }
                let _ = child.kill();
                break child.wait();
            }
            match child.try_wait() {
                Ok(Some(status)) => break Ok(status),
                Ok(None) => std::thread::sleep(std::time::Duration::from_millis(20)),
                Err(error) => {
                    #[cfg(unix)]
                    unsafe {
                        libc::killpg(child.id() as libc::pid_t, libc::SIGKILL);
                    }
                    let _ = child.kill();
                    let _ = child.wait();
                    break Err(error);
                }
            }
        }?;
        // A runner can exit while a descendant still holds an output pipe.
        // Keep cancellation responsive during pipe drain as well as wait().
        while !out.is_finished() || !err.is_finished() {
            if cancelled.is_some_and(|flag| flag.load(std::sync::atomic::Ordering::Acquire)) {
                #[cfg(unix)]
                unsafe {
                    libc::killpg(child.id() as libc::pid_t, libc::SIGKILL);
                }
            }
            std::thread::sleep(std::time::Duration::from_millis(20));
        }
        let stdout = out
            .join()
            .map_err(|_| std::io::Error::other("phase stdout reader panicked"))??;
        let stderr = err
            .join()
            .map_err(|_| std::io::Error::other("phase stderr reader panicked"))??;
        Ok(std::process::Output {
            status,
            stdout,
            stderr,
        })
    })
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
        let phase_tool = phase_tool_for_task(task, coordinator_tool_override)
            .unwrap_or_default()
            .to_string();
        if phase_tool.is_empty() {
            return Ok(Err(format!(
                "phase '{}' cannot run for task {}: missing coordinator tool",
                mode, task_id
            )));
        }
        if let Some(wait) =
            crate::coordinator::decisions::pending_for_task(self.repo_root, task_id)?
        {
            return Ok(Err(format!("E903: {}", wait.reason())));
        }
        let worktree = std::path::PathBuf::from(worktree_path);
        let review_head_before = if mode == "review" {
            Some(crate::git::head_commit(&worktree)?)
        } else {
            None
        };
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
        let mut prompt = coordinator_runtime::build_phase_prompt(mode, task_id, &phase_tool, task)?;
        let decision_attempt = self
            .prepared_context
            .map(|prepared| prepared.attempt.clone())
            .unwrap_or_else(|| uuid::Uuid::new_v4().to_string());
        let decision_context = if let Some(prepared) = self.prepared_context {
            prepared.context.clone()
        } else {
            match crate::coordinator::decisions::prepare_context(
                self.repo_root,
                task_id,
                mode,
                &worktree,
                &decision_attempt,
            ) {
                Ok(context) => context,
                Err(error)
                    if error
                        .to_string()
                        .contains(crate::coordinator::decisions::CONTEXT_BUDGET_CODE)
                        || error.to_string().contains("DM_PENDING:") =>
                {
                    return Ok(Err(format!("E903: {error}")))
                }
                Err(error) => return Err(error),
            }
        };
        if let Some((context, _)) = &decision_context {
            prompt.push_str(&context.render());
        }
        let prompt_dir = worktree.join(".macc").join("tmp");
        std::fs::create_dir_all(&prompt_dir).map_err(|e| MaccError::Io {
            path: prompt_dir.to_string_lossy().into(),
            action: "create coordinator phase prompt directory".into(),
            source: e,
        })?;
        let prompt = format!(
            "MACC_PROMPT_BEGIN: instructions-only\n{prompt}\nMACC_PROMPT_END: instructions-only\n"
        );
        let prompt_path = prompt_dir.join(format!(
            "coordinator-phase-{}-{}.prompt.txt",
            mode,
            task_id.replace('/', "-")
        ));
        std::fs::write(&prompt_path, &prompt).map_err(|e| MaccError::Io {
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
        // Skip entirely when the tool has session.enabled = false.
        let session_role = crate::coordinator::session_identity::role_for_phase(mode);
        let mut session_id = if tool_session_enabled(&tool_json) {
            read_session_id_from_state(
                self.repo_root,
                &phase_tool,
                &worktree,
                task_id,
                session_role,
            )
        } else {
            None
        };
        let mut last_reason = String::new();
        for attempt in 1..=attempts {
            if self
                .cancelled
                .is_some_and(|cancel| cancel.load(std::sync::atomic::Ordering::Acquire))
            {
                return Ok(Err("phase cancelled by coordinator".into()));
            }
            append_performer_log(
                &worktree,
                task_id,
                &format!("### Attempt {}/{}\n", attempt, attempts),
            );
            let mut command = std::process::Command::new(&runner_path);
            command.env_remove("MACC_WEB_ACCESS_TOKEN");
            if let Some((context, file)) = &decision_context {
                let mut store = crate::coordinator::decisions::Store::open(self.repo_root)?;
                store.issued(
                    &context.plan_id,
                    &decision_attempt,
                    mode,
                    &prompt,
                    &crate::coordinator::decisions::policy(self.repo_root)?,
                )?;
                command
                    .env("MACC_DECISION_ROOT", self.repo_root)
                    .env("MACC_DECISION_PLAN", &context.plan_id)
                    .env("MACC_DECISION_ATTEMPT", &decision_attempt)
                    .env("MACC_DECISION_PHASE", mode)
                    .env("MACC_DECISION_CONTEXT_FILE", file)
                    .env(
                        "MACC_DECISION_CLI",
                        std::env::current_exe()
                            .map_err(|e| MaccError::Validation(e.to_string()))?,
                    );
            }
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
                .env("MACC_SESSION_ROLE", session_role)
                .env("MACC_SESSION_PHASE", mode)
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
            let event_source = command
                .get_envs()
                .find(|(k, _)| *k == "MACC_EVENT_SOURCE")
                .and_then(|(_, v)| v)
                .map(|v| v.to_string_lossy().into_owned())
                .unwrap_or_default();
            let output = phase_output(&mut command, self.cancelled);
            if self
                .cancelled
                .is_some_and(|cancel| cancel.load(std::sync::atomic::Ordering::Acquire))
            {
                return Ok(Err("phase cancelled by coordinator".into()));
            }
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
            let mut evidence = crate::coordinator::phase_evidence::PhaseEvidence::capture(
                self.repo_root,
                task_id,
                mode,
                &decision_attempt,
                &event_source,
                &out,
                decision_context
                    .as_ref()
                    .map(|(c, _)| c.context_digest.as_str()),
            )?;
            if let Some(wait) = crate::session_store::decision_wait::waiting_for_invocation(
                self.repo_root,
                task_id,
                &event_source,
            )? {
                evidence.settle("waiting_decision", Some(&wait.reason))?;
                return Ok(Err(format!("E903: {}", wait.reason)));
            }
            if let Some(reason) =
                crate::session_store::checkpoints::precondition_reason(&combined_output)
            {
                evidence.settle("precondition_unmet", Some(&reason))?;
                return Ok(Err(reason));
            }
            if out.status.success() {
                if let Some((context, _)) = &decision_context {
                    let mut store = crate::coordinator::decisions::Store::open(self.repo_root)?;
                    let policy = crate::coordinator::decisions::policy(self.repo_root)?;
                    if let Err(error) = store.complete(
                        &context.plan_id,
                        &decision_attempt,
                        mode,
                        &combined_output,
                        &policy,
                    ) {
                        evidence.settle("validation_failed", Some(&error.to_string()))?;
                        if error.to_string().contains("DM_COMPLIANCE:") {
                            let diagnostic = store.diagnostic_delivery(
                                context,
                                &serde_json::to_value(task)
                                    .map_err(|e| crate::MaccError::Validation(e.to_string()))?,
                                &worktree,
                                review_head_before.as_deref().unwrap_or_default(),
                                &combined_output,
                            );
                            let diagnostic = match diagnostic {
                                Ok(value) => value,
                                Err(error) => {
                                    return Ok(Err(format!(
                                        "E103: Invalid diagnostic delivery: {error}"
                                    )))
                                }
                            };
                            if diagnostic.is_some() {
                                return Ok(Err("E903: DM_EXTERNAL_VERIFICATION: Native diagnostic report delivered; independent intervention verification is pending".into()));
                            }
                        }
                        return Ok(Err(error.to_string()));
                    }
                }
                evidence.settle(
                    if mode == "review"
                        && coordinator_runtime::parse_review_verdict(&combined_output)
                            == Some(coordinator_engine::ReviewVerdict::ChangesRequested)
                    {
                        "changes_requested"
                    } else {
                        "validated"
                    },
                    None,
                )?;
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
                    let elapsed = chrono::Utc::now()
                        .signed_duration_since(phase_started_at)
                        .num_seconds();
                    let (tool_model, tool_effort) = read_tool_meta_from_tool_json(&worktree);
                    let mut commit_builder = crate::commit_message::task_commit(
                        commit_type,
                        task_id,
                        task.title.as_deref(),
                        Some(mode),
                    )
                    .with_tool(&phase_tool)
                    .with_duration(format!("{}s", elapsed.max(0)));
                    if let Some(m) = tool_model {
                        commit_builder = commit_builder.with_model(m);
                    }
                    if let Some(e) = tool_effort {
                        commit_builder = commit_builder.with_effort(e);
                    }
                    let commit_msg = commit_builder.format();
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
                    &format!(
                        "Phase {} completed successfully (tool={})",
                        mode, phase_tool
                    ),
                    "info",
                );
                return Ok(Ok(combined_output));
            }
            evidence.settle("process_failed", Some(&out.status.to_string()))?;
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
            session_id = read_session_id_from_state(
                self.repo_root,
                &phase_tool,
                &worktree,
                task_id,
                session_role,
            );
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
    let executor = NativePhaseExecutor {
        repo_root,
        logger,
        cancelled: None,
        prepared_context: None,
    };
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
    let executor = NativePhaseExecutor {
        repo_root,
        logger,
        cancelled: None,
        prepared_context: None,
    };
    coordinator_runtime::run_review_phase(&executor, task, coordinator_tool_override, max_attempts)
}

#[cfg(all(test, unix))]
mod cancellation_tests {
    use super::phase_output;
    use std::{
        process::Command,
        sync::{
            atomic::{AtomicBool, Ordering},
            Arc,
        },
        time::Duration,
    };

    #[test]
    fn phase_process_cancellation_reaps_child_and_descendants() {
        let root = tempfile::tempdir().unwrap();
        let pids = root.path().join("pids");
        let cancelled = Arc::new(AtomicBool::new(false));
        let cancel = cancelled.clone();
        let pid_file = pids.clone();
        let worker = std::thread::spawn(move || {
            let mut command = Command::new("sh");
            command
                .arg("-c")
                .arg("sleep 30 & printf '%s %s' \"$$\" \"$!\" > \"$1\"; wait")
                .arg("phase-test")
                .arg(pid_file);
            phase_output(&mut command, Some(&cancel)).unwrap()
        });
        for _ in 0..100 {
            if pids.exists()
                && !std::fs::read_to_string(&pids)
                    .unwrap_or_default()
                    .is_empty()
            {
                break;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        let pids = std::fs::read_to_string(pids).unwrap();
        cancelled.store(true, Ordering::Release);
        let output = worker.join().unwrap();
        assert!(!output.status.success());
        let parent: i32 = pids.split_whitespace().next().unwrap().parse().unwrap();
        assert_eq!(unsafe { libc::kill(parent, 0) }, -1, "child must be reaped");
        let child: i32 = pids.split_whitespace().nth(1).unwrap().parse().unwrap();
        // A descendant can briefly remain a zombie until the system reaps it;
        // neither a live sleeper nor an output pipe may outlive cancellation.
        let stopped = || {
            let stat = std::fs::read_to_string(format!("/proc/{child}/stat")).unwrap_or_default();
            stat.is_empty() || stat.split_whitespace().nth(2) == Some("Z")
        };
        for _ in 0..100 {
            if stopped() {
                break;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        assert!(
            stopped(),
            "descendant must have received group cancellation"
        );
    }

    #[test]
    fn cancellation_also_interrupts_pipe_drain_after_the_runner_exits() {
        let root = tempfile::tempdir().unwrap();
        let parent = root.path().join("parent");
        let file = parent.clone();
        let cancelled = Arc::new(AtomicBool::new(false));
        let cancel = cancelled.clone();
        let worker = std::thread::spawn(move || {
            let mut command = Command::new("sh");
            command
                .arg("-c")
                .arg("sleep 30 & printf '%s' \"$$\" > \"$1\"; exit 0")
                .arg("phase-test")
                .arg(file);
            phase_output(&mut command, Some(&cancel)).unwrap()
        });
        for _ in 0..100 {
            let pid = std::fs::read_to_string(&parent)
                .ok()
                .and_then(|value| value.parse::<i32>().ok());
            if pid.is_some_and(|pid| unsafe { libc::kill(pid, 0) } == -1) {
                break;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        cancelled.store(true, Ordering::Release);
        let (done, rx) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            let _ = done.send(worker.join().unwrap());
        });
        rx.recv_timeout(Duration::from_secs(2))
            .expect("cancellation must interrupt inherited output pipes");
    }

    #[test]
    fn phase_output_drains_large_stdout_and_stderr_without_deadlock() {
        let mut command = Command::new("sh");
        command
            .arg("-c")
            .arg("head -c 131072 /dev/zero; head -c 131072 /dev/zero >&2; exit 7");
        let output = phase_output(&mut command, None).unwrap();
        assert_eq!(output.status.code(), Some(7));
        assert_eq!(output.stdout.len(), 131072);
        assert_eq!(output.stderr.len(), 131072);
    }
}
