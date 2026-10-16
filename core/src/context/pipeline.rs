//! Model-facing execution seam. Adapter hooks are optional; coordinator reduction is the fallback.
use crate::{
    config::CanonicalConfig,
    skills_runner::{SkillDefinition, SkillRunRequest, SkillRunResult, SummaryMetadata},
    tool::ToolAdapter,
    ProjectPaths, Result,
};
pub struct ToolOutput<'a> {
    pub root: &'a std::path::Path,
    pub skill: &'a SkillDefinition,
    pub raw: &'a str,
    pub bundles: &'a [String],
}
pub fn execute(
    paths: &ProjectPaths,
    skill: &SkillDefinition,
    request: &SkillRunRequest,
    adapter: Option<&dyn ToolAdapter>,
) -> Result<SkillRunResult> {
    let config = if paths.config_path.exists() {
        crate::load_canonical_config(&paths.config_path)?
    } else {
        CanonicalConfig::default()
    };
    let mut result =
        crate::skills_runner::SkillRunner::run(skill, request, &paths.macc_dir.join("log/run"))?;
    apply(paths, skill, request, &mut result, adapter, &config)?;
    Ok(result)
}
pub fn apply(
    paths: &ProjectPaths,
    skill: &SkillDefinition,
    request: &SkillRunRequest,
    result: &mut SkillRunResult,
    adapter: Option<&dyn ToolAdapter>,
    config: &CanonicalConfig,
) -> Result<()> {
    if request.watch || result.stdout.is_empty() && result.stderr.is_empty() {
        return Ok(());
    }
    let o2 = crate::o2::policy::from_config(config).context_engine;
    let settings = config
        .context
        .as_ref()
        .and_then(|c| c.summarization.as_ref());
    let o2_active = o2.enabled && o2.output_reduction;
    let enabled = o2_active || settings.is_some_and(|s| s.enabled);
    let tool = settings.and_then(|s| request.tool_id.as_ref().and_then(|id| s.per_tool.get(id)));
    if !enabled || tool.is_some_and(|t| !t.enabled) {
        return Ok(());
    }
    if super::reduction::is_protocol(&result.stdout)
        || super::reduction::is_protocol(&result.stderr)
    {
        tracing::info!(skill=%skill.id,"Protocol output preserved without model-facing reduction");
        return Ok(());
    }
    let mut bundles = settings
        .map(|s| {
            s.per_skill
                .get(&skill.id)
                .map(|p| p.bundles.clone())
                .filter(|v| !v.is_empty())
                .unwrap_or_else(|| s.default_bundles.clone())
        })
        .unwrap_or_default();
    if o2_active && bundles.is_empty() {
        bundles.push("log-grep-error-first".into());
    }
    let legacy_limit = tool
        .and_then(|t| t.token_budget)
        .or_else(|| {
            config
                .context
                .as_ref()
                .and_then(|c| c.token_budget.as_ref())
                .map(|t| t.tool_output)
        })
        .unwrap_or(4000);
    let tokens = if o2_active {
        legacy_limit.min(o2.pull_limit_tokens as usize)
    } else {
        legacy_limit
    };
    let bytes = if o2_active {
        o2.max_output_bytes as usize
    } else {
        usize::MAX
    };
    let raw_size = result.stdout.len() + result.stderr.len();
    let raw_ref = result
        .log_path
        .as_ref()
        .map(|p| p.with_extension("log"))
        .filter(|p| p.is_file())
        .and_then(|p| {
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                if let Err(e) = std::fs::set_permissions(&p, std::fs::Permissions::from_mode(0o600))
                {
                    tracing::warn!(error=%e,"Raw artifact permissions could not be restricted");
                    return None;
                }
            }
            p.strip_prefix(&paths.root)
                .ok()
                .map(|p| p.to_string_lossy().into_owned())
        });
    let channels = usize::from(!result.stdout.is_empty()) + usize::from(!result.stderr.is_empty());
    let mut hook_used = false;
    for raw in [&mut result.stdout, &mut result.stderr] {
        if raw.is_empty() {
            continue;
        }
        let hook = if let Some(adapter) = adapter.filter(|_| raw.len() <= 8 * 1024 * 1024) {
            match adapter.transform_tool_output(&ToolOutput {
                root: &paths.root,
                skill,
                raw,
                bundles: &bundles,
            }) {
                Ok(value) => value,
                Err(e) => {
                    tracing::warn!(error=%e,skill=%skill.id,"Adapter output hook failed; coordinator fallback selected");
                    None
                }
            }
        } else {
            None
        };
        hook_used |= hook.is_some();
        if o2_active {
            let reduced = crate::o2::output::render(
                &paths.root,
                raw,
                &bundles,
                tokens / channels,
                bytes / channels,
                hook,
                raw_ref.clone(),
            )?;
            *raw = reduced.summary;
        } else {
            let text = hook
                .map(Ok)
                .unwrap_or_else(|| super::reduction::apply(raw, &bundles))?;
            *raw = super::enforce_budget(&text, tokens / channels).0;
        }
    }
    if hook_used {
        bundles.push("adapter_output_hook".into());
    }
    result.summary = Some(SummaryMetadata {
        raw_size_chars: raw_size,
        summary_size_chars: result.stdout.len() + result.stderr.len(),
        bundles_applied: bundles,
        was_truncated: raw_size > result.stdout.len() + result.stderr.len(),
    });
    Ok(())
}
