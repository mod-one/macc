use super::model::*;
use crate::{MaccError, Result};
use chrono::Utc;
use std::path::Path;
use std::process::Command;
use std::time::Instant;

pub struct SkillRunner;

impl SkillRunner {
    pub fn run(
        skill: &SkillDefinition,
        request: &SkillRunRequest,
        log_dir: &Path,
    ) -> Result<SkillRunResult> {
        let started = Instant::now();
        let started_at = Utc::now().to_rfc3339();

        std::fs::create_dir_all(log_dir).ok();
        let ts = Utc::now().format("%Y%m%d-%H%M%S").to_string();
        let log_base = format!("{}-{}", ts, skill.id);
        let jsonl_path = log_dir.join(format!("{}.jsonl", log_base));

        emit_event(
            &jsonl_path,
            &serde_json::json!({
                "type": "skill_started",
                "skill_id": skill.id,
                "tool": request.tool_id,
                "cwd": request.cwd.display().to_string(),
            }),
        );

        let (stdout, stderr, exit_code) = match skill.kind {
            SkillKind::LocalCommand | SkillKind::Hybrid => {
                run_local_commands(skill, &request.cwd)?
            }
            SkillKind::Prompt | SkillKind::Agent | SkillKind::Coordinator => {
                ("(prompt skill — run via tool adapter)".to_string(), String::new(), Some(0))
            }
        };

        let duration_ms = started.elapsed().as_millis() as u64;
        let status = if exit_code.unwrap_or(0) == 0 {
            "success"
        } else {
            "failed"
        };

        emit_event(
            &jsonl_path,
            &serde_json::json!({
                "type": "skill_finished",
                "skill_id": skill.id,
                "status": status,
                "duration_ms": duration_ms,
            }),
        );

        Ok(SkillRunResult {
            skill_id: skill.id.clone(),
            status: status.to_string(),
            tool: request.tool_id.clone(),
            started_at,
            duration_ms,
            stdout,
            stderr,
            exit_code,
            log_path: Some(jsonl_path),
        })
    }
}

fn run_local_commands(
    skill: &SkillDefinition,
    cwd: &Path,
) -> Result<(String, String, Option<i32>)> {
    let mut combined_stdout = String::new();
    let mut combined_stderr = String::new();
    let mut last_exit = Some(0i32);

    for step in &skill.steps {
        let Some(cmd_str) = step.run.as_deref() else {
            continue;
        };

        let output = if cfg!(unix) {
            Command::new("sh")
                .arg("-c")
                .arg(cmd_str)
                .current_dir(cwd)
                .output()
        } else {
            Command::new("cmd")
                .args(["/C", cmd_str])
                .current_dir(cwd)
                .output()
        }
        .map_err(|e| MaccError::Io {
            path: cwd.to_string_lossy().into(),
            action: format!("run skill step '{}'", cmd_str),
            source: e,
        })?;

        combined_stdout.push_str(&String::from_utf8_lossy(&output.stdout));
        combined_stderr.push_str(&String::from_utf8_lossy(&output.stderr));
        last_exit = output.status.code();

        if output.status.code().unwrap_or(0) != 0 {
            break;
        }
    }

    Ok((combined_stdout, combined_stderr, last_exit))
}

fn emit_event(path: &Path, event: &serde_json::Value) {
    use std::io::Write;
    if let Ok(mut file) = std::fs::OpenOptions::new().append(true).create(true).open(path) {
        if let Ok(line) = serde_json::to_string(event) {
            let _ = writeln!(file, "{}", line);
        }
    }
}
