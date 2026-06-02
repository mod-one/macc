use crate::config::CoordinatorConfig;
use crate::coordinator::managed_command_registry::{
    list_managed_commands, remove_managed_command, upsert_managed_command,
};
use crate::{ensure_embedded_automation_scripts, MaccError, ProjectPaths, Result};
#[cfg(unix)]
use libc;
use std::collections::HashMap;
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Mutex, OnceLock};
use std::time::Duration;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct CoordinatorProcessHandle(pub u64);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CoordinatorProcessPoll {
    Running,
    Exited { success: bool, code: Option<i32> },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CoordinatorStopResult {
    pub targets: usize,
    pub used_group: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CoordinatorManagedCommandPoll {
    Idle,
    Running {
        command: String,
        elapsed_secs: u64,
    },
    Exited {
        command: String,
        success: bool,
        code: Option<i32>,
        elapsed_secs: u64,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CoordinatorManagedCommandState {
    Idle,
    Running {
        command: String,
        elapsed_secs: u64,
    },
    Succeeded {
        command: String,
        elapsed_secs: u64,
        /// Human-readable reason when the coordinator stopped intentionally
        /// before all tasks completed (e.g. dispatch limit reached).
        finish_reason: Option<String>,
    },
    Failed {
        command: String,
        elapsed_secs: u64,
        reason: String,
        task_id: Option<String>,
        phase: Option<String>,
    },
}

struct ManagedCoordinatorProcess {
    child: Child,
}

fn process_table() -> &'static Mutex<HashMap<u64, ManagedCoordinatorProcess>> {
    static TABLE: OnceLock<Mutex<HashMap<u64, ManagedCoordinatorProcess>>> = OnceLock::new();
    TABLE.get_or_init(|| Mutex::new(HashMap::new()))
}

fn process_id_gen() -> &'static AtomicU64 {
    static ID: OnceLock<AtomicU64> = OnceLock::new();
    ID.get_or_init(|| AtomicU64::new(1))
}

fn local_handles_by_root() -> &'static Mutex<HashMap<String, CoordinatorProcessHandle>> {
    static TABLE: OnceLock<Mutex<HashMap<String, CoordinatorProcessHandle>>> = OnceLock::new();
    TABLE.get_or_init(|| Mutex::new(HashMap::new()))
}

fn handle_key(paths: &ProjectPaths, kind: &str) -> String {
    format!("{}::{kind}", paths.root.to_string_lossy())
}

fn active_managed_command(
    paths: &ProjectPaths,
) -> Result<Option<crate::coordinator::managed_command_registry::ManagedCommandRecord>> {
    Ok(list_managed_commands(paths)?.into_iter().next())
}

pub fn coordinator_start_managed_command_process(
    paths: &ProjectPaths,
    command: &str,
    args: &[String],
    cfg: Option<&CoordinatorConfig>,
) -> Result<()> {
    coordinator_start_managed_command_process_with_pid(paths, command, args, cfg).map(|_| ())
}

/// Like `coordinator_start_managed_command_process` but also returns the
/// coordinator child process ID so callers can pass it to the supervisor.
pub fn coordinator_start_managed_command_process_with_pid(
    paths: &ProjectPaths,
    command: &str,
    args: &[String],
    cfg: Option<&CoordinatorConfig>,
) -> Result<i32> {
    let key = handle_key(paths, command);
    if let Some(existing) = active_managed_command(paths)? {
        return Err(MaccError::Validation(format!(
            "coordinator command '{}' is already running for this project",
            existing.kind
        )));
    }

    let (handle, pid) = coordinator_start_command_process_with_pid(paths, command, args, cfg)?;
    upsert_managed_command(paths, command, pid)?;
    local_handles_by_root()
        .lock()
        .map_err(|_| MaccError::Validation("coordinator local handle table lock poisoned".into()))?
        .insert(key, handle);
    Ok(pid)
}

pub fn coordinator_poll_managed_command_process(
    paths: &ProjectPaths,
) -> Result<CoordinatorManagedCommandPoll> {
    let Some(record) = active_managed_command(paths)? else {
        return Ok(CoordinatorManagedCommandPoll::Idle);
    };
    let command = record.kind.clone();
    let elapsed_secs = record.elapsed_secs();
    let key = handle_key(paths, &record.kind);
    let local_handle = local_handles_by_root()
        .lock()
        .map_err(|_| MaccError::Validation("coordinator local handle table lock poisoned".into()))?
        .get(&key)
        .copied();

    if let Some(handle) = local_handle {
        match coordinator_poll_command_process(handle)? {
            CoordinatorProcessPoll::Running => {
                return Ok(CoordinatorManagedCommandPoll::Running {
                    command,
                    elapsed_secs,
                });
            }
            CoordinatorProcessPoll::Exited { success, code } => {
                let _ = remove_managed_command(paths, &record.kind)?;
                local_handles_by_root()
                    .lock()
                    .map_err(|_| {
                        MaccError::Validation("coordinator local handle table lock poisoned".into())
                    })?
                    .remove(&key);
                return Ok(CoordinatorManagedCommandPoll::Exited {
                    command,
                    success,
                    code,
                    elapsed_secs,
                });
            }
        }
    }

    if pid_is_alive(record.pid) {
        Ok(CoordinatorManagedCommandPoll::Running {
            command,
            elapsed_secs,
        })
    } else {
        let _ = remove_managed_command(paths, &record.kind)?;
        Ok(CoordinatorManagedCommandPoll::Exited {
            command,
            success: false,
            code: None,
            elapsed_secs,
        })
    }
}

pub fn coordinator_managed_command_state(
    paths: &ProjectPaths,
) -> Result<CoordinatorManagedCommandState> {
    coordinator_poll_managed_command_state(paths)
}

pub fn coordinator_poll_managed_command_state(
    paths: &ProjectPaths,
) -> Result<CoordinatorManagedCommandState> {
    match coordinator_poll_managed_command_process(paths)? {
        CoordinatorManagedCommandPoll::Idle => Ok(CoordinatorManagedCommandState::Idle),
        CoordinatorManagedCommandPoll::Running {
            command,
            elapsed_secs,
        } => Ok(CoordinatorManagedCommandState::Running {
            command,
            elapsed_secs,
        }),
        CoordinatorManagedCommandPoll::Exited {
            command,
            success,
            code,
            elapsed_secs,
        } => {
            if success {
                let finish_reason = read_dispatch_limit_reason(paths);
                return Ok(CoordinatorManagedCommandState::Succeeded {
                    command,
                    elapsed_secs,
                    finish_reason,
                });
            }
            let failure = crate::service::diagnostic::analyze_last_failure(paths)?;
            let reason = failure
                .as_ref()
                .map(|f| f.message.clone())
                .unwrap_or_else(|| {
                    format!(
                        "Coordinator '{}' failed ({})",
                        command,
                        code.map(|v| format!("exit status: {}", v))
                            .unwrap_or_else(|| "unknown exit status".to_string())
                    )
                });
            Ok(CoordinatorManagedCommandState::Failed {
                command,
                elapsed_secs,
                reason,
                task_id: failure.as_ref().and_then(|f| f.task_id.clone()),
                phase: failure.as_ref().and_then(|f| f.phase.clone()),
            })
        }
    }
}

pub fn coordinator_stop_managed_command_process(
    paths: &ProjectPaths,
    graceful: bool,
) -> Result<CoordinatorStopResult> {
    let Some(record) = active_managed_command(paths)? else {
        return Ok(CoordinatorStopResult {
            targets: 0,
            used_group: false,
        });
    };
    let key = handle_key(paths, &record.kind);
    let local_handle = local_handles_by_root()
        .lock()
        .map_err(|_| MaccError::Validation("coordinator local handle table lock poisoned".into()))?
        .remove(&key);
    let Some(record) = remove_managed_command(paths, &record.kind)? else {
        return Ok(CoordinatorStopResult {
            targets: 0,
            used_group: false,
        });
    };
    if let Some(handle) = local_handle {
        coordinator_stop_command_process(handle, graceful)
    } else {
        let (targets, used_group) = stop_coordinator_process_group_or_tree(record.pid)?;
        Ok(CoordinatorStopResult {
            targets,
            used_group,
        })
    }
}

pub fn coordinator_start_command_process(
    paths: &ProjectPaths,
    command: &str,
    args: &[String],
    _cfg: Option<&CoordinatorConfig>,
) -> Result<CoordinatorProcessHandle> {
    coordinator_start_command_process_with_pid(paths, command, args, _cfg).map(|(handle, _)| handle)
}

fn coordinator_start_command_process_with_pid(
    paths: &ProjectPaths,
    command: &str,
    args: &[String],
    _cfg: Option<&CoordinatorConfig>,
) -> Result<(CoordinatorProcessHandle, i32)> {
    let root = &paths.root;
    let mut cmd = if command == "run" {
        let current_exe = std::env::current_exe().map_err(|e| MaccError::Io {
            path: root.to_string_lossy().into(),
            action: "resolve current executable for coordinator command".into(),
            source: e,
        })?;
        let mut cmd = Command::new(current_exe);
        cmd.current_dir(root)
            .arg("--cwd")
            .arg(root)
            .arg("coordinator")
            .arg("control-plane-run")
            .arg("--no-tui")
            .args(args);
        cmd
    } else {
        ensure_embedded_automation_scripts(paths)?;
        let script = paths.automation_coordinator_path();
        if !script.exists() {
            return Err(MaccError::Validation(format!(
                "coordinator script not found: {}",
                script.display()
            )));
        }
        let mut cmd = Command::new(script);
        cmd.current_dir(root)
            .arg(command)
            .args(args)
            .env("REPO_DIR", root);
        cmd
    };

    cmd.env("MACC_INTERNAL_INVOCATION", "1")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());

    // Detach from the controlling terminal so the coordinator survives SSH
    // session close.  setsid(2) creates a new session with no controlling
    // terminal; the child becomes the session leader and is no longer in the
    // parent's process group, so SIGHUP on terminal close never reaches it.
    // This applies to all coordinator commands (control-plane-run, dispatch,
    // sync, …) — performers and merge workers inherit the same independence
    // because they are spawned by this child.
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        // SAFETY: setsid(2) is async-signal-safe. The restriction is that the
        // calling process must not be a process-group leader; a freshly forked
        // child always satisfies this.
        unsafe {
            cmd.pre_exec(|| {
                libc::setsid();
                Ok(())
            });
        }
    }

    let child = cmd.spawn().map_err(|e| MaccError::Io {
        path: root.to_string_lossy().into(),
        action: format!("spawn coordinator command '{}'", command),
        source: e,
    })?;
    let pid = child.id() as i32;

    let id = process_id_gen().fetch_add(1, Ordering::Relaxed);
    let handle = CoordinatorProcessHandle(id);
    let mut table = process_table()
        .lock()
        .map_err(|_| MaccError::Validation("coordinator process table lock poisoned".into()))?;
    table.insert(id, ManagedCoordinatorProcess { child });
    Ok((handle, pid))
}

