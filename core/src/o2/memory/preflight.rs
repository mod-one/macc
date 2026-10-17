//! Derived, request-local verification outside authority locks; never a second store.
use super::*;
use std::{
    collections::BTreeMap,
    time::{Duration, Instant},
};
#[derive(Clone, PartialEq, Eq)]
struct Source {
    sha: String,
    stamp: super::super::paths::Stamp,
}
#[derive(Clone, PartialEq, Eq)]
struct ToolProof {
    fingerprint: String,
    path: std::path::PathBuf,
    stamp: super::super::paths::Stamp,
}
fn tool_proof(root: &Path, db: &rusqlite::Connection, tool: &str) -> Result<ToolProof> {
    let p = super::super::compaction::observations::identity(root, db, tool)?;
    let fingerprint = super::super::compaction::observations::fingerprint(&p)?;
    let path = std::path::PathBuf::from(
        p.command_path
            .ok_or_else(|| error("Provider executable provenance unavailable"))?,
    );
    let stamp = super::super::paths::stamp(&path)?;
    Ok(ToolProof {
        fingerprint,
        path,
        stamp,
    })
}
struct Entry {
    checksum: String,
    sources: BTreeMap<String, Source>,
    executables: BTreeMap<String, ToolProof>,
    result: std::result::Result<(), String>,
}
#[derive(Default)]
pub(crate) struct PreparedFreshness {
    root: Option<std::path::PathBuf>,
    project: String,
    policy: String,
    at: Option<Instant>,
    entries: BTreeMap<String, Entry>,
}
fn policy_hash(p: &impl serde::Serialize) -> Result<String> {
    Ok(digest(
        &serde_json::to_vec(p).map_err(|e| error(e.to_string()))?,
    ))
}
fn sources(target: &Path, m: &Memory) -> Result<BTreeMap<String, Source>> {
    let mut values = BTreeMap::new();
    for e in &m.proposal.evidence {
        if ["file", "prd", "test", "symbol"].contains(&e.kind.as_str()) {
            if values.contains_key(&e.reference) {
                continue;
            }
            let (path, raw) = super::super::paths::source(target, &e.reference, 1048576)?;
            values.insert(
                e.reference.clone(),
                Source {
                    sha: digest(&raw),
                    stamp: super::super::paths::stamp(&path)?,
                },
            );
        }
    }
    Ok(values)
}
fn executables(
    root: &Path,
    db: &rusqlite::Connection,
    m: &Memory,
) -> Result<BTreeMap<String, ToolProof>> {
    let mut values = BTreeMap::new();
    for e in &m.proposal.evidence {
        if let Some(Binding::Tool { tool_id, .. }) = &e.binding {
            values.insert(tool_id.clone(), tool_proof(root, db, tool_id)?);
        }
    }
    Ok(values)
}
fn prepare(
    root: &Path,
    target: &Path,
    db: &rusqlite::Connection,
    project: &str,
    records: Vec<Memory>,
) -> Result<PreparedFreshness> {
    let p = super::super::policy::load(root)?;
    let mut result = PreparedFreshness {
        root: Some(root.canonicalize().map_err(|e| error(e.to_string()))?),
        project: project.into(),
        policy: policy_hash(&p)?,
        at: Some(Instant::now()),
        entries: BTreeMap::new(),
    };
    let mut checker = Checker::new(root, target, db, &p.memory);
    for m in records {
        let mut before = BTreeMap::new();
        let mut commands = BTreeMap::new();
        let checked = (|| {
            before = sources(target, &m)?;
            commands = executables(root, db, &m)?;
            checker.check(&m)?;
            if sources(target, &m)? != before || executables(root, db, &m)? != commands {
                return Err(error("Source changed during freshness preflight"));
            }
            Ok(())
        })()
        .map_err(|e: crate::MaccError| e.to_string());
        result.entries.insert(
            m.id.clone(),
            Entry {
                checksum: hit(m, 0, "")?.checksum,
                sources: before,
                executables: commands,
                result: checked,
            },
        );
    }
    Ok(result)
}
pub(crate) fn prepare_search(root: &Path, r: &Search) -> Result<PreparedFreshness> {
    let p = super::super::policy::load(root)?;
    let (db, a) = super::super::readonly_database(root)?;
    if !p.memory.enabled && !r.include_inactive {
        return Ok(Default::default());
    }
    let target = super::super::providers::native_worktree(root, r.access.worktree.as_deref())?;
    let records = super::search::candidates(&db, &a.project_id, r)?
        .into_iter()
        .filter(|m| m.status == "active")
        .collect();
    prepare(root, &target, &db, &a.project_id, records)
}
pub(crate) fn prepare_records(root: &Path, ids: &[String]) -> Result<PreparedFreshness> {
    let (db, a) = super::super::readonly_database(root)?;
    let mut result = PreparedFreshness::default();
    for id in ids {
        let m = load(&db, id, &a.project_id)?;
        let target = if m.proposal.scope == "worktree" {
            super::super::providers::native_worktree(root, m.proposal.scope_ref.as_deref())?
        } else {
            root.to_path_buf()
        };
        let next = prepare(root, &target, &db, &a.project_id, vec![m])?;
        if result.at.is_none() {
            result = next;
        } else {
            result.entries.extend(next.entries);
        }
    }
    Ok(result)
}
pub(crate) fn prepare_record_at(root: &Path, id: &str, target: &Path) -> Result<PreparedFreshness> {
    let (db, a) = super::super::readonly_database(root)?;
    let m = load(&db, id, &a.project_id)?;
    prepare(root, target, &db, &a.project_id, vec![m])
}
pub(crate) fn prepare_review(root: &Path, r: &Review) -> Result<PreparedFreshness> {
    let (db, a) = super::super::readonly_database(root)?;
    let mut m = load(&db, &r.id, &a.project_id)?;
    if let Some(e) = &r.evidence {
        m.proposal.evidence = e.clone();
    }
    validate(
        &m.proposal,
        super::super::policy::load(root)?.memory.max_body_chars,
    )?;
    let target = if m.proposal.scope == "worktree" {
        super::super::providers::native_worktree(root, m.proposal.scope_ref.as_deref())?
    } else {
        root.to_path_buf()
    };
    prepare(root, &target, &db, &a.project_id, vec![m])
}
pub(crate) struct Verifier<'a> {
    root: &'a Path,
    target: &'a Path,
    db: &'a rusqlite::Connection,
    policy: &'a super::super::policy::MemoryPolicy,
    prepared: &'a PreparedFreshness,
    valid: std::result::Result<(), String>,
    files: BTreeMap<String, std::result::Result<super::super::paths::Stamp, String>>,
    tools: BTreeMap<String, std::result::Result<ToolProof, String>>,
}
impl<'a> Verifier<'a> {
    pub(crate) fn new(
        root: &'a Path,
        target: &'a Path,
        db: &'a rusqlite::Connection,
        policy: &'a super::super::policy::MemoryPolicy,
        prepared: &'a PreparedFreshness,
    ) -> Self {
        let valid = (|| {
            let current = super::super::policy::load(root)?;
            if prepared.root.as_deref()
                != Some(
                    root.canonicalize()
                        .map_err(|e| error(e.to_string()))?
                        .as_path(),
                )
                || prepared.policy != policy_hash(&current)?
                || *policy != current.memory
            {
                return Err(error("Freshness authority/policy changed; retry retrieval"));
            }
            Ok(())
        })()
        .map_err(|e: crate::MaccError| e.to_string());
        Self {
            root,
            target,
            db,
            policy,
            prepared,
            valid,
            files: BTreeMap::new(),
            tools: BTreeMap::new(),
        }
    }
    pub(crate) fn check(&mut self, m: &Memory) -> Result<()> {
        self.valid.clone().map_err(error)?;
        if self.prepared.project != m.project_id
            || self
                .prepared
                .at
                .is_none_or(|at| at.elapsed() > Duration::from_secs(60))
        {
            return Err(error(
                "Freshness preflight expired or project changed; retry retrieval",
            ));
        }
        let e = self.prepared.entries.get(&m.id).ok_or_else(|| {
            error("No current freshness preflight for this record; retry retrieval")
        })?;
        if e.checksum != hit(m.clone(), 0, "")?.checksum {
            return Err(error("Memory changed after freshness preflight"));
        }
        e.result.clone().map_err(error)?;
        for (path, source) in &e.sources {
            let current = self.files.entry(path.clone()).or_insert_with(|| {
                super::super::paths::source_metadata(self.target, path, 1048576)
                    .and_then(|(path, _)| super::super::paths::stamp(&path))
                    .map_err(|e| e.to_string())
            });
            if current.as_ref().map_err(|e| error(e.clone()))? != &source.stamp {
                return Err(error("Source changed after freshness preflight"));
            }
        }
        let expected_commands = &e.executables;
        for e in &m.proposal.evidence {
            if let Some(Binding::Tool {
                tool_id,
                observed_at,
            }) = &e.binding
            {
                if now().saturating_sub(*observed_at)
                    >= (self.policy.provider_note_ttl_days * 86400) as i64
                {
                    return Err(error("Provider observation expired"));
                }
                let current = self.tools.entry(tool_id.clone()).or_insert_with(|| {
                    tool_proof(self.root, self.db, tool_id).map_err(|e| e.to_string())
                });
                let current = current.as_ref().map_err(|e| error(e.clone()))?;
                if Some(&current.fingerprint) != e.sha256.as_ref()
                    || expected_commands.get(tool_id) != Some(current)
                {
                    return Err(error("Provider observation changed after preflight"));
                }
            }
        }
        Ok(())
    }
}
