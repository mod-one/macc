//! Heartbeats describe an AI subprocess without claiming repair progress or success.
use super::report::write_json;
use macc_core::{supervisor::SupervisorConfig, Result};
use serde_json::{json, Value};
use std::path::Path;

pub(super) fn record(
    root: &Path,
    config: &SupervisorConfig,
    dir: &Path,
    phase: &str,
    pid: Option<u32>,
    elapsed: u64,
    state: &str,
) -> Result<()> {
    let bytes = |stream: &str| {
        std::fs::metadata(dir.join(format!("{phase}-{stream}.log")))
            .map(|m| m.len())
            .unwrap_or(0)
    };
    let stdout = bytes("stdout");
    let stderr = bytes("stderr");
    let activity = json!({"phase":phase,"pid":pid,"elapsed_seconds":elapsed,"state":state,"stdout_bytes":stdout,"stderr_bytes":stderr,"observed_at":chrono::Utc::now().to_rfc3339(),"notice":"Process liveness and output size do not prove repair progress or success"});
    write_json(&dir.join("agent-status.json"), &activity)?;
    let latest = root.join(".macc/log/supervisor/latest.json");
    let raw = match std::fs::read(&latest) {
        Ok(raw) => raw,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(error) => return Err(super::io(&latest, error)),
    };
    let mut report: Value = serde_json::from_slice(&raw)
        .map_err(|e| macc_core::MaccError::Validation(e.to_string()))?;
    // Never overwrite another intervention, including after recovery/restart.
    if report
        .pointer("/intervention/log_dir")
        .and_then(Value::as_str)
        != dir.to_str()
    {
        return Ok(());
    }
    report["timestamp"] = activity["observed_at"].clone();
    report["agent_activity"] = activity.clone();
    report["intervention"]["activity"] = json!(format!(
        "AI {phase}: {state}, elapsed {elapsed}s, stdout {stdout} B / stderr {stderr} B{}",
        if stdout + stderr == 0 {
            "; awaiting first output (progress unverified)"
        } else {
            "; output available, result pending"
        }
    ));
    write_json(&dir.join("report.json"), &report)?;
    write_json(&latest, &report)?;
    let target = if config.report_output_path.is_absolute() {
        config.report_output_path.clone()
    } else {
        root.join(&config.report_output_path)
    };
    write_json(&target, &report)?;
    write_json(
        &root.join(".macc/state/supervisor-health.json"),
        &json!({"checked_at":activity["observed_at"],"health":{"status":"intervening"},"tool":config.tool,"agent_activity":activity}),
    )?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn silent_agent_is_visible_without_claiming_success_or_overwriting_other_incidents() {
        let t = tempfile::tempdir().unwrap();
        let root = t.path();
        let dir = root.join(".macc/log/supervisor/test");
        std::fs::create_dir_all(&dir).unwrap();
        let latest = root.join(".macc/log/supervisor/latest.json");
        write_json(
            &latest,
            &json!({"intervention":{"log_dir":dir,"status":"repairing"}}),
        )
        .unwrap();
        record(
            root,
            &SupervisorConfig::default(),
            &dir,
            "repair",
            Some(123),
            30,
            "running",
        )
        .unwrap();
        let value: Value = serde_json::from_slice(&std::fs::read(&latest).unwrap()).unwrap();
        assert_eq!(value["intervention"]["status"], "repairing");
        assert_eq!(value["agent_activity"]["state"], "running");
        assert!(value["intervention"]["activity"]
            .as_str()
            .unwrap()
            .contains("progress unverified"));
        write_json(
            &latest,
            &json!({"intervention":{"log_dir":"other","status":"recovered"}}),
        )
        .unwrap();
        let before = std::fs::read(&latest).unwrap();
        record(
            root,
            &SupervisorConfig::default(),
            &dir,
            "repair",
            Some(123),
            35,
            "running",
        )
        .unwrap();
        assert_eq!(std::fs::read(&latest).unwrap(), before);
    }
}
