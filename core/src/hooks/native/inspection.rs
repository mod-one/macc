use super::*;
use crate::{config::CanonicalConfig, ProjectPaths, Result};
use serde_json::Value;
use std::path::Path;
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Deployment {
    pub tool: String,
    pub supported: bool,
    pub configured: bool,
    pub installed: bool,
    pub locally_verified: bool,
    pub execution_observed: bool,
    pub applied: Vec<String>,
    pub detail: String,
}
pub fn inspect(root: &Path, config: &CanonicalConfig) -> Result<Vec<Deployment>> {
    let registry = crate::ToolRegistry::default_registry();
    let revision = crate::config::transaction::revision(config)?;
    let mut result = Vec::new();
    for tool in crate::service::preparation::selected_tool_ids(config) {
        let spec = registry.get(&tool).and_then(|a| a.native_output_hook());
        let mut state = Deployment {tool:tool.clone(),supported:spec.is_some() && cfg!(unix),configured:false,installed:false,locally_verified:false,execution_observed:false,applied:vec![],detail:"Native hooks unsupported by this adapter/platform; internal MACC processors remain available".into()};
        if let Some(spec) = spec.filter(|_| cfg!(unix)) {
            let document: Value = std::fs::read(root.join(&spec.config))
                .ok()
                .and_then(|s| serde_json::from_slice(&s).ok())
                .unwrap_or_default();
            let command = registration_command(root, &spec);
            state.configured = ["SessionStart", "PostToolUse"].iter().all(|event| {
                document["hooks"][event].as_array().is_some_and(|groups| {
                    groups.iter().any(|group| {
                        group["matcher"]
                            == if *event == "PostToolUse" {
                                "^Bash$"
                            } else {
                                ".*"
                            }
                            && group["hooks"].as_array().is_some_and(|handlers| {
                                handlers.iter().any(|h| {
                                    h["type"] == "command"
                                        && h["command"] == command
                                        && h["timeout"] == 10
                                })
                            })
                    })
                })
            });
            state.installed = std::fs::read_to_string(root.join(&spec.launcher))
                .ok()
                .as_deref()
                == Some(deployment::launcher(&spec).as_str());
            state.locally_verified = state.installed && state.configured;
            let receipt: Value =
                std::fs::read(root.join(format!(".macc/state/native-hooks/{tool}.json")))
                    .ok()
                    .and_then(|s| serde_json::from_slice(&s).ok())
                    .unwrap_or_default();
            state.execution_observed = state.locally_verified && receipt["revision"] == revision;
            if state.execution_observed {
                state.applied = receipt["applied"]
                    .as_array()
                    .map(|a| {
                        a.iter()
                            .filter_map(|s| s.as_str().map(String::from))
                            .collect()
                    })
                    .unwrap_or_default();
            }
            state.detail = if !enabled(config)? {"Native output hooks disabled; Apply removes MACC registrations"}
                else if !state.locally_verified {"Run Apply to install and verify the native registration and launcher"}
                else if state.execution_observed {"Native bridge invocation observed for current configuration; applied lists the last event's reducers"}
                else if spec.protocol == Protocol::Feedback {"Local files verified; tool execution unconfirmed. In Codex use /hooks to review and trust the definition; MACC does not bypass trust"}
                else {"Local files verified; tool execution unconfirmed. Start a new trusted tool session; runtime/version policy can still prevent hooks"}.into();
        }
        result.push(state);
    }
    Ok(result)
}
/// Apply verifies deployment integrity, never equates it with provider trust or execution.
pub fn verify(paths: &ProjectPaths) -> Result<Vec<Deployment>> {
    let config = crate::load_canonical_config(&paths.config_path)?;
    let states = inspect(&paths.root, &config)?;
    if enabled(&config)? {
        for state in &states {
            if state.supported && !state.locally_verified {
                return Err(super::super::error(format!(
                    "Native hook deployment verification failed for {}: {}",
                    state.tool, state.detail
                )));
            }
        }
    }
    Ok(states)
}
