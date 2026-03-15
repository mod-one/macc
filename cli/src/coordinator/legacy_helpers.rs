#[cfg(test)]
use macc_core::coordinator::types::CoordinatorEnvConfig;
#[cfg(test)]
use macc_core::coordinator::{
    args::{RuntimeTransitionArgs, WorkflowTransitionArgs},
    is_valid_runtime_transition, is_valid_workflow_transition,
};
use macc_core::{MaccError, Result};

#[cfg(test)]
pub(crate) const COORDINATOR_TASK_REGISTRY_REL_PATH: &str =
    macc_core::coordinator::COORDINATOR_TASK_REGISTRY_REL_PATH;

pub(crate) struct NativeCoordinatorLogger {
    pub(crate) file: std::path::PathBuf,
    state: std::sync::Mutex<NativeCoordinatorLoggerState>,
    flush_every_lines: usize,
    flush_every_interval: std::time::Duration,
}

struct NativeCoordinatorLoggerState {
    writer: std::io::BufWriter<std::fs::File>,
    pending_lines: usize,
    last_flush: std::time::Instant,
}

impl NativeCoordinatorLogger {
    pub(crate) fn new_with_flush(
        repo_root: &std::path::Path,
        action: &str,
        flush_lines_override: Option<usize>,
        flush_ms_override: Option<u64>,
    ) -> Result<Self> {
        let dir = repo_root.join(".macc").join("log").join("coordinator");
        std::fs::create_dir_all(&dir).map_err(|e| MaccError::Io {
            path: dir.to_string_lossy().into(),
            action: "create coordinator log dir".into(),
            source: e,
        })?;
        let ts = chrono::Utc::now().format("%Y%m%dT%H%M%SZ");
        let file = dir.join(format!("{}-{}.md", action, ts));
        let header = format!(
            "# Coordinator log\n\n- Command: {}\n- Repository: {}\n- Started (UTC): {}\n\n",
            action,
            repo_root.display(),
            chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Secs, true)
        );
        std::fs::write(&file, header).map_err(|e| MaccError::Io {
            path: file.to_string_lossy().into(),
            action: "write coordinator log header".into(),
            source: e,
        })?;
        let file_handle = std::fs::OpenOptions::new()
            .append(true)
            .open(&file)
            .map_err(|e| MaccError::Io {
                path: file.to_string_lossy().into(),
                action: "open coordinator log writer".into(),
                source: e,
            })?;
        let flush_every_lines = flush_lines_override
            .filter(|v| *v > 0)
            .unwrap_or(500);
        let flush_every_interval = std::time::Duration::from_millis(
            flush_ms_override
                .filter(|v| *v > 0)
                .unwrap_or(60_000),
        );

        Ok(Self {
            file,
            state: std::sync::Mutex::new(NativeCoordinatorLoggerState {
                writer: std::io::BufWriter::new(file_handle),
                pending_lines: 0,
                last_flush: std::time::Instant::now(),
            }),
            flush_every_lines,
            flush_every_interval,
        })
    }

    pub(crate) fn note(&self, msg: impl AsRef<str>) -> Result<()> {
        use std::io::Write as _;
        let line = format!("{}\n", msg.as_ref());
        tracing::info!(target: "macc.coordinator.log", "{}", msg.as_ref());
        let mut state = self
            .state
            .lock()
            .map_err(|_| MaccError::Validation("Coordinator logger lock poisoned".to_string()))?;
        state
            .writer
            .write_all(line.as_bytes())
            .map_err(|e| MaccError::Io {
                path: self.file.to_string_lossy().into(),
                action: "append coordinator log".into(),
                source: e,
            })?;
        state.pending_lines += 1;
        let should_flush = state.pending_lines >= self.flush_every_lines
            || state.last_flush.elapsed() >= self.flush_every_interval;
        if should_flush {
            state.writer.flush().map_err(|e| MaccError::Io {
                path: self.file.to_string_lossy().into(),
                action: "flush coordinator log".into(),
                source: e,
            })?;
            state.pending_lines = 0;
            state.last_flush = std::time::Instant::now();
        }
        Ok(())
    }
}

impl Drop for NativeCoordinatorLogger {
    fn drop(&mut self) {
        use std::io::Write as _;
        if let Ok(mut state) = self.state.lock() {
            let _ = state.writer.flush();
        }
    }
}

#[cfg(test)]
pub(crate) fn validate_coordinator_transition_action(args: &[String]) -> Result<()> {
    let parsed = WorkflowTransitionArgs::try_from(args)?;
    let from = parsed.from;
    let to = parsed.to;
    if is_valid_workflow_transition(from, to) {
        return Ok(());
    }
    Err(MaccError::Validation(format!(
        "invalid transition {} -> {}",
        from.as_str(),
        to.as_str()
    )))
}

