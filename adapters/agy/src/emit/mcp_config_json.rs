use crate::map::AgyConfig;
use macc_adapter_shared::render::format::render_json_pretty;
use serde_json::{Map as JsonMap, Value as JsonValue};

pub fn render_mcp_config_json(config: &AgyConfig) -> String {
    let mut root = JsonMap::new();
    let mut servers = JsonMap::new();

    for (name, spec) in &config.mcp_servers {
        let mut mapped_spec = spec.clone();
        if let JsonValue::Object(ref mut spec_map) = mapped_spec {
            let url = spec_map.remove("url")
                .or_else(|| spec_map.remove("httpUrl"))
                .or_else(|| spec_map.remove("serverUrl"))
                .or_else(|| spec_map.remove("serverURL"));
            if let Some(u) = url {
                spec_map.insert("serverURL".to_string(), u);
            }
        }
        servers.insert(name.clone(), mapped_spec);
    }

    root.insert("mcpServers".to_string(), JsonValue::Object(servers));
    render_json_pretty(&JsonValue::Object(root))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::collections::BTreeMap;

    #[test]
    fn test_render_mcp_config_json() {
        let mut mcp_servers = BTreeMap::new();
        mcp_servers.insert(
            "test-server".to_string(),
            json!({
                "url": "https://mcp.example.com",
                "auth": "oauth"
            }),
        );

        let config = AgyConfig {
            model: "auto-gemini-3".to_string(),
            sandbox: true,
            skills: vec![],
            agents: vec![],
            standards_inline: BTreeMap::new(),
            standards_path: None,
            mcp_servers,
            tool_config: JsonValue::Object(JsonMap::new()),
        };

        let output = render_mcp_config_json(&config);
        assert!(output.contains("\"mcpServers\": {"));
        assert!(output.contains("\"test-server\": {"));
        assert!(output.contains("\"serverURL\": \"https://mcp.example.com\""));
    }
}
