use super::*;
use rusqlite::params;
pub fn inspect(root: &Path) -> Result<View> {
    let _guard = store::lock(root)?;
    let (db, _) = database(root)?;
    let policy = super::super::policy::load(root)?;
    let mut q = sql(db.prepare("SELECT receipt_json FROM provider_receipts ORDER BY provider_id"))?;
    let receipts: Vec<Receipt> = sql(q.query_map([], |r| r.get::<_, String>(0)))?
        .map(|r| sql(r).and_then(|s| store::json(s.as_bytes())))
        .collect::<Result<Vec<_>>>()?;
    drop(q);
    drop(_guard);
    let mut providers = registry()?
        .into_iter()
        .map(|p| {
            let r = receipts.iter().find(|r| r.provider_id == p.descriptor().id);
            p.health(root, r, &policy.code_intelligence)
        })
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
    let adapter_details = provider.plan_details(root, action, &p.code_intelligence)?;
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

    let plan=Plan{id:plan_id,provider_id:id.into(),action:action.into(),project_id:a.project_id,before_checksum:checksum(&previous)?,source_revision:index.revision,snapshot_digest:index.digest,affected_paths:paths,network:provider.network_operation(action),package_origin:d.origin,provider_version:d.version,diagnostics:vec!["Review owned paths, origin and network policy before applying; memory and Git work are preserved".into()],details:Some(serde_json::json!({"policy_sha256":digest(&serde_json::to_vec(&p.code_intelligence).map_err(|e|error(e.to_string()))?),"adapter":adapter_details}))};
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
        if std::fs::symlink_metadata(&current).is_ok_and(|m| m.file_type().is_symlink()) {
            return Err(error("Provider path symlink rejected"));
        }
    }
    Ok(current)
}
