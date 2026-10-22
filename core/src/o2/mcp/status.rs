//! Read-only client status: registration, control proof and actual run observations.
use super::*;

pub(crate) fn tool_states(root: &Path) -> Result<Value> {
    let paths = crate::ProjectPaths::from_root(root);
    if !paths.config_path.exists() {
        return Ok(json!([]));
    }
    let config = crate::load_canonical_config(&paths.config_path)?;
    let enabled = policy::load(root).map(|p| p.memory.enabled && p.memory.mcp_enabled)?;
    let (specs, _) =
        crate::tool::ToolSpecLoader::new(crate::tool::ToolSpecLoader::default_search_paths(root))
            .load_all_with_embedded();
    let db = readonly_database(root);
    let mut rows = vec![];
    for tool in &crate::service::preparation::selected_tool_ids(&config) {
        let registration = specs
            .iter()
            .find(|s| s.id == *tool)
            .and_then(|s| s.performer.as_ref())
            .and_then(|p| p.scoped_mcp.as_ref());
        let configured = registration.is_some_and(|r| r.verify(root).is_ok());
        let proof = if enabled && configured {
            registration
                .map(|r| super::verification::current(root, tool, r))
                .transpose()
        } else {
            Ok(Some(false))
        };
        let verified = proof.as_ref().is_ok_and(|p| *p == Some(true));
        let observations = match &db {
            Ok((db, _)) => super::activity::observations(db, tool)?,
            Err(_) => {
                json!({"available_in_run":false,"connected_in_run":false,"consulted":false,"consultations_observed":0})
            }
        };
        let available = enabled && configured && observations["available_in_run"] == true;
        let state = if !enabled {
            "disabled"
        } else if !configured {
            "not_configured"
        } else if !verified {
            "needs_verification"
        } else if available {
            "available_in_run"
        } else {
            "connection_verified"
        };
        let diagnostic = if !enabled {
            "Memory or MCP is disabled. Enable both in project settings and run Apply".into()
        } else if registration.is_none() {
            "Tool has no scoped MCP adapter".into()
        } else if let Err(e) = proof {
            e.to_string().chars().take(200).collect::<String>()
        } else if !verified {
            "Run Apply to verify the registered stdio server; reconnect an already open tool if needed".into()
        } else {
            "Control server verified; native client adoption is observed separately during a run"
                .into()
        };
        rows.push(json!({"tool_id":tool,"state":state,"configured":configured,"connection_verified":verified,"available_in_run":available,"connected_in_run":enabled && observations["connected_in_run"]==true,"consulted":observations["consulted"],"consultations_observed":observations["consultations_observed"],"native_client_verified":false,"diagnostic":diagnostic}));
    }
    Ok(json!(rows))
}
