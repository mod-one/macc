//! Cross-process advisory file locking.
//!
//! # Why not a `create_new` lock file
//!
//! The obvious lock — `OpenOptions::create_new(true)` on a path, removed in
//! `Drop` — is unsound for any process that can be killed. `Drop` does not run
//! on `SIGKILL`, on a panic with `panic=abort`, or on a power loss, so a single
//! hard kill leaves the lock file behind and **every subsequent acquisition
//! fails forever**. Nothing reclaims it, and each attempt burns its full retry
//! budget first.
//!
//! This module uses a kernel-backed lock instead:
//!
//! * **unix** — `flock(2)` with `LOCK_EX | LOCK_NB`. The lock lives on the open
//!   file description, so the kernel releases it when the process dies, however
//!   it dies.
//! * **windows** — `CreateFile` with a share mode of 0 (via
//!   [`OpenOptionsExt::share_mode`]), which denies other openers until the
//!   handle is closed. Handles are closed by the OS on process exit, giving the
//!   same self-healing property.
//!
//! # The lock file is never deleted
//!
//! Unlinking it would reintroduce the race the lock exists to prevent: process
//! A holds the lock on inode X and unlinks the path; process B creates a *new*
//! file at that path (inode Y) and locks that instead. Both would believe they
//! hold the lock. The lock file is therefore a persistent zero-byte marker —
//! its *existence* means nothing, only the kernel lock on it does.

use crate::{MaccError, Result};
use std::fs::{File, OpenOptions};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

/// Default delay between acquisition attempts.
pub const DEFAULT_RETRY_DELAY: Duration = Duration::from_millis(10);

/// An exclusive advisory lock held on a file, released by the kernel when this
/// process exits — including on `SIGKILL`.
#[derive(Debug)]
pub struct AdvisoryLock {
    /// Held open for the lifetime of the guard: dropping the `File` closes the
    /// descriptor, which is what releases the lock.
    file: File,
    path: PathBuf,
}

impl AdvisoryLock {
    /// Acquire the lock at `path`, retrying until `timeout` elapses.
    ///
    /// `what` names the protected resource and appears in error messages, e.g.
    /// `"process ownership store"`.
    pub fn acquire(path: &Path, timeout: Duration, what: &str) -> Result<Self> {
        Self::acquire_with_retry(path, timeout, DEFAULT_RETRY_DELAY, what)
    }

    /// Like [`Self::acquire`] with an explicit retry delay.
    pub fn acquire_with_retry(
        path: &Path,
        timeout: Duration,
        retry_delay: Duration,
        what: &str,
    ) -> Result<Self> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(|e| MaccError::Io {
                path: parent.to_string_lossy().into(),
                action: format!("create {} lock parent directory", what),
                source: e,
            })?;
        }

        let started = Instant::now();
        loop {
            match Self::try_acquire(path, what)? {
                Some(lock) => return Ok(lock),
                None => {
                    if started.elapsed() >= timeout {
                        return Err(MaccError::Io {
                            path: path.to_string_lossy().into(),
                            action: format!("acquire {} lock", what),
                            source: std::io::Error::new(
                                std::io::ErrorKind::TimedOut,
                                format!(
                                    "timed out after {:?} waiting for another macc process to release the {} lock",
                                    timeout, what
                                ),
                            ),
                        });
                    }
                    std::thread::sleep(retry_delay);
                }
            }
        }
    }

    /// Try to take the lock once. `Ok(None)` means another process holds it.
    #[cfg(unix)]
    pub fn try_acquire(path: &Path, what: &str) -> Result<Option<Self>> {
        use std::os::unix::io::AsRawFd;

        let file = Self::open_lock_file(path, what)?;
        // SAFETY: `file` owns a valid descriptor for the duration of the call.
        let rc = unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) };
        if rc == 0 {
            return Ok(Some(Self {
                file,
                path: path.to_path_buf(),
            }));
        }
        let err = std::io::Error::last_os_error();
        match err.raw_os_error() {
            // Held by someone else, or the call was interrupted — both retryable.
            Some(code) if code == libc::EWOULDBLOCK || code == libc::EINTR => Ok(None),
            _ => Err(MaccError::Io {
                path: path.to_string_lossy().into(),
                action: format!("lock {}", what),
                source: err,
            }),
        }
    }

    /// Try to take the lock once. `Ok(None)` means another process holds it.
    #[cfg(windows)]
    pub fn try_acquire(path: &Path, what: &str) -> Result<Option<Self>> {
        use std::os::windows::fs::OpenOptionsExt;

        // share_mode(0) denies all other openers until this handle closes.
        match OpenOptions::new()
            .create(true)
            .truncate(false)
            .write(true)
            .share_mode(0)
            .open(path)
        {
            Ok(file) => Ok(Some(Self {
                file,
                path: path.to_path_buf(),
            })),
            Err(err) => match err.raw_os_error() {
                // ERROR_SHARING_VIOLATION / ERROR_LOCK_VIOLATION: held elsewhere.
                Some(32) | Some(33) => Ok(None),
                _ if err.kind() == std::io::ErrorKind::PermissionDenied => Ok(None),
                _ => Err(MaccError::Io {
                    path: path.to_string_lossy().into(),
                    action: format!("lock {}", what),
                    source: err,
                }),
            },
        }
    }

    /// Platforms without a supported locking primitive get an unlocked handle.
    /// Correctness then relies on in-process serialisation only.
    #[cfg(not(any(unix, windows)))]
    pub fn try_acquire(path: &Path, what: &str) -> Result<Option<Self>> {
        let file = Self::open_lock_file(path, what)?;
        Ok(Some(Self {
            file,
            path: path.to_path_buf(),
        }))
    }

    #[cfg(not(windows))]
    fn open_lock_file(path: &Path, what: &str) -> Result<File> {
        OpenOptions::new()
            .create(true)
            .truncate(false)
            .write(true)
            .open(path)
            .map_err(|e| MaccError::Io {
                path: path.to_string_lossy().into(),
                action: format!("open {} lock file", what),
                source: e,
            })
    }

    /// Path of the lock file backing this guard.
    pub fn path(&self) -> &Path {
        &self.path
    }
}