pub fn coordinator_poll_command_process(
    handle: CoordinatorProcessHandle,
) -> Result<CoordinatorProcessPoll> {
    let mut table = process_table()
        .lock()
        .map_err(|_| MaccError::Validation("coordinator process table lock poisoned".into()))?;
    let Some(proc_state) = table.get_mut(&handle.0) else {
        return Ok(CoordinatorProcessPoll::Exited {
            success: false,
            code: None,
        });
    };
    let status = proc_state.child.try_wait().map_err(|e| MaccError::Io {
        path: "<process>".into(),
        action: "poll coordinator process".into(),
        source: e,
    })?;
    match status {
        Some(status) => {
            table.remove(&handle.0);
            Ok(CoordinatorProcessPoll::Exited {
                success: status.success(),
                code: status.code(),
            })
        }
        None => Ok(CoordinatorProcessPoll::Running),
    }
}

pub fn coordinator_stop_command_process(
    handle: CoordinatorProcessHandle,
    _graceful: bool,
) -> Result<CoordinatorStopResult> {
    let mut table = process_table()
        .lock()
        .map_err(|_| MaccError::Validation("coordinator process table lock poisoned".into()))?;
    let Some(mut proc_state) = table.remove(&handle.0) else {
        return Ok(CoordinatorStopResult {
            targets: 0,
            used_group: false,
        });
    };
    let coordinator_pid = proc_state.child.id() as i32;
    let (count, used_group) = stop_coordinator_process_group_or_tree(coordinator_pid)?;
    let _ = proc_state.child.kill();
    let _ = proc_state.child.wait();
    Ok(CoordinatorStopResult {
        targets: count,
        used_group,
    })
}

