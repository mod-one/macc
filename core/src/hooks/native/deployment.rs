use super::*;
use crate::{
    plan::{Action, ActionPlan, Scope},
    ProjectPaths, Result,
};
use serde_json::{json, Value};
use std::path::Path;
fn quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\\''"))
}
pub fn registration_command(root: &Path, spec: &Spec) -> String {
    format!("sh {}", quote(&root.join(&spec.launcher).to_string_lossy()))
}
pub(super) fn launcher(spec: &Spec) -> String {
    let exe = std::env::current_exe()
        .ok()
        .filter(|p| p.file_stem().is_some_and(|s| s == "macc"))
        .map(|p| p.to_string_lossy().into_owned())
        .unwrap_or_else(|| "macc".into());
    format!("#!/bin/sh\n# MACC native output bridge v1; managed by Apply.\nset -eu\nroot=$(CDPATH= cd -- \"$(dirname -- \"$0\")/../..\" && pwd)\nexec {} --cwd \"$root\" hooks dispatch --tool {}\n", quote(&exe), quote(&spec.tool))
}
fn merge(base: &mut Value, patch: Value) {
    match (base, patch) {
        (Value::Object(base), Value::Object(patch)) => {
            for (key, value) in patch {
                merge(base.entry(key).or_insert(Value::Null), value);
            }
        }
        (base, patch) => *base = patch,
    }
}
/// Preserve all unrelated settings/handlers; remove only our exact handler command.
pub(super) fn document(mut value: Value, spec: &Spec, root: &Path, active: bool) -> Result<Value> {
    if !value.is_object() {
        return Err(super::super::error(
            "Native hook configuration must be a JSON object",
        ));
    }
    let command = registration_command(root, spec);
    if let Some(hooks) = value.get_mut("hooks") {
        let hooks = hooks
            .as_object_mut()
            .ok_or_else(|| super::super::error("Native hooks must be an object"))?;
        for event in ["SessionStart", "PostToolUse"] {
            if let Some(groups) = hooks.get_mut(event) {
                let groups = groups
                    .as_array_mut()
                    .ok_or_else(|| super::super::error("Native hook event must be an array"))?;
                for group in groups.iter_mut() {
                    if let Some(handlers) = group.get_mut("hooks").and_then(Value::as_array_mut) {
                        handlers.retain(|h| {
                            let owned = h["statusMessage"] == "MACC native output bridge (managed)"
                                && h["type"] == "command"
                                && h["command"].as_str().is_some_and(|s| {
                                    s.starts_with("sh '")
                                        && s.ends_with(&format!("/{}'", spec.launcher))
                                });
                            !owned
                                && h.get("command").and_then(Value::as_str)
                                    != Some(command.as_str())
                        });
                    }
                }
                groups.retain(|g| {
                    !g.get("hooks")
                        .and_then(Value::as_array)
                        .is_some_and(Vec::is_empty)
                });
            }
        }
    }
    if active {
        if value.get("hooks").is_none() {
            value["hooks"] = json!({});
        }
        for event in ["SessionStart", "PostToolUse"] {
            if value["hooks"].get(event).is_none() {
                value["hooks"][event] = json!([]);
            }
            value["hooks"][event]
                .as_array_mut()
                .ok_or_else(|| super::super::error("Invalid native hook event"))?
                .push(json!({
                "matcher": if event == "PostToolUse" {"^Bash$"} else {".*"},
                "hooks": [{"type":"command", "command":command,"timeout":10,"statusMessage":"MACC native output bridge (managed)"}]
                }));
        }
    }
    Ok(value)
}
pub fn plan(paths: &ProjectPaths, plan: &mut ActionPlan, spec: &Spec) -> Result<()> {
    if !paths.config_path.is_file() {
        return Ok(());
    }
    let config = crate::load_canonical_config(&paths.config_path)?;
    let active = cfg!(unix)
        && enabled(&config)?
        && crate::service::preparation::selected_tool_ids(&config).contains(&spec.tool);
    let path = paths.root.join(&spec.config);
    // Inactive adapters do not own arbitrary existing provider configuration.
    // Only revisit a file carrying a registration previously deployed by MACC.
    if !active
        && !std::fs::read_to_string(&path)
            .is_ok_and(|s| s.contains("MACC native output bridge (managed)"))
    {
        return Ok(());
    }
    if path
        .symlink_metadata()
        .is_ok_and(|m| m.file_type().is_symlink())
    {
        return Err(super::super::error(format!(
            "Native hook configuration is a symlink: {}",
            path.display()
        )));
    }
    let mut value = if path.exists() {
        serde_json::from_slice(
            &std::fs::read(&path).map_err(|e| super::super::error(e.to_string()))?,
        )
        .map_err(|e| super::super::error(format!("Invalid {}: {e}", path.display())))?
    } else {
        json!({})
    };
    // Fold the adapter's generated settings into existing settings before adding hooks.
    for action in &plan.actions {
        if let Action::WriteFile { path, content, .. } = action {
            if path == &spec.config {
                let generated: Value = serde_json::from_slice(content)
                    .map_err(|e| super::super::error(e.to_string()))?;
                // Native hooks from both user settings and canonical tool config survive.
                let existing_hooks = value.get("hooks").cloned();
                let generated_hooks = generated.get("hooks").cloned();
                merge(&mut value, generated);
                if let Some(mut previous) = existing_hooks {
                    if let Some(extra) = generated_hooks {
                        let previous = previous
                            .as_object_mut()
                            .ok_or_else(|| super::super::error("Invalid existing hooks"))?;
                        for (event, groups) in extra
                            .as_object()
                            .ok_or_else(|| super::super::error("Invalid generated hooks"))?
                        {
                            let list = previous
                                .entry(event.clone())
                                .or_insert(json!([]))
                                .as_array_mut()
                                .ok_or_else(|| super::super::error("Invalid hook groups"))?;
                            for group in groups.as_array().ok_or_else(|| {
                                super::super::error("Invalid generated hook groups")
                            })? {
                                if !list.contains(group) {
                                    list.push(group.clone());
                                }
                            }
                        }
                    }
                    value["hooks"] = previous;
                }
            }
        }
    }
    let value = document(value, spec, &paths.root, active)?;
    plan.actions
        .retain(|a| !matches!(a, Action::WriteFile{path,..} if path == &spec.config));
    crate::plan::builders::write_text(
        plan,
        &spec.config,
        &format!(
            "{}\n",
            serde_json::to_string_pretty(&value).map_err(|e| super::super::error(e.to_string()))?
        ),
    );
    if active {
        crate::plan::builders::write_text(plan, &spec.launcher, &launcher(spec));
        plan.add_action(Action::SetExecutable {
            path: spec.launcher.clone(),
            scope: Scope::Project,
        });
    }
    tracing::debug!(tool=%spec.tool, active, config=%spec.config, "Native hook deployment planned");
    Ok(())
}
