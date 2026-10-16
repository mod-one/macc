//! Reviewed install/activation and proof-based removal. No persistent LSP processes.
use super::*;
pub(super) fn execute(
    root: &Path,
    plan: &Plan,
    previous: &Option<Receipt>,
    p: &CodePolicy,
) -> Result<Receipt> {
    let mut r = previous.clone().unwrap_or(Receipt {
        provider_id: "serena".into(),
        version: runtime::VERSION.into(),
        state: "missing".into(),
        index: None,
        index_sha256: None,
        source_revision: None,
        snapshot_digest: None,
        owned_paths: vec![],
        previous: None,
        metadata: None,
    });
    match plan.action.as_str() {
        "install" | "update" => {
            if plan.action == "update" && !["healthy", "deployed"].contains(&r.state.as_str()) {
                return Err(error(
                    "Deploy Serena before update; use install for a new runtime",
                ));
            }
            super::super::operations::progress(
                root,
                plan,
                "running",
                "Installing hash-verified Serena SolidLSP subset and pinned Python/Jedi server",
            )?;
            let mut meta = runtime::install(root, plan, p)?;
            let generation = runtime::field(&meta, "runtime")?.to_string();
            let hash = runtime::field(&meta, "runtime_manifest")?.to_string();
            meta["generations"] = r
                .metadata
                .as_ref()
                .and_then(|m| m.get("generations"))
                .cloned()
                .unwrap_or_else(|| json!({}));
            meta["generations"][&generation] = json!(hash);
            r.owned_paths.push(generation);
            r.metadata = Some(meta);
            r.state = if plan.action == "update" {
                "healthy"
            } else {
                "installed"
            }
            .into();
        }
        "deploy" => {
            let meta = r
                .metadata
                .as_ref()
                .ok_or_else(|| error("Install managed Serena first"))?;
            runtime::verify(root, meta)?;
            runtime::smoke(root, meta, p)?;
            r.state = "healthy".into();
        }
        "disable" => r.state = "disabled".into(),
        "undeploy" => {
            r.state = "installed".into();
            r.previous = None;
        }
        "uninstall" => {
            if let Some(meta) = r.metadata.as_mut() {
                let generations = meta["generations"]
                    .as_object()
                    .ok_or_else(|| error("Missing Serena ownership proofs; preserving files"))?
                    .clone();
                for (name, hash) in generations {
                    if !plan.affected_paths.contains(&name) {
                        return Err(error("Serena generation outside reviewed removal paths"));
                    }
                    if !super::super::generation::remove(
                        root,
                        "serena",
                        &name,
                        hash.as_str()
                            .ok_or_else(|| error("Invalid Serena ownership hash"))?,
                    )? {
                        meta["generations"].as_object_mut().unwrap().remove(&name);
                        r.owned_paths.retain(|s| s != &name);
                    }
                }
                meta.as_object_mut()
                    .ok_or_else(|| error("Invalid Serena metadata"))?
                    .remove("runtime");
            }
            r.state = "missing".into();
            r.previous = None;
        }
        "rollback" => {
            let mut restored = r
                .previous
                .as_deref()
                .cloned()
                .ok_or_else(|| error("No previous Serena deployment"))?;
            let meta = restored
                .metadata
                .as_mut()
                .ok_or_else(|| error("Previous Serena runtime missing"))?;
            runtime::verify(root, meta)?;
            runtime::smoke(root, meta, p)?;
            if let Some(gens) = r
                .metadata
                .as_ref()
                .and_then(|m| m["generations"].as_object())
            {
                for (name, hash) in gens {
                    meta["generations"][name] = hash.clone();
                }
            }
            restored.owned_paths.extend(r.owned_paths);
            restored.previous = None;
            r = restored;
        }
        _ => return Err(error("Unsupported Serena lifecycle action")),
    }
    if !["rollback", "undeploy", "uninstall"].contains(&plan.action.as_str()) {
        r.previous = previous.clone().map(|mut old| {
            old.previous = None;
            Box::new(old)
        });
    }
    r.owned_paths.sort();
    r.owned_paths.dedup();
    r.version = runtime::VERSION.into();
    Ok(r)
}
