//! Durable approval bridge: retain the existing role/quorum evaluator while
//! accepting only currently verifiable, policy-bound evidence.
use super::{store::*, *};
use crate::coordinator::model::{ApprovalRecord, TaskRegistry};
use serde_json::{json, Value};
use std::{collections::BTreeMap, path::Path};

fn full_commit(root: &Path, revision: &str) -> crate::Result<String> {
    if revision.is_empty() || !revision.chars().all(|c| c.is_ascii_hexdigit()) {
        return Err(error("Invalid subject revision"));
    }
    let object = format!("{revision}^{{commit}}");
    let out = crate::git::run_git_output_mapped(
        root,
        &["rev-parse", "--verify", &object],
        "verify approval subject",
    )?;
    if !out.status.success() {
        return Err(error("Unknown or ambiguous approval subject revision"));
    }
    Ok(String::from_utf8_lossy(&out.stdout).trim().into())
}
pub(super) fn approval_manifest(
    root: &Path,
    plan: &Plan,
    task: &str,
    revision: Option<&str>,
) -> crate::Result<Value> {
    let policy = policy(root)?;
    let paths = policy
        .approval_manifests
        .get(task)
        .ok_or_else(|| error("Content-equivalent reuse requires an explicit subject manifest"))?;
    if paths.is_empty() {
        return Err(error("Empty approval manifest"));
    }
    let revision = full_commit(
        root,
        revision.ok_or_else(|| error("Unknown manifest revision"))?,
    )?;
    let mut entries = BTreeMap::new();
    for path in paths {
        if path.is_empty()
            || Path::new(path).is_absolute()
            || Path::new(path)
                .components()
                .any(|c| !matches!(c, std::path::Component::Normal(_)))
        {
            return Err(error(
                "Manifest paths must be explicit project-relative files",
            ));
        }
        let object = format!("{revision}:{path}");
        let out = crate::git::run_git_output_mapped(
            root,
            &["cat-file", "blob", &object],
            "verify subject manifest bytes",
        )?;
        if !out.status.success() {
            return Err(error(format!("Missing manifest artifact {path}")));
        }
        entries.insert(path, digest(&out.stdout));
    }
    let node = super::graph::task(&plan.graph, task)?;
    let dependencies = node["dependencies"]
        .as_array()
        .cloned()
        .unwrap_or_default()
        .into_iter()
        .map(|id| {
            let id = id.as_str().unwrap_or_default();
            super::graph::task(&plan.graph, id)
                .map(super::graph::contract)
                .map(|v| (id.to_owned(), v))
        })
        .collect::<crate::Result<BTreeMap<_, _>>>()?;
    Ok(
        json!({"canonicalizer":"exact-git-blob-bytes-v1","entries":entries,"contract":super::graph::contract(node),"dependencies":dependencies,"policy":digest(&policy)}),
    )
}
impl Store {
    pub fn record_approval(
        &mut self,
        root: &Path,
        plan: &Plan,
        task: &str,
        record: &ApprovalRecord,
    ) -> crate::Result<()> {
        let _lock = self.mutation_lock()?;
        if chrono::DateTime::parse_from_rfc3339(&record.recorded_at).is_err() {
            return Err(error("Invalid approval timestamp"));
        }
        let p = policy(root)?;
        let revision = full_commit(
            root,
            record
                .revision
                .as_deref()
                .ok_or_else(|| error("Approval requires a verified current subject"))?,
        )?;
        let payload = json!({"task_id":task,"decision":record.decision,"role":record.role,"actor":record.actor,"revision":revision,"evidence":record.evidence,"reason":record.reason,"recorded_at":record.recorded_at,"trust":"local_trust","binding_verified":true,"gate_digest":digest(&super::graph::contract(super::graph::task(&plan.graph,task)?)),"policy_digest":digest(&p),"manifest":approval_manifest(root,plan,task,Some(&revision)).ok(),"execution_id":registry(root)?.extra.get("decision_execution_id")});
        let tx = sql(self
            .db
            .transaction_with_behavior(rusqlite::TransactionBehavior::Immediate))?;
        let key = format!("native:{}", digest(&payload));
        if sql(tx.execute(
            "INSERT OR IGNORE INTO legacy_approvals VALUES(?1,?2,?3)",
            rusqlite::params![plan.id, key, encode(&payload)?],
        ))? > 0
        {
            event(&tx, &plan.id, "approval.recorded", &payload)?;
        }
        sql(tx.commit())?;
        Ok(())
    }
    pub fn verified_approvals(
        &self,
        root: &Path,
        plan: &Plan,
    ) -> crate::Result<BTreeMap<String, Vec<ApprovalRecord>>> {
        let p = policy(root)?;
        let mut stmt = sql(self
            .db
            .prepare("SELECT payload FROM legacy_approvals WHERE plan_id=?1 ORDER BY rowid"))?;
        let rows = sql(stmt.query_map([&plan.id], |r| r.get::<_, String>(0)))?;
        let mut out = BTreeMap::<String, Vec<ApprovalRecord>>::new();
        for row in rows {
            let value: Value = decode(&sql(row)?)?;
            if value["binding_verified"] != true {
                continue;
            }
            let Some(task) = value["task_id"].as_str() else {
                continue;
            };
            let node = super::graph::task(&plan.graph, task)?;
            if value["gate_digest"] != digest(&super::graph::contract(node))
                || value["policy_digest"] != digest(&p)
            {
                continue;
            }
            let subject = node
                .pointer("/gate/subject_task")
                .and_then(Value::as_str)
                .unwrap_or_default();
            let Some(current) = crate::coordinator::approval::subject_revision(
                root,
                &plan.reference_branch,
                subject,
            ) else {
                continue;
            };
            let Ok(current) = full_commit(root, &current) else {
                continue;
            };
            let Some(old) = value["revision"].as_str() else {
                continue;
            };
            let Ok(old) = full_commit(root, old) else {
                continue;
            };
            let valid = match p.approval_reuse {
                ApprovalReuse::ExactRevision => current == old,
                ApprovalReuse::ContentEquivalent => {
                    current == old
                        || (!value["manifest"].is_null()
                            && approval_manifest(root, plan, task, Some(&current))
                                .is_ok_and(|manifest| manifest == value["manifest"]))
                }
                // A one-shot approval cannot silently authorize another run.
                ApprovalReuse::Never => {
                    current == old
                        && value["execution_id"].as_str().is_some_and(|id| {
                            registry(root)
                                .ok()
                                .and_then(|r| r.extra.get("decision_execution_id").cloned())
                                .as_ref()
                                .and_then(Value::as_str)
                                == Some(id)
                        })
                }
            };
            if !valid {
                continue;
            }
            let mut record: ApprovalRecord =
                serde_json::from_value(value.clone()).map_err(|e| error(e.to_string()))?;
            if chrono::DateTime::parse_from_rfc3339(&record.recorded_at).is_err() {
                continue;
            }
            record.revision = Some(current);
            out.entry(task.into()).or_default().push(record);
        }
        Ok(out)
    }
    /// Reserve one operation exactly once. Replaying the same operation is
    /// idempotent; the authorization cannot authorize a different operation.
    pub fn consume_authorization(&mut self, id: &str, operation: &str) -> crate::Result<()> {
        use rusqlite::OptionalExtension;
        let _lock = self.mutation_lock()?;
        if operation.trim().is_empty() {
            return Err(error("Operation identity is required"));
        }
        let d = self.decision(id)?;
        let plan = self.plan(&d.plan_id)?;
        if digest(&super::graph::contract(super::graph::task(
            &plan.graph,
            &d.request.task_id,
        )?)) != d.subject_digest
        {
            return Err(error("Authorization subject changed"));
        }
        let current_policy = digest(&policy(&self.root)?);
        if !d
            .resolution
            .as_ref()
            .is_some_and(|r| r.policy_digest == current_policy)
        {
            return Err(error("Authorization policy changed"));
        }
        if d.request.operation_id.as_deref() != Some(operation) {
            return Err(error("Authorization is bound to a different operation"));
        }
        if d.status != "resolved" || d.request.kind != "authorization" {
            return Err(error("No resolved operation authorization"));
        }
        let tx = sql(self
            .db
            .transaction_with_behavior(rusqlite::TransactionBehavior::Immediate))?;
        let existing:Option<(String,String)>=sql(tx.query_row("SELECT operation_id,decision_id FROM authorizations WHERE plan_id=?1 AND (decision_id=?2 OR operation_id=?3)",rusqlite::params![d.plan_id,id,operation],|r|Ok((r.get(0)?,r.get(1)?))).optional())?;
        if let Some((op, decision)) = existing {
            if op == operation && decision == id {
                return Ok(());
            }
            return Err(error(
                "Authorization already consumed or operation has a different authorization",
            ));
        }
        sql(tx.execute(
            "INSERT INTO authorizations VALUES(?1,?2,?3,?4)",
            rusqlite::params![d.plan_id, operation, id, now()],
        ))?;
        event(
            &tx,
            &d.plan_id,
            "authorization.consumed",
            &json!({"operation":operation,"decision":id}),
        )?;
        sql(tx.commit())?;
        Ok(())
    }
}
pub fn approval_ledger_for_project(
    root: &Path,
    _registry: &TaskRegistry,
) -> crate::Result<BTreeMap<String, Vec<ApprovalRecord>>> {
    let (store, plan) = active_plan(root)?;
    store.verified_approvals(root, &plan)
}
