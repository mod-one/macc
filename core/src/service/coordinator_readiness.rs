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

/// Observe only events belonging to this launcher's child, within the CLI latency budget.
pub(crate) fn observe_resume(paths: &ProjectPaths, pid: i32, timeout: Duration) -> Result<String> {
    let results = paths.macc_dir.join("state/managed-command-results");
    let ready = results.join(format!("run-{pid}.ready"));
    let exit = results.join(format!("run-{pid}.exit"));
    let deadline = Instant::now() + timeout;
    loop {
        if let Ok(code) = std::fs::read_to_string(&exit) {
            if code.trim() == "0" {
                return Ok("Run completed.".into());
            }
            return Err(startup_error(
                paths,
                pid,
                &format!("Run stopped (exit {})", code.trim()),
            ));
        }
        let child = std::fs::read_to_string(&ready)
            .ok()
            .and_then(|s| s.trim().parse::<u32>().ok());
        if let Some(active) = crate::coordinator::tick_progress::read(&paths.root)?
            .filter(|tick| Some(tick.pid) == child)
            .and_then(|tick| tick.live_activity())
        {
            return Ok(format!(
                "{} · {} phase{}",
                active.task,
                active.phase,
                active.worker.map(|w| format!(" · {w}")).unwrap_or_default()
            ));
        }
        if !crate::coordinator::helpers::is_pid_running(pid.into()) {
            return Err(startup_error(
                paths,
                pid,
                "Coordinator stopped before task activity could be confirmed",
            ));
        }
        if Instant::now() >= deadline {
            return Ok("Startup in progress; awaiting a task or terminal result.".into());
        }
        std::thread::sleep(Duration::from_millis(100));
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
    fn resume_reports_only_activity_from_the_ready_child() {
        let temp = tempfile::tempdir().unwrap();
        let paths = ProjectPaths::from_root(temp.path());
        let results = paths.macc_dir.join("state/managed-command-results");
        std::fs::create_dir_all(&results).unwrap();
        let pid = std::process::id() as i32;
        let activity = crate::coordinator::tick_progress::Activity {
            task: "T1".into(),
            phase: "fix".into(),
            worker: Some("worker-01".into()),
        };
        crate::coordinator::tick_progress::enter(temp.path(), "run", 2, "advance", Some(activity))
            .unwrap();
        assert!(observe_resume(&paths, pid, Duration::ZERO)
            .unwrap()
            .contains("Startup in progress"));
        std::fs::write(
            results.join(format!("run-{pid}.ready")),
            std::process::id().to_string(),
        )
        .unwrap();
        assert_eq!(
            observe_resume(&paths, pid, Duration::ZERO).unwrap(),
            "T1 · fix phase · worker-01"
        );
        std::fs::write(results.join(format!("run-{pid}.exit")), "3").unwrap();
        assert!(observe_resume(&paths, pid, Duration::ZERO)
            .unwrap_err()
            .to_string()
            .contains("exit 3"));
    }
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
