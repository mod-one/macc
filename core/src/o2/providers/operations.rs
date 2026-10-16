//! Durable progress documents; only receipts in SQLite activate a provider.
use super::*;
use serde_json::{json, Value};
pub(super) fn path(root: &Path, id: &str) -> Result<std::path::PathBuf> {
    uuid::Uuid::parse_str(id).map_err(|_| error("Invalid provider operation ID"))?;
    super::lifecycle::owned_path(
        root,
        "operations",
        &format!(".macc/code-intelligence/operations/{id}.json"),
    )
}
pub(super) fn progress(root: &Path, plan: &Plan, status: &str, detail: &str) -> Result<()> {
    paths::safe_text(detail)?;
    store::write(root,&path(root,&plan.id)?,&serde_json::to_vec(&json!({"id":plan.id,"provider":plan.provider_id,"action":plan.action,"status":status,"detail":detail,"updated_at":now(),"principal":std::env::var("MACC_SESSION_ROLE").unwrap_or_else(|_|"operator".into()),"version":plan.provider_version,"origin":plan.package_origin,"affected_paths":plan.affected_paths})).map_err(|e|error(e.to_string()))?)
}
pub fn operation(root: &Path, id: &str) -> Result<Value> {
    let path = path(root, id)?;
    if !path.exists() {
        return Ok(json!({"id":id,"status":"pending"}));
    }
    store::json(&store::bytes(&path)?)
}

pub(super) fn audit(root: &Path, plan: &Plan, event: &str) -> Result<()> {
    let _guard = store::lock(root)?;
    let (db, _) = database(root)?;
    super::super::schema::audit(
        &db,
        event,
        &plan.id,
        json!({"provider":plan.provider_id,"action":plan.action,"version":plan.provider_version,"origin":plan.package_origin,"affected_paths":plan.affected_paths,"principal":std::env::var("MACC_SESSION_ROLE").unwrap_or_else(|_|"operator".into())}),
    )
}
