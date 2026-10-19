//! Prepare project defaults without fabricating an agent session or lease.
use super::*;
pub fn prepare_configuration(root: &Path, tool: &str) -> Result<()> {
    let policy = super::super::policy::load(root)?.adaptive_compaction;
    if !policy.enabled || !policy.configure_native_auto {
        return Ok(());
    }
    let spec = specs(root)
        .into_iter()
        .find(|s| s.id == tool)
        .ok_or_else(|| error("Unknown tool"))?;
    let Some(native) = spec
        .performer
        .as_ref()
        .and_then(|p| p.compaction.as_ref())
        .and_then(|p| p.native_auto.as_ref())
    else {
        return Ok(());
    };
    if native
        .disabled_env
        .iter()
        .any(|key| std::env::var_os(key).is_some_and(|value| value != "0" && value != "false"))
    {
        return Err(error(format!(
            "Native compaction for {tool} is disabled by its runtime environment"
        )));
    }
    let desired = match native.unit.as_str() {
        "tokens" if policy.native_token_threshold == 0 => {
            return Err(error(
                "Choose a nonzero native token threshold in Apply configuration",
            ))
        }
        "tokens" => serde_json::json!(policy.native_token_threshold),
        "percent" => serde_json::json!((policy.action_ratio * 100.).floor() as u64),
        _ => serde_json::json!(policy.action_ratio),
    };
    if native.format == "env" {
        return Ok(());
    }
    store::require_stopped(root)?;
    let _guard = store::lock(root)?;
    let (mut db, _) = store::active_database(root)?;
    let tx = sql(db.transaction())?;
    native_settings::manage(
        &tx,
        root,
        root.to_str()
            .ok_or_else(|| error("Non-UTF8 project path"))?,
        native,
        Some(desired.clone()),
    )?;
    if !native_settings::matches(root, native, &desired)? {
        return Err(error(format!("User native setting {} is preserved. Select its matching threshold in Apply configuration, or remove the conflicting override before approving native configuration.",native.key)));
    }
    sql(tx.commit())?;
    Ok(())
}
