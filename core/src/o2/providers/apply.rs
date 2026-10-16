//! Long external work never holds session authority or a SQLite write transaction.
use super::*;
use rusqlite::params;
use serde_json::json;
fn validate(
    root: &Path,
    plan: &Plan,
    adapter: &dyn CodeIntelligenceProvider,
    policy: &super::super::policy::CodePolicy,
) -> Result<()> {
    let descriptor = adapter.descriptor();
    if descriptor.version != plan.provider_version
        || descriptor.origin != plan.package_origin
        || adapter.network_operation(&plan.action) != plan.network
    {
        return Err(error("Provider version/origin changed; preview a new plan"));
    }
    if plan.network
        && crate::load_canonical_config(crate::ProjectPaths::from_root(root).config_path)?
            .settings
            .offline
    {
        return Err(error(
            "Network provider operations are disabled in offline mode",
        ));
    }
    if let Some(details) = &plan.details {
        if details["policy_sha256"]
            != digest(&serde_json::to_vec(policy).map_err(|e| error(e.to_string()))?)
            || details["adapter"] != adapter.plan_details(root, &plan.action, policy)?
        {
            return Err(error("Provider policy/runtime changed; preview a new plan"));
        }
    }
    Ok(())
}
pub fn apply(root: &Path, id: &str, expected: &str) -> Result<Receipt> {
    let plan: Plan = {
        let _guard = store::lock(root)?;
        let (db, _) = database(root)?;
        let raw: String = sql(db.query_row(
            "SELECT plan_json FROM provider_plans WHERE id=?1",
            [id],
            |r| r.get(0),
        ))?;
        if digest(raw.as_bytes()) != expected {
            return Err(error("Reviewed provider plan checksum mismatch"));
        }
        store::json(raw.as_bytes())?
    };
    let lock = lifecycle::owned_path(
        root,
        &plan.provider_id,
        &format!(
            ".macc/code-intelligence/{}/operation.lock",
            plan.provider_id
        ),
    )?;
    let _operation = crate::fs_lock::AdvisoryLock::acquire(
        &lock,
        std::time::Duration::from_secs(1),
        "code provider operation",
    )?;
    let adapter = provider(&plan.provider_id)?;
    let policy = super::super::policy::load(root)?.code_intelligence;
    let previous = {
        let _guard = store::lock(root)?;
        let (db, a) = database(root)?;
        if plan.project_id != a.project_id {
            return Err(error("Foreign provider plan"));
        }
        let (applied, saved): (bool, Option<String>) = sql(db.query_row(
            "SELECT applied,result_json FROM provider_plans WHERE id=?1",
            [id],
            |r| Ok((r.get(0)?, r.get(1)?)),
        ))?;
        if applied {
            return store::json(
                saved
                    .ok_or_else(|| error("Applied receipt missing"))?
                    .as_bytes(),
            );
        }
        let previous = receipt(&db, &plan.provider_id)?;
        if checksum(&previous)? != plan.before_checksum {
            return Err(error("Provider changed; preview a new plan"));
        }
        if ["uninstall", "undeploy"].contains(&plan.action.as_str()) {
            store::require_stopped(root)?;
            let active: i64 =
                sql(db.query_row("SELECT count(*) FROM session_leases", [], |r| r.get(0)))?;
            if active != 0 {
                return Err(error("Stop runners before provider removal"));
            }
        }
        previous
    };
    validate(root, &plan, adapter.as_ref(), &policy)?;
    let source = source_snapshot(root, &policy)?;
    if source.revision != plan.source_revision || source.digest != plan.snapshot_digest {
        return Err(error("Repository changed; preview a new provider plan"));
    }
    operations::audit(root, &plan, "provider_plan_started")?;
    operations::progress(
        root,
        &plan,
        "running",
        "Executing the exact reviewed plan; prior receipt remains authoritative",
    )?;
    let execute = || -> Result<Receipt> {
        let mut result = match adapter.execute_lifecycle(root, &plan, &previous, &policy)? {
            Capability::Supported(r) => r,
            Capability::Unsupported { reason, .. } => return Err(error(reason)),
        };
        validate(root, &plan, adapter.as_ref(), &policy)?;
        let after = source_snapshot(root, &policy)?;
        if after.digest != source.digest || after.revision != source.revision {
            return Err(error(
                "Source changed during provider operation; cache is not activated; preview again",
            ));
        }
        if ["index", "update"].contains(&plan.action.as_str()) {
            result.snapshot_digest = Some(source.digest.clone());
        }
        if result.provider_id != plan.provider_id {
            return Err(error("Provider returned a foreign receipt"));
        }
        for path in &result.owned_paths {
            lifecycle::owned_path(root, &plan.provider_id, path)?;
        }
        let _guard = store::lock(root)?;
        let (mut db, _) = database(root)?;
        let tx = sql(db.transaction())?;
        let current = receipt(&tx, &plan.provider_id)?;
        if checksum(&current)? != plan.before_checksum {
            return Err(error(
                "Provider changed during operation; activation refused",
            ));
        }
        let current_policy = super::super::policy::load(root)?.code_intelligence;
        if current_policy != policy {
            return Err(error(
                "Provider policy changed during operation; activation refused",
            ));
        }
        sql(tx.execute("INSERT INTO provider_receipts VALUES(?1,?2) ON CONFLICT(provider_id) DO UPDATE SET receipt_json=excluded.receipt_json",params![result.provider_id,serde_json::to_string(&result).map_err(|e|error(e.to_string()))?]))?;
        sql(tx.execute(
            "UPDATE provider_plans SET applied=1,result_json=?2 WHERE id=?1",
            params![
                id,
                serde_json::to_string(&result).map_err(|e| error(e.to_string()))?
            ],
        ))?;
        super::super::schema::audit(
            &tx,
            "provider_plan_applied",
            id,
            json!({"provider":result.provider_id,"state":result.state}),
        )?;
        sql(tx.commit())?;
        Ok(result)
    };
    match execute() {
        Ok(result) => {
            operations::progress(
                root,
                &plan,
                "complete",
                "Receipt activated; operation is replayable",
            )?;
            Ok(result)
        }
        Err(e) => {
            operations::audit(root, &plan, "provider_plan_failed")?;
            let message = e.to_string();
            operations::progress(
                root,
                &plan,
                "failed",
                if paths::safe_text(&message).is_ok() {
                    &message
                } else {
                    "Operation failed; sensitive error omitted"
                },
            )?;
            Err(e)
        }
    }
}