#[cfg(unix)]
impl Drop for AdvisoryLock {
    fn drop(&mut self) {
        use std::os::unix::io::AsRawFd;
        // Closing the descriptor would release the lock on its own; unlocking
        // explicitly makes the release point obvious. The file itself is
        // deliberately left in place — see the module docs.
        // SAFETY: the descriptor is still open until `self.file` drops.
        unsafe {
            libc::flock(self.file.as_raw_fd(), libc::LOCK_UN);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lock_is_exclusive_while_held() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("nested/dir/test.lock");

        let held =
            AdvisoryLock::acquire(&path, Duration::from_millis(50), "test").expect("acquire");
        assert!(path.exists(), "lock file should be created");

        let contended = AdvisoryLock::acquire(&path, Duration::from_millis(50), "test");
        assert!(
            contended.is_err(),
            "a second acquisition must not succeed while the first is held"
        );

        drop(held);
        AdvisoryLock::acquire(&path, Duration::from_millis(50), "test")
            .expect("lock should be free again after the holder drops");
    }

    #[test]
    fn lock_file_survives_release_so_the_inode_stays_stable() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("test.lock");

        let lock =
            AdvisoryLock::acquire(&path, Duration::from_millis(50), "test").expect("acquire");
        drop(lock);

        // Unlinking on release would let two processes lock different inodes at
        // the same path and both believe they hold the lock.
        assert!(
            path.exists(),
            "the lock file must not be removed when the lock is released"
        );
    }

    /// The property the old `create_new` lock lacked: a leftover lock file from
    /// a process that died without cleanup must not block anyone.
    #[test]
    fn a_stale_lock_file_does_not_block_acquisition() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("test.lock");

        // Simulate what a SIGKILLed process leaves behind: the file, no holder.
        std::fs::write(&path, b"").expect("write stale lock file");

        AdvisoryLock::acquire(&path, Duration::from_millis(50), "test")
            .expect("a leftover lock file must never wedge acquisition");
    }

    /// End-to-end proof of the self-healing property: a `SIGKILL`ed holder
    /// leaves the lock available. This is what a `create_new` lock file could
    /// never do, because `Drop` never runs.
    ///
    /// Linux-only because it drives `flock(1)` from util-linux, which macOS
    /// does not ship.
    #[cfg(target_os = "linux")]
    #[test]
    fn lock_is_released_when_the_holding_process_is_killed() {
        use std::os::unix::process::CommandExt;
        use std::process::{Command, Stdio};

        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("test.lock");
        std::fs::write(&path, b"").expect("create lock file");

        // `flock(1)` takes the same kernel lock. It forks the command it runs,
        // and the child inherits the locked descriptor, so the whole process
        // group has to go — exactly how the coordinator kills its own jobs.
        let mut cmd = Command::new("flock");
        cmd.arg("-x")
            .arg(&path)
            .arg("sleep")
            .arg("30")
            .stdout(Stdio::null())
            .stderr(Stdio::null());
        unsafe {
            cmd.pre_exec(|| {
                // New session => child pid is its own process group leader.
                libc::setsid();
                Ok(())
            });
        }
        // Silently skip on the rare Linux image without util-linux rather than
        // failing for an unrelated reason.
        let Ok(mut child) = cmd.spawn() else {
            return;
        };
        let pgid = child.id() as libc::pid_t;

        // Wait for the child to actually take the lock.
        let mut held_by_child = false;
        for _ in 0..100 {
            std::thread::sleep(Duration::from_millis(20));
            if matches!(AdvisoryLock::try_acquire(&path, "test"), Ok(None)) {
                held_by_child = true;
                break;
            }
        }
        // SIGKILL the whole group: no Drop, no cleanup — only the kernel can
        // release the lock now.
        // SAFETY: pgid names the process group created above.
        unsafe { libc::killpg(pgid, libc::SIGKILL) };
        let _ = child.wait();
        assert!(held_by_child, "flock(1) child never took the lock");

        AdvisoryLock::acquire(&path, Duration::from_secs(5), "test")
            .expect("kernel must release the lock when the holder is killed");
    }
}
