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
    if !store::is_sqlite(root)? {
        return Ok(ExecutionOptions {
            reason: "Native auto configuration left to existing provider settings".into(),
            ..Default::default()
        });
    }
    let _guard = store::lock(root)?;
    let (mut db, _) = store::active_database(root)?;
    if !super::super::ready(&db)? {
        if policy.adaptive_compaction.enabled {
            return Err(error(
                "O2 schema upgrade required before native configuration",
            ));
        }
        return Ok(ExecutionOptions {
            reason: "O2 inactive; existing provider configuration retained".into(),
            ..Default::default()
        });
    }
    let tx = sql(db.transaction())?;
    store::verify_lease(&tx, key)?;
    let raw: String = sql(tx.query_row(
        "SELECT identity_json FROM logical_sessions WHERE id=?1",
        [&key.session_id],
        |r| r.get(0),
    ))?;
    let identity: serde_json::Value = store::json(raw.as_bytes())?;
    let profile: Option<crate::tool::compaction::CompactionSpec> =
        serde_json::from_value(identity["execution_config"]["performer"]["compaction"].clone())
            .map_err(|e| error(e.to_string()))?;
    let Some(profile) = profile else {
        return Ok(ExecutionOptions {
            reason: "Adapter has no native configuration; checkpoint rotation retained".into(),
            ..Default::default()
        });
    };
    profile.validate()?;
    let tool = identity["tool_id"]
        .as_str()
        .ok_or_else(|| error("Missing tool identity"))?;
    let Some(native) = &profile.native_auto else {
        return Ok(ExecutionOptions {
            reason: "Native setting unsupported; checkpoint rotation retained".into(),
            ..Default::default()
        });
    };
    let disabled = !policy.adaptive_compaction.enabled
        || !policy.adaptive_compaction.configure_native_auto
        || native
            .disabled_env
            .iter()
            .any(|k| std::env::var_os(k).is_some_and(|v| v != "0" && v != "false"))
        || native.unit == "tokens" && policy.adaptive_compaction.native_token_threshold == 0;
    if disabled {
        let result = if native.format == "env" {
            ExecutionOptions {
                reason: "Native environment override disabled for this invocation".into(),
                ..Default::default()
            }
        } else {
            native_settings::manage(
                &tx,
                root,
                identity["execution_root"]
                    .as_str()
                    .ok_or_else(|| error("Missing worktree"))?,
                native,
                None,
            )?
        };
        sql(tx.commit())?;
        return Ok(result);
    }
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
    let mut observed: Probe = store::json(observed.as_bytes())?;
    let command = identity["execution_config"]["performer"]["command"]
        .as_str()
        .ok_or_else(|| error("Missing executable"))?;
    if let Err(e) = freshness::validate(root, command, &profile, &observed) {
        let reason =
            format!("Native capability stale/unavailable: {e}; checkpoint fallback remains active");
        // An older leased profile must not invalidate a newer operator observation.
        if observed.state == "version_help_observed"
            && freshness::same_identity(command, &profile, &observed)?
        {
            freshness::stale(&mut observed, &reason);
            sql(tx.execute(
                "UPDATE compaction_probes SET probe_json=?2 WHERE tool_id=?1",
                params![
                    tool,
                    serde_json::to_string(&observed).map_err(|e| error(e.to_string()))?
                ],
            ))?;
            super::super::schema::audit(
                &tx,
                "compaction_capability_stale",
                tool,
                serde_json::json!({"reason":reason}),
            )?;
        }
        sql(tx.commit())?;
        return Ok(ExecutionOptions {
            reason,
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
        result = native_settings::manage(
            &tx,
            root,
            identity["execution_root"]
                .as_str()
                .ok_or_else(|| error("Missing worktree"))?,
            native,
            Some(value),
        )?;
        super::super::schema::audit(
            &tx,
            "native_auto_configuration",
            tool,
            serde_json::json!({"session":key.session_id,"applied":result.applied,"reason":result.reason}),
        )?;
        sql(tx.commit())?;
        return Ok(result);
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
