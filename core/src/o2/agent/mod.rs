//! Restricted agent port. Operator Action is deliberately not a dispatch surface here.
mod auth;
pub(crate) mod presentation;
mod reads;
mod writes;
use super::*;
pub(crate) use auth::{authorize, Scope};
pub use auth::{options, Credentials};
pub use reads::call_read;
use serde::Deserialize;
use serde_json::{json, Value};
pub use writes::{challenge, propose, AgentProposal, Challenge};
pub fn status(root: &Path) -> Result<Value> {
    let p = policy::load(root)?;
    Ok(
        json!({"enabled":p.memory.enabled && p.memory.mcp_enabled,"transport":"stdio","protocol":"2025-11-25","authorization":"current project/task/role/worktree lease; renewed by the runner; revoked on release","proposal_status":"proposed","phase_extraction":p.memory.phase_extraction_enabled,"agent_proposals":p.memory.agent_proposals_allowed,"tools":crate::o2::mcp::catalog::names(&p),"registration":{"command":"macc","args":["o2","mcp"],"required_inherited_environment":["MACC_O2_PROJECT_ROOT","MACC_O2_MCP_SESSION","MACC_O2_MCP_GENERATION","MACC_O2_MCP_TOKEN"]},"tool_states":super::mcp::status::tool_states(root)?,"notice":"Apply registers and verifies enabled MCP servers. Runtime access requires a live lease; actual memory consultations are observed separately. No credentials are stored in project configuration."}),
    )
}
pub(crate) fn enabled(root: &Path) -> Result<policy::Policy> {
    let p = policy::load(root)?;
    if !p.memory.enabled || !p.memory.mcp_enabled {
        return Err(error("Scoped memory MCP is disabled"));
    }
    Ok(p)
}
pub(crate) fn clean(value: &str) -> Result<()> {
    paths::safe_text(value)?;
    if value.chars().any(|c| {
        (c.is_control() && c != '\n' && c != '\t')
            || matches!(c,'\u{202a}'..='\u{202e}'|'\u{2066}'..='\u{2069}')
    }) {
        return Err(error("Control sequences are not allowed in agent memory"));
    }
    Ok(())
}

/// Fail closed on secret/control-bearing evidence rather than rewrite its checksum.
pub(crate) fn clean_output(value: &Value) -> Result<()> {
    match value {
        Value::String(s) => clean(s)?,
        Value::Array(a) => {
            for v in a {
                clean_output(v)?;
            }
        }
        Value::Object(o) => {
            for (k, v) in o {
                clean(k)?;
                clean_output(v)?;
            }
        }
        _ => {}
    }
    Ok(())
}
