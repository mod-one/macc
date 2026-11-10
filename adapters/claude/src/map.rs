use macc_core::resolve::ResolvedConfig;
use serde::Deserialize;
use std::collections::{BTreeMap, BTreeSet};

#[derive(Debug, Clone)]
pub struct ClaudeConfig {
    pub standards_inline: BTreeMap<String, String>,
    pub agents: Vec<String>,
    pub rules_enabled: bool,
    pub tool_config: serde_json::Value,
}

#[derive(Debug, Deserialize, Default)]
struct ClaudeConfigSource {
    #[serde(default)]
    agents: Vec<String>,
    #[serde(default)]
    rules_enabled: bool,
}

impl ClaudeConfig {
    pub fn from_resolved(resolved: &ResolvedConfig) -> Self {
        let tool_config = resolved
            .tools
            .config
            .get("claude")
            .or_else(|| resolved.tools.specific.get("claude"))
            .cloned()
            .unwrap_or_else(|| serde_json::Value::Object(Default::default()));

        let source: ClaudeConfigSource =
            serde_json::from_value(tool_config.clone()).unwrap_or_default();

        let mut agents_set = BTreeSet::new();
        for agent in &resolved.selections.agents {
            agents_set.insert(agent.clone());
        }
        for agent in &source.agents {
            agents_set.insert(agent.clone());
        }
        let rules_enabled = source.rules_enabled;

        let mut standards_inline = resolved.standards.inline.clone();
        if let Some(language) = tool_config.get("language").and_then(|v| v.as_str()) {
            standards_inline.insert("language".into(), language.into());
        }
        Self {
            standards_inline,
            agents: agents_set.into_iter().collect(),
            rules_enabled,
            tool_config,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use macc_core::resolve::{
        ResolvedConfig, ResolvedSelectionsConfig, ResolvedStandardsConfig, ResolvedToolsConfig,
    };
    use serde_json::json;

    fn base_resolved() -> ResolvedConfig {
        ResolvedConfig {
            version: "v1".to_string(),
            tools: ResolvedToolsConfig {
                enabled: vec!["claude".to_string()],
                ..Default::default()
            },
            standards: ResolvedStandardsConfig {
                path: None,
                inline: Default::default(),
            },
            selections: ResolvedSelectionsConfig {
                skills: vec![],
                agents: vec![],
                mcp: vec![],
            },
            mcp_templates: Vec::new(),
            automation: macc_core::config::AutomationConfig::default(),
            settings: macc_core::config::SettingsConfig::default(),
            context: None,
            skills_run_policy: None,
        }
    }

    #[test]
    fn merges_global_and_tool_specific_agents_with_stable_ordering() {
        let mut resolved = base_resolved();
        resolved.selections.agents = vec!["reviewer".to_string()];

        let claude_json = json!({
            "agents": ["architect", "prompt-engineer"]
        });
        resolved
            .tools
            .specific
            .insert("claude".to_string(), claude_json);

        let cfg = ClaudeConfig::from_resolved(&resolved);

        assert_eq!(
            cfg.agents,
            vec![
                "architect".to_string(),
                "prompt-engineer".to_string(),
                "reviewer".to_string()
            ]
        );
    }

    #[test]
    fn no_agents_when_not_selected() {
        let resolved = base_resolved();
        let cfg = ClaudeConfig::from_resolved(&resolved);
        assert!(cfg.agents.is_empty());
    }

    #[test]
    fn rules_enabled_defaults_to_false() {
        let resolved = base_resolved();
        let cfg = ClaudeConfig::from_resolved(&resolved);
        assert!(!cfg.rules_enabled);
    }
}
