use super::*;
use crate::{config::CanonicalConfig, Result};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Catalog {
    version: u32,
    hooks: Vec<Hook>,
}
pub fn catalog(config: &CanonicalConfig) -> Result<Vec<Hook>> {
    let base: Catalog = serde_json::from_str(include_str!("../../../catalog/hooks.catalog.json"))
        .map_err(|e| error(format!("Invalid bundled hook catalogue: {e}")))?;
    if base.version != 1 {
        return Err(error("Unsupported hook catalogue version"));
    }
    let mut hooks = base.hooks;
    if let Some(config) = &config.hooks {
        for external in &config.external {
            validate_external(external)?;
            if hooks.iter().any(|h| h.id == external.definition.id) {
                return Err(error(format!(
                    "Duplicate or reserved hook ID: {}",
                    external.definition.id
                )));
            }
            hooks.push(external.definition.clone());
        }
    }
    if hooks.len() > 128 {
        return Err(error("Hook catalogue exceeds 128 entries"));
    }
    Ok(hooks)
}
pub(super) fn validate_external(external: &ExternalHook) -> Result<()> {
    let h = &external.definition;
    if h.id.is_empty()
        || h.id.len() > 80
        || !h
            .id
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
    {
        return Err(error(
            "Hook ID must contain 1–80 lowercase letters, digits or hyphens",
        ));
    }
    if [
        &h.name,
        &h.description,
        &h.version,
        &h.source.repository,
        &h.source.subpath,
    ]
    .iter()
    .any(|s| s.chars().any(char::is_control))
    {
        return Err(error(
            "Hook metadata cannot contain terminal control characters",
        ));
    }
    if h.name.is_empty()
        || h.name.len() > 120
        || h.description.len() > 2048
        || h.version.is_empty()
        || h.version.len() > 80
        || h.source.repository.trim().is_empty()
        || h.source.subpath.trim().is_empty()
        || h.source.repository.len() > 2048
        || h.source.subpath.len() > 2048
        || h.source.distribution != "snapshot"
        || h.skills.len() > 64
        || h.skills
            .iter()
            .any(|s| s.is_empty() || s.len() > 120 || s.chars().any(char::is_control))
    {
        return Err(error(
            "Invalid external hook metadata; distribution must be snapshot",
        ));
    }
    if h.default_enabled {
        return Err(error(
            "External hooks must be disabled by default; review and enable explicitly",
        ));
    }
    if external.sha256 != checksum(h)? {
        return Err(error(
            "External hook checksum mismatch; import the reviewed manifest again",
        ));
    }
    let Handler::Rules { steps } = &h.handler else {
        return Err(error(
            "External hooks require declarative rules; builtin IDs are reserved",
        ));
    };
    if steps.is_empty() || steps.len() > 16 {
        return Err(error("External hooks require 1–16 steps"));
    }
    for step in steps {
        let (Step::RetainLines { pattern } | Step::Replace { pattern, .. }) = step;
        if pattern.len() > 4096 {
            return Err(error("Hook pattern exceeds 4096 bytes"));
        }
        regex::RegexBuilder::new(pattern)
            .size_limit(1024 * 1024)
            .build()
            .map_err(|e| error(format!("Invalid hook pattern: {e}")))?;
        if let Step::Replace { replacement, .. } = step {
            if replacement.len() > 4096 {
                return Err(error("Hook replacement exceeds 4096 bytes"));
            }
        }
    }
    Ok(())
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HookState {
    pub hook: Hook,
    pub enabled: bool,
    pub manual: Option<bool>,
    pub required_by: Vec<String>,
    pub integration: String,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Status {
    pub revision: String,
    pub hooks: Vec<HookState>,
    pub tools: Vec<String>,
}
fn owners(config: &CanonicalConfig, id: &str) -> Vec<String> {
    let mut owners = BTreeSet::new();
    let context = crate::o2::policy::from_config(config).context_engine;
    if id == "performer-log-summary" && context.enabled && context.output_reduction {
        owners.insert("O2 context engine: output reduction".to_string());
    }
    if config.automation.supervisor.is_some() {
        match id {
            "coordinator-event-summarizer" => {
                owners.insert("MACC supervisor configuration: coordinator evidence".to_string());
            }
            "performer-log-summary" => {
                owners.insert("MACC supervisor configuration: performer evidence".to_string());
            }
            _ => {}
        }
    }
    if let Some(s) = config
        .context
        .as_ref()
        .and_then(|c| c.summarization.as_ref())
        .filter(|s| s.enabled || context.enabled && context.output_reduction)
    {
        let uses = s
            .default_bundles
            .iter()
            .chain(s.per_skill.values().flat_map(|s| s.bundles.iter()));
        if uses.into_iter().any(|b| canonical_id(b) == id) {
            owners.insert("Context summarization: configured output contract".to_string());
        }
    }
    owners.into_iter().collect()
}
pub(crate) fn canonical_id(id: &str) -> &str {
    match id {
        "test" => "test-output-failures-only",
        "lint" => "lint-errors-only",
        "trace" => "stacktrace-collapse",
        "diff" => "git-diff-stat-before-full-diff",
        "log" => "log-grep-error-first",
        _ => id,
    }
}
pub fn status(config: &CanonicalConfig) -> Result<Status> {
    let hooks = catalog(config)?.into_iter().map(|hook| {
        let required_by = owners(config, &hook.id);
        let manual = config.hooks.as_ref().and_then(|c| c.overrides.get(&hook.id)).copied();
        let enabled = !required_by.is_empty() || manual.unwrap_or(hook.default_enabled);
        HookState { hook, enabled, manual, required_by, integration: "MACC-owned executions; all adapters; tool-internal commands are not intercepted".into() }
    }).collect();
    Ok(Status {
        revision: crate::config::transaction::revision(config)?,
        hooks,
        tools: config.tools.enabled.clone(),
    })
}
pub fn validate(config: &CanonicalConfig) -> Result<()> {
    let status = status(config)?;
    if let Some(settings) = &config.hooks {
        for id in settings.overrides.keys() {
            status
                .hooks
                .iter()
                .find(|h| h.hook.id == *id)
                .ok_or_else(|| error(format!("Unknown hook: {id}")))?;
            // Feature claims always win in effective activation, including a manual
            // opt-out saved before the feature was enabled.
        }
    }
    Ok(())
}