#[cfg(test)]
pub(crate) fn validate_coordinator_runtime_transition_action(args: &[String]) -> Result<()> {
    let parsed = RuntimeTransitionArgs::try_from(args)?;
    let from = parsed.from;
    let to = parsed.to;
    if is_valid_runtime_transition(from, to) {
        return Ok(());
    }
    Err(MaccError::Validation(format!(
        "invalid runtime transition {} -> {}",
        from.as_str(),
        to.as_str()
    )))
}

pub(crate) fn stop_coordinator_process_groups(
    repo_root: &std::path::Path,
    coordinator_path: &std::path::Path,
    graceful: bool,
) -> Result<usize> {
    let repo = repo_root
        .canonicalize()
        .unwrap_or_else(|_| repo_root.to_path_buf());
    let mut pids = pgrep_pids(&coordinator_path.to_string_lossy())?;
    if pids.is_empty() {
        pids = pgrep_pids("coordinator.sh")?;
    }

    let mut pgids = std::collections::BTreeSet::new();
    for pid in pids {
        if pid == std::process::id() as i32 {
            continue;
        }
        if !pid_in_repo(pid, &repo) {
            continue;
        }
        if let Some(pgid) = get_pgid(pid)? {
            pgids.insert(pgid);
        }
    }

    for pgid in &pgids {
        signal_process_group(*pgid, "-TERM")?;
    }
    if !pgids.is_empty() {
        std::thread::sleep(std::time::Duration::from_secs(1));
    }

    if !graceful {
        for _ in 0..20 {
            if pgids.iter().all(|pgid| !pgid_is_alive(*pgid)) {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(250));
        }
        for pgid in &pgids {
            if pgid_is_alive(*pgid) {
                signal_process_group(*pgid, "-KILL")?;
            }
        }
    }

    Ok(pgids.len())
}

fn pgrep_pids(pattern: &str) -> Result<Vec<i32>> {
    let output = std::process::Command::new("pgrep")
        .arg("-f")
        .arg(pattern)
        .output()
        .map_err(|e| MaccError::Io {
            path: "pgrep".into(),
            action: "find coordinator processes".into(),
            source: e,
        })?;
    if !output.status.success() {
        return Ok(Vec::new());
    }
    let text = String::from_utf8_lossy(&output.stdout);
    Ok(text
        .lines()
        .filter_map(|line| line.trim().parse::<i32>().ok())
        .collect())
}

fn pid_in_repo(pid: i32, repo_root: &std::path::Path) -> bool {
    let proc_cwd = std::path::PathBuf::from(format!("/proc/{}/cwd", pid));
    let Ok(cwd) = std::fs::read_link(proc_cwd) else {
        return false;
    };
    let cwd = cwd.canonicalize().unwrap_or(cwd);
    cwd.starts_with(repo_root)
}

fn get_pgid(pid: i32) -> Result<Option<i32>> {
    let output = std::process::Command::new("ps")
        .arg("-o")
        .arg("pgid=")
        .arg("-p")
        .arg(pid.to_string())
        .output()
        .map_err(|e| MaccError::Io {
            path: "ps".into(),
            action: "read process group".into(),
            source: e,
        })?;
    if !output.status.success() {
        return Ok(None);
    }
    let value = String::from_utf8_lossy(&output.stdout).trim().to_string();
    Ok(value.parse::<i32>().ok())
}

fn signal_process_group(pgid: i32, signal: &str) -> Result<()> {
    let target = format!("-{}", pgid);
    let status = std::process::Command::new("kill")
        .arg(signal)
        .arg(target)
        .status()
        .map_err(|e| MaccError::Io {
            path: "kill".into(),
            action: format!("send {} to process group", signal),
            source: e,
        })?;
    let _ = status;
    Ok(())
}

