//! Guided three-way reconciliation of signed histories. Foreign votes, contexts,
//! operation reservations and runtime ownership never become local authority.
use super::{store::*, *};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::{
    collections::{BTreeMap, BTreeSet},
    path::Path,
};
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum MergeSide {
    Local,
    Remote,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ArchiveConflict {
    pub path: String,
    pub base: Option<Value>,
    pub local: Option<Value>,
    pub remote: Option<Value>,
    pub choice: Option<MergeSide>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ArchiveReview {
    pub archive_digest: String,
    pub publisher: String,
    pub local_head: String,
    pub common_events: usize,
    pub plan_id: String,
    pub conflicts: Vec<ArchiveConflict>,
    pub patch: Option<Patch>,
    pub incoming_decisions: Vec<Value>,
    pub warning: String,
}
fn merge(
    path: &str,
    base: Option<&Value>,
    local: Option<&Value>,
    remote: Option<&Value>,
    choices: &BTreeMap<String, MergeSide>,
    conflicts: &mut Vec<ArchiveConflict>,
) -> Option<Value> {
    if local == remote {
        return local.cloned();
    }
    if local == base {
        return remote.cloned();
    }
    if remote == base {
        return local.cloned();
    }
    if let (Some(Value::Object(a)), Some(Value::Object(b)), Some(Value::Object(c))) =
        (base, local, remote)
    {
        let keys = a
            .keys()
            .chain(b.keys())
            .chain(c.keys())
            .collect::<BTreeSet<_>>();
        let mut out = serde_json::Map::new();
        for key in keys {
            let escaped = key.replace('~', "~0").replace('/', "~1");
            if let Some(value) = merge(
                &format!("{path}/{escaped}"),
                a.get(key),
                b.get(key),
                c.get(key),
                choices,
                conflicts,
            ) {
                out.insert(key.clone(), value);
            }
        }
        return Some(Value::Object(out));
    }
    let choice = choices.get(path).copied();
    conflicts.push(ArchiveConflict {
        path: path.into(),
        base: base.cloned(),
        local: local.cloned(),
        remote: remote.cloned(),
        choice,
    });
    match choice {
        Some(MergeSide::Remote) => remote.cloned(),
        _ => local.cloned(),
    }
}
fn graph_map(graph: &Value) -> crate::Result<Value> {
    let mut graph = graph.clone();
    let mut nodes = serde_json::Map::new();
    for node in super::graph::tasks(&graph)? {
        let id = node["id"]
            .as_str()
            .ok_or_else(|| error("Archive task missing identity"))?;
        if nodes.insert(id.into(), node.clone()).is_some() {
            return Err(error("Duplicate archive task identity"));
        }
    }
    graph["tasks"] = Value::Object(nodes);
    Ok(graph)
}
fn from_map(mut graph: Value, local: &Value, remote: &Value) -> crate::Result<Value> {
    let mut nodes = graph["tasks"]
        .as_object()
        .ok_or_else(|| error("Invalid merged task map"))?
        .clone();
    let mut ordered = Vec::new();
    for node in super::graph::tasks(local)?
        .iter()
        .chain(super::graph::tasks(remote)?)
    {
        if let Some(node) = node["id"].as_str().and_then(|id| nodes.remove(id)) {
            ordered.push(node);
        }
    }
    ordered.extend(nodes.into_values());
    graph["tasks"] = json!(ordered);
    Ok(graph)
}
impl Store {
    pub(super) fn verified_archive(
        &self,
        bundle: &SignedArchive,
        signers: &Path,
    ) -> crate::Result<Store> {
        super::secure::transport_authority(&policy(&self.root)?, signers, &bundle.principal)?;
        super::secure::verify_signature(
            &self.root,
            signers,
            &bundle.principal,
            "macc-decisions",
            &super::secure::canonical_bytes(&bundle.archive)?,
            &bundle.signature,
        )?;
        if bundle.archive.schema_version != 1
            || bundle.archive.project_id != self.project_id
            || bundle.archive.repository_roots != super::transport::roots(&self.root)?
            || bundle.archive.tables.len() != super::transport::TABLES.len()
            || super::transport::TABLES
                .iter()
                .any(|t| !bundle.archive.tables.contains_key(*t))
        {
            return Err(error("Archive reconciliation requires the same enrolled project, repository and complete schema"));
        }
        let db = sql(rusqlite::Connection::open_in_memory())?;
        for table in super::transport::TABLES {
            let schema: String = sql(self.db.query_row(
                "SELECT sql FROM sqlite_master WHERE type='table' AND name=?1",
                [table],
                |r| r.get(0),
            ))?;
            sql(db.execute_batch(&schema))?;
            for row in &bundle.archive.tables[*table] {
                let values = row
                    .iter()
                    .map(|v| match v {
                        Value::Null => Ok(rusqlite::types::Value::Null),
                        Value::String(s) => Ok(rusqlite::types::Value::Text(s.clone())),
                        Value::Number(n) => n
                            .as_i64()
                            .map(rusqlite::types::Value::Integer)
                            .ok_or_else(|| error("Unsupported archive integer")),
                        _ => Err(error("Unsupported archive cell")),
                    })
                    .collect::<crate::Result<Vec<_>>>()?;
                let placeholders = (1..=row.len())
                    .map(|n| format!("?{n}"))
                    .collect::<Vec<_>>()
                    .join(",");
                sql(db.execute(
                    &format!("INSERT INTO {table} VALUES({placeholders})"),
                    rusqlite::params_from_iter(values),
                ))?;
            }
        }
        let id: String = sql(db.query_row(
            "SELECT value FROM metadata WHERE key='project_id'",
            [],
            |r| r.get(0),
        ))?;
        if id != self.project_id {
            return Err(error(
                "Archive metadata identity differs from its signed envelope",
            ));
        }
        let incoming = Store {
            root: self.root.clone(),
            path: self.path.clone(),
            project_id: id,
            db,
        };
        incoming.verify()?;
        Ok(incoming)
    }
    pub fn archive_review(
        &self,
        bundle: &SignedArchive,
        signers: &Path,
        plan_id: &str,
        choices: &BTreeMap<String, MergeSide>,
    ) -> crate::Result<ArchiveReview> {
        let _lock = self.mutation_lock()?;
        self.archive_review_locked(bundle, signers, plan_id, choices)
    }
    fn archive_review_locked(
        &self,
        bundle: &SignedArchive,
        signers: &Path,
        plan_id: &str,
        choices: &BTreeMap<String, MergeSide>,
    ) -> crate::Result<ArchiveReview> {
        self.verify()?;
        let incoming = self.verified_archive(bundle, signers)?;
        let local = self.plan(plan_id)?;
        let remote = incoming.plan(plan_id)?;
        if local.source != remote.source || local.reference_branch != remote.reference_branch {
            return Err(error(
                "Incoming plan has a different source/reference scope",
            ));
        }
        let mut stmt = sql(self.db.prepare("SELECT hash FROM events ORDER BY seq"))?;
        let hashes = sql(stmt.query_map([], |r| r.get::<_, String>(0)))?
            .map(sql)
            .collect::<crate::Result<Vec<_>>>()?;
        let events = &bundle.archive.tables["events"];
        let common = hashes
            .iter()
            .zip(events)
            .take_while(|(hash, row)| row.last().and_then(Value::as_str) == Some(hash.as_str()))
            .count();
        if common == 0 {
            return Err(error(
                "No shared verified event ancestor; enroll or review as a new proposal instead",
            ));
        }
        let mut base = None;
        for row in &events[..common] {
            if row.get(1).and_then(Value::as_str) != Some(plan_id) {
                continue;
            }
            let payload: Value = decode(
                row.get(3)
                    .and_then(Value::as_str)
                    .ok_or_else(|| error("Invalid archive event"))?,
            )?;
            if matches!(
                row.get(2).and_then(Value::as_str),
                Some("plan.imported" | "graph.patch.committed" | "graph.source.reconciled")
            ) && payload.get("graph").is_some()
            {
                base = Some(payload["graph"].clone());
            }
        }
        let base =
            base.ok_or_else(|| error("Shared ancestor does not include the selected plan"))?;
        let mut conflicts = Vec::new();
        let merged = merge(
            "",
            Some(&graph_map(&base)?),
            Some(&graph_map(&local.graph)?),
            Some(&graph_map(&remote.graph)?),
            choices,
            &mut conflicts,
        )
        .ok_or_else(|| error("A plan cannot be deleted"))?;
        if choices
            .keys()
            .any(|path| !conflicts.iter().any(|c| &c.path == path))
        {
            return Err(error("Conflict choices reference an obsolete or unknown field; refresh the reconciliation preview"));
        }
        let merged = from_map(merged, &local.graph, &remote.graph)?;
        let unresolved = conflicts.iter().any(|c| c.choice.is_none());
        let archive_digest = digest(&bundle.archive);
        let patch = if unresolved || merged == local.graph {
            None
        } else {
            let mut p = self.edit_patch(
                &local,
                &merged,
                local.graph_version,
                "archive reconciliation",
            )?;
            p.reason = format!(
                "Explicit reconciliation of signed archive {archive_digest} published by {}",
                bundle.principal
            );
            p.idempotency_key = format!(
                "reconcile:{}",
                digest(&json!([archive_digest, local.graph_version, choices]))
            );
            self.preview_patch(&local, &p, &registry(&self.root)?)?;
            Some(p)
        };
        Ok(ArchiveReview{archive_digest,publisher:bundle.principal.clone(),local_head:hashes.last().cloned().unwrap_or_default(),common_events:common,plan_id:plan_id.into(),conflicts,patch,incoming_decisions:incoming.decisions(plan_id)?.into_iter().map(|d|json!({"id":d.id,"version":d.version,"status":d.status,"question":d.request.question,"resolution":d.resolution,"imported_as_authority":false})).collect(),warning:"Local votes, decisions, delivered history, contexts, operation outcomes and runtime ownership are retained. Incoming answers are evidence only; adopt directions through an explicit local revision/answer. No branch or worktree is removed.".into()})
    }
    pub fn apply_archive_review(
        &mut self,
        bundle: &SignedArchive,
        signers: &Path,
        plan_id: &str,
        choices: &BTreeMap<String, MergeSide>,
        expected_head: &str,
        actor: &str,
        proof: Option<&Attestation>,
    ) -> crate::Result<Plan> {
        crate::session_store::require_stopped(&self.root)?;
        let _controller = crate::fs_lock::AdvisoryLock::acquire(
            &self.path.with_file_name("controller.lock"),
            std::time::Duration::ZERO,
            "signed history reconciliation",
        )?;
        let _lock = self.mutation_lock()?;
        let review = self.archive_review_locked(bundle, signers, plan_id, choices)?;
        if review.local_head != expected_head {
            return Err(error(
                "Local history changed after reconciliation preview; inspect a new preview",
            ));
        }
        let patch = review
            .patch
            .as_ref()
            .ok_or_else(|| error("Resolve every archive conflict explicitly before applying"))?;
        let mut plan = self.plan(plan_id)?;
        let actor = self.graph_authority(&plan, patch, None, actor, proof)?;
        let graph = self.preview_patch(&plan, patch, &registry(&self.root)?)?;
        let tx = sql(self
            .db
            .transaction_with_behavior(rusqlite::TransactionBehavior::Immediate))?;
        event(
            &tx,
            plan_id,
            "archive.reconciled",
            &json!({"actor":actor,"attestation":proof,"review":review,"signed_evidence":bundle}),
        )?;
        super::graph::commit_graph(&tx, &mut plan, patch, graph, &actor)?;
        sql(tx.commit())?;
        Ok(plan)
    }
}
