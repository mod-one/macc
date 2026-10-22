//! Human-readable shared-core observations; no independent status calculation.
use serde_json::Value;

pub(super) fn format(value: &Value) -> Option<String> {
    let rows = value.get("tool_states")?.as_array()?;
    let flag = |v: &Value| if v == true { "yes" } else { "no" };
    let mut text = String::from("MEMORY MCP ACCESS\n\nApply checks the registered server. Run observations show actual access separately.\n");
    if rows.is_empty() {
        text.push_str("\nNo enabled tools in this project.\n");
    }
    for row in rows {
        text.push_str(&format!("\n{} — {}\n  Configured: {}\n  Connection verified by Apply: {}\n  Available in a live run: {}\n  Catalog connected in a live run: {}\n  Memory consulted: {} ({} observed calls)\n  {}\n",
            row["tool_id"].as_str().unwrap_or("unknown"),row["state"].as_str().unwrap_or("unknown"),
            flag(&row["configured"]),flag(&row["connection_verified"]),flag(&row["available_in_run"]),flag(&row["connected_in_run"]),flag(&row["consulted"]),row["consultations_observed"].as_u64().unwrap_or(0),row["diagnostic"].as_str().unwrap_or("")));
    }
    text.push_str("\nA control read never counts as agent consultation. A consultation does not prove the memory was useful.\nRefresh to observe changes. No live credentials are displayed.");
    Some(text)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn access_states_remain_distinct_and_visible() {
        let text=format(&serde_json::json!({"tool_states":[{"tool_id":"future","state":"connection_verified","configured":true,"connection_verified":true,"available_in_run":false,"consulted":false}]})).unwrap();
        assert!(text.contains("Configured: yes"));
        assert!(text.contains("Connection verified by Apply: yes"));
        assert!(text.contains("Available in a live run: no"));
        assert!(text.contains("Memory consulted: no"));
    }
}
