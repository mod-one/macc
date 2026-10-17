//! Execution settings shared by the live task clients.
use super::{model::Task, model_routing};
use crate::{config::CanonicalConfig, tool::ToolSpec};

pub fn resolve(task: &Task, config: &CanonicalConfig, specs: &[ToolSpec]) -> (String, String) {
    let phase = task
        .task_runtime
        .current_phase
        .as_deref()
        .unwrap_or("implementation");
    let phase_override = task
        .task_runtime
        .extra
        .get("phase_tool_override")
        .and_then(|v| v.as_str());
    let tool = if matches!(phase, "review" | "fix") {
        phase_override
            .or(task.coordinator_tool.as_deref())
            .or(task.tool.as_deref())
    } else {
        task.tool.as_deref().or(task.coordinator_tool.as_deref())
    }
    .unwrap_or("");
    let decision = model_routing::decide(task, phase, config.automation.model_routing.as_ref());
    let tier = decision.tier.as_str();
    let cfg = config.tools.config.get(tool);
    let spec = specs.iter().find(|spec| spec.id == tool);
    let selected_tier = if decision.mode == "auto" {
        cfg.and_then(|cfg| cfg.get("model_tiers"))
            .and_then(|tiers| tiers.get(tier))
            .and_then(|value| {
                serde_json::from_value::<crate::tool::ModelTierSpec>(value.clone()).ok()
            })
            .or_else(|| spec.and_then(|spec| spec.model_tiers.get(tier)).cloned())
    } else {
        None
    };
    let nonempty = |value: Option<&serde_json::Value>| {
        value
            .and_then(|v| v.as_str())
            .filter(|v| !v.trim().is_empty())
            .map(str::to_owned)
    };
    let model = selected_tier
        .as_ref()
        .map(|tier| tier.model.clone())
        .filter(|v| !v.trim().is_empty())
        .or_else(|| nonempty(cfg.and_then(|cfg| cfg.get("model"))))
        .or_else(|| {
            nonempty(cfg.and_then(|cfg| {
                cfg.pointer("/settings/model_name")
                    .or_else(|| cfg.pointer("/settings/model"))
            }))
        })
        .or_else(|| spec.and_then(|spec| crate::worktree::resolve_tool_model(spec, config)))
        .unwrap_or_else(|| {
            if tool.is_empty() {
                "-".into()
            } else {
                tier.into()
            }
        });
    let effort = selected_tier
        .as_ref()
        .and_then(|tier| tier.effort.clone())
        .filter(|v| !v.trim().is_empty())
        .or_else(|| spec.and_then(|spec| crate::worktree::resolve_tool_effort(spec, config)))
        .unwrap_or_else(|| "-".into());
    (model, effort)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn tier_model_and_effort_follow_the_current_phase_tool() {
        let mut config = CanonicalConfig::default();
        config.tools.config.insert(
            "worker-tool".into(),
            json!({"model_tiers":{"standard":{"model":"worker-model","effort":"medium"}}}),
        );
        config.tools.config.insert(
            "phase-tool".into(),
            json!({"model_tiers":{"standard":{"model":"review-model","effort":"high"}}}),
        );
        let mut task = Task::default();
        task.tool = Some("worker-tool".into());
        assert_eq!(
            resolve(&task, &config, &[]),
            ("worker-model".into(), "medium".into())
        );
        task.task_runtime.current_phase = Some("review".into());
        task.task_runtime
            .extra
            .insert("phase_tool_override".into(), json!("phase-tool"));
        // Explicit standard tier makes this independent of review phase routing defaults.
        task.extra
            .insert("routing_hints".into(), json!({"model_tier":"standard"}));
        config.tools.config.get_mut("phase-tool").unwrap()["model_tiers"]["mini"] =
            json!({"model":"review-model","effort":"high"});
        config.tools.config.get_mut("phase-tool").unwrap()["model_tiers"]["heavy"] =
            json!({"model":"review-model","effort":"high"});
        assert_eq!(
            resolve(&task, &config, &[]),
            ("review-model".into(), "high".into())
        );
    }

    #[test]
    fn manual_effort_uses_the_tool_configuration_field() {
        let (specs, _) = crate::tool::ToolSpecLoader::new(Vec::new()).load_all_with_embedded();
        let mut spec = specs
            .into_iter()
            .find(|spec| {
                spec.performer
                    .as_ref()
                    .and_then(|p| p.effort_config.as_ref())
                    .is_some()
            })
            .expect("embedded tool with effort configuration");
        let original_id = spec.id.clone();
        spec.id = "test-tool".into();
        for field in &mut spec.fields {
            if let Some(pointer) = &mut field.pointer {
                *pointer = pointer.replace(&original_id, "test-tool");
            }
        }
        let key = spec
            .performer
            .as_ref()
            .unwrap()
            .effort_config
            .as_ref()
            .unwrap()
            .key
            .clone();
        let mut config = CanonicalConfig::default();
        config.automation.model_routing =
            Some(serde_json::from_value(json!({"mode":"manual"})).unwrap());
        let mut settings = json!({"model":"manual-model"});
        settings[&key] = json!("high");
        config.tools.config.insert("test-tool".into(), settings);
        let mut task = Task::default();
        task.tool = Some("test-tool".into());
        assert_eq!(
            resolve(&task, &config, &[spec]),
            ("manual-model".into(), "high".into())
        );
    }

    #[test]
    fn manual_mode_and_missing_effort_do_not_invent_a_tier_effort() {
        let mut config = CanonicalConfig::default();
        config.automation.model_routing =
            Some(serde_json::from_value(json!({"mode":"manual"})).unwrap());
        config.tools.config.insert("test-tool".into(), json!({"model":"base-model","model_tiers":{"standard":{"model":"tier-model","effort":"high"}}}));
        let mut task = Task::default();
        task.tool = Some("test-tool".into());
        assert_eq!(
            resolve(&task, &config, &[]),
            ("base-model".into(), "-".into())
        );
        assert_eq!(
            resolve(&Task::default(), &config, &[]),
            ("-".into(), "-".into())
        );
    }
}
