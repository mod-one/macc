pub(super) use super::snapshot::{source_snapshot as snapshot, SourceSnapshot as Index};
use super::*;
pub(super) struct Native;
impl CodeIntelligenceProvider for Native {
    fn priority(&self, q: &Query) -> u32 {
        if q.kind == "file_search" {
            0
        } else {
            1000
        }
    }
    fn descriptor(&self) -> Descriptor {
        Descriptor {
            schema_version: 1,
            id: "native".into(),
            version: crate::version().into(),
            origin: "bundled MACC binary".into(),
            operations: [
                "detect",
                "install",
                "deploy",
                "index",
                "update",
                "rollback",
                "disable",
                "undeploy",
                "uninstall",
                "query",
            ]
            .into_iter()
            .map(str::to_string)
            .collect(),
            local_only: true,
            unsupported: vec![
                "dependency_path: no parsed graph".into(),
                "symbol_definition: literal search only".into(),
            ],
        }
    }
    fn plan_paths(
        &self,
        action: &str,
        plan_id: &str,
        previous: &Option<Receipt>,
    ) -> Result<Capability<Vec<String>>> {
        Ok(Capability::Supported(
            if ["index", "update"].contains(&action) {
                vec![format!(
                    ".macc/code-intelligence/native/index-{plan_id}.json"
                )]
            } else if ["uninstall", "undeploy"].contains(&action) {
                previous
                    .as_ref()
                    .map(|r| r.owned_paths.clone())
                    .unwrap_or_default()
            } else {
                vec![]
            },
        ))
    }
    fn execute_lifecycle(
        &self,
        root: &Path,
        plan: &Plan,
        previous: &Option<Receipt>,
        policy: &super::super::policy::CodePolicy,
    ) -> Result<Capability<Receipt>> {
        super::native_lifecycle::execute(root, plan, previous, policy).map(Capability::Supported)
    }
    fn detect(&self, root: &Path) -> Result<Detection> {
        let healthy = crate::git::head_commit(root).is_ok();
        Ok(Detection {
            provider: self.descriptor(),
            status: if healthy { "healthy" } else { "degraded" }.into(),
            diagnostics: vec![if healthy {
                "Git repository detected; bundled literal search requires no network/runtime dependency"
            } else {
                "Git repository with a commit required"
            }
            .into()],
        })
    }
    fn query(
        &self,
        root: &Path,
        q: &Query,
        p: &super::super::policy::CodePolicy,
    ) -> Result<Capability<QueryResult>> {
        if !["file_search", "references"].contains(&q.kind.as_str()) {
            return Ok(Capability::Unsupported{capability:q.kind.clone(),reason:"Native fallback provides literal file/reference search; it does not infer symbol definitions or graph paths".into()});
        }
        let wt = worktree(root, q.worktree.as_deref())?;
        let snapshot = snapshot(&wt, p)?;
        let mut hits = Vec::new();
        let mut snapshot_mismatch = false;
        for (path, hash) in &snapshot.files {
            if q.path.as_ref().is_some_and(|scope| scope != path) {
                continue;
            }
            let Ok((_, data)) = paths::source(&wt, path, p.max_file_bytes) else {
                snapshot_mismatch = true;
                continue;
            };
            if digest(&data) != *hash {
                snapshot_mismatch = true;
                continue;
            }
            let Ok(text) = std::str::from_utf8(&data) else {
                continue;
            };
            for (line, value) in text.lines().enumerate() {
                if value.contains(&q.text) || path.contains(&q.text) {
                    if paths::safe_text(value).is_err() {
                        continue;
                    }
                    let excerpt = value.chars().take(300).collect::<String>();
                    if hits.len() >= p.query_limit as usize {
                        break;
                    }
                    hits.push(CodeHit {
                        path: path.clone(),
                        line: line as u64 + 1,
                        text: excerpt,
                        file_sha256: hash.clone(),
                        provenance:
                            "literal_match_current_worktree; not a parsed symbol relationship"
                                .into(),
                    });
                }
            }
            if hits.len() >= p.query_limit as usize {
                break;
            }
        }
        response::bound(Capability::Supported(QueryResult{provider_id:"native".into(),hits,snapshot_mismatch,diagnostics:vec!["Read-only current tracked files; binary, secret, generated and symlink sources excluded".into()],estimated_tokens:0}), p)
    }
}
pub(crate) fn worktree(root: &Path, requested: Option<&str>) -> Result<std::path::PathBuf> {
    let wt = Path::new(requested.unwrap_or(root.to_str().ok_or_else(|| error("Non-UTF8 root"))?))
        .canonicalize()
        .map_err(|e| error(e.to_string()))?;
    if crate::git::git_common_dir(root)?
        .canonicalize()
        .map_err(|e| error(e.to_string()))?
        != crate::git::git_common_dir(&wt)?
            .canonicalize()
            .map_err(|e| error(e.to_string()))?
    {
        return Err(error("Foreign worktree"));
    }
    Ok(wt)
}
