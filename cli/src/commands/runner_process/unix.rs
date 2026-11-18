//! Synchronous dedicated supervisor: it is the only waiter for its children.
//! Do not move this into the long-lived/multithreaded coordinator: subreaping
//! and signal dispositions are process-wide.
#[cfg(target_os = "linux")]
use std::collections::HashSet;
use std::io;
use std::os::unix::process::{CommandExt, ExitStatusExt};
use std::process::{Command, ExitStatus};
use std::sync::atomic::{AtomicI32, Ordering};
use std::time::{Duration, Instant};

static SIGNAL: AtomicI32 = AtomicI32::new(0);
const GRACE: Duration = Duration::from_secs(2);

extern "C" fn interrupted(signal: libc::c_int) {
    SIGNAL.store(signal, Ordering::Relaxed);
}

pub(super) fn supervise(mut command: Command, owner_pid: Option<u32>) -> io::Result<i32> {
    // The CLI is a fresh, single-threaded process at this entry point.
    let parent = owner_pid
        .map(|pid| pid as libc::pid_t)
        .unwrap_or_else(|| unsafe { libc::getppid() });
    // A forced kill of the outer runner's group must not kill this lifetime
    // boundary before it has reaped detached tools. Parent loss below triggers
    // the same drain even when the shell cannot execute its EXIT trap.
    if unsafe { libc::setsid() } == -1 {
        return Err(io::Error::last_os_error());
    }
    #[cfg(target_os = "linux")]
    if unsafe { libc::prctl(libc::PR_SET_CHILD_SUBREAPER, 1, 0, 0, 0) } != 0 {
        return Err(io::Error::last_os_error());
    }
    for signal in [libc::SIGTERM, libc::SIGINT, libc::SIGHUP] {
        let mut action: libc::sigaction = unsafe { std::mem::zeroed() };
        action.sa_sigaction = interrupted as *const () as usize;
        unsafe { libc::sigemptyset(&mut action.sa_mask) };
        if unsafe { libc::sigaction(signal, &action, std::ptr::null_mut()) } != 0 {
            return Err(io::Error::last_os_error());
        }
    }
    command.process_group(0);
    let child = command.spawn()?;
    let provider = child.id() as libc::pid_t;
    // waitpid below owns reaping, including adopted descendants. Dropping Child
    // does not reap it and leaves all three stdio descriptors inherited.
    drop(child);
    let mut status = None;
    let mut stopping = None;
    #[cfg(target_os = "linux")]
    let mut signalled = HashSet::new();
    loop {
        if unsafe { libc::getppid() } != parent {
            let _ = SIGNAL.compare_exchange(0, libc::SIGHUP, Ordering::Relaxed, Ordering::Relaxed);
        }
        let no_children = reap(provider, &mut status)?;
        let signal = SIGNAL.load(Ordering::Relaxed);
        if status.is_some() || signal != 0 {
            let since = *stopping.get_or_insert_with(Instant::now);
            if no_children {
                #[cfg(not(target_os = "linux"))]
                unsafe {
                    libc::killpg(provider, libc::SIGKILL);
                }
                // Receipt/output reconciliation remains the caller's job.
                return Ok(if signal != 0 {
                    128 + signal
                } else {
                    let status = status.expect("provider exit observed");
                    status
                        .code()
                        .unwrap_or_else(|| 128 + status.signal().unwrap_or(1))
                });
            }
            // Never return while owned children remain: no checkpoint may
            // declare an idle worker that can still receive late writes.
            let force = since.elapsed() >= GRACE;
            #[cfg(target_os = "linux")]
            for pid in children()? {
                // These are unreaped children of THIS supervisor, so their PIDs
                // cannot be reused by another run. New adopted children are
                // discovered after each parent exits, regardless of its PGID.
                if force || signalled.insert(pid) {
                    unsafe {
                        libc::kill(pid, if force { libc::SIGKILL } else { libc::SIGTERM });
                    }
                }
            }
            #[cfg(not(target_os = "linux"))]
            unsafe {
                libc::killpg(provider, if force { libc::SIGKILL } else { libc::SIGTERM });
            }
        }
        std::thread::sleep(Duration::from_millis(10));
    }
}

fn reap(provider: libc::pid_t, status: &mut Option<ExitStatus>) -> io::Result<bool> {
    loop {
        let mut raw = 0;
        let pid = unsafe { libc::waitpid(-1, &mut raw, libc::WNOHANG) };
        if pid == provider {
            *status = Some(ExitStatus::from_raw(raw));
        }
        if pid > 0 {
            continue;
        }
        if pid == 0 {
            return Ok(false);
        }
        let error = io::Error::last_os_error();
        match error.raw_os_error() {
            Some(libc::ECHILD) => return Ok(true),
            Some(libc::EINTR) => continue,
            _ => return Err(error),
        }
    }
}

#[cfg(target_os = "linux")]
fn children() -> io::Result<Vec<libc::pid_t>> {
    // Single-threaded supervisor, so all children belong to this task.
    let pid = std::process::id();
    std::fs::read_to_string(format!("/proc/self/task/{pid}/children"))?
        .split_whitespace()
        .map(|pid| {
            pid.parse()
                .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))
        })
        .collect()
}
