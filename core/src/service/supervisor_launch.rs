//! Supervisor bootstrap shared by the CLI, TUI and Web launch service.
use crate::{supervisor::instance::Instance, MaccError, Result};
use std::{
    path::Path,
    process::{Command as ProcessCommand, Stdio},
};
const COORDINATOR_SUPERVISOR_REL_PATH: &str = ".macc/state/coordinator-supervisor.json";
pub fn attach(project_root: &Path, coordinator_pid: u32) -> Result<()> {
    if let Some(instance) = Instance::running(project_root)? {
        return record_attachment(project_root, coordinator_pid, &instance);
    }
    let current_exe = std::env::current_exe().map_err(|e| MaccError::Io {
        path: project_root.to_string_lossy().into(),
        action: "resolve current executable for coordinator supervisor bootstrap".into(),
        source: e,
    })?;
    let status = ProcessCommand::new(current_exe)
        .current_dir(project_root)
        .env("MACC_INTERNAL_INVOCATION", "1")
        .arg("--cwd")
        .arg(project_root)
        .arg("supervisor")
        .arg("start")
        .arg("--daemon")
        .arg("--attach")
        .arg("--coordinator-pid")
        .arg(coordinator_pid.to_string())
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .map_err(|e| MaccError::Io {
            path: project_root.to_string_lossy().into(),
            action: "spawn supervisor from coordinator run --supervisor".into(),
            source: e,
        })?;

    if !status.success() {
        return Err(MaccError::Validation(format!(
            "failed to start supervisor for coordinator run (exit code: {:?})",
            status.code()
        )));
    }

    let instance = Instance::running(project_root)?
        .ok_or_else(|| MaccError::Validation("Supervisor did not register its instance".into()))?;
    record_attachment(project_root, coordinator_pid, &instance)
}

fn record_attachment(root: &Path, coordinator_pid: u32, instance: &Instance) -> Result<()> {
    let marker = serde_json::json!({"coordinator_pid": coordinator_pid, "supervisor_pid": instance.pid, "generation": instance.generation});
    crate::atomic_write(
        &crate::ProjectPaths::from_root(root),
        &root.join(COORDINATOR_SUPERVISOR_REL_PATH),
        marker.to_string().as_bytes(),
    )
}

pub fn stop_attached(project_root: &Path) -> Result<()> {
    let path = project_root.join(COORDINATOR_SUPERVISOR_REL_PATH);
    let raw = match std::fs::read(&path) {
        Ok(raw) => raw,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(source) => {
            return Err(MaccError::Io {
                path: path.display().to_string(),
                action: "read supervisor attachment".into(),
                source,
            })
        }
    };
    let marker: serde_json::Value =
        serde_json::from_slice(&raw).map_err(|e| MaccError::Validation(e.to_string()))?;
    if let Some(generation) = marker["generation"]
        .as_str()
        .and_then(|v| uuid::Uuid::parse_str(v).ok())
    {
        crate::supervisor::instance::stop(project_root, Some(generation))?;
    }
    if let Some(_idle) = crate::supervisor::instance::lock_if_idle(project_root)? {
        if std::fs::read(&path).ok().as_ref() == Some(&raw) {
            std::fs::remove_file(&path).map_err(|source| MaccError::Io {
                path: path.display().to_string(),
                action: "clear supervisor attachment".into(),
                source,
            })?;
        }
    }
    Ok(())
}
