use macc_core::resolve::ResolvedConfig;
use serde_json::{Map as JsonMap, Value as JsonValue};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Debug, Clone)]
pub struct GeminiConfig {
    pub standards_inline: BTreeMap<String, String>,
    pub standards_path: Option<String>,
    pub user_mcp_merge: bool,
    pub mcp_servers: BTreeMap<String, serde_json::Value>,
    pub tool_config: JsonValue,
}

impl GeminiConfig {
    pub fn from_resolved(resolved: &ResolvedConfig) -> Self {
        let tool_config = resolved
            .tools
            .config
            .get("gemini")
            .or_else(|| resolved.tools.specific.get("gemini"))
            .cloned()
            .unwrap_or_else(|| JsonValue::Object(JsonMap::new()));

        let mut mcp_servers = BTreeMap::new();
        let selection_ids: BTreeSet<String> = resolved.selections.mcp.iter().cloned().collect();
        for template in &resolved.mcp_templates {
            if selection_ids.contains(&template.id) {
                mcp_servers.insert(
                    template.id.clone(),
                    macc_core::mcp_json::template_to_value(template),
                );
            }
        }

        let mut standards_inline = resolved.standards.inline.clone();
        if let Some(language) = tool_config.get("language").and_then(|v| v.as_str()) {
            standards_inline.insert("language".into(), language.into());
        }
        Self {
            standards_inline,
            standards_path: resolved.standards.path.clone(),
            user_mcp_merge: read_bool(&tool_config, "/user_mcp_merge").unwrap_or(false),
            mcp_servers,
            tool_config,
        }
    }
}

fn read_bool(value: &JsonValue, pointer: &str) -> Option<bool> {
    value.pointer(pointer).and_then(|v| v.as_bool())
}

#[cfg(test)]
mod tests {
    use super::*;
    use macc_core::resolve::{ResolvedConfig, ResolvedSelectionsConfig, ResolvedStandardsConfig};

    #[test]
    fn user_mcp_merge_defaults_to_false() {
        let resolved = ResolvedConfig {
            version: "v1".to_string(),
            tools: macc_core::resolve::ResolvedToolsConfig {
                enabled: vec!["gemini".to_string()],
                ..Default::default()
            },
            standards: ResolvedStandardsConfig {
                path: None,
                inline: BTreeMap::new(),
            },
            selections: ResolvedSelectionsConfig {
                skills: Vec::new(),
                agents: Vec::new(),
                mcp: Vec::new(),
            },
            mcp_templates: Vec::new(),
            automation: macc_core::config::AutomationConfig::default(),
            settings: macc_core::config::SettingsConfig::default(),
            context: None,
            skills_run_policy: None,
        };

        let config = GeminiConfig::from_resolved(&resolved);
        assert!(!config.user_mcp_merge);
    }
}
