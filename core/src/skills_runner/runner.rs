use super::model::*;
use crate::{MaccError, Result};
use chrono::Utc;
use std::io::Write;
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

        // Spec §3.10: two log files — .jsonl for structured events, .log for raw stdout/stderr.
        let jsonl_path = log_dir.join(format!("{}.jsonl", log_base));
        let log_path = log_dir.join(format!("{}.log", log_base));

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
                if request.watch {
                    // In watch mode, stream directly to the terminal so the user sees live output.
                    run_local_commands_streaming(skill, &request.cwd, &log_path)?
                } else {
                    let (out, err, code) = run_local_commands_captured(skill, &request.cwd)?;
                    write_log_file(&log_path, &out, &err);
                    (out, err, code)
                }
            }
            SkillKind::Prompt | SkillKind::Agent | SkillKind::Coordinator => {
                // Prompt skills require a tool adapter invocation.  Without an adapter
                // the runner emits a human-readable message rather than silently succeeding.
                let msg = match &request.tool_id {
                    Some(tool) => format!(
                        "Prompt skill '{}' requires tool adapter '{}'. \
                         Run 'macc apply' to ensure the adapter is configured, \
                         then retry with 'macc run {} --tool {}'.",
                        skill.id, tool, skill.id, tool
                    ),
                    None => format!(
                        "Prompt skill '{}' requires a tool adapter. \
                         Specify one with --tool, or configure skills.run_policy.default_tool \
                         in .macc/macc.yaml.",
                        skill.id
                    ),
                };
                tracing::warn!("{}", msg);
                write_log_file(&log_path, &msg, "");
                (msg, String::new(), Some(0))
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
            // `summary` is populated by engine.run_skill() after the hook pipeline runs.
            summary: None,
        })
    }
}

fn run_local_commands_captured(
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

        let output = shell_command(cmd_str, cwd)
            .output()
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

/// Stream each command to the terminal (`--watch` mode).  Returns empty stdout/stderr
/// since the output went directly to the terminal; the .log file captures a header only.
fn run_local_commands_streaming(
    skill: &SkillDefinition,
    cwd: &Path,
    log_path: &Path,
) -> Result<(String, String, Option<i32>)> {
    let mut last_exit = Some(0i32);
    let mut log_header = format!("# Skill: {} (streaming mode)\n", skill.id);

    for step in &skill.steps {
        let Some(cmd_str) = step.run.as_deref() else {
            continue;
        };

        tracing::debug!("$ {}", cmd_str);
        log_header.push_str(&format!("$ {}\n", cmd_str));

        let status = shell_command(cmd_str, cwd)
            .status()
            .map_err(|e| MaccError::Io {
                path: cwd.to_string_lossy().into(),
                action: format!("run skill step '{}'", cmd_str),
                source: e,
            })?;

        last_exit = status.code();
        if status.code().unwrap_or(0) != 0 {
            break;
        }
    }

    write_log_file(log_path, &log_header, "");
    Ok((String::new(), String::new(), last_exit))
}

fn shell_command(cmd_str: &str, cwd: &Path) -> Command {
    let mut cmd = if cfg!(unix) {
        let mut c = Command::new("sh");
        c.arg("-c").arg(cmd_str);
        c
    } else {
        let mut c = Command::new("cmd");
        c.args(["/C", cmd_str]);
        c
    };
    cmd.current_dir(cwd);
    cmd
}

/// Write combined stdout/stderr to the plain-text `.log` file (spec §3.10).
fn write_log_file(path: &Path, stdout: &str, stderr: &str) {
    if let Ok(mut f) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
    {
        if !stdout.is_empty() {
            let _ = write!(f, "{}", stdout);
        }
        if !stderr.is_empty() {
            let _ = write!(f, "\n--- stderr ---\n{}", stderr);
        }
    }
}

fn emit_event(path: &Path, event: &serde_json::Value) {
    if let Ok(mut file) = std::fs::OpenOptions::new()
        .append(true)
        .create(true)
        .open(path)
    {
        if let Ok(line) = serde_json::to_string(event) {
            let _ = writeln!(file, "{}", line);
        }
    }
}
