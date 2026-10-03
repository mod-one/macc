use super::io;
use super::repair;
use macc_core::supervisor::{incident::Incident, SupervisorConfig};
use macc_core::{MaccError, ProjectPaths, Result};
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::path::{Path, PathBuf};
#[derive(Debug, Serialize, Deserialize)]
pub(super) struct Intervention {
    pub(super) incident: Incident,
    pub(super) attempt: u32,
    pub(super) status: String,
    pub(super) tool: Option<String>,
    pub(super) diagnosis: Option<repair::Diagnosis>,
    pub(super) worktree: Option<PathBuf>,
    pub(super) validation: Vec<String>,
    pub(super) commit: Option<String>,
    pub(super) restart_pid: Option<i32>,
    pub(super) detail: String,
}
pub(super) fn write_report(
    paths: &ProjectPaths,
    config: &SupervisorConfig,
    dir: &Path,
    report: &Intervention,
) -> Result<()> {
    let hints: Vec<_> = report
        .diagnosis
        .as_ref()
        .map(|d| {
            d.macc_findings
                .iter()
                .map(|finding| {
                    json!({
                        "problem_class":"ai_diagnosis", "detection_hook":"supervisor_incident",
                        "suggested_coordinator_behavior":finding
                    })
                })
                .collect()
        })
        .unwrap_or_default();
    let value = json!({"timestamp":chrono::Utc::now().to_rfc3339(),"analysis_window_seconds":config.log_analysis_window_seconds,
        "health":{"status":if report.status == "recovered" {"healthy"} else {"degraded"},"reasons":[report.status.clone()]},"findings":[],"recommendations":[],"actions_taken":[],"suggested_code_changes":[],"improvement_hints":hints,"intervention":report});
    write_json(&dir.join("report.json"), &value)?;
    let report_path = if config.report_output_path.is_absolute() {
        config.report_output_path.clone()
    } else {
        paths.root.join(&config.report_output_path)
    };
    write_json(&report_path, &value)?;
    // Stable latest file for status/UI consumers, independent of custom report location.
    write_json(&paths.root.join(".macc/log/supervisor/latest.json"), &value)
}
pub(super) fn write_json(path: &Path, value: &serde_json::Value) -> Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| io(path, e))?;
    }
    let temp = path.with_extension("tmp");
    std::fs::write(
        &temp,
        serde_json::to_vec_pretty(value).map_err(|e| MaccError::Validation(e.to_string()))?,
    )
    .map_err(|e| io(path, e))?;
    std::fs::rename(temp, path).map_err(|e| io(path, e))
}
pub(super) fn slug(id: &str) -> String {
    id.chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' || c == '_' {
                c
            } else {
                '_'
            }
        })
        .collect()
}
/// Finalize evidence even when SIGTERM cancels an in-flight tool or validator.
pub fn record_shutdown(paths: &ProjectPaths) -> Result<()> {
    let latest = paths.root.join(".macc/log/supervisor/latest.json");
    if !latest.exists() {
        return Ok(());
    }
    let mut value: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(&latest).map_err(|e| io(&latest, e))?)
            .map_err(|e| MaccError::Validation(e.to_string()))?;
    if value
        .pointer("/intervention/status")
        .and_then(|v| v.as_str())
        .is_some_and(|s| !matches!(s, "recovered" | "escalated"))
    {
        value["intervention"]["status"] = json!("interrupted");
        value["intervention"]["detail"] =
            json!("Supervisor stopped during intervention; isolated worktree and logs retained");
        write_json(&latest, &value)?;
        if let Some(worktree) = value
            .pointer("/intervention/worktree")
            .and_then(|v| v.as_str())
        {
            if let Some(dir) = Path::new(worktree).parent() {
                write_json(&dir.join("report.json"), &value)?;
            }
        }
        let cfg = macc_core::load_canonical_config(&paths.config_path)?
            .automation
            .supervisor
            .unwrap_or_default();
        let report = if cfg.report_output_path.is_absolute() {
            cfg.report_output_path
        } else {
            paths.root.join(cfg.report_output_path)
        };
        write_json(&report, &value)?;
    }
    Ok(())
}

/// Persist failures in the monitoring infrastructure even before an incident can be read.
pub fn record_failure(paths: &ProjectPaths, config: &SupervisorConfig, error: &str) -> Result<()> {
    let value = json!({"timestamp":chrono::Utc::now().to_rfc3339(),
        "analysis_window_seconds":config.log_analysis_window_seconds,
        "health":{"status":"degraded","reasons":[error]},"runtime_error":error});
    write_json(
        &paths.root.join(".macc/log/supervisor/runtime-error.json"),
        &value,
    )?;
    let output = if config.report_output_path.is_absolute() {
        config.report_output_path.clone()
    } else {
        paths.root.join(&config.report_output_path)
    };
    write_json(&output, &value)?;
    write_json(
        &paths.root.join(".macc/state/supervisor-health.json"),
        &json!({
            "checked_at":chrono::Utc::now().to_rfc3339(),"health":{"status":"error"},"detail":error
        }),
    )
}
