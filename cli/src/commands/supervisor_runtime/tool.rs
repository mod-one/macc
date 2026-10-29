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
    root: std::path::PathBuf,
    config: SupervisorConfig,
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
        let canonical_value =
            serde_json::to_value(canonical).map_err(|e| MaccError::Validation(e.to_string()))?;
        let setting = |field: Option<&macc_core::tool::spec::FieldSpec>| -> Option<String> {
            let field = field?;
            field
                .pointer
                .as_deref()
                .and_then(|p| canonical_value.pointer(p))
                .and_then(|v| v.as_str())
                .map(str::to_owned)
                .or_else(|| {
                    canonical
                        .tools
                        .config
                        .get(&id)
                        .and_then(|v| v.get(&field.id))
                        .and_then(|v| v.as_str())
                        .map(str::to_owned)
                })
                .or_else(|| {
                    field
                        .default
                        .as_ref()
                        .and_then(|v| v.as_str())
                        .map(str::to_owned)
                })
        };
        let model = config
            .model
            .clone()
            .or_else(|| setting(spec.fields.iter().find(|f| f.id == "model")))
            .unwrap_or_default();
        let effort_key = spec
            .performer
            .as_ref()
            .and_then(|p| p.effort_config.as_ref())
            .map(|c| c.key.as_str());
        let effort_field = effort_key
            .and_then(|key| spec.fields.iter().find(|f| f.id == key))
            .or_else(|| {
                spec.fields
                    .iter()
                    .find(|f| f.id.to_ascii_lowercase().contains("effort"))
            });
        let effort = config
            .effort
            .clone()
            .or_else(|| setting(effort_field))
            .unwrap_or_default();
        let performer = spec.performer.ok_or_else(|| {
            MaccError::Validation(format!("Supervisor tool '{id}' has no performer"))
        })?;
        Ok(Self {
            root: root.into(),
            config: config.clone(),
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
        cmd.env_remove("MACC_WEB_ACCESS_TOKEN");
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
                let file = std::fs::canonicalize(log_dir.join(format!("{phase}-prompt.txt")))
                    .map_err(|e| io(log_dir, e))?;
                cmd.arg(super::prompt::argument(prompt, &file));
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
        let started = std::time::Instant::now();
        let agent_pid = child.id();
        let execution = async {
            if let Some(mut stdin) = child.stdin.take() {
                stdin
                    .write_all(prompt.as_bytes())
                    .await
                    .map_err(|e| io(cwd, e))?;
            }
            let mut interval = tokio::time::interval(Duration::from_secs(5));
            let status = loop {
                tokio::select! {
                    result=child.wait()=>break result.map_err(|e|io(cwd,e))?,
                    _=interval.tick()=>super::activity::record(&self.root,&self.config,log_dir,phase,agent_pid,started.elapsed().as_secs(),"running")?,
                }
            };
            super::activity::record(
                &self.root,
                &self.config,
                log_dir,
                phase,
                agent_pid,
                started.elapsed().as_secs(),
                if status.success() { "exited" } else { "failed" },
            )?;
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
        let outcome = tokio::time::timeout(self.timeout, execution).await;
        if outcome.is_err() {
            super::activity::record(
                &self.root,
                &self.config,
                log_dir,
                phase,
                agent_pid,
                started.elapsed().as_secs(),
                "timed_out",
            )?;
        }
        outcome.map_err(|_| MaccError::Coordinator {
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
    macc_core::tool::render_performer_args(args, model, effort)
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn nested_model_and_effort_pointers_are_inherited_and_overridden() {
        let temp = tempfile::tempdir().unwrap();
        let dir = temp.path().join(".macc/tools.d");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            dir.join("fake.tool.yaml"),
            r#"api_version: v1
id: fake
display_name: Fake
fields:
  - id: model
    label: Model
    kind: {type: text}
    pointer: /tools/config/fake/model/name
    default: fallback
  - id: effortLevel
    label: Effort
    kind: {type: text}
    pointer: /tools/config/fake/effortLevel
    default: medium
performer:
  command: unused
  runner: unused
  args: []
  prompt: {mode: arg}
"#,
        )
        .unwrap();
        let mut canonical = CanonicalConfig::default();
        canonical.tools.enabled = vec!["fake".into()];
        canonical.tools.config.insert(
            "fake".into(),
            serde_json::json!({"model":{"name":"nested-model"},"effortLevel":"high"}),
        );
        let mut config = SupervisorConfig::default();
        let agent = Agent::resolve(temp.path(), &canonical, &config).unwrap();
        assert_eq!(agent.model, "nested-model");
        assert_eq!(agent.effort, "high");
        config.model = Some("override".into());
        config.effort = Some("low".into());
        let agent = Agent::resolve(temp.path(), &canonical, &config).unwrap();
        assert_eq!(
            (agent.model.as_str(), agent.effort.as_str()),
            ("override", "low")
        );
    }
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
    #[tokio::test]
    async fn agy_receives_supervisor_model_as_argument() {
        use std::os::unix::fs::PermissionsExt;
        let temp = tempfile::tempdir().unwrap();
        let capture = temp.path().join("argv.txt");
        let fake = temp.path().join("fake-agy.sh");
        std::fs::write(
            &fake,
            format!(
                "#!/bin/sh\nprintf '%s\\n' \"$@\" > '{}'\n",
                capture.display()
            ),
        )
        .unwrap();
        std::fs::set_permissions(&fake, std::fs::Permissions::from_mode(0o755)).unwrap();
        let (specs, _) = ToolSpecLoader::new(vec![]).load_all_with_embedded();
        let mut performer = specs
            .into_iter()
            .find(|s| s.id == "agy")
            .and_then(|s| s.performer)
            .unwrap();
        assert!(performer.model_config.is_none());
        performer.command = fake.display().to_string();
        let mut config = SupervisorConfig::default();
        config.model = Some("Gemini 3.8 Flash (Medium)".into());
        let agent = Agent {
            root: temp.path().into(),
            config,
            id: "agy".into(),
            performer,
            model: "Gemini 3.8 Flash (Medium)".into(),
            effort: String::new(),
            timeout: Duration::from_secs(10),
        };
        agent
            .run(temp.path(), "hello", &temp.path().join("log"), "t")
            .await
            .unwrap();
        let argv = std::fs::read_to_string(&capture).unwrap();
        let argv: Vec<&str> = argv.lines().collect();
        let pos = argv.iter().position(|a| *a == "--model").unwrap();
        assert_eq!(argv[pos + 1], "Gemini 3.8 Flash (Medium)");
        assert!(!temp.path().join(".agents/settings.json").exists());
    }
}
