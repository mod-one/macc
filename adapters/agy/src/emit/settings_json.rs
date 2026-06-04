use crate::map::AgyConfig;
use macc_adapter_shared::render::format::render_json_pretty;
use serde_json::{Map as JsonMap, Value as JsonValue};

pub fn render_settings_json(config: &AgyConfig) -> String {
    let mut settings = JsonMap::new();

    // Map model and sandbox directly
    settings.insert("model".to_string(), JsonValue::String(config.model.clone()));
    settings.insert("sandbox".to_string(), JsonValue::Bool(config.sandbox));

    // If there were other options under the resolved tool config, merge them
    if let JsonValue::Object(map) = &config.tool_config {
        for (k, v) in map {
            if k != "model"
                && k != "sandbox"
                && k != "skills"
                && k != "agents"
                && k != "mcp_servers"
                && k != "model_tiers"
            {
                settings.insert(k.clone(), v.clone());
            }
        }
    }

    render_json_pretty(&JsonValue::Object(settings))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;

    #[test]
    fn test_render_settings_json() {
        let config = AgyConfig {
            model: "auto-gemini-3".to_string(),
            sandbox: true,
            skills: vec![],
            agents: vec![],
            standards_inline: BTreeMap::new(),
            standards_path: None,
            mcp_servers: BTreeMap::new(),
            tool_config: serde_json::json!({
                "model": "auto-gemini-3",
                "sandbox": true,
                "custom_option": "value"
            }),
        };

        let output = render_settings_json(&config);
        assert!(output.contains("\"model\": \"auto-gemini-3\""));
        assert!(output.contains("\"sandbox\": true"));
        assert!(output.contains("\"custom_option\": \"value\""));
    }
}
