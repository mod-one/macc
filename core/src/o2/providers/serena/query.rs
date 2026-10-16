//! Current curated source copies and hash-verified LSP citations, never agent edits.
use super::*;
pub(super) fn query(root: &Path, q: &Query, p: &CodePolicy) -> Result<Capability<QueryResult>> {
    if !["symbol_definition", "references"].contains(&q.kind.as_str()) {
        return Ok(Capability::Unsupported {
            capability: q.kind.clone(),
            reason: "Serena supports Python/Jedi definitions and references only".into(),
        });
    }
    if q.path.is_none() {
        return Ok(Capability::Unsupported { capability:q.kind.clone(), reason:"Serena requires a tracked Python definition file (--path); unscoped queries retain cross-language fallback".into() });
    }
    if !p.serena_enabled {
        return Err(error("Serena disabled by policy"));
    }
    if q.text
        .strip_prefix('/')
        .unwrap_or(&q.text)
        .split('/')
        .any(|s| s.is_empty() || !s.chars().all(|c| c.is_alphanumeric() || c == '_'))
    {
        return Err(error(
            "Serena requires an exact symbol name or qualified Class/method name",
        ));
    }
    let target = super::super::native_worktree(root, q.worktree.as_deref())?;
    let mut snapshot = source_snapshot(&target, p)?;
    let names = snapshot.files.keys().cloned().collect::<Vec<_>>();
    let ignored = super::super::snapshot::ignored_paths(&target, &names, None)?;
    snapshot.files.retain(|path, _| !ignored.contains(path));
    if snapshot.truncated {
        return Err(error(
            "Serena source file limit exceeded; refusing partial symbol analysis",
        ));
    }
    if q.path
        .as_ref()
        .is_some_and(|path| !snapshot.files.contains_key(path) || !path.ends_with(".py"))
    {
        return Ok(Capability::Unsupported{capability:q.kind.clone(),reason:"Serena definition path must be an allowed tracked Python file; other languages are disabled".into()});
    }
    let (db, _) = database(root)?;
    let r = super::super::receipt(&db, "serena")?.ok_or_else(|| error("Deploy Serena first"))?;
    if r.state != "healthy" {
        return Err(error("Serena deployment is not healthy"));
    }
    let meta = r
        .metadata
        .as_ref()
        .ok_or_else(|| error("Missing Serena runtime receipt"))?;
    runtime::verify(root, meta)?;
    let work = runtime::owned(
        root,
        &format!(
            ".macc/code-intelligence/serena/queries/{}",
            uuid::Uuid::new_v4()
        ),
    )?;
    let result = (|| {
        let mut files = Vec::new();
        let mut bytes = 0u64;
        for (path, hash) in &snapshot.files {
            if !path.ends_with(".py") {
                continue;
            }
            let (_, raw) = paths::source(&target, path, p.max_file_bytes)?;
            if digest(&raw) != *hash {
                return Err(error("Serena source changed during export"));
            }
            bytes += raw.len() as u64;
            if bytes > p.serena_corpus_max_bytes {
                return Err(error("Serena Python corpus byte limit exceeded"));
            }
            store::write(root, &work.join("sources").join(path), &raw)?;
            files.push(path.clone());
        }
        if files.is_empty() {
            return Err(error("No allowed tracked Python files; Serena languages other than Python/Jedi are disabled"));
        }
        let request = json!({"files":files,"text":q.text,"kind":q.kind,"path":q.path,"limit":p.query_limit,"timeout":p.serena_timeout_seconds});
        store::write(
            root,
            &work.join("request.json"),
            &serde_json::to_vec(&request).map_err(|e| error(e.to_string()))?,
        )?;
        let response = runtime::command(root, meta, &work, p)?;
        let hits = normalize(&target, &snapshot, &response, p)?;
        let after = source_snapshot(&target, p)?;
        if after.digest != snapshot.digest
            || after.revision != snapshot.revision
            || super::super::snapshot::ignored_paths(&target, &names, None)? != ignored
        {
            return Err(error(
                "Serena source changed during query; native fallback retained",
            ));
        }
        Ok(Capability::Supported(QueryResult{provider_id:"serena".into(),hits,snapshot_mismatch:false,diagnostics:vec!["Python/Jedi LSP; exact names; curated tracked sources only; other languages disabled".into()],estimated_tokens:0}))
    })();
    let _ = std::fs::remove_dir_all(work);
    result
}
pub(super) fn normalize(
    target: &Path,
    snapshot: &SourceSnapshot,
    value: &Value,
    p: &CodePolicy,
) -> Result<Vec<CodeHit>> {
    if value["schema"] != 1 || value["language"] != "python" || value["backend"] != "jedi" {
        return Err(error("Incompatible Serena response"));
    }
    let raw = value["hits"]
        .as_array()
        .ok_or_else(|| error("Invalid Serena hits"))?;
    if raw.len() > p.query_limit as usize {
        return Err(error("Serena result limit exceeded"));
    }
    let mut hits = Vec::new();
    for hit in raw {
        let path = runtime::field(hit, "path")?;
        let line = hit["line"]
            .as_u64()
            .filter(|n| *n > 0)
            .ok_or_else(|| error("Invalid Serena citation line"))?;
        let hash = snapshot
            .files
            .get(path)
            .filter(|_| path.ends_with(".py"))
            .ok_or_else(|| error("Serena cited an unexported source"))?;
        let (_, raw) = paths::source(target, path, p.max_file_bytes)?;
        if digest(&raw) != *hash {
            return Err(error("Serena citation source changed"));
        }
        let source = std::str::from_utf8(&raw).map_err(|e| error(e.to_string()))?;
        let text = source
            .lines()
            .nth((line - 1) as usize)
            .ok_or_else(|| error("Serena citation line outside source"))?;
        paths::safe_text(text)?;
        hits.push(CodeHit {
            path: path.into(),
            line,
            text: text.chars().take(500).collect(),
            file_sha256: hash.clone(),
            provenance: "serena:python:jedi:LSP".into(),
        });
    }
    Ok(hits)
}
