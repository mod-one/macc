//! Lifecycle implementation of the built-in provider; manager logic stays generic.
use super::lifecycle::owned_path;
use super::*;
pub(super) fn execute(
    root: &Path,
    plan: &Plan,
    previous: &Option<Receipt>,
    policy: &super::super::policy::CodePolicy,
) -> Result<Receipt> {
    let index = native::snapshot(root, policy)?;
    let mut result = previous.clone().unwrap_or(Receipt {
        provider_id: "native".into(),
        version: crate::version().into(),
        state: "bundled".into(),
        index: None,
        index_sha256: None,
        source_revision: None,
        snapshot_digest: None,
        owned_paths: vec![],
        previous: None,
        metadata: None,
    });
    match plan.action.as_str() {
        "install" => result.state = "installed".into(),
        "deploy" => result.state = "deployed".into(),
        "disable" => result.state = "disabled".into(),
        "index" | "update" => {
            if !["deployed", "healthy"].contains(&result.state.as_str()) {
                return Err(error("Deploy before indexing/updating"));
            }
            let path = &plan.affected_paths[0];
            let raw = if policy.native_map_enabled {
                let cached = match super::native_map::load(root, previous.as_ref(), policy) {
                    Ok(value) => value,
                    Err(e) => {
                        tracing::warn!("Native cache rebuild: {e}");
                        None
                    }
                };
                let map = super::native_map::build(root, policy, cached.as_ref())?;
                result.metadata = Some(map.metadata());
                serde_json::to_vec(&map).map_err(|e| error(e.to_string()))?
            } else {
                result.metadata = None;
                serde_json::to_vec(&index).map_err(|e| error(e.to_string()))?
            };
            if raw.len() as u64 > policy.native_index_max_bytes {
                return Err(error("Native index exceeds configured artifact bound"));
            }
            store::write(root, &root.join(path), &raw)?;
            result.index = Some(path.clone());
            result.index_sha256 = Some(digest(&raw));
            result.source_revision = Some(index.revision);
            result.owned_paths.push(path.clone());
            result.state = "healthy".into();
        }
        "rollback" => {
            let prior = previous
                .as_ref()
                .and_then(|r| r.previous.as_deref())
                .ok_or_else(|| error("No previous deployment receipt"))?;
            if let (Some(path), Some(hash)) = (&prior.index, &prior.index_sha256) {
                let path = owned_path(root, &plan.provider_id, path)?;
                if digest(&store::bytes(&path)?) != *hash {
                    return Err(error("Rollback index missing/corrupt"));
                }
            }
            result = prior.clone();
            // Keep ownership of superseded cache files so later uninstall can remove them.
            if let Some(current) = previous {
                result
                    .owned_paths
                    .extend(current.owned_paths.iter().cloned());
                result.owned_paths.sort();
                result.owned_paths.dedup();
            }
        }
        "undeploy" | "uninstall" => {
            for rel in &plan.affected_paths {
                let path = owned_path(root, &plan.provider_id, rel)?;
                if path.exists() {
                    std::fs::remove_file(&path).map_err(|e| error(e.to_string()))?;
                }
            }
            result.index = None;
            result.index_sha256 = None;
            result.owned_paths.clear();
            result.state = "bundled".into();
        }
        _ => return Err(error("Unsupported provider action")),
    }
    if plan.action != "rollback" {
        let mut old = previous.clone();
        if let Some(r) = old.as_mut() {
            r.previous = None;
        }
        result.previous = old.map(Box::new);
    }
    result.version = crate::version().into();
    Ok(result)
}