fn stop_coordinator_process_group_or_tree(pid: i32) -> Result<(usize, bool)> {
    let current_pgid = pgid_for_pid(std::process::id() as i32).unwrap_or(-1);
    if let Some(target_pgid) = pgid_for_pid(pid) {
        if target_pgid > 0 && target_pgid != current_pgid {
            let _ = signal_process_group(target_pgid, "-TERM");
            std::thread::sleep(Duration::from_millis(800));
            if pgid_is_alive(target_pgid) {
                let _ = signal_process_group(target_pgid, "-KILL");
            }
            return Ok((1, true));
        }
    }

    let descendants = collect_descendant_pids(pid);
    let mut targets = descendants;
    targets.push(pid);
    targets.sort_unstable();
    targets.dedup();

    let mut signaled = 0usize;
    for target in &targets {
        if signal_pid(*target, "-TERM") {
            signaled += 1;
        }
    }

    for _ in 0..20 {
        if targets.iter().all(|target| !pid_is_alive(*target)) {
            break;
        }
        std::thread::sleep(Duration::from_millis(120));
    }

    for target in &targets {
        if pid_is_alive(*target) {
            let _ = signal_pid(*target, "-KILL");
        }
    }
    Ok((signaled, false))
}

fn collect_descendant_pids(root_pid: i32) -> Vec<i32> {
    let mut stack = vec![root_pid];
    let mut seen = std::collections::HashSet::new();
    let mut out = Vec::new();

    while let Some(pid) = stack.pop() {
        for child in child_pids(pid) {
            if !seen.insert(child) {
                continue;
            }
            out.push(child);
            stack.push(child);
        }
    }
    out
}

