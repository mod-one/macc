use crate::map::VibeConfig;
use macc_adapter_shared::render::format::ensure_trailing_newline;

pub fn render_config_toml(config: &VibeConfig) -> String {
    let mut toml_str = String::new();

    toml_str.push_str("# MACC Generated Mistral Vibe Settings - DO NOT EDIT MANUALLY\n\n");
    toml_str.push_str(&format!("default_agent = \"{}\"\n", config.agent));
    toml_str.push_str(&format!("active_model = \"{}\"\n", config.model));
    toml_str.push_str("enable_telemetry = false\n");

    ensure_trailing_newline(toml_str)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::map::VibeConfig;
    use macc_core::resolve::ResolvedConfig;

    #[test]
    fn test_render_config_toml() {
        let resolved = ResolvedConfig {
            version: "v1".to_string(),
            tools: macc_core::resolve::ResolvedToolsConfig::default(),
            standards: macc_core::resolve::ResolvedStandardsConfig {
                path: None,
                inline: Default::default(),
            },
            selections: macc_core::resolve::ResolvedSelectionsConfig {
                skills: vec![],
                agents: vec![],
                mcp: vec![],
            },
            mcp_templates: Vec::new(),
            automation: Default::default(),
            settings: Default::default(),
        };
        let vibe_cfg = VibeConfig::from_resolved(&resolved);
        let rendered = render_config_toml(&vibe_cfg);
        assert!(rendered.contains("default_agent = \"auto-approve\""));
        assert!(rendered.contains("active_model = \"mistral-medium-3.5\""));
    }
}
