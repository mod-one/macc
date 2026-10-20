//! Wait for the managed runtime, not merely successful fork/exec of its launcher.
use crate::{MaccError, ProjectPaths, Result};
use std::time::{Duration, Instant};
pub fn wait(paths: &ProjectPaths, pid: i32, timeout: Duration) -> Result<()> {
    let timeout = timeout.max(super::preparation::startup_timeout(&paths.root)?);
    let deadline = Instant::now() + timeout;
    loop {
        let exit = paths.root.join(format!(
            ".macc/state/managed-command-results/run-{pid}.exit"
        ));
        if let Ok(code) = std::fs::read_to_string(&exit) {
            if code.trim() == "0" {
                return Ok(());
            }
            return Err(startup_error(
                paths,
                pid,
                &format!("Coordinator failed during startup (exit {})", code.trim()),
            ));
        }
        let ready = paths.root.join(format!(
            ".macc/state/managed-command-results/run-{pid}.ready"
        ));
        if let Ok(child) = std::fs::read_to_string(ready) {
            if child
                .trim()
                .parse::<i64>()
                .ok()
                .is_some_and(crate::coordinator::helpers::is_pid_running)
            {
                return Ok(());
            }
        }
        if Instant::now() >= deadline {
            return Err(startup_error(
                paths,
                pid,
                &format!(
                    "Coordinator readiness timed out after {} seconds",
                    timeout.as_secs()
                ),
            ));
        }
        std::thread::sleep(Duration::from_millis(50));
    }
}

fn startup_error(paths: &ProjectPaths, pid: i32, reason: &str) -> MaccError {
    use std::io::{Read, Seek, SeekFrom};
    let log = paths
        .macc_dir
        .join(format!("log/coordinator/run-{pid}.stderr.log"));
    let mut tail = Vec::new();
    if let Ok(mut file) = std::fs::File::open(&log) {
        if let Ok(meta) = file.metadata() {
            let _ = file.seek(SeekFrom::Start(meta.len().saturating_sub(8192)));
            let _ = file.take(8192).read_to_end(&mut tail);
        }
    }
    let cause = crate::save::redact_secrets_in_text(&String::from_utf8_lossy(&tail));
    let cause: String = cause
        .chars()
        .filter(|c| !c.is_control() || matches!(c, '\n' | '\t'))
        .collect();
    MaccError::Validation(format!(
        "{reason}; evidence: {}{}",
        log.display(),
        if cause.trim().is_empty() {
            String::new()
        } else {
            format!("\nCause: {}", cause.trim())
        }
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn startup_failure_reports_current_run_cause_instead_of_stale_daemon_error() {
        let temp = tempfile::tempdir().unwrap();
        let paths = ProjectPaths::from_root(temp.path());
        let results = paths.macc_dir.join("state/managed-command-results");
        std::fs::create_dir_all(&results).unwrap();
        std::fs::create_dir_all(paths.macc_dir.join("log/coordinator")).unwrap();
        std::fs::write(results.join("run-999.exit"), "17").unwrap();
        std::fs::write(
            paths.macc_dir.join("log/coordinator/daemon-stderr.log"),
            "stale unrelated error",
        )
        .unwrap();
        std::fs::write(
            paths.macc_dir.join("log/coordinator/run-999.stderr.log"),
            "Tool readiness Timeout: probe exceeded 15 seconds; evidence: probe.log",
        )
        .unwrap();
        let error = wait(&paths, 999, Duration::ZERO).unwrap_err().to_string();
        assert!(error.contains("exit 17"));
        assert!(error.contains("probe exceeded 15 seconds"));
        assert!(error.contains("probe.log"));
        assert!(!error.contains("stale unrelated"));
    }
}
