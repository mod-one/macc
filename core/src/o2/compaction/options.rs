use super::*;
use std::collections::BTreeMap;
#[derive(Debug, Default, Serialize, Deserialize)]
pub struct ExecutionOptions {
    pub env: BTreeMap<String, String>,
    pub applied: bool,
    pub reason: String,
}
pub fn execution_options(root: &Path, key: &store::LeaseKey) -> Result<ExecutionOptions> {
    let policy = super::super::policy::load(root)?;
    if !policy.adaptive_compaction.enabled || !policy.adaptive_compaction.configure_native_auto {
        return Ok(ExecutionOptions {
            reason: "Native auto configuration left to existing provider settings".into(),
            ..Default::default()
        });
    }
    let _guard = store::lock(root)?;
    let (mut db, _) = database(root)?;
    let tx = sql(db.transaction())?;
    store::verify_lease(&tx, key)?;
    let raw: String = sql(tx.query_row(
        "SELECT identity_json FROM logical_sessions WHERE id=?1",
        [&key.session_id],
        |r| r.get(0),
    ))?;
    let identity: serde_json::Value = store::json(raw.as_bytes())?;
    let p: crate::tool::ToolPerformerSpec =
        serde_json::from_value(identity["execution_config"]["performer"].clone())
            .map_err(|e| error(e.to_string()))?;
    let Some(profile) = p.compaction.as_ref() else {
        return Ok(ExecutionOptions {
            reason: "Adapter has no native auto configuration; checkpoint rotation retained".into(),
            ..Default::default()
        });
    };
    profile.validate()?;
    let tool = identity["tool_id"]
        .as_str()
        .ok_or_else(|| error("Missing tool identity"))?;
    let observed: Option<String> = sql(tx
        .query_row(
            "SELECT probe_json FROM compaction_probes WHERE tool_id=?1",
            [tool],
            |r| r.get(0),
        )
        .optional())?;
    let Some(observed) = observed else {
        return Ok(ExecutionOptions {
            reason: "Probe installed CLI before opting into native configuration".into(),
            ..Default::default()
        });
    };
    let observed: Probe = store::json(observed.as_bytes())?;
    let executable = probe::executable(&p.command)?;
    if observed.state != "version_help_observed"
        || observed.executable_sha256 != Some(probe::binary_hash(&executable)?)
        || observed.profile_sha256
            != Some(digest(
                &serde_json::to_vec(profile).map_err(|e| error(e.to_string()))?,
            ))
    {
        return Ok(ExecutionOptions{reason:"Native capability observation stale/unavailable; re-probe; checkpoint fallback remains active".into(),..Default::default()});
    }
    let Some(native) = &profile.native_auto else {
        return Ok(ExecutionOptions {
            reason: "Native setting unsupported for this adapter".into(),
            ..Default::default()
        });
    };
    if native
        .disabled_env
        .iter()
        .any(|k| std::env::var_os(k).is_some_and(|v| v != "0" && v != "false"))
    {
        return Ok(ExecutionOptions {
            reason: "An explicit native compaction disable is present; preserved".into(),
            ..Default::default()
        });
    }
    let value = match native.unit.as_str() {
        "tokens" => {
            if policy.adaptive_compaction.native_token_threshold == 0 {
                return Ok(ExecutionOptions{reason:"Absolute native token threshold is unknown/unconfigured; preserving provider default".into(),..Default::default()});
            }
            serde_json::json!(policy.adaptive_compaction.native_token_threshold)
        }
        "percent" => {
            serde_json::json!((policy.adaptive_compaction.action_ratio * 100.).floor() as u64)
        }
        _ => serde_json::json!(policy.adaptive_compaction.action_ratio),
    };
    let mut result = ExecutionOptions::default();
    if native.format == "env" {
        if std::env::var_os(&native.key).is_some() {
            result.reason = "Existing explicit environment setting preserved".into();
            return Ok(result);
        }
        result.env.insert(native.key.clone(), value.to_string());
    } else {
        let wt = identity["execution_root"]
            .as_str()
            .ok_or_else(|| error("Missing worktree"))?;
        super::super::providers::native_worktree(root, Some(wt))?;
        let relative = native.path.as_ref().unwrap();
        let mut path = Path::new(wt).to_path_buf();
        for part in Path::new(relative).components() {
            path.push(part);
            if std::fs::symlink_metadata(&path).is_ok_and(|m| m.file_type().is_symlink()) {
                return Err(error("Native configuration path symlink rejected"));
            }
        }
        let existing = if path.exists() {
            let size = std::fs::metadata(&path)
                .map_err(|e| error(e.to_string()))?
                .len();
            if size > 1048576 {
                return Err(error("Native config too large"));
            }
            std::fs::read_to_string(&path).map_err(|e| error(e.to_string()))?
        } else {
            String::new()
        };
        let mut document = if native.format == "toml" {
            if existing.is_empty() {
                serde_json::json!({})
            } else {
                serde_json::to_value(
                    toml::from_str::<toml::Value>(&existing).map_err(|e| error(e.to_string()))?,
                )
                .map_err(|e| error(e.to_string()))?
            }
        } else if existing.is_empty() {
            serde_json::json!({})
        } else {
            serde_json::from_str(&existing).map_err(|e| error(e.to_string()))?
        };
        let keys = native.key.split('.').collect::<Vec<_>>();
        let mut node = &mut document;
        for k in &keys[..keys.len() - 1] {
            let o = node
                .as_object_mut()
                .ok_or_else(|| error("Native config object expected"))?;
            node = o.entry(*k).or_insert(serde_json::json!({}));
        }
        let object = node
            .as_object_mut()
            .ok_or_else(|| error("Native config object expected"))?;
        let key = keys[keys.len() - 1];
        if object.contains_key(key) {
            return Ok(ExecutionOptions {
                reason: "Existing explicit native setting preserved".into(),
                ..Default::default()
            });
        }
        object.insert(key.into(), value);
        let raw = if native.format == "toml" {
            toml::to_string_pretty(&document).map_err(|e| error(e.to_string()))?
        } else {
            serde_json::to_string_pretty(&document).map_err(|e| error(e.to_string()))?
        };
        store::write(root, &path, raw.as_bytes())?;
    }
    result.applied = true;
    result.reason="Documented native auto setting configured; compression execution remains unobserved; checkpoint rotation retained".into();
    super::super::schema::audit(
        &tx,
        "native_auto_configured",
        tool,
        serde_json::json!({"session":key.session_id,"format":native.format,"key":native.key,"quality":"configured_not_observed"}),
    )?;
    sql(tx.commit())?;
    Ok(result)
}