fn child_pids(pid: i32) -> Vec<i32> {
    let output = Command::new("pgrep")
        .arg("-P")
        .arg(pid.to_string())
        .output();
    let Ok(output) = output else {
        return Vec::new();
    };
    if !output.status.success() {
        return Vec::new();
    }
    String::from_utf8_lossy(&output.stdout)
        .lines()
        .filter_map(|line| line.trim().parse::<i32>().ok())
        .collect()
}

fn pgid_for_pid(pid: i32) -> Option<i32> {
    let output = Command::new("ps")
        .arg("-o")
        .arg("pgid=")
        .arg("-p")
        .arg(pid.to_string())
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    String::from_utf8_lossy(&output.stdout)
        .trim()
        .parse::<i32>()
        .ok()
}

fn signal_process_group(pgid: i32, signal: &str) -> bool {
    if pgid <= 0 {
        return false;
    }
    Command::new("kill")
        .arg(signal)
        .arg(format!("-{}", pgid))
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

fn pgid_is_alive(pgid: i32) -> bool {
    if pgid <= 0 {
        return false;
    }
    Command::new("kill")
        .arg("-0")
        .arg(format!("-{}", pgid))
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

fn signal_pid(pid: i32, signal: &str) -> bool {
    if pid <= 0 {
        return false;
    }
    Command::new("kill")
        .arg(signal)
        .arg(pid.to_string())
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

fn pid_is_alive(pid: i32) -> bool {
    if pid <= 0 {
        return false;
    }
    let proc_cwd = PathBuf::from(format!("/proc/{}/cwd", pid));
    if !proc_cwd.exists() {
        return false;
    }
    Command::new("kill")
        .arg("-0")
        .arg(pid.to_string())
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

/// Check whether the most recent coordinator run ended because the dispatch
/// limit was reached. Returns a user-friendly message if so, or `None` for
/// a normal full-completion.
fn read_dispatch_limit_reason(paths: &ProjectPaths) -> Option<String> {
    use crate::coordinator_storage::{
        CoordinatorSnapshot, CoordinatorStorage, CoordinatorStoragePaths, JsonStorage, SqliteStorage,
    };
    let storage_paths = CoordinatorStoragePaths::from_project_paths(paths);
    let sqlite = SqliteStorage::new(storage_paths.clone());
    let snapshot: CoordinatorSnapshot = if sqlite.has_snapshot_data().unwrap_or(false) {
        sqlite.load_snapshot().ok()?
    } else {
        JsonStorage::new(storage_paths).load_snapshot().ok()?
    };
    // Scan the last 20 events newest-first for the dispatch_limit_reached marker.
    for event in snapshot.events.iter().rev().take(20) {
        if event.event_type == "dispatch_limit_reached" {
            let detail = event.message().unwrap_or("").to_string();
            let dispatched = detail.split_whitespace().find_map(|s| {
                s.strip_prefix("run_total=")
                    .and_then(|v| v.parse::<usize>().ok())
            });
            let max = detail.split_whitespace().find_map(|s| {
                s.strip_prefix("max_dispatch=")
                    .and_then(|v| v.parse::<usize>().ok())
            });
            return Some(match (dispatched, max) {
                (Some(d), Some(m)) => format!(
                    "Stopped: dispatch limit reached ({}/{} tasks dispatched). \
                     Restart the coordinator to continue.",
                    d, m
                ),
                _ => "Stopped: dispatch limit reached. Restart the coordinator to continue."
                    .to_string(),
            });
        }
    }
    None
}
