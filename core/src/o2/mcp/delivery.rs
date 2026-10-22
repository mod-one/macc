//! Versioned presentation receipts. Only bytes surviving the wire budget count.
use super::*;

pub(super) fn record(root: &Path, c: &Credentials, name: &str, wire: &Value) -> Result<()> {
    if wire["result"]["isError"] != false {
        return Ok(());
    }
    let text = wire["result"]["content"][0]["text"]
        .as_str()
        .ok_or_else(|| error("Missing MCP response content"))?;
    let value: Value = store::json(text.as_bytes())?;
    let _guard = store::lock(root)?;
    let (db, _) = database(root)?;
    let scope = agent::authorize(root, &db, c)?;
    if name == "memory.get" && value["fragment"].is_string() {
        let start = value["offset"].as_u64().unwrap_or(0);
        let length = value["fragment"].as_str().unwrap().chars().count() as u64;
        super::super::schema::audit(
            &db,
            "mcp_fragment_delivered",
            &c.session_id,
            json!({"generation":c.generation,"id":value["id"],"checksum":value["checksum"],"start":start,"end":start+length}),
        )?;
    }
    if name.starts_with("memory.") && !["memory.propose", "memory.challenge"].contains(&name) {
        super::activity::record_in(&db, c, &scope, "consulted")?;
    }
    Ok(())
}
