//! Scoped query-only readers. No caller-supplied Access or project/worktree selector.
use super::*;
use rusqlite::Connection;
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Search {
    #[serde(default)]
    query: String,
    #[serde(default)]
    cursor: usize,
    #[serde(default = "limit")]
    limit: usize,
}
fn limit() -> usize {
    5
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Get {
    id: String,
    #[serde(default)]
    offset: usize,
    #[serde(default = "chars")]
    max_chars: usize,
}
fn chars() -> usize {
    512
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Code {
    text: String,
    #[serde(default = "kind")]
    kind: String,
    #[serde(default)]
    path: Option<String>,
}
fn kind() -> String {
    "file_search".into()
}
fn parse<T: serde::de::DeserializeOwned>(v: Value) -> Result<T> {
    serde_json::from_value(v).map_err(|_| error("Invalid scoped tool arguments"))
}
pub(super) fn approved(
    db: &Connection,
    root: &Path,
    scope: &Scope,
    id: &str,
) -> Result<memory::Hit> {
    let m = memory::load(db, id, &scope.project)?;
    let p = policy::load(root)?;
    if m.status != "active"
        || !memory::in_scope(&m.proposal, &scope.access)
        || m.proposal.confidence < p.memory.min_confidence
    {
        return Err(error("Approved in-scope memory not available"));
    }
    let conflict:bool=sql(db.query_row("SELECT EXISTS(SELECT 1 FROM memory_conflicts WHERE status='open' AND (left_id=?1 OR right_id=?1))",[id],|r|r.get(0)))?;
    if conflict {
        return Err(error("Memory has an unresolved conflict"));
    }
    let prepared = memory::prepare_record_at(root, id, &scope.worktree)?;
    memory::preflight::Verifier::new(root, &scope.worktree, db, &p.memory, &prepared).check(&m)?;
    memory::hit(m, 0, "approved; scope and current evidence verified")
}
pub fn call_read(root: &Path, c: &Credentials, name: &str, args: Value) -> Result<Value> {
    let (db, scope) = super::auth::connection(root, c)?;
    let value = match name {
        "memory.search" | "memory.related_to_diff" => {
            let request: Search = parse(args)?;
            clean(&request.query)?;
            if !(1..=20).contains(&request.limit) {
                return Err(error("Result limit must be 1..20"));
            }
            let search = memory::Search {
                query: request.query,
                access: scope.access.clone(),
                include_inactive: false,
            };
            let prepared = memory::prepare_search(root, &search)?;
            let mut hits = memory::search::search_in(
                &db,
                &scope.project,
                root,
                &scope.worktree,
                &search,
                false,
                &prepared,
            )?
            .hits;
            if name == "memory.related_to_diff" {
                let changed = changed_paths(&scope.worktree)?;
                hits.retain(|h| {
                    h.memory
                        .proposal
                        .evidence
                        .iter()
                        .any(|e| changed.contains(&e.reference))
                });
            }
            if request.cursor > hits.len() {
                return Err(error("Cursor outside current result set"));
            }
            let end = (request.cursor + request.limit).min(hits.len());
            json!({"records":&hits[request.cursor..end],"next_cursor":(end<hits.len()).then_some(end),"cursor":request.cursor,"trust":"untrusted_evidence; PRD remains authoritative"})
        }
        "memory.get" => {
            let request: Get = parse(args)?;
            if !(1..=4096).contains(&request.max_chars) {
                return Err(error("Fragment size must be 1..4096 characters"));
            }
            let hit = approved(&db, root, &scope, &request.id)?;
            let raw = serde_json::to_string(&hit)
                .map_err(|e| error(e.to_string()))?
                .chars()
                .collect::<Vec<_>>();
            if request.offset > raw.len() {
                return Err(error("Fragment offset outside record"));
            }
            let end = (request.offset + request.max_chars).min(raw.len());
            json!({"id":request.id,"checksum":hit.checksum,"offset":request.offset,"fragment":raw[request.offset..end].iter().collect::<String>(),"next_offset":(end<raw.len()).then_some(end),"total_chars":raw.len(),"trust":"untrusted_evidence; PRD remains authoritative"})
        }
        "context.get_checkpoint" => {
            #[derive(Deserialize)]
            #[serde(deny_unknown_fields)]
            struct Empty {}
            let _: Empty = parse(args)?;
            let cp = scope
                .checkpoint
                .as_ref()
                .ok_or_else(|| error("No verified checkpoint for this invocation"))?;
            json!({"id":cp.id,"task_id":cp.task_id,"phase":cp.phase,"role":cp.role,"source_revision":cp.source_revision,"next_action":cp.next_action,"completed_steps":cp.completed_steps,"blocking_issues":cp.blocking_issues,"summary_only":true,"notice":"Checkpoint evidence never proves task delivery. Mandatory PRD restore remains in the runner."})
        }
        "code.query" | "code.path" | "context.search_sources" => {
            let p = enabled(root)?;
            if !p.code_intelligence.enabled {
                return Err(error("Code Intelligence disabled"));
            }
            let mut request: Code = parse(args)?;
            if name == "code.path" {
                request.kind = "dependency_path".into();
            }
            if name == "context.search_sources" {
                request.kind = "file_search".into();
            }
            drop(db);
            let q = providers::Query {
                text: request.text,
                kind: request.kind,
                path: request.path,
                worktree: Some(scope.worktree.to_string_lossy().into_owned()),
            };
            let value = serde_json::to_value(providers::query(root, &q)?)
                .map_err(|e| error(e.to_string()))?;
            super::auth::connection(root, c)?;
            super::clean_output(&value)?;
            return Ok(value);
        }
        _ => return Err(error("Unknown read capability")),
    };
    // A revoked lease cannot receive the result of a long source/evidence lookup.
    drop(db);
    super::auth::connection(root, c)?;
    super::clean_output(&value)?;
    Ok(value)
}

fn changed_paths(root: &Path) -> Result<std::collections::BTreeSet<String>> {
    let mut names = std::collections::BTreeSet::new();
    for args in [
        vec!["diff", "--no-ext-diff", "--name-only", "-z", "HEAD", "--"],
        vec!["ls-files", "--others", "--exclude-standard", "-z"],
    ] {
        let out = std::process::Command::new("git")
            .arg("-C")
            .arg(root)
            .args(args)
            .output()
            .map_err(|e| error(e.to_string()))?;
        if !out.status.success() || out.stdout.len() > 1048576 {
            return Err(error("Bounded Git diff unavailable"));
        }
        let text = String::from_utf8(out.stdout).map_err(|e| error(e.to_string()))?;
        names.extend(
            text.split('\0')
                .filter(|s| !s.is_empty())
                .map(str::to_string),
        );
    }
    Ok(names)
}
