//! ToolSpec-based, bounded AI execution with durable stdout/stderr.
use macc_core::config::CanonicalConfig;
use macc_core::supervisor::SupervisorConfig;
use macc_core::tool::{ToolPerformerSpec, ToolSpecLoader};
use macc_core::{MaccError, Result};
use std::path::Path;
use std::process::Stdio;
use std::time::Duration;
use tokio::io::AsyncWriteExt;

pub struct Agent {
    pub id: String,
    performer: ToolPerformerSpec,
    model: String,
    effort: String,
    timeout: Duration,
}
impl Agent {
    pub fn resolve(
        root: &Path,
        canonical: &CanonicalConfig,
        config: &SupervisorConfig,
    ) -> Result<Self> {
        let id = config
            .tool
            .clone()
            .or_else(|| {
                canonical
                    .automation
                    .coordinator
                    .as_ref()
                    .and_then(|c| c.coordinator_tool.clone())
            })
            .or_else(|| canonical.tools.enabled.first().cloned())
            .ok_or_else(|| {
                MaccError::Validation("Supervisor requires an enabled AI tool".into())
            })?;
        let (specs, _) = ToolSpecLoader::new(ToolSpecLoader::default_search_paths(root))
            .load_all_with_embedded();
        let spec = specs.into_iter().find(|s| s.id == id).ok_or_else(|| {
            MaccError::Validation(format!("Supervisor tool '{id}' has no ToolSpec"))
        })?;
        let setting = |key: &str| -> Option<String> {
            canonical
                .tools
                .config
                .get(&id)
                .and_then(|v| v.get(key))
                .and_then(|v| v.as_str())
                .map(str::to_owned)
                .or_else(|| {
                    spec.fields
                        .iter()
                        .find(|f| f.id == key)
                        .and_then(|f| f.default.as_ref())
                        .and_then(|v| v.as_str())
                        .map(str::to_owned)
                })
        };
        let model = config
            .model
            .clone()
            .or_else(|| setting("model"))
            .unwrap_or_default();
        let effort = config
            .effort
            .clone()
            .or_else(|| setting("model_reasoning_effort"))
            .or_else(|| setting("effort"))
            .unwrap_or_default();
        let performer = spec.performer.ok_or_else(|| {
            MaccError::Validation(format!("Supervisor tool '{id}' has no performer"))
        })?;
        Ok(Self {
            id,
            performer,
            model,
            effort,
            timeout: Duration::from_secs(config.intervention_timeout_seconds.max(1)),
        })
    }
    pub async fn run(&self, cwd: &Path, prompt: &str, log_dir: &Path, phase: &str) -> Result<()> {
        std::fs::create_dir_all(log_dir).map_err(|e| io(log_dir, e))?;
        std::fs::write(log_dir.join(format!("{phase}-prompt.txt")), prompt)
            .map_err(|e| io(log_dir, e))?;
        if let Some(spec) = &self.performer.model_config {
            macc_core::supervisor::tool_settings::write(cwd, spec, &self.model)?;
        }
        if let Some(spec) = &self.performer.effort_config {
            macc_core::supervisor::tool_settings::write(cwd, spec, &self.effort)?;
        }
        let mut cmd = tokio::process::Command::new(&self.performer.command);
        cmd.args(render_args(
            &self.performer.args,
            &self.model,
            &self.effort,
        )?)
        .current_dir(cwd)
        .env("MACC_INTERNAL_INVOCATION", "1")
        .kill_on_drop(true)
        .stdout(Stdio::from(
            std::fs::File::create(log_dir.join(format!("{phase}-stdout.log")))
                .map_err(|e| io(log_dir, e))?,
        ))
        .stderr(Stdio::from(
            std::fs::File::create(log_dir.join(format!("{phase}-stderr.log")))
                .map_err(|e| io(log_dir, e))?,
        ));
        if self.performer.effort_config.is_none() && !self.effort.is_empty() {
            if let Some(flag) = &self.performer.effort_flag {
                cmd.arg(flag).arg(&self.effort);
            }
        }
        let mode = self
            .performer
            .prompt
            .as_ref()
            .map(|p| p.mode.as_str())
            .unwrap_or("stdin");
        match mode {
            "arg" => {
                if let Some(arg) = self.performer.prompt.as_ref().and_then(|p| p.arg.as_ref()) {
                    cmd.arg(arg);
                }
                cmd.arg(prompt);
                cmd.stdin(Stdio::null());
            }
            "stdin" => {
                cmd.stdin(Stdio::piped());
            }
            other => {
                return Err(MaccError::Validation(format!(
                    "Unsupported supervisor prompt mode '{other}'"
                )))
            }
        }
        #[cfg(unix)]
        {
            use std::os::unix::process::CommandExt;
            cmd.as_std_mut().process_group(0);
        }
        let mut child = cmd.spawn().map_err(|e| io(cwd, e))?;
        let _group = ProcessGroup(child.id());
        let execution = async {
            if let Some(mut stdin) = child.stdin.take() {
                stdin
                    .write_all(prompt.as_bytes())
                    .await
                    .map_err(|e| io(cwd, e))?;
            }
            let status = child.wait().await.map_err(|e| io(cwd, e))?;
            if !status.success() {
                let stderr = std::fs::read_to_string(log_dir.join(format!("{phase}-stderr.log")))
                    .unwrap_or_default();
                let stdout = std::fs::read_to_string(log_dir.join(format!("{phase}-stdout.log")))
                    .unwrap_or_default();
                if let Some(error) =
                    macc_core::coordinator::error_normalizer::NormalizerRegistry::from_inventory()
                        .get(&self.id)
                        .and_then(|normalizer| {
                            normalizer.normalize(status.code().unwrap_or(1), &stderr, &stdout)
                        })
                {
                    super::report::write_json(
                        &log_dir.join("tool-error.json"),
                        &serde_json::to_value(&error)
                            .map_err(|e| MaccError::Validation(e.to_string()))?,
                    )?;
                    if error.retryable || matches!(error.error_code.as_str(), "E601" | "E602") {
                        return Err(MaccError::Coordinator {
                            code: macc_core::coordinator::error_normalizer::canonical_to_error_code(
                                &error.canonical_class,
                            ),
                            message: format!(
                                "Supervisor {} {phase} failed: {}; see {}",
                                self.id,
                                error.raw_message,
                                log_dir.display()
                            ),
                        });
                    }
                }
                return Err(MaccError::Validation(format!(
                    "Supervisor {} {phase} failed ({status}); see {}",
                    self.id,
                    log_dir.display()
                )));
            }
            Ok(())
        };
        tokio::time::timeout(self.timeout, execution)
            .await
            .map_err(|_| MaccError::Coordinator {
                code: "E101",
                message: format!("Supervisor {phase} timed out; see {}", log_dir.display()),
            })?
    }
}
pub(super) struct ProcessGroup(pub(super) Option<u32>);
impl Drop for ProcessGroup {
    fn drop(&mut self) {
        #[cfg(unix)]
        if let Some(pid) = self.0 {
            unsafe {
                libc::kill(-(pid as i32), libc::SIGKILL);
            }
        }
    }
}
fn io(path: &Path, source: std::io::Error) -> MaccError {
    MaccError::Io {
        path: path.display().to_string(),
        action: "supervisor AI execution".into(),
        source,
    }
}
pub fn render_args(args: &[String], model: &str, effort: &str) -> Result<Vec<String>> {
    let mut rendered = Vec::new();
    let mut i = 0;
    while i < args.len() {
        let a = &args[i];
        if i + 1 < args.len()
            && ((args[i + 1].contains("{model}") && model.is_empty())
                || (args[i + 1].contains("{effort}") && effort.is_empty()))
        {
            i += 2;
            continue;
        }
        let value = a.replace("{model}", model).replace("{effort}", effort);
        if value.contains('{') && value.contains('}') {
            return Err(MaccError::Validation(format!(
                "Unresolved supervisor tool argument: {value}"
            )));
        }
        rendered.push(value);
        i += 1;
    }
    Ok(rendered)
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn codex_arguments_preserve_exec_yolo_and_effort() {
        let args = [
            "--model",
            "{model}",
            "-c",
            "model_reasoning_effort=\"{effort}\"",
            "--yolo",
            "exec",
        ]
        .map(str::to_owned);
        assert_eq!(
            render_args(&args, "model-test", "high").unwrap(),
            vec![
                "--model",
                "model-test",
                "-c",
                "model_reasoning_effort=\"high\"",
                "--yolo",
                "exec"
            ]
        );
        assert_eq!(render_args(&args, "", "").unwrap(), vec!["--yolo", "exec"]);
    }
}
