use macc_core::resolve::ResolvedConfig;
use serde_json::{Map as JsonMap, Value as JsonValue};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Debug, Clone)]
pub struct AgyConfig {
    pub model: String,
    pub sandbox: bool,
    pub skills: Vec<String>,
    pub agents: Vec<String>,
    pub standards_inline: BTreeMap<String, String>,
    pub standards_path: Option<String>,
    pub mcp_servers: BTreeMap<String, serde_json::Value>,
    pub tool_config: JsonValue,
}

impl AgyConfig {
    pub fn from_resolved(resolved: &ResolvedConfig) -> Self {
        let tool_config = resolved
            .tools
            .config
            .get("agy")
            .or_else(|| resolved.tools.specific.get("agy"))
            .cloned()
            .unwrap_or_else(|| JsonValue::Object(JsonMap::new()));

        let mut skills_set = BTreeSet::new();
        for skill in &resolved.selections.skills {
            skills_set.insert(skill.clone());
        }
        for skill in read_string_list(&tool_config, "/skills") {
            skills_set.insert(skill);
        }

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

        let mut agents = BTreeSet::new();
        for agent in &resolved.selections.agents {
            agents.insert(agent.clone());
        }
        for agent in read_string_list(&tool_config, "/agents") {
            agents.insert(agent);
        }

        let model = tool_config
            .pointer("/model")
            .and_then(|v| v.as_str())
            .unwrap_or("auto-gemini-3")
            .to_string();

        let sandbox = tool_config
            .pointer("/sandbox")
            .and_then(|v| v.as_bool())
            .unwrap_or(true);

        Self {
            model,
            sandbox,
            skills: skills_set.into_iter().collect(),
            agents: agents.into_iter().collect(),
            standards_inline: resolved.standards.inline.clone(),
            standards_path: resolved.standards.path.clone(),
            mcp_servers,
            tool_config,
        }
    }
}

fn read_string_list(value: &JsonValue, pointer: &str) -> Vec<String> {
    let Some(node) = value.pointer(pointer) else {
        return Vec::new();
    };
    match node {
        JsonValue::Array(items) => items
            .iter()
            .filter_map(|item| item.as_str().map(|s| s.to_string()))
            .collect(),
        JsonValue::String(text) => text
            .split(',')
            .map(|entry| entry.trim().to_string())
            .filter(|entry| !entry.is_empty())
            .collect(),
        _ => Vec::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use macc_core::resolve::{ResolvedConfig, ResolvedSelectionsConfig, ResolvedStandardsConfig};

    #[test]
    fn test_agy_config_defaults() {
        let resolved = ResolvedConfig {
            version: "v1".to_string(),
            tools: macc_core::resolve::ResolvedToolsConfig {
                enabled: vec!["agy".to_string()],
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
        };

        let config = AgyConfig::from_resolved(&resolved);
        assert_eq!(config.model, "auto-gemini-3");
        assert!(config.sandbox);
    }
}
