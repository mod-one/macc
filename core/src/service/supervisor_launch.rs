//! Supervisor bootstrap shared by the CLI, TUI and Web launch service.
use crate::{MaccError, Result};
use std::{
    path::Path,
    process::{Command as ProcessCommand, Stdio},
};
const SUPERVISOR_PID_REL_PATH: &str = ".macc/state/supervisor.pid";
const COORDINATOR_SUPERVISOR_REL_PATH: &str = ".macc/state/coordinator-supervisor.json";
pub fn attach(project_root: &Path, coordinator_pid: u32) -> Result<()> {
    let pid_file = project_root.join(SUPERVISOR_PID_REL_PATH);
    if let Ok(raw) = std::fs::read_to_string(&pid_file) {
        if let Ok(pid) = raw.trim().parse::<u32>() {
            if crate::coordinator::helpers::is_pid_running(pid as i64) {
                let marker =
                    serde_json::json!({"coordinator_pid":coordinator_pid,"supervisor_pid":pid});
                std::fs::write(
                    project_root.join(COORDINATOR_SUPERVISOR_REL_PATH),
                    format!("{marker}\n"),
                )
                .map_err(|source| MaccError::Io {
                    path: pid_file.display().to_string(),
                    action: "attach existing supervisor".into(),
                    source,
                })?;
                return Ok(());
            }
        }
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

    // Poll for the supervisor daemon child to write its PID file (up to 2 s).
    // Once found, record the binding so `macc coordinator stop` can stop the
    // supervisor along with the coordinator.
    let supervisor_pid_path = project_root.join(SUPERVISOR_PID_REL_PATH);
    let mut supervisor_pid: Option<u32> = None;
    for _ in 0..20 {
        if let Ok(raw) = std::fs::read_to_string(&supervisor_pid_path) {
            if let Ok(pid) = raw.trim().parse::<u32>() {
                supervisor_pid = Some(pid);
                break;
            }
        }
        std::thread::sleep(std::time::Duration::from_millis(100));
    }

    let spid = supervisor_pid
        .ok_or_else(|| MaccError::Validation("Supervisor did not register its PID".into()))?;
    {
        let marker = serde_json::json!({
            "coordinator_pid": coordinator_pid,
            "supervisor_pid": spid
        });
        let marker_path = project_root.join(COORDINATOR_SUPERVISOR_REL_PATH);
        if let Some(parent) = marker_path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        std::fs::write(&marker_path, format!("{}\n", marker)).map_err(|source| MaccError::Io {
            path: marker_path.display().to_string(),
            action: "record supervisor attachment".into(),
            source,
        })?;
    }

    Ok(())
}
