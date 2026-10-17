use super::*;
use serde_json::json;

fn render(raw: JsonValue) -> Value {
    render_config_toml(&CodexToolConfig {
        raw,
        ..Default::default()
    })
    .parse()
    .expect("valid TOML")
}

#[test]
fn macc_context_and_selections_do_not_leak_into_codex_config() {
    let raw = json!({
        "context": { "protect": true, "fileName": "AGENTS.md" },
        "skills": ["validate"],
        "agents": ["reviewer"],
        "rules_enabled": true,
        "model_tiers": { "heavy": { "model": "test-model" } },
        "profiles": { "review": { "model": "test-model" } },
        "sandbox_mode": "workspace-write",
        "approval_policy": "never",
        "model_context_window": 128000,
        "mcp_servers": { "docs": { "url": "https://developers.openai.com/mcp" } }
    });
    let config = render(raw.clone());
    for key in [
        "context",
        "skills",
        "agents",
        "rules_enabled",
        "model_tiers",
        "profiles",
    ] {
        assert!(config.get(key).is_none(), "MACC key leaked: {key}");
    }
    assert_eq!(config["sandbox_mode"].as_str(), Some("workspace-write"));
    assert_eq!(config["approval_policy"].as_str(), Some("never"));
    assert_eq!(config["model_context_window"].as_integer(), Some(128000));
    assert_eq!(
        config["mcp_servers"]["docs"]["url"].as_str(),
        Some("https://developers.openai.com/mcp")
    );
    assert_eq!(
        raw["context"]["protect"], true,
        "source config is unchanged"
    );
}

#[test]
fn native_codex_skills_and_agents_tables_are_preserved() {
    let config = render(json!({
        "skills": { "config": [{ "path": "/skills/review", "enabled": false }] },
        "agents": { "max_threads": 3 },
        "features": { "web_search_request": true, "shell_snapshot": false }
    }));
    assert_eq!(
        config["skills"]["config"][0]["enabled"].as_bool(),
        Some(false)
    );
    assert_eq!(config["agents"]["max_threads"].as_integer(), Some(3));
    assert!(config["features"].get("web_search_request").is_none());
    assert_eq!(config["features"]["shell_snapshot"].as_bool(), Some(false));
}

#[test]
fn legacy_comma_separated_selections_are_not_codex_settings() {
    let config = render(json!({ "skills": "validate,implement", "agents": "reviewer" }));
    assert!(config.get("skills").is_none());
    assert!(config.get("agents").is_none());
}
