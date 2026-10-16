use super::*;
use rusqlite::params;
pub fn inspect(root: &Path) -> Result<View> {
    let _guard = store::lock(root)?;
    let (db, _) = database(root)?;
    let policy = super::super::policy::load(root)?;
    let mut providers = registry()?
        .into_iter()
        .map(|p| p.detect(root))
        .collect::<Result<Vec<_>>>()?;
    let mut q = sql(db.prepare("SELECT receipt_json FROM provider_receipts ORDER BY provider_id"))?;
    let receipts: Vec<Receipt> = sql(q.query_map([], |r| r.get::<_, String>(0)))?
        .map(|r| sql(r).and_then(|s| store::json(s.as_bytes())))
        .collect::<Result<Vec<_>>>()?;
    for detection in &mut providers {
        if let Some(receipt) = receipts
            .iter()
            .find(|r| r.provider_id == detection.provider.id)
        {
            if receipt.state == "disabled" {
                detection.status = "disabled".into();
            }
            if receipt.version != detection.provider.version {
                detection.status = "incompatible".into();
                detection
                    .diagnostics
                    .push("Provider version changed; re-probe and review update".into());
            }
            if let (Some(path), Some(expected)) = (&receipt.index, &receipt.index_sha256) {
                let valid = owned_path(root, &receipt.provider_id, path)
                    .and_then(|p| store::bytes(&p))
                    .is_ok_and(|data| digest(&data) == *expected);
                let dirty_stale = if valid && receipt.provider_id == "native" {
                    let cached: native::Index = store::json(&store::bytes(&owned_path(
                        root,
                        &receipt.provider_id,
                        path,
                    )?)?)?;
                    cached.digest != native::snapshot(root, &policy.code_intelligence)?.digest
                } else {
                    false
                };
                if !valid
                    || dirty_stale
                    || receipt
                        .source_revision
                        .as_ref()
                        .is_some_and(|rev| crate::git::head_commit(root).ok().as_ref() != Some(rev))
                {
                    if detection.status != "disabled" && detection.status != "incompatible" {
                        detection.status = "degraded".into();
                    }
                    detection.diagnostics.push("Cached index missing, corrupt or stale; rebuild explicitly. Native current-file fallback remains available.".into());
                }
            }
        }
    }
    let mut q = sql(db.prepare(
        "SELECT plan_json FROM provider_plans WHERE applied=0 ORDER BY rowid DESC LIMIT 50",
    ))?;
    let plans = sql(q.query_map([], |r| r.get::<_, String>(0)))?
        .map(|r| sql(r).and_then(|s| store::json(s.as_bytes())))
        .collect::<Result<Vec<_>>>()?;
    Ok(View {
        providers,
        receipts,
        plans,
    })
}
pub fn plan(root: &Path, id: &str, action: &str) -> Result<Capability<Plan>> {
    let provider = provider(id)?;
    let d = provider.descriptor();
    if !d.operations.iter().any(|a| a == action) {
        return Ok(Capability::Unsupported{capability:action.into(),reason:"External lifecycle belongs to its verified adapter; no untrusted installer command is executed".into()});
    }
    let p = super::super::policy::load(root)?;
    let index = native::snapshot(root, &p.code_intelligence)?;
    let _guard = store::lock(root)?;
    let (mut db, a) = database(root)?;
    let tx = sql(db.transaction())?;
    let previous = receipt(&tx, id)?;
    let plan_id = uuid::Uuid::new_v4().to_string();
    let paths = match provider.plan_paths(action, &plan_id, &previous)? {
        Capability::Supported(paths) => paths,
        Capability::Unsupported { capability, reason } => {
            return Ok(Capability::Unsupported { capability, reason })
        }
    };
    for path in &paths {
        owned_path(root, id, path)?;
    }

    let plan=Plan{id:plan_id,provider_id:id.into(),action:action.into(),project_id:a.project_id,before_checksum:checksum(&previous)?,source_revision:index.revision,snapshot_digest:index.digest,affected_paths:paths,network:!d.local_only,package_origin:d.origin,provider_version:d.version,diagnostics:vec!["Review owned paths, origin and network policy before applying; memory and Git work are preserved".into()]};
    sql(tx.execute(
        "INSERT INTO provider_plans(id,plan_json) VALUES(?1,?2)",
        params![
            plan.id,
            serde_json::to_string(&plan).map_err(|e| error(e.to_string()))?
        ],
    ))?;
    super::super::schema::audit(
        &tx,
        "provider_plan_created",
        &plan.id,
        serde_json::json!({"action":action,"provider":id}),
    )?;
    sql(tx.commit())?;
    Ok(Capability::Supported(plan))
}
pub fn apply(root: &Path, id: &str, expected: &str) -> Result<Receipt> {
    let p = super::super::policy::load(root)?;
    let _guard = store::lock(root)?;
    let (mut db, a) = database(root)?;
    let tx = sql(db.transaction())?;
    let (raw, applied, saved_result): (String, bool, Option<String>) = sql(tx.query_row(
        "SELECT plan_json,applied,result_json FROM provider_plans WHERE id=?1",
        [id],
        |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
    ))?;
    let plan: Plan = store::json(raw.as_bytes())?;
    if digest(raw.as_bytes()) != expected {
        return Err(error("Reviewed provider plan checksum mismatch"));
    }
    let adapter = provider(&plan.provider_id)?;
    let descriptor = adapter.descriptor();
    if descriptor.version != plan.provider_version
        || descriptor.origin != plan.package_origin
        || (!descriptor.local_only) != plan.network
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
    if plan.project_id != a.project_id {
        return Err(error("Foreign/unsupported provider plan"));
    }
    if applied {
        return store::json(
            saved_result
                .ok_or_else(|| error("Applied receipt missing"))?
                .as_bytes(),
        );
    }
    let previous = receipt(&tx, &plan.provider_id)?;
    if checksum(&previous)? != plan.before_checksum {
        return Err(error("Provider changed; preview a new plan"));
    }
    let index = native::snapshot(root, &p.code_intelligence)?;
    if index.revision != plan.source_revision || index.digest != plan.snapshot_digest {
        return Err(error("Repository changed; preview a new provider plan"));
    }
    if ["uninstall", "undeploy"].contains(&plan.action.as_str()) {
        store::require_stopped(root)?;
        let active: i64 =
            sql(tx.query_row("SELECT count(*) FROM session_leases", [], |r| r.get(0)))?;
        if active != 0 {
            return Err(error("Stop runners before provider removal"));
        }
    }
    let mut result =
        match adapter.execute_lifecycle(root, &plan, &previous, &p.code_intelligence)? {
            Capability::Supported(receipt) => receipt,
            Capability::Unsupported { reason, .. } => return Err(error(reason)),
        };
    if ["index", "update"].contains(&plan.action.as_str()) {
        let after = native::snapshot(root, &p.code_intelligence)?;
        if after.digest != index.digest || after.revision != index.revision {
            return Err(error(
                "Source changed during indexing; preview again; cache is not activated",
            ));
        }
        result.snapshot_digest = Some(index.digest.clone());
    }
    if result.provider_id != plan.provider_id {
        return Err(error("Provider returned a foreign receipt"));
    }
    for path in &result.owned_paths {
        owned_path(root, &plan.provider_id, path)?;
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
        serde_json::json!({"provider":result.provider_id,"state":result.state}),
    )?;
    sql(tx.commit())?;
    Ok(result)
}
pub(super) fn owned_path(
    root: &Path,
    provider: &str,
    relative: &str,
) -> Result<std::path::PathBuf> {
    if !relative.starts_with(&format!(".macc/code-intelligence/{provider}/"))
        || Path::new(relative)
            .components()
            .any(|c| !matches!(c, std::path::Component::Normal(_)))
    {
        return Err(error("Unowned provider path"));
    }
    let mut current = root.to_path_buf();
    for component in Path::new(relative).components() {
        current.push(component);
        if current.exists()
            && std::fs::symlink_metadata(&current)
                .map_err(|e| error(e.to_string()))?
                .file_type()
                .is_symlink()
        {
            return Err(error("Provider path symlink rejected"));
        }
    }
    Ok(current)
}