fn pgid_is_alive(pgid: i32) -> bool {
    let target = format!("-{}", pgid);
    std::process::Command::new("kill")
        .arg("-0")
        .arg(target)
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

#[cfg(test)]
fn apply_coordinator_env(
    command: &mut std::process::Command,
    canonical: &macc_core::config::CanonicalConfig,
    coordinator: Option<&macc_core::config::CoordinatorConfig>,
    env_cfg: &CoordinatorEnvConfig,
) {
    for (key, value) in coordinator_env_pairs(canonical, coordinator, env_cfg) {
        command.env(key, value);
    }
}

#[cfg(test)]
fn coordinator_env_pairs(
    canonical: &macc_core::config::CanonicalConfig,
    coordinator: Option<&macc_core::config::CoordinatorConfig>,
    env_cfg: &CoordinatorEnvConfig,
) -> Vec<(String, String)> {
    let mut out = Vec::new();
    out.push((
        "ENABLED_TOOLS_CSV".to_string(),
        canonical.tools.enabled.join(","),
    ));
    out.push((
        "TASK_REGISTRY_FILE".to_string(),
        COORDINATOR_TASK_REGISTRY_REL_PATH.to_string(),
    ));

    if let Some(value) = env_cfg
        .prd
        .clone()
        .or_else(|| coordinator.and_then(|c| c.prd_file.clone()))
    {
        out.push(("PRD_FILE".to_string(), value));
    }
    if let Some(value) = env_cfg
        .coordinator_tool
        .clone()
        .or_else(|| coordinator.and_then(|c| c.coordinator_tool.clone()))
    {
        out.push(("COORDINATOR_TOOL".to_string(), value));
    }
    if let Some(value) = env_cfg
        .reference_branch
        .clone()
        .or_else(|| coordinator.and_then(|c| c.reference_branch.clone()))
    {
        out.push(("DEFAULT_BASE_BRANCH".to_string(), value));
    }
    if let Some(value) = env_cfg.tool_priority.clone().or_else(|| {
        coordinator.and_then(|c| {
            if c.tool_priority.is_empty() {
                None
            } else {
                Some(c.tool_priority.join(","))
            }
        })
    }) {
        out.push(("TOOL_PRIORITY_CSV".to_string(), value));
    }
    if let Some(value) = env_cfg.max_parallel_per_tool_json.clone().or_else(|| {
        coordinator.and_then(|c| {
            if c.max_parallel_per_tool.is_empty() {
                None
            } else {
                serde_json::to_string(&c.max_parallel_per_tool).ok()
            }
        })
    }) {
        out.push(("MAX_PARALLEL_PER_TOOL_JSON".to_string(), value));
    }
    if let Some(value) = env_cfg.tool_specializations_json.clone().or_else(|| {
        coordinator.and_then(|c| {
            if c.tool_specializations.is_empty() {
                None
            } else {
                serde_json::to_string(&c.tool_specializations).ok()
            }
        })
    }) {
        out.push(("TOOL_SPECIALIZATIONS_JSON".to_string(), value));
    }
    if let Some(value) = env_cfg
        .max_dispatch
        .or_else(|| coordinator.and_then(|c| c.max_dispatch))
    {
        out.push(("MAX_DISPATCH".to_string(), value.to_string()));
    }
    if let Some(value) = env_cfg
        .max_parallel
        .or_else(|| coordinator.and_then(|c| c.max_parallel))
    {
        out.push(("MAX_PARALLEL".to_string(), value.to_string()));
    }
    if let Some(value) = env_cfg
        .timeout_seconds
        .or_else(|| coordinator.and_then(|c| c.timeout_seconds))
    {
        out.push(("TIMEOUT_SECONDS".to_string(), value.to_string()));
    }
    if let Some(value) = env_cfg
        .phase_runner_max_attempts
        .or_else(|| coordinator.and_then(|c| c.phase_runner_max_attempts))
    {
        out.push(("PHASE_RUNNER_MAX_ATTEMPTS".to_string(), value.to_string()));
    }
    if let Some(value) = env_cfg
        .stale_claimed_seconds
        .or_else(|| coordinator.and_then(|c| c.stale_claimed_seconds))
    {
        out.push(("STALE_CLAIMED_SECONDS".to_string(), value.to_string()));
    }
    if let Some(value) = env_cfg
        .stale_in_progress_seconds
        .or_else(|| coordinator.and_then(|c| c.stale_in_progress_seconds))
    {
        out.push(("STALE_IN_PROGRESS_SECONDS".to_string(), value.to_string()));
    }
    if let Some(value) = env_cfg
        .stale_changes_requested_seconds
        .or_else(|| coordinator.and_then(|c| c.stale_changes_requested_seconds))
    {
        out.push((
            "STALE_CHANGES_REQUESTED_SECONDS".to_string(),
            value.to_string(),
        ));
    }
    if let Some(value) = env_cfg
        .stale_action
        .clone()
        .or_else(|| coordinator.and_then(|c| c.stale_action.clone()))
    {
        out.push(("STALE_ACTION".to_string(), value));
    }
    if let Some(value) = env_cfg
        .storage_mode
        .clone()
        .or_else(|| coordinator.and_then(|c| c.storage_mode.clone()))
    {
        out.push(("COORDINATOR_STORAGE_MODE".to_string(), value));
    }
    if let Some(value) = env_cfg
        .mirror_json_debounce_ms
        .or_else(|| coordinator.and_then(|c| c.mirror_json_debounce_ms))
    {
        out.push((
            "COORDINATOR_JSON_EXPORT_DEBOUNCE_MS".to_string(),
            value.to_string(),
        ));
    }
    out
}

#[cfg(test)]
pub(crate) fn run_coordinator_command(
    repo_root: &std::path::Path,
    coordinator_path: &std::path::Path,
    command_name: &str,
    extra_args: &[String],
    canonical: &macc_core::config::CanonicalConfig,
    coordinator: Option<&macc_core::config::CoordinatorConfig>,
    env_cfg: &CoordinatorEnvConfig,
) -> Result<()> {
    run_coordinator_command_with_options(
        repo_root,
        coordinator_path,
        command_name,
        extra_args,
        canonical,
        coordinator,
        env_cfg,
        false,
    )
}

#[cfg(test)]
fn run_coordinator_command_with_options(
    repo_root: &std::path::Path,
    coordinator_path: &std::path::Path,
    command_name: &str,
    extra_args: &[String],
    canonical: &macc_core::config::CanonicalConfig,
    coordinator: Option<&macc_core::config::CoordinatorConfig>,
    env_cfg: &CoordinatorEnvConfig,
    skip_storage_sync: bool,
) -> Result<()> {
    let mut command = std::process::Command::new(coordinator_path);
    command.current_dir(repo_root);
    command.arg(command_name);
    command.args(extra_args);
    apply_coordinator_env(&mut command, canonical, coordinator, env_cfg);
    if skip_storage_sync {
        command.env("COORDINATOR_SKIP_STORAGE_SYNC", "1");
    }

    let status = command.status().map_err(|e| MaccError::Io {
        path: coordinator_path.to_string_lossy().into(),
        action: format!("run coordinator command '{}'", command_name),
        source: e,
    })?;
    if !status.success() {
        let hint = coordinator_command_hint(command_name);
        return Err(MaccError::Validation(format!(
            "Coordinator '{}' failed with status: {}. {}",
            command_name, status, hint
        )));
    }
    if let Err(err) = macc_core::coordinator::logs::aggregate_performer_logs(repo_root) {
        tracing::warn!("failed to aggregate performer logs: {}", err);
    }
    Ok(())
}

#[cfg(test)]
fn coordinator_command_hint(command_name: &str) -> &'static str {
    match command_name {
        "dispatch" => {
            "Run `macc coordinator status` and inspect logs with `macc logs tail --component coordinator`."
        }
        "advance" => {
            "Run `macc coordinator reconcile`, then `macc coordinator unlock --all` if tasks are stuck."
        }
        "reconcile" | "cleanup" => {
            "Run `macc worktree prune` and retry; if locks remain, run `macc coordinator unlock --all`."
        }
        "run" => {
            "Run `macc coordinator status`, then inspect events with `macc logs tail --component coordinator`."
        }
        "retry-phase" => {
            "Verify task/worktree consistency with `macc coordinator status` and inspect errors in `macc logs tail --component coordinator`."
        }
        "resume" => {
            "After fixing merge conflicts manually, run `macc coordinator run` to continue orchestration."
        }
        "cutover-gate" => {
            "Inspect cutover metrics in .macc/log/coordinator/events.jsonl and rerun `macc coordinator cutover-gate`."
        }
        "unlock" => {
            "Inspect lock owners in .macc/automation/task/task_registry.json then retry dispatch."
        }
        "sync" => "Check PRD/registry JSON validity and rerun `macc coordinator sync`.",
        _ => "Inspect logs with `macc logs tail --component coordinator`.",
    }
}

