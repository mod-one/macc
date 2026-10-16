//! Normalize verified CLI node/edge lines; keep EXTRACTED versus INFERRED visible.
use super::*;
pub(super) fn query(
    root: &Path,
    q: &Query,
    p: &super::super::super::policy::CodePolicy,
) -> Result<Capability<QueryResult>> {
    if !["references", "architecture_summary", "file_search"].contains(&q.kind.as_str()) {
        return Ok(Capability::Unsupported{capability:q.kind.clone(),reason:"Graphify adapter exposes bounded graph traversal; precise LSP/path/impact operations remain unsupported".into()});
    }
    if !p.graphify_enabled {
        return Ok(Capability::Unsupported {
            capability: q.kind.clone(),
            reason: "Managed Graphify is disabled".into(),
        });
    }
    let receipt = {
        let (db, _) = database(root)?;
        super::super::receipt(&db, "graphify")?.ok_or_else(|| error("Graphify not deployed"))?
    };
    let meta = receipt
        .metadata
        .as_ref()
        .ok_or_else(|| error("Missing runtime receipt"))?;
    runtime::verify(root, meta)?;
    let index = index::load(
        root,
        receipt
            .index
            .as_deref()
            .ok_or_else(|| error("Graphify index unavailable"))?,
        p,
    )?;
    let target = super::super::native_worktree(root, q.worktree.as_deref())?;
    if index::settings(&target, p)? != index.settings_sha256 {
        return Err(error(
            "Worktree exclusion settings differ from Graphify snapshot; native fallback retained",
        ));
    }
    let source = source_snapshot(&target, p)?;
    if source.digest != index.source.digest || source.revision != index.source.revision {
        return Err(error(
            "Graphify snapshot mismatch; native fallback required",
        ));
    }
    let scratch_rel = format!(
        ".macc/code-intelligence/graphify/queries/{}",
        uuid::Uuid::new_v4()
    );
    let scratch = storage::owned(root, &scratch_rel)?;
    let result = (|| {
        let raw = store::bytes(&storage::owned(root, &index.graph_path)?)?;
        let graph = scratch.join("graphify-out/graph.json");
        store::write(root, &graph, &raw)?;
        let text = runtime::command(
            root,
            meta,
            &[
                "query".into(),
                q.text.clone(),
                "--graph".into(),
                graph.to_string_lossy().into_owned(),
                "--budget".into(),
                p.output_tokens.to_string(),
            ],
            &scratch,
            p,
        )?;
        let node = regex::Regex::new(r"^NODE .*? \[src=(.*?) loc=(?:L([0-9]+))? community=")
            .map_err(|e| error(e.to_string()))?;
        let edge =
            regex::Regex::new(r"^EDGE .* at=(.*):L([0-9]+)$").map_err(|e| error(e.to_string()))?;
        let mut hits = Vec::new();
        let mut mismatch = false;
        for line in text.lines() {
            let Some(capture) = node.captures(line).or_else(|| edge.captures(line)) else {
                continue;
            };
            let path = &capture[1];
            let Some(hash) = index.files.get(path) else {
                continue;
            };
            let (_, data) = paths::source(&target, path, p.max_file_bytes)?;
            if digest(&data) != *hash {
                mismatch = true;
                continue;
            }
            let location = capture
                .get(2)
                .and_then(|n| n.as_str().parse::<u64>().ok())
                .unwrap_or(1);
            if location == 0 || location > data.split(|b| *b == b'\n').count() as u64 {
                continue;
            }
            paths::safe_text(line)?;
            hits.push(CodeHit{path:path.into(),line:location,text:line.into(),file_sha256:hash.clone(),provenance:format!("graphify {}; CLI graph traversal; EXTRACTED/INFERRED/AMBIGUOUS preserved as labeled",runtime::VERSION)});
        }
        let after = source_snapshot(&target, p)?;
        if after.digest != index.source.digest
            || after.revision != index.source.revision
            || index::settings(&target, p)? != index.settings_sha256
        {
            return Err(error(
                "Sources or exclusions changed during Graphify query; native fallback retained",
            ));
        }
        if hits.is_empty() {
            return Err(error(
                "Graphify returned no verifiable source hits; native fallback retained",
            ));
        }
        Ok(Capability::Supported(QueryResult {
            provider_id: "graphify".into(),
            hits,
            snapshot_mismatch: mismatch,
            diagnostics: vec![
                "Local code-only AST graph; inferred relations are not literal source declarations"
                    .into(),
            ],
            estimated_tokens: 0,
        }))
    })();
    let _ = std::fs::remove_dir_all(scratch);
    result
}
