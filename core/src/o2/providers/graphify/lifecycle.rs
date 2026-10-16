//! Stage immutable generations and activate only after local validation.
use super::super::super::policy::CodePolicy;
use super::*;
fn metadata(r: &Receipt) -> Result<Value> {
    r.metadata
        .clone()
        .ok_or_else(|| error("Install managed Graphify first"))
}
fn remember(meta: &mut Value, generation: &str, hash: &str, kind: &str) {
    if !meta["generations"].is_object() {
        meta["generations"] = json!({});
    }
    meta["generations"][generation] = json!({"sha256":hash,"kind":kind});
}
fn smoke_index(root: &Path, meta: &Value, path: &str, p: &CodePolicy) -> Result<()> {
    let index = index::load(root, path, p)?;
    let raw = store::bytes(&storage::owned(root, &index.graph_path)?)?;
    let graph: Value = store::json(&raw)?;
    let label = graph["nodes"]
        .as_array()
        .and_then(|nodes| {
            nodes
                .iter()
                .find_map(|n| n["label"].as_str().filter(|s| !s.is_empty()))
        })
        .ok_or_else(|| error("No queryable Graphify node"))?;
    let scratch = storage::owned(
        root,
        &format!(
            ".macc/code-intelligence/graphify/queries/{}",
            uuid::Uuid::new_v4()
        ),
    )?;
    let result = (|| {
        let graph = scratch.join("graphify-out/graph.json");
        store::write(root, &graph, &raw)?;
        let text = runtime::command(
            root,
            meta,
            &[
                "query".into(),
                label.into(),
                "--graph".into(),
                graph.to_string_lossy().into_owned(),
                "--budget".into(),
                "400".into(),
            ],
            &scratch,
            p,
        )?;
        if !text.lines().any(|s| s.starts_with("NODE ")) {
            return Err(error("Graphify index query smoke failed"));
        }
        Ok(())
    })();
    let _ = std::fs::remove_dir_all(scratch);
    result
}
pub(super) fn execute(
    root: &Path,
    plan: &Plan,
    previous: &Option<Receipt>,
    p: &CodePolicy,
) -> Result<Receipt> {
    let mut r = previous.clone().unwrap_or(Receipt {
        provider_id: "graphify".into(),
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
            if plan.action == "update"
                && !previous
                    .as_ref()
                    .is_some_and(|r| ["deployed", "healthy"].contains(&r.state.as_str()))
            {
                return Err(error(
                    "Deploy managed Graphify before updating; use install for a new runtime",
                ));
            }
            super::super::operations::progress(
                root,
                plan,
                "running",
                "Installing pinned, hash-verified wheels into a new isolated generation",
            )?;
            let generation = storage::attempt(root, plan)?;
            let mut meta = runtime::install(root, plan, &generation, p)?;
            if let Some(old) = r.metadata.as_ref().and_then(|m| m.get("generations")) {
                meta["generations"] = old.clone();
            }
            let hash = meta["runtime_manifest"]
                .as_str()
                .ok_or_else(|| error("Runtime smoke receipt missing"))?
                .to_string();
            remember(&mut meta, &generation, &hash, "runtime");
            r.owned_paths.push(generation);
            r.metadata = Some(meta);
            r.state = "installed".into();
            r.index = None;
            r.index_sha256 = None;
            r.source_revision = None;
            r.snapshot_digest = None;
            if plan.action == "update" {
                build(root, plan, &mut r, p)?;
            }
        }
        "deploy" => {
            let meta = metadata(&r)?;
            runtime::verify(root, &meta)?;
            if let Some(path) = &r.index {
                let index = index::load(root, path, p)?;
                let live = source_snapshot(root, p)?;
                if index.source.digest != live.digest || index.source.revision != live.revision {
                    return Err(error("Deployment index stale; rebuild explicitly"));
                }
                smoke_index(root, &meta, path, p)?;
            }
            r.state = if r.index.is_some() {
                "healthy"
            } else {
                "deployed"
            }
            .into();
        }
        "index" => {
            if !["deployed", "healthy"].contains(&r.state.as_str()) {
                return Err(error("Deploy managed Graphify before indexing"));
            }
            build(root, plan, &mut r, p)?;
        }
        "rollback" => {
            let prior = r
                .previous
                .as_ref()
                .ok_or_else(|| error("No previous Graphify deployment"))?;
            let mut restored = (**prior).clone();
            let mut meta = metadata(&restored)?;
            runtime::verify(root, &meta)?;
            if let Some(path) = &restored.index {
                let cached = index::load(root, path, p)?;
                let live = source_snapshot(root, p)?;
                if cached.source.digest != live.digest || cached.source.revision != live.revision {
                    return Err(error("Rollback snapshot stale; explicitly rebuild instead"));
                }
                smoke_index(root, &meta, path, p)?;
            }
            if let Some(gens) = r
                .metadata
                .as_ref()
                .and_then(|m| m["generations"].as_object())
            {
                for (name, proof) in gens {
                    meta["generations"][name] = proof.clone();
                }
            }
            restored.metadata = Some(meta);
            restored.owned_paths.extend(r.owned_paths);
            restored.previous = None;
            r = restored;
        }
        "disable" => r.state = "disabled".into(),
        "undeploy" | "uninstall" => {
            let mut meta = r.metadata.clone().unwrap_or_else(|| json!({}));
            if !meta.is_object() {
                return Err(error(
                    "Invalid Graphify ownership metadata; preserving all files",
                ));
            }
            let generations = meta["generations"].as_object().cloned().unwrap_or_default();
            for (name, proof) in generations {
                if plan.action == "undeploy" && proof["kind"] != "index" {
                    continue;
                }
                if !plan.affected_paths.contains(&name) {
                    return Err(error(
                        "Generation was not included in reviewed removal paths",
                    ));
                }
                let retained = storage::remove(
                    root,
                    &name,
                    proof["sha256"]
                        .as_str()
                        .ok_or_else(|| error("Missing ownership proof"))?,
                )?;
                if !retained {
                    meta["generations"].as_object_mut().unwrap().remove(&name);
                    r.owned_paths.retain(|s| s != &name);
                }
            }
            r.index = None;
            r.index_sha256 = None;
            r.source_revision = None;
            r.snapshot_digest = None;
            r.previous = None;
            if plan.action == "uninstall" {
                meta.as_object_mut().unwrap().remove("runtime");
                r.state = "missing".into();
            } else {
                r.state = "installed".into();
            }
            r.metadata = Some(meta);
        }
        _ => return Err(error("Unsupported Graphify lifecycle action")),
    }
    if !["rollback", "uninstall", "undeploy"].contains(&plan.action.as_str()) {
        r.previous = previous.clone().map(|mut r| {
            r.previous = None;
            Box::new(r)
        });
    }
    r.owned_paths.sort();
    r.owned_paths.dedup();
    r.version = runtime::VERSION.into();
    Ok(r)
}
fn build(root: &Path, plan: &Plan, r: &mut Receipt, p: &CodePolicy) -> Result<()> {
    super::super::operations::progress(
        root,
        plan,
        "running",
        "Extracting a restricted local code snapshot; no semantic/model calls",
    )?;
    let mut meta = metadata(r)?;
    runtime::verify(root, &meta)?;
    let generation = storage::attempt(root, plan)?;
    let path = index::build(root, &generation, &meta, p)?;
    smoke_index(root, &meta, &path, p)?;
    let hash = storage::seal(root, &generation)?;
    remember(&mut meta, &generation, &hash, "index");
    r.index_sha256 = Some(digest(&store::bytes(&storage::owned(root, &path)?)?));
    r.index = Some(path);
    r.source_revision = Some(plan.source_revision.clone());
    r.owned_paths.push(generation);
    r.metadata = Some(meta);
    r.state = "healthy".into();
    Ok(())
}
