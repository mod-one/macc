//! Adapter-owned quota identities; no provider/model names in orchestration.
use super::{
    error_normalizer::NormalizerRegistry,
    model::{Task, TaskRegistry},
};
use crate::{
    config::CanonicalConfig,
    tool::{ToolSpec, ToolSpecLoader},
};
use std::{collections::BTreeMap, path::Path};
pub type TaskKeys = BTreeMap<String, BTreeMap<String, String>>;
pub fn key(
    task: &Task,
    tool: &str,
    config: &CanonicalConfig,
    specs: &[ToolSpec],
    normalizers: &NormalizerRegistry,
) -> String {
    let Some(adapter) = normalizers.get(tool) else {
        return tool.into();
    };
    let mut selected = task.clone();
    selected.tool = Some(tool.into());
    selected.coordinator_tool = Some(tool.into());
    selected
        .task_runtime
        .extra
        .insert("phase_tool_override".into(), tool.into());
    let (model, _) = super::execution_labels::resolve(&selected, config, specs);
    adapter
        .quota_scope(&model)
        .map(|scope| format!("{tool}::{scope}"))
        .unwrap_or_else(|| tool.into())
}
pub fn keys(
    root: &Path,
    registry: &serde_json::Value,
    config: &CanonicalConfig,
    normalizers: &NormalizerRegistry,
) -> TaskKeys {
    let (specs, _) =
        ToolSpecLoader::new(ToolSpecLoader::default_search_paths(root)).load_all_with_embedded();
    let Ok(tasks) = TaskRegistry::from_value(registry) else {
        return TaskKeys::default();
    };
    tasks
        .tasks
        .iter()
        .map(|task| {
            (
                task.id.clone(),
                specs
                    .iter()
                    .map(|s| (s.id.clone(), key(task, &s.id, config, &specs, normalizers)))
                    .collect(),
            )
        })
        .collect()
}
pub fn assignment(task: &Task, tool: &str) -> String {
    task.task_runtime
        .extra
        .get("quota_scope_key")
        .and_then(|v| v.as_str())
        .filter(|key| *key == tool || key.starts_with(&format!("{tool}::")))
        .map(str::to_owned)
        .unwrap_or_else(|| tool.into())
}
pub fn assigned(registry: &serde_json::Value, task_id: &str, tool: &str) -> String {
    TaskRegistry::from_value(registry)
        .ok()
        .and_then(|r| r.find_task(task_id).map(|t| assignment(t, tool)))
        .unwrap_or_else(|| tool.into())
}

pub fn configured_keys(
    root: &Path,
    registry: &serde_json::Value,
    normalizers: &NormalizerRegistry,
) -> TaskKeys {
    crate::load_canonical_config(root.join(".macc/macc.yaml"))
        .ok()
        .map(|config| keys(root, registry, &config, normalizers))
        .unwrap_or_default()
}
pub fn available(
    registry: &super::rate_limit::ToolThrottleRegistry,
    key: &str,
    tool: &str,
    now: &str,
) -> bool {
    !super::rate_limit::is_tool_throttled(registry, tool, now)
        && !super::rate_limit::is_tool_throttled(registry, key, now)
}

/// Effective availability respects both an account-wide and a pool cooldown.
pub fn until(registry: &super::rate_limit::ToolThrottleRegistry, key: &str, tool: &str) -> u64 {
    [key, tool]
        .iter()
        .filter_map(|key| registry.get(*key).map(|s| s.throttled_until))
        .max()
        .unwrap_or(0)
}
