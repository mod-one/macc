//! Wait for the managed runtime, not merely successful fork/exec of its launcher.
use crate::{MaccError, ProjectPaths, Result};
use std::time::{Duration, Instant};
pub fn wait(paths: &ProjectPaths, pid: i32, timeout: Duration) -> Result<()> {
    let deadline = Instant::now() + timeout;
    loop {
        let exit = paths.root.join(format!(
            ".macc/state/managed-command-results/run-{pid}.exit"
        ));
        if let Ok(code) = std::fs::read_to_string(&exit) {
            if code.trim() == "0" {
                return Ok(());
            }
            return Err(MaccError::Validation(format!("Coordinator failed during startup (exit {}); see {}/.macc/log/coordinator/daemon-stderr.log",code.trim(),paths.root.display())));
        }
        let ready = paths.root.join(format!(
            ".macc/state/managed-command-results/run-{pid}.ready"
        ));
        if let Ok(child) = std::fs::read_to_string(ready) {
            if child
                .trim()
                .parse::<i64>()
                .ok()
                .is_some_and(|child| crate::coordinator::helpers::is_pid_running(child))
            {
                return Ok(());
            }
        }
        if Instant::now() >= deadline {
            return Err(MaccError::Validation(
                "Coordinator readiness timed out; inspect coordinator logs".into(),
            ));
        }
        std::thread::sleep(Duration::from_millis(50));
    }
}
