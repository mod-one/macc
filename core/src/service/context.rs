use crate::service::interaction::InteractionHandler;
use crate::tool::ToolPerformerSpec;
use crate::{load_canonical_config, MaccError, ProjectPaths, Result};

pub fn run_generation(
    paths: &ProjectPaths,
    tool_filter: Option<&str>,
    from_files: &[String],
    dry_run: bool,
    print_prompt: bool,
    reporter: &dyn InteractionHandler,
) -> Result<usize> {
    require_apply_before_context(paths)?;

    let canonical = load_canonical_config(&paths.config_path)?;
    let mut resolved = crate::resolve::resolve(&canonical, &Default::default());
    let registry = crate::tool::ToolRegistry::from_inventory();
    if let Some(tool) = tool_filter {
        if !resolved.tools.enabled.iter().any(|id| id == tool) {
            return Err(MaccError::Validation(format!(
                "Enable {tool} before generating its project context"
            )));
        }
        // Shared files still include all enabled tools, even with a filter.
    }
    resolved = crate::resolve::with_mandatory_skills(paths, &resolved)?;
    let ctx = crate::resolve::PlanningContext {
        paths,
        resolved: &resolved,
        materialized_units: &[],
    };
    let mut plan = crate::project_context::plan_tools(&ctx, &registry)?;
    let mut targets = std::collections::BTreeSet::new();
    for tool in &resolved.tools.enabled {
        if tool_filter.is_some_and(|selected| selected != tool) {
            continue;
        }
        if let Some(adapter) = registry.get(tool) {
            targets.extend(adapter.context_file_targets(&ctx)?);
        }
    }
    if targets.is_empty() {
        return Err(MaccError::Validation(
            "No enabled native context target; check the tool's context settings".into(),
        ));
    }
    let mut imports = std::collections::BTreeSet::new();
    for path in from_files {
        crate::project_ownership::validate_path(paths, path)?;
        if !paths.root.join(path).is_file() {
            return Err(MaccError::Validation(format!(
                "Context source is unavailable: {path}"
            )));
        }
        imports.insert(path);
    }
    for action in &mut plan.actions {
        if let crate::plan::Action::WriteFile { path, content, .. } = action {
            if targets.contains(path) && !imports.is_empty() {
                content.extend_from_slice(b"\n## Additional project sources\n");
                for source in &imports {
                    content.extend_from_slice(format!("- `{source}`\n").as_bytes());
                }
            }
            if print_prompt && targets.contains(path) {
                reporter.info(&String::from_utf8_lossy(content));
            }
        }
    }
    plan.actions.retain(|action| match action {
        crate::plan::Action::WriteFile { path, .. } => {
            targets.contains(path)
                || (path.ends_with(".md")
                    && !path.ends_with("SKILL.md")
                    && registry
                        .list_ids()
                        .iter()
                        .filter_map(|id| registry.get(id))
                        .any(|a| a.context_reference_files().contains(path)))
        }
        crate::plan::Action::Noop { .. } => true,
        _ => false,
    });
    for warning in &plan.warnings {
        reporter.warn(warning);
    }
    if dry_run {
        reporter.info(&plan.render_summary(&paths.root));
    } else {
        crate::project_context::validate_writes(&plan)?;
        let operations = crate::plan::collect_plan_operations(paths, &plan);
        crate::apply_operations(paths, &operations, false, |_, _, _| {})?;
        reporter.info("Context files generated deterministically; no AI invocation was needed.");
    }
    Ok(targets.len())
}

pub fn context_apply_marker_path(paths: &ProjectPaths) -> std::path::PathBuf {
    paths
        .macc_dir
        .join("state")
        .join("context_ready_after_apply")
}

pub fn require_apply_before_context(paths: &ProjectPaths) -> Result<()> {
    let marker = context_apply_marker_path(paths);
    if marker.exists() {
        return Ok(());
    }
    Err(MaccError::Validation(
        "macc context is locked until at least one successful 'macc apply' has completed in this project.".into(),
    ))
}

pub fn mark_apply_completed(paths: &ProjectPaths) -> Result<()> {
    let marker = context_apply_marker_path(paths);
    if let Some(parent) = marker.parent() {
        std::fs::create_dir_all(parent).map_err(|e| MaccError::Io {
            path: parent.to_string_lossy().into(),
            action: "create apply marker directory".into(),
            source: e,
        })?;
    }
    std::fs::write(&marker, b"applied\n").map_err(|e| MaccError::Io {
        path: marker.to_string_lossy().into(),
        action: "write apply marker".into(),
        source: e,
    })?;
    Ok(())
}

/// Public entry point for invoking a tool with a prompt string.
///
/// Used by explicit AI operations such as PRD audit. Context generation is deterministic.
/// Pass `logger` to capture the tool's stdout into the coordinator log.
pub fn invoke_tool_with_prompt(
    paths: &ProjectPaths,
    performer: &ToolPerformerSpec,
    run: &ToolRun,
    prompt: &str,
    logger: Option<&dyn crate::coordinator::control_plane::CoordinatorLog>,
) -> crate::Result<()> {
    invoke_context_tool(paths, performer, run, prompt, logger)
}

/// Model and effort of a direct tool call (no performer script involved).
#[derive(Debug, Clone, Default, PartialEq)]
pub struct ToolRun {
    pub model: String,
    pub effort: String,
}

impl ToolRun {
    /// `explicit_model` wins, then the tier named by `MACC_MODEL_TIER`
    /// (`--model-tier`), then the tool's configured model and effort. Empty
    /// values leave the choice to the tool itself.
    pub fn resolve(
        spec: &crate::tool::ToolSpec,
        canonical: &crate::config::CanonicalConfig,
        explicit_model: Option<&str>,
    ) -> Self {
        let tier = std::env::var("MACC_MODEL_TIER").ok();
        Self::resolve_with_tier(spec, canonical, explicit_model, tier.as_deref())
    }

