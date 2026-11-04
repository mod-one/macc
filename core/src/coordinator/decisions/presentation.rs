//! Shared semantic presentation; clients need not infer authority from raw logs.
use super::{store::*, *};
use serde_json::{json, Value};
use std::collections::BTreeSet;
pub fn graph_diff(before: &Value, after: &Value) -> Value {
    let tasks = |g: &Value| {
        g["tasks"]
            .as_array()
            .cloned()
            .unwrap_or_default()
            .into_iter()
            .filter_map(|n| n["id"].as_str().map(str::to_owned).map(|id| (id, n)))
            .collect::<std::collections::BTreeMap<_, _>>()
    };
    let old = tasks(before);
    let new = tasks(after);
    let ids = old
        .keys()
        .chain(new.keys())
        .cloned()
        .collect::<BTreeSet<_>>();
    let changes=ids.into_iter().filter_map(|id|{let a=old.get(&id);let b=new.get(&id);if a==b{return None;}let fields=a.and_then(Value::as_object).into_iter().flat_map(|o|o.keys()).chain(b.and_then(Value::as_object).into_iter().flat_map(|o|o.keys())).filter(|k|a.and_then(|v|v.get(*k))!=b.and_then(|v|v.get(*k))).cloned().collect::<BTreeSet<_>>();Some(json!({"task":id,"title":b.or(a).and_then(|v|v["title"].as_str()),"change":if a.is_none(){"added"}else if b.is_none(){"removed"}else{"updated"},"fields":fields,"dependencies_before":a.map(|v|v["dependencies"].clone()),"dependencies_after":b.map(|v|v["dependencies"].clone()),"disposition":b.map(|v|v["decision_disposition"].clone())}))}).collect::<Vec<_>>();
    let metadata = before
        .as_object()
        .into_iter()
        .flat_map(|o| o.keys())
        .chain(after.as_object().into_iter().flat_map(|o| o.keys()))
        .filter(|k| k.as_str() != "tasks" && before.get(*k) != after.get(*k))
        .cloned()
        .collect::<BTreeSet<_>>();
    json!({"tasks":changes,"plan_fields":metadata})
}
impl Store {
    pub fn delivery_status(&self, plan: &Plan, p: &Policy) -> crate::Result<Vec<Value>> {
        let mut statement=sql(self.db.prepare("SELECT c.payload,c.receipt FROM contexts c WHERE c.plan_id=?1 AND NOT EXISTS(SELECT 1 FROM contexts n WHERE n.plan_id=c.plan_id AND json_extract(n.payload,'$.task_id')=json_extract(c.payload,'$.task_id') AND n.phase=c.phase AND n.rowid>c.rowid) ORDER BY c.rowid DESC LIMIT 200"))?;
        let rows = sql(statement.query_map([&plan.id], |r| {
            Ok((r.get::<_, String>(0)?, r.get::<_, Option<String>>(1)?))
        }))?;
        let mut result = Vec::new();
        for row in rows {
            let (payload, receipt) = sql(row)?;
            let c: ContextPackage = decode(&payload)?;
            let r = receipt.map(|s| decode::<ContextReceipt>(&s)).transpose()?;
            let fresh = self.context(plan, &c.task_id, &c.attempt_id, &c.phase, p);
            let compatible = fresh
                .as_ref()
                .is_ok_and(|n| n.context_digest == c.context_digest);
            result.push(json!({"task":c.task_id,"phase":c.phase,"attempt":c.attempt_id,"graph_version":c.graph_version,"decision_epoch":c.decision_epoch,"context_digest":c.context_digest,"budget":c.budget(),"current":compatible,"blocked_reason":fresh.err().map(|e|e.to_string()),"delivered":r.is_some(),"acknowledged":r.as_ref().is_some_and(|r|r.acknowledged),"completion":r.as_ref().map(|r|r.completion_status.clone()),"compliance":r.as_ref().map(|r|r.compliance.clone()),"reviewed_commit":r.as_ref().and_then(|r|r.reviewed_commit.clone()),"decisions":c.bindings.iter().map(|d|json!({"id":d.id,"version":d.version})).collect::<Vec<_>>() }));
        }
        Ok(result)
    }
}

impl Patch {
    pub fn describe(&self) -> Vec<String> {
        let mut rows = self
            .operations
            .iter()
            .map(|op| match op {
                Operation::AddTask { task } => format!(
                    "Add task {}: {}",
                    task["id"].as_str().unwrap_or("?"),
                    task["title"].as_str().unwrap_or("")
                ),
                Operation::UpdateTask { task_id, changes } => format!(
                    "Update {task_id}: {}",
                    changes.keys().cloned().collect::<Vec<_>>().join(", ")
                ),
                Operation::AddDependency {
                    task_id,
                    depends_on,
                } => format!("{task_id} must wait for {depends_on}"),
                Operation::RemoveDependency {
                    task_id,
                    depends_on,
                } => format!("Remove {task_id}'s dependency on {depends_on}"),
                Operation::DisableTask { task_id } => {
                    format!("Disable {task_id}; preserve its retained work")
                }
                Operation::SupersedeTask {
                    task_id,
                    replacement_task_id,
                } => {
                    format!("Replace {task_id} with {replacement_task_id}; retain original history")
                }
                Operation::BindDecision {
                    task_id,
                    decision_id,
                    version,
                } => format!("Bind decision {decision_id} v{version} to {task_id}"),
            })
            .collect::<Vec<_>>();
        for key in self.metadata_changes.keys() {
            rows.push(format!("Update plan field {key}"));
        }
        rows
    }
}

impl Store {
    pub fn graph_preview(&self, plan: &Plan, patch: &Patch) -> crate::Result<Value> {
        let p = policy(&self.root)?;
        let proposed = self.preview_patch(plan, patch, &registry(&self.root)?)?;
        let projection = self.projection_diff(&self.root, plan)?;
        Ok(
            json!({"base":plan.graph_version,"diff":graph_diff(&plan.graph,&proposed),"proposed":proposed,"challenge":super::secure::graph_challenge(plan,patch,None,&p),"reconcile_challenge":super::secure::graph_challenge(plan,patch,projection["source_digest"].as_str(),&p),"projection":projection}),
        )
    }
}