#[cfg(test)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct RegistryCounts {
    pub(crate) total: usize,
    pub(crate) todo: usize,
    pub(crate) active: usize,
    pub(crate) blocked: usize,
    pub(crate) merged: usize,
}

#[cfg(test)]
pub(crate) fn read_registry_counts(path: &std::path::Path) -> Result<RegistryCounts> {
    let content = std::fs::read_to_string(path).map_err(|e| MaccError::Io {
        path: path.to_string_lossy().into(),
        action: "read task registry".into(),
        source: e,
    })?;
    let root: serde_json::Value = serde_json::from_str(&content).map_err(|e| {
        MaccError::Validation(format!(
            "Failed to parse task registry JSON '{}': {}",
            path.display(),
            e
        ))
    })?;
    let tasks = root
        .get("tasks")
        .and_then(|v| v.as_array())
        .ok_or_else(|| MaccError::Validation("Task registry missing 'tasks' array".into()))?;

    let mut counts = RegistryCounts {
        total: tasks.len(),
        todo: 0,
        active: 0,
        blocked: 0,
        merged: 0,
    };

    for task in tasks {
        let state = task
            .get("state")
            .and_then(|v| v.as_str())
            .unwrap_or("todo")
            .to_ascii_lowercase();
        match state.as_str() {
            "todo" => counts.todo += 1,
            "claimed" | "in_progress" | "pr_open" | "changes_requested" | "queued" => {
                counts.active += 1
            }
            "blocked" => counts.blocked += 1,
            "merged" => counts.merged += 1,
            _ => {}
        }
    }
    Ok(counts)
}
