use macc_core::resolve::ResolvedConfig;
use serde::Deserialize;
use serde_json::{Map as JsonMap, Value as JsonValue};
use std::collections::{BTreeMap, BTreeSet};

#[allow(dead_code)]
#[derive(Debug, Clone)]
pub struct VibeConfig {
    pub language: String,
    pub model: String,
    pub agent: String,
    pub standards_inline: BTreeMap<String, String>,
    pub standards_path: Option<String>,
    pub skills: Vec<String>,
    pub agents: Vec<String>,
    pub tool_config: JsonValue,
}

#[derive(Debug, Deserialize, Default)]
struct VibeConfigSource {
    #[serde(default)]
    model: Option<String>,
    #[serde(default)]
    language: Option<String>,
    #[serde(default)]
    agent: Option<String>,
    #[serde(default)]
    skills: Vec<String>,
    #[serde(default)]
    agents: Vec<String>,
}

impl VibeConfig {
    pub fn from_resolved(resolved: &ResolvedConfig) -> Self {
        let tool_config = resolved
            .tools
            .config
            .get("vibe")
            .or_else(|| resolved.tools.specific.get("vibe"))
            .cloned()
            .unwrap_or_else(|| JsonValue::Object(JsonMap::new()));

        let source: VibeConfigSource =
            serde_json::from_value(tool_config.clone()).unwrap_or_default();

        let language = source
            .language
            .or_else(|| resolved.standards.inline.get("language").cloned())
            .unwrap_or_else(|| "English".to_string());

        let model = source
            .model
            .unwrap_or_else(|| "mistral-medium-3.5".to_string());
        let agent = source.agent.unwrap_or_else(|| "auto-approve".to_string());

        let mut skills_set = BTreeSet::new();
        for skill in &resolved.selections.skills {
            skills_set.insert(skill.clone());
        }
        for skill in &source.skills {
            skills_set.insert(skill.clone());
        }

        let mut agents_set = BTreeSet::new();
        for ag in &resolved.selections.agents {
            agents_set.insert(ag.clone());
        }
        for ag in &source.agents {
            agents_set.insert(ag.clone());
        }

        Self {
            language,
            model,
            agent,
            standards_inline: resolved.standards.inline.clone(),
            standards_path: resolved.standards.path.clone(),
            skills: skills_set.into_iter().collect(),
            agents: agents_set.into_iter().collect(),
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
                enabled: vec!["vibe".to_string()],
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
    fn test_default_model_when_no_config() {
        let resolved = base_resolved();
        let config = VibeConfig::from_resolved(&resolved);
        assert_eq!(config.model, "mistral-medium-3.5");
        assert_eq!(config.agent, "auto-approve");
    }

    #[test]
    fn test_skills_merged() {
        let mut resolved = base_resolved();
        resolved.selections.skills = vec!["global-skill".to_string()];
        resolved.tools.specific.insert(
            "vibe".to_string(),
            json!({
                "skills": ["local-skill"]
            }),
        );

        let config = VibeConfig::from_resolved(&resolved);
        assert_eq!(config.skills, vec!["global-skill", "local-skill"]);
    }
}