    fn resolve_with_tier(
        spec: &crate::tool::ToolSpec,
        canonical: &crate::config::CanonicalConfig,
        explicit_model: Option<&str>,
        tier: Option<&str>,
    ) -> Self {
        let nonempty = |value: Option<String>| value.filter(|v| !v.trim().is_empty());
        let tier = tier
            .filter(|name| !name.trim().is_empty())
            .and_then(|name| {
                canonical
                    .tools
                    .config
                    .get(&spec.id)
                    .and_then(|cfg| cfg.get("model_tiers"))
                    .and_then(|tiers| tiers.get(name))
                    .and_then(|value| {
                        serde_json::from_value::<crate::tool::ModelTierSpec>(value.clone()).ok()
                    })
                    .or_else(|| spec.model_tiers.get(name).cloned())
            });
        let explicit = nonempty(explicit_model.map(str::to_owned));
        // An explicit model is not paired with a tier's effort.
        let tier = tier.filter(|_| explicit.is_none());
        let model = explicit
            .or_else(|| nonempty(tier.as_ref().map(|t| t.model.clone())))
            .or_else(|| nonempty(crate::worktree::resolve_tool_model(spec, canonical)))
            .unwrap_or_default();
        let effort = nonempty(tier.and_then(|t| t.effort))
            .or_else(|| nonempty(crate::worktree::resolve_tool_effort(spec, canonical)))
            .unwrap_or_default();
        Self { model, effort }
    }
}

fn invoke_context_tool(
    paths: &ProjectPaths,
    performer: &ToolPerformerSpec,
    run: &ToolRun,
    prompt: &str,
    logger: Option<&dyn crate::coordinator::control_plane::CoordinatorLog>,
) -> Result<()> {
    if !command_exists(&performer.command) {
        return Err(MaccError::Validation(format!(
            "Tool command '{}' not found in PATH. Run 'macc doctor' and install/login the tool first.",
            performer.command
        )));
    }

    let mut cmd = std::process::Command::new(&performer.command);
    cmd.env_remove("MACC_WEB_ACCESS_TOKEN");
    cmd.current_dir(&paths.root);
    // The spec's arguments are a template: a literal `{model}` must not reach the tool.
    cmd.args(crate::tool::render_performer_args(
        &performer.args,
        &run.model,
        &run.effort,
    )?);
    if performer.effort_config.is_none() && !run.effort.is_empty() {
        if let Some(flag) = &performer.effort_flag {
            cmd.arg(flag).arg(&run.effort);
        }
    }

    let prompt_mode = performer
        .prompt
        .as_ref()
        .map(|p| p.mode.as_str())
        .unwrap_or("stdin");

    match prompt_mode {
        "arg" => {
            let arg = performer
                .prompt
                .as_ref()
                .and_then(|p| p.arg.as_ref())
                .ok_or_else(|| {
                    MaccError::Validation(format!(
                        "Tool '{}' prompt mode is 'arg' but no prompt arg is configured.",
                        performer.command
                    ))
                })?;
            cmd.arg(arg);
            cmd.arg(prompt);
            let output = cmd.output().map_err(|e| MaccError::Io {
                path: performer.command.clone(),
                action: "run tool context generation command".into(),
                source: e,
            })?;
            validate_context_tool_exit(&performer.command, output, logger)
        }
        "stdin" => {
            use std::io::Write;
            let mut child = cmd
                .stdin(std::process::Stdio::piped())
                .stdout(std::process::Stdio::piped())
                .stderr(std::process::Stdio::piped())
                .spawn()
                .map_err(|e| MaccError::Io {
                    path: performer.command.clone(),
                    action: "spawn tool context generation command".into(),
                    source: e,
                })?;
            if let Some(mut stdin) = child.stdin.take() {
                stdin
                    .write_all(prompt.as_bytes())
                    .map_err(|e| MaccError::Io {
                        path: performer.command.clone(),
                        action: "write prompt to tool stdin".into(),
                        source: e,
                    })?;
            }
            let output = child.wait_with_output().map_err(|e| MaccError::Io {
                path: performer.command.clone(),
                action: "wait for tool context generation command".into(),
                source: e,
            })?;
            validate_context_tool_exit(&performer.command, output, logger)
        }
        other => Err(MaccError::Validation(format!(
            "Unsupported prompt mode '{}' for tool '{}'.",
            other, performer.command
        ))),
    }
}

fn validate_context_tool_exit(
    command: &str,
    output: std::process::Output,
    logger: Option<&dyn crate::coordinator::control_plane::CoordinatorLog>,
) -> Result<()> {
    let stdout = String::from_utf8_lossy(&output.stdout).to_string();
    let stderr = String::from_utf8_lossy(&output.stderr).to_string();

    if let Some(log) = logger {
        for line in stdout.lines() {
            let _ = log.note(format!("- [tool stdout] {}", line));
        }
    }

    if !output.status.success() {
        let reason = if !stderr.trim().is_empty() {
            stderr.trim().to_string()
        } else if !stdout.trim().is_empty() {
            stdout.trim().to_string()
        } else {
            format!("exit status {}", output.status)
        };
        return Err(MaccError::Validation(format!(
            "Context generation command '{}' failed: {}",
            command, reason
        )));
    }
    Ok(())
}

fn command_exists(cmd: &str) -> bool {
    std::process::Command::new("sh")
        .arg("-lc")
        .arg(format!("command -v {} >/dev/null 2>&1", cmd))
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}
