//! Read-only queries overlay current files; dirty worktrees never mutate a base receipt.
use super::*;
pub(in crate::o2::providers) fn query(
    root: &Path,
    q: &Query,
    p: &crate::o2::policy::CodePolicy,
) -> Result<Capability<QueryResult>> {
    if !p.native_map_enabled {
        return Ok(Capability::Unsupported {
            capability: q.kind.clone(),
            reason: "Enable the optional native symbol map in Code Intelligence policies".into(),
        });
    }
    let target = native_worktree(root, q.worktree.as_deref())?;
    let (db, _) = readonly_database(root)?;
    let receipt = super::super::receipt(&db, "native")?;
    drop(db);
    let mut diagnostics = vec![];
    let cached = match load(root, receipt.as_ref(), p) {
        Ok(value) => value,
        Err(e) => {
            diagnostics.push(format!(
                "Native cache unavailable: {e}; current files parsed"
            ));
            None
        }
    };
    let map = build(&target, p, cached.as_ref())?;
    let mut hits = vec![];
    let selector = q.text.trim_start_matches('/');
    let qualified = q.text.starts_with('/') || selector.contains('/');
    for (path, file) in &map.entries {
        if q.path.as_ref().is_some_and(|scope| scope != path) {
            continue;
        }
        if let Some(reason) = &file.diagnostic {
            if diagnostics.len() < 16 {
                diagnostics.push(format!("{path}: {reason}"));
            }
            continue;
        }
        for symbol in &file.symbols {
            if (if qualified {
                symbol.qualified == selector
            } else {
                symbol.name == selector
            }) && hits.len() < p.query_limit as usize
            {
                hits.push(CodeHit{path:path.clone(),line:symbol.line,text:format!("{}: {}",symbol.qualified,symbol.signature),file_sha256:file.sha256.clone(),provenance:format!("native:tree-sitter:{}:EXTRACTED:{}; syntactic declaration, no type/graph inference",file.language,symbol.kind)});
            }
        }
    }
    let after = source_snapshot(&target, p)?;
    let mismatch = after.digest != map.source.digest || after.revision != map.source.revision;
    if mismatch {
        hits.clear();
        diagnostics.push("Source changed during query; symbol results discarded, retry".into());
    }
    diagnostics.push(format!("Native map: parsed={}, reused={}, removed={}, unsupported={}, failed={}, corpus_bytes={}, elapsed_ms={}, truncated={}; read-only current-worktree overlay",map.stats.parsed_files,map.stats.reused_files,map.stats.removed_files,map.stats.unsupported_files,map.stats.failed_files,map.stats.corpus_bytes,map.stats.elapsed_ms,map.symbols_truncated));
    response::bound(
        Capability::Supported(QueryResult {
            provider_id: "native".into(),
            hits,
            snapshot_mismatch: mismatch,
            diagnostics,
            estimated_tokens: 0,
        }),
        p,
    )
}
