//! Pin the process before checking its birth identity. A recycled PID cannot
//! redirect a signal sent through the original kernel handle.
use super::{error, Instance, LOCK};
use crate::Result;
use std::{
    io,
    os::fd::{AsRawFd, FromRawFd, OwnedFd},
    os::unix::fs::MetadataExt,
    path::Path,
};

// PID and birth are persisted in the project. Independently verify with the
// kernel that the named process holds this project's lifetime lock. If procfs
// cannot supply that proof, only the generation-specific request is used.
fn holds_project_lock(root: &Path, pid: u32) -> bool {
    let (Ok(file), Ok(locks)) = (
        std::fs::metadata(root.join(LOCK)),
        std::fs::read_to_string("/proc/locks"),
    ) else {
        return false;
    };
    let device_inode = format!(
        "{:02x}:{:02x}:{}",
        libc::major(file.dev()),
        libc::minor(file.dev()),
        file.ino()
    );
    let owner = pid.to_string();
    locks.lines().any(|line| {
        line.split_whitespace().skip(1).take(5).eq([
            "FLOCK",
            "ADVISORY",
            "WRITE",
            owner.as_str(),
            device_inode.as_str(),
        ])
    })
}

pub(super) struct Handle(OwnedFd);
impl Handle {
    pub(super) fn open(pid: u32) -> Result<Option<Self>> {
        // SAFETY: pidfd_open takes an integer PID and zero flags.
        let fd = unsafe { libc::syscall(libc::SYS_pidfd_open, pid as libc::pid_t, 0u32) };
        if fd < 0 {
            let e = io::Error::last_os_error();
            return match e.raw_os_error() {
                Some(libc::ESRCH | libc::ENOSYS | libc::EPERM | libc::EACCES) => Ok(None),
                _ => Err(error(e)),
            };
        }
        // SAFETY: the successful syscall returned a new owned descriptor.
        Ok(Some(Self(unsafe { OwnedFd::from_raw_fd(fd as i32) })))
    }

    pub(super) fn send(&self, root: &Path, instance: &Instance, sig: i32) -> Result<()> {
        if !holds_project_lock(root, instance.pid)
            || Instance::running(root)?.as_ref() != Some(instance)
        {
            return Ok(());
        }
        // SAFETY: self keeps this pidfd open; null siginfo requests ordinary delivery.
        let rc = unsafe {
            libc::syscall(
                libc::SYS_pidfd_send_signal,
                self.0.as_raw_fd(),
                sig,
                std::ptr::null::<libc::siginfo_t>(),
                0u32,
            )
        };
        if rc < 0 {
            let e = io::Error::last_os_error();
            if !matches!(
                e.raw_os_error(),
                Some(libc::ESRCH | libc::ENOSYS | libc::EPERM | libc::EACCES)
            ) {
                return Err(error(e));
            }
        }
        Ok(())
    }
}
