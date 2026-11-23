//! A project lock elects the owner. The numeric PID is a compatibility projection,
//! never authority to signal a process or clean up a newer generation.
use crate::{fs_lock::AdvisoryLock, MaccError, ProjectPaths, Result};
use serde::{Deserialize, Serialize};
use std::{
    fs, io,
    path::{Path, PathBuf},
    time::{Duration, Instant},
};
use uuid::Uuid;

#[cfg(target_os = "linux")]
mod signal;

const PID: &str = ".macc/state/supervisor.pid";
const RECORD: &str = ".macc/state/supervisor-instance.json";
const LOCK: &str = ".macc/state/supervisor.lock";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Instance {
    project: PathBuf,
    pub pid: u32,
    pub generation: Uuid,
    #[cfg(target_os = "linux")]
    birth: String,
}

fn error(e: impl std::fmt::Display) -> MaccError {
    MaccError::Validation(format!("Supervisor instance: {e}"))
}

fn read(path: &Path) -> Result<Option<Vec<u8>>> {
    match fs::read(path) {
        Ok(bytes) => Ok(Some(bytes)),
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(source) => Err(MaccError::Io {
            path: path.display().to_string(),
            action: "read supervisor registration".into(),
            source,
        }),
    }
}

fn remove(path: &Path) -> Result<()> {
    match fs::remove_file(path) {
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(()),
        result => result.map_err(error),
    }
}

pub fn lock_if_idle(root: &Path) -> Result<Option<AdvisoryLock>> {
    AdvisoryLock::try_acquire(&root.join(LOCK), "supervisor")
}

impl Instance {
    fn registered(root: &Path) -> Result<Option<Self>> {
        let Some(raw) = read(&root.join(RECORD))? else {
            return Ok(None);
        };
        let instance: Self = serde_json::from_slice(&raw).map_err(error)?;
        if instance.project != root.canonicalize().map_err(error)?
            || instance.pid == 0
            || instance.pid > i32::MAX as u32
        {
            return Err(error("invalid project or process in registration"));
        }
        let pid = read(&root.join(PID))?;
        Ok(pid
            .as_deref()
            .and_then(|v| std::str::from_utf8(v).ok())
            .and_then(|s| s.trim().parse::<u32>().ok())
            .filter(|pid| *pid == instance.pid)
            .map(|_| instance))
    }

    fn matches(&self, root: &Path) -> Result<bool> {
        Ok(Self::registered(root)?.as_ref() == Some(self))
    }

    pub fn running(root: &Path) -> Result<Option<Self>> {
        let Some(instance) = Self::registered(root)? else {
            return Ok(None);
        };
        #[cfg(target_os = "linux")]
        if crate::process_ownership::process_identity(i64::from(instance.pid)).as_ref()
            != Some(&instance.birth)
        {
            return Ok(None);
        }
        if lock_if_idle(root)?.is_some() {
            return Ok(None);
        }
        Ok(Some(instance))
    }

    fn request_path(&self) -> PathBuf {
        self.project
            .join(format!(".macc/state/supervisor-stop-{}", self.generation))
    }

    pub fn stop_requested(&self) -> bool {
        self.request_path().is_file()
    }

    fn clear(&self, root: &Path) -> Result<()> {
        // Caller holds the lifetime lock, excluding registration by a successor.
        if self.matches(root)? {
            remove(&root.join(PID))?;
            remove(&root.join(RECORD))?;
        }
        remove(&self.request_path())
    }
}

/// Held through shutdown reporting and process-owner unregistration.
pub struct Owner {
    pub instance: Instance,
    _lock: AdvisoryLock,
}

impl Owner {
    pub fn acquire(root: &Path) -> Result<Self> {
        let project = root.canonicalize().map_err(error)?;
        let lock = AdvisoryLock::acquire(&project.join(LOCK), Duration::ZERO, "supervisor")?;
        remove(&project.join(".macc/state/supervisor-health.json"))?;
        let pid = std::process::id();
        let instance = Instance {
            project,
            pid,
            generation: Uuid::new_v4(),
            #[cfg(target_os = "linux")]
            birth: crate::process_ownership::process_identity(i64::from(pid))
                .ok_or_else(|| error("cannot identify supervisor process"))?,
        };
        let paths = ProjectPaths::from_root(root);
        crate::atomic_write(
            &paths,
            &root.join(RECORD),
            &serde_json::to_vec(&instance).map_err(error)?,
        )?;
        crate::atomic_write(&paths, &root.join(PID), format!("{pid}\n").as_bytes())?;
        Ok(Self {
            instance,
            _lock: lock,
        })
    }
}
impl Drop for Owner {
    fn drop(&mut self) {
        if let Err(err) = self.instance.clear(&self.instance.project) {
            tracing::warn!(%err, "Cannot clean up supervisor registration");
        }
    }
}

/// An attachment may stop only the generation that was originally attached.
pub fn stop(root: &Path, expected: Option<Uuid>) -> Result<bool> {
    let pid_file = read(&root.join(PID))?;
    if pid_file.is_none() && !root.join(LOCK).exists() {
        return Ok(false);
    }
    let registered = Instance::registered(root)?;
    if expected.is_some() && registered.as_ref().map(|i| i.generation) != expected {
        return Ok(false);
    }
    if let Some(_idle) = lock_if_idle(root)? {
        // No supervisor owns this project. Re-read under lock: another start/exit
        // could have occurred since the first read.
        if Instance::registered(root)? == registered {
            if let Some(instance) = registered {
                instance.clear(root)?;
            } else if read(&root.join(RECORD))?.is_none() && read(&root.join(PID))? == pid_file {
                remove(&root.join(PID))?;
            }
        }
        return Ok(false);
    }
    let instance =
        registered.ok_or_else(|| error("running supervisor has no verifiable registration"))?;
    if Instance::running(root)?.as_ref() != Some(&instance) {
        if instance.matches(root)? && lock_if_idle(root)?.is_none() {
            return Err(error(
                "project lock held but process identity could not be verified; stop refused",
            ));
        }
        return Ok(false);
    }

    // The cooperative request also works on kernels without process handles.
    let request = instance.request_path();
    match fs::File::create_new(&request) {
        Ok(_) => {}
        Err(e) if e.kind() == io::ErrorKind::AlreadyExists => {}
        Err(e) => return Err(error(e)),
    }
    #[cfg(target_os = "linux")]
    let handle = signal::Handle::open(instance.pid)?;
    #[cfg(target_os = "linux")]
    if let Some(handle) = &handle {
        handle.send(root, &instance, libc::SIGTERM)?;
    }
    let deadline = Instant::now() + Duration::from_secs(5);
    let mut forced = false;
    loop {
        if !instance.matches(root)? {
            break;
        }
        if let Some(_idle) = lock_if_idle(root)? {
            instance.clear(root)?;
            break;
        }
        if Instant::now() >= deadline && !forced {
            #[cfg(target_os = "linux")]
            if let Some(handle) = &handle {
                handle.send(root, &instance, libc::SIGKILL)?;
            }
            forced = true;
        }
        if Instant::now() >= deadline + Duration::from_secs(2) {
            return Err(error("shutdown has not been confirmed"));
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    remove(&request)?;
    Ok(true)
}

#[cfg(all(test, target_os = "linux"))]
mod tests;
