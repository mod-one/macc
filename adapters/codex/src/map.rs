use macc_core::resolve::ResolvedConfig;
use serde_json::Value;

#[derive(Debug, Clone)]
pub struct CodexConfig {
    pub tool_config: CodexToolConfig,
}

#[derive(Debug, Clone, Default)]
pub struct CodexToolConfig {
    pub model: Option<String>,
    pub model_reasoning_effort: Option<String>,
    pub approval_policy: Option<String>,
    pub sandbox_mode: Option<String>,
    pub features_undo: Option<bool>,
    pub features_shell_snapshot: Option<bool>,
    pub rules_enabled: Option<bool>,
    pub raw: serde_json::Value,
}

impl CodexConfig {
    pub fn from_resolved(resolved: &ResolvedConfig) -> Self {
        let tool_config = extract_tool_config(resolved);
        Self { tool_config }
    }
}

fn extract_tool_config(resolved: &ResolvedConfig) -> CodexToolConfig {
    let tool_config = resolved
        .tools
        .config
        .get("codex")
        .or_else(|| resolved.tools.specific.get("codex"));

    let Some(tool_config) = tool_config else {
        return CodexToolConfig {
            raw: serde_json::Value::Object(Default::default()),
            ..CodexToolConfig::default()
        };
    };

    CodexToolConfig {
        raw: tool_config.clone(),
        model: get_string(tool_config, &["model"]),
        model_reasoning_effort: get_string(tool_config, &["model_reasoning_effort"]),
        approval_policy: get_string(tool_config, &["approval_policy"]),
        sandbox_mode: get_string(tool_config, &["sandbox_mode"]),
        features_undo: get_bool(tool_config, &["features", "undo"]),
        features_shell_snapshot: get_bool(tool_config, &["features", "shell_snapshot"]),
        rules_enabled: get_bool(tool_config, &["rules_enabled"]),
    }
}

fn get_string(root: &Value, path: &[&str]) -> Option<String> {
    let mut current = root;
    for key in path {
        current = current.get(*key)?;
    }
    current.as_str().map(|s| s.to_string())
}

fn get_bool(root: &Value, path: &[&str]) -> Option<bool> {
    let mut current = root;
    for key in path {
        current = current.get(*key)?;
    }
    current.as_bool()
}
