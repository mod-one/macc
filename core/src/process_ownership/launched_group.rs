//! Bind stop requests to a launcher-created process group and generation.
use crate::{MaccError, Result};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub enum Scope {
    CoordinatorSession,
    PerformerGroup,
}

impl Scope {
    pub fn env(self) -> &'static str {
        match self {
            Self::CoordinatorSession => "MACC_COORDINATOR_GENERATION",
            Self::PerformerGroup => "MACC_PERFORMER_GENERATION",
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct LaunchedGroup {
    pub leader: i32,
    pub generation: Uuid,
    pub leader_identity: Option<String>,
    pub scope: Scope,
}

impl LaunchedGroup {
    pub fn from_child(leader: i32, generation: Uuid, scope: Scope) -> Self {
        Self {
            leader,
            generation,
            scope,
            leader_identity: super::process_identity(leader.into()),
        }
    }

    pub fn is_running(&self) -> Result<bool> {
        #[cfg(target_os = "linux")]
        {
            linux::leader_matches(self)
        }
        #[cfg(not(target_os = "linux"))]
        {
            Ok(self.leader_identity.is_some()
                && self.leader_identity == super::process_identity(self.leader.into()))
        }
    }

    pub fn stop(&self, graceful: bool, grace: std::time::Duration) -> Result<usize> {
        #[cfg(target_os = "linux")]
        {
            let members = linux::members(self)?;
            let mut sent = Vec::new();
            for member in members {
                if member.send(self, libc::SIGTERM)? {
                    sent.push(member);
                }
            }
            let mut signaled: std::collections::HashSet<_> = sent.iter().map(|p| p.pid).collect();
            if !graceful && !sent.is_empty() {
                let start = std::time::Instant::now();
                while start.elapsed() < grace {
                    let mut alive = false;
                    for target in &sent {
                        alive |= target.send(self, 0)?;
                    }
                    if !alive {
                        break;
                    }
                    std::thread::sleep(std::time::Duration::from_millis(40));
                }
                for member in sent {
                    member.send(self, libc::SIGKILL)?;
                }
                // Include children spawned during graceful shutdown.
                for member in linux::members(self)? {
                    if member.send(self, libc::SIGKILL)? {
                        signaled.insert(member.pid);
                    }
                }
            }
            Ok(signaled.len())
        }
        #[cfg(not(target_os = "linux"))]
        {
            let _ = (graceful, grace);
            Err(MaccError::Validation(
                "Verified process signalling requires Linux pidfd support".into(),
            ))
        }
    }
}

#[cfg(target_os = "linux")]
mod linux {
    use super::*;
    use std::{
        fs, io,
        os::fd::{AsRawFd, FromRawFd, OwnedFd},
    };

    fn gone(e: &io::Error) -> bool {
        matches!(e.raw_os_error(), Some(libc::ESRCH | libc::ENOENT))
    }

    fn error(e: io::Error) -> MaccError {
        MaccError::Io {
            path: "/proc".into(),
            action: "verify or signal managed process".into(),
            source: e,
        }
    }

    fn member(group: &LaunchedGroup, pid: i32) -> bool {
        // SAFETY: both calls only inspect kernel membership for a positive PID.
        (unsafe {
            match group.scope {
                Scope::CoordinatorSession => libc::getsid(pid),
                Scope::PerformerGroup => libc::getpgid(pid),
            }
        }) == group.leader
    }

    pub(super) struct Target {
        pub(super) pid: i32,
        fd: OwnedFd,
    }

    impl Target {
        fn alive(&self) -> Result<bool> {
            // An exited child can remain in /proc until its parent reaps it.
            let mut state = libc::pollfd {
                fd: self.fd.as_raw_fd(),
                events: libc::POLLIN,
                revents: 0,
            };
            // SAFETY: poll receives one initialized descriptor and never blocks.
            match unsafe { libc::poll(&mut state, 1, 0) } {
                0 => Ok(true),
                -1 => Err(error(io::Error::last_os_error())),
                _ => Ok(false),
            }
        }

        pub(super) fn send(&self, group: &LaunchedGroup, sig: i32) -> Result<bool> {
            if !member(group, self.pid) {
                return Ok(false);
            }
            if !self.alive()? {
                return Ok(false);
            }
            // SAFETY: this descriptor pins the process authenticated at discovery.
            let rc = unsafe {
                libc::syscall(
                    libc::SYS_pidfd_send_signal,
                    self.fd.as_raw_fd(),
                    sig,
                    std::ptr::null::<libc::siginfo_t>(),
                    0u32,
                )
            };
            if rc == 0 {
                return Ok(true);
            }
            let e = io::Error::last_os_error();
            if gone(&e) {
                Ok(false)
            } else {
                Err(MaccError::Io {
                    path: format!("/proc/{}", self.pid),
                    action: format!("send signal {sig} to verified process"),
                    source: e,
                })
            }
        }
    }

    fn has_marker(pid: i32, marker: &[u8]) -> Result<bool> {
        match fs::read(format!("/proc/{pid}/environ")) {
            Ok(env) => Ok(env.split(|b| *b == 0).any(|v| v == marker)),
            Err(e) if gone(&e) => Ok(false),
            Err(e) => Err(error(e)),
        }
    }

    pub(super) fn leader_matches(group: &LaunchedGroup) -> Result<bool> {
        if group.leader <= 1 || !member(group, group.leader) {
            return Ok(false);
        }
        if let Some(expected) = &group.leader_identity {
            return Ok(
                super::super::process_identity(group.leader.into()).as_ref() == Some(expected)
            );
        }
        has_marker(
            group.leader,
            format!("{}={}", group.scope.env(), group.generation).as_bytes(),
        )
    }

    pub(super) fn members(group: &LaunchedGroup) -> Result<Vec<Target>> {
        if group.leader <= 1 || group.generation.is_nil() {
            return Err(MaccError::Validation(
                "Invalid managed process registration".into(),
            ));
        }
        let marker = format!("{}={}", group.scope.env(), group.generation);
        let mut targets = Vec::new();
        for entry in fs::read_dir("/proc").map_err(error)? {
            let entry = entry.map_err(error)?;
            let Some(pid) = entry
                .file_name()
                .to_str()
                .and_then(|s| s.parse::<i32>().ok())
            else {
                continue;
            };
            if !member(group, pid) {
                continue;
            }
            // SAFETY: pidfd_open takes a positive PID and zero flags.
            let fd = unsafe { libc::syscall(libc::SYS_pidfd_open, pid, 0u32) };
            if fd < 0 {
                let e = io::Error::last_os_error();
                if gone(&e) {
                    continue;
                }
                return Err(error(e));
            }
            // SAFETY: the kernel returned a new owned descriptor.
            let fd = unsafe { OwnedFd::from_raw_fd(fd as i32) };
            if !member(group, pid) {
                continue;
            }
            // Only a private session guarantees every member descends from the
            // launcher. A worker group can share the invoking shell's session.
            let target = Target { pid, fd };
            if !target.alive()? {
                continue;
            }
            let leader = leader_matches(group)?;
            if !leader && super::super::process_identity(group.leader.into()).is_some() {
                continue;
            }
            if (leader && (group.scope == Scope::CoordinatorSession || pid == group.leader))
                || has_marker(pid, marker.as_bytes())?
            {
                targets.push(target);
            }
        }
        Ok(targets)
    }
}
