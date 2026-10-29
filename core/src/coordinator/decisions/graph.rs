use super::{store::*, *};
use crate::coordinator::model::TaskRegistry;
use rusqlite::{OptionalExtension, TransactionBehavior};
use serde_json::{json, Value};
use std::{
    collections::{BTreeMap, BTreeSet},
    path::Path,
};

pub(super) fn projection_path(root: &Path, source: &str) -> crate::Result<std::path::PathBuf> {
    let relative = Path::new(source);
    if relative.is_absolute()
        || relative.components().any(|c| {
            !matches!(
                c,
                std::path::Component::Normal(_) | std::path::Component::CurDir
            )
        })
    {
        return Err(error("Plan projection must be inside the project"));
    }
    let mut path = root.to_path_buf();
    for part in relative.components() {
        path.push(part);
        if std::fs::symlink_metadata(&path).is_ok_and(|m| m.file_type().is_symlink()) {
            return Err(error("PRD projection cannot traverse a symlink"));
        }
    }
    Ok(path)
}
pub(super) fn tasks(graph: &Value) -> crate::Result<&Vec<Value>> {
    graph
        .get("tasks")
        .and_then(Value::as_array)
        .ok_or_else(|| error("Graph requires tasks"))
}
pub(super) fn task<'a>(graph: &'a Value, id: &str) -> crate::Result<&'a Value> {
    tasks(graph)?
        .iter()
        .find(|t| t["id"].as_str() == Some(id))
        .ok_or_else(|| error(format!("Unknown task '{id}'")))
}
pub(super) fn contract(task: &Value) -> Value {
    let mut v = task.clone();
    if let Some(o) = v.as_object_mut() {
        for key in [
            "state",
            "passes",
            "updated_at",
            "state_changed_at",
            "task_runtime",
            "worktree",
            "tool",
        ] {
            o.remove(key);
        }
    }
    v
}
pub(super) fn validate(graph: &Value, external: &BTreeSet<String>) -> crate::Result<()> {
    let nodes = tasks(graph)?;
    let mut edges = BTreeMap::<String, Vec<String>>::new();
    for node in nodes {
        let id = node["id"]
            .as_str()
            .filter(|s| !s.trim().is_empty())
            .ok_or_else(|| error("Task ID required"))?;
        if id.len() > 128
            || !id
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.'))
            || matches!(id, "." | "..")
            || edges.contains_key(id)
        {
            return Err(error(format!("Duplicate/invalid task ID '{id}'")));
        }
        let parsed: crate::coordinator::model::PrdTaskInput =
            serde_json::from_value(node.clone()).map_err(|e| error(e.to_string()))?;
        if let Some(disposition) = node.get("decision_disposition") {
            if !matches!(disposition.as_str(), Some("disabled" | "superseded")) {
                return Err(error("Unknown task disposition"));
            }
        }
        if let Some(gate) = &parsed.gate {
            let problems = gate.validate_human_approval(id, &parsed.dependencies);
            if !problems.is_empty() {
                return Err(error(problems.join("; ")));
            }
        }
        if node.get("decision_disposition").is_none() {
            for dep in &parsed.dependencies {
                if nodes.iter().any(|n| {
                    n["id"].as_str() == Some(dep) && n.get("decision_disposition").is_some()
                }) {
                    return Err(error(format!("Active task {id} depends on retired task {dep}; rewire or retire its consumers in the same patch")));
                }
            }
        }
        edges.insert(id.into(), parsed.dependencies);
    }
    fn visit(
        id: &str,
        edges: &BTreeMap<String, Vec<String>>,
        external: &BTreeSet<String>,
        visiting: &mut BTreeSet<String>,
        done: &mut BTreeSet<String>,
    ) -> crate::Result<()> {
        if done.contains(id) {
            return Ok(());
        }
        if !visiting.insert(id.into()) {
            return Err(error(format!("Dependency cycle at '{id}'")));
        }
        for dep in &edges[id] {
            if edges.contains_key(dep) {
                visit(dep, edges, external, visiting, done)?;
            } else if !external.contains(dep) {
                return Err(error(format!("Missing dependency '{dep}'")));
            }
        }
        visiting.remove(id);
        done.insert(id.into());
        Ok(())
    }
    let mut done = BTreeSet::new();
    for id in edges.keys() {
        visit(id, &edges, external, &mut BTreeSet::new(), &mut done)?;
    }
    Ok(())
}

impl Store {
    pub fn ensure_plan(
        &mut self,
        root: &Path,
        source: &Path,
        reference: &str,
        registry: &TaskRegistry,
    ) -> crate::Result<Plan> {
        let source = source.strip_prefix(root).unwrap_or(source);
        if source.is_absolute()
            || source.components().any(|c| {
                !matches!(
                    c,
                    std::path::Component::Normal(_) | std::path::Component::CurDir
                )
            })
        {
            return Err(error("Plan source must remain inside the project"));
        }
        let source = source.to_string_lossy().into_owned();
        let key = digest(&json!([self.project_id, source, reference]));
        let raw =
            std::fs::read(projection_path(root, &source)?).map_err(|e| error(e.to_string()))?;
        let graph: Value = serde_json::from_slice(&raw).map_err(|e| error(e.to_string()))?;
        let tx = sql(self
            .db
            .transaction_with_behavior(TransactionBehavior::Immediate))?;
        let old: Option<String> = sql(tx
            .query_row(
                "SELECT payload FROM plans WHERE scope_key=?1",
                [&key],
                |r| r.get(0),
            )
            .optional())?;
        if let Some(old) = old {
            let plan: Plan = decode(&old)?;
            if digest(&graph) != plan.source_digest {
                return Err(error("PRD differs from the durable plan. Preview/reconcile an explicit graph patch; no dispatch allowed"));
            }
            sql(tx.commit())?;
            return Ok(plan);
        }
        validate(&graph, &registry.external_merged_task_ids)?;
        let plan = Plan {
            id: uuid::Uuid::new_v4().to_string(),
            project_id: self.project_id.clone(),
            source,
            reference_branch: reference.into(),
            graph_version: 1,
            decision_epoch: 0,
            graph: graph.clone(),
            source_digest: digest(&graph),
            projected_version: 1,
        };
        sql(tx.execute(
            "INSERT INTO plans VALUES(?1,?2,?3)",
            rusqlite::params![plan.id, key, encode(&plan)?],
        ))?;
        event(&tx, &plan.id, "plan.imported", &plan)?;
        sql(tx.commit())?;
        Ok(plan)
    }
    pub fn preview_patch(
        &self,
        plan: &Plan,
        patch: &Patch,
        registry: &TaskRegistry,
    ) -> crate::Result<Value> {
        if patch.plan_id != plan.id || patch.base_graph_version != plan.graph_version {
            return Err(error(
                "Stale graph version; rebase and preview the patch again",
            ));
        }
        if patch.idempotency_key.trim().is_empty()
            || patch.reason.trim().is_empty()
            || patch.operations.is_empty()
            || patch.operations.len() > 256
        {
            return Err(error(
                "Patch requires an idempotency key, reason and 1..256 operations",
            ));
        }
        for id in &patch.decision_refs {
            let decision = self.decision(id)?;
            if decision.plan_id != plan.id || decision.status != "resolved" {
                return Err(error("Patch cites an unresolved or foreign decision"));
            }
        }
        let mut graph = plan.graph.clone();
        let nodes = graph["tasks"]
            .as_array_mut()
            .ok_or_else(|| error("Invalid plan graph"))?;
        for op in &patch.operations {
            let id = match op {
                Operation::AddTask { task } => task["id"].as_str().unwrap_or_default(),
                Operation::UpdateTask { task_id, .. }
                | Operation::AddDependency { task_id, .. }
                | Operation::RemoveDependency { task_id, .. }
                | Operation::DisableTask { task_id }
                | Operation::SupersedeTask { task_id, .. }
                | Operation::BindDecision { task_id, .. } => task_id,
            };
            if let Operation::AddTask { task } = op {
                if nodes.iter().any(|t| t["id"].as_str() == Some(id)) {
                    return Err(error("Task IDs cannot be reused"));
                }
                if [
                    "state",
                    "passes",
                    "task_runtime",
                    "worktree",
                    "task_worktree",
                    "coordinator",
                ]
                .iter()
                .any(|key| task.get(*key).is_some())
                {
                    return Err(error(
                        "New graph tasks cannot manufacture execution history",
                    ));
                }
                nodes.push(task.clone());
                continue;
            }
            let index = nodes
                .iter()
                .position(|t| t["id"].as_str() == Some(id))
                .ok_or_else(|| error(format!("Unknown patch task '{id}'")))?;
            if registry.find_task(id).is_some_and(|t| t.is_merged())
                || crate::coordinator::approval::subject_revision(
                    &self.root,
                    &plan.reference_branch,
                    id,
                )
                .is_some()
            {
                return Err(error(
                    "Delivered history is immutable; add a corrective task instead",
                ));
            }
            if registry.find_task(id).is_some_and(|t| {
                t.task_runtime
                    .pid
                    .is_some_and(crate::coordinator::helpers::is_pid_running)
            }) {
                // Explicit quarantine invalidates the task contract immediately;
                // incompatible completions are fenced rather than silently merged.
                if !matches!(
                    op,
                    Operation::DisableTask { .. } | Operation::SupersedeTask { .. }
                ) {
                    return Err(error(
                        "Active task must be stopped/quarantined before changing its contract",
                    ));
                }
            }
            let node = &mut nodes[index];
            match op {
                Operation::UpdateTask { changes, .. } => {
                    for (key, value) in changes {
                        if matches!(
                            key.as_str(),
                            "id" | "state"
                                | "passes"
                                | "task_runtime"
                                | "worktree"
                                | "decision_disposition"
                                | "replacement_task_id"
                                | "decision_refs"
                        ) {
                            return Err(error("Patch cannot write execution history/authority"));
                        }
                        node.as_object_mut()
                            .unwrap()
                            .insert(key.clone(), value.clone());
                    }
                }
                Operation::AddDependency { depends_on, .. }
                | Operation::RemoveDependency { depends_on, .. } => {
                    let deps = node
                        .as_object_mut()
                        .unwrap()
                        .entry("dependencies")
                        .or_insert(json!([]))
                        .as_array_mut()
                        .ok_or_else(|| error("Invalid dependencies"))?;
                    if matches!(op, Operation::AddDependency { .. }) {
                        if !deps.iter().any(|d| d.as_str() == Some(depends_on)) {
                            deps.push(json!(depends_on));
                        }
                    } else {
                        deps.retain(|d| d.as_str() != Some(depends_on));
                    }
                }
                Operation::DisableTask { .. } => node["decision_disposition"] = json!("disabled"),
                Operation::SupersedeTask {
                    replacement_task_id,
                    ..
                } => {
                    if replacement_task_id == id {
                        return Err(error("Task cannot supersede itself"));
                    }
                    node["decision_disposition"] = json!("superseded");
                    node["replacement_task_id"] = json!(replacement_task_id);
                }
                Operation::BindDecision {
                    decision_id,
                    version,
                    ..
                } => {
                    let decision = self.decision(decision_id)?;
                    if decision.plan_id != plan.id
                        || decision.version != *version
                        || decision.status != "resolved"
                    {
                        return Err(error(
                            "Decision binding is unresolved, stale or from another plan",
                        ));
                    }
                    let bindings = node
                        .as_object_mut()
                        .unwrap()
                        .entry("decision_refs")
                        .or_insert(json!([]))
                        .as_array_mut()
                        .ok_or_else(|| error("Invalid decision refs"))?;
                    if !bindings.iter().any(|d| d == decision_id) {
                        bindings.push(json!(decision_id));
                    }
                }
                Operation::AddTask { .. } => unreachable!(),
            }
        }
        for node in nodes.iter() {
            if let Some(replacement) = node["replacement_task_id"].as_str() {
                if !nodes.iter().any(|n| n["id"].as_str() == Some(replacement)) {
                    return Err(error("Superseding task is missing"));
                }
            }
        }
        validate(&graph, &registry.external_merged_task_ids)?;
        Ok(graph)
    }
    pub fn projection_diff(&self, root: &Path, plan: &Plan) -> crate::Result<Value> {
        let bytes = std::fs::read(projection_path(root, &plan.source)?)
            .map_err(|e| error(e.to_string()))?;
        let actual: Value = serde_json::from_slice(&bytes).map_err(|e| error(e.to_string()))?;
        Ok(
            json!({"expected":plan.graph,"actual":actual,"source_digest":digest(&actual),"changed":actual!=plan.graph}),
        )
    }
    /// Explicit operator reconciliation, bound to the exact inspected disk
    /// projection. Preserve the original content in the same graph transaction.
    pub fn reconcile_projection(
        &mut self,
        root: &Path,
        patch: &Patch,
        source_digest: &str,
        actor: &str,
    ) -> crate::Result<Plan> {
        let _fence = self.mutation_lock()?;
        let mut plan = self.plan(&patch.plan_id)?;
        let diff = self.projection_diff(root, &plan)?;
        if diff["source_digest"] != source_digest {
            return Err(error(
                "PRD changed since preview; inspect the current disk projection before reconciling",
            ));
        }
        let graph = self.preview_patch(&plan, patch, &super::registry(root)?)?;
        let tx = sql(self
            .db
            .transaction_with_behavior(TransactionBehavior::Immediate))?;
        event(
            &tx,
            &plan.id,
            "graph.source.reconciled",
            &json!({"actor":actor,"source_digest":source_digest,"graph":diff["actual"]}),
        )?;
        commit_graph(&tx, &mut plan, patch, graph, actor)?;
        sql(tx.commit())?;
        Ok(plan)
    }
    pub fn apply_patch(
        &mut self,
        plan_id: &str,
        patch: &Patch,
        registry: &TaskRegistry,
        actor: &str,
    ) -> crate::Result<Plan> {
        let _fence = self.mutation_lock()?;
        let plan = self.plan(plan_id)?;
        let mut registry = registry.clone();
        let latest = super::registry(&self.root)?;
        for task in latest.tasks {
            if task.is_merged()
                || task
                    .task_runtime
                    .pid
                    .is_some_and(crate::coordinator::helpers::is_pid_running)
            {
                if let Some(existing) = registry.find_task_mut(&task.id) {
                    *existing = task;
                } else {
                    registry.tasks.push(task);
                }
            }
        }
        let fingerprint = digest(patch);
        let previous: Option<String> = sql(self
            .db
            .query_row(
                "SELECT payload FROM operations WHERE plan_id=?1 AND key=?2",
                rusqlite::params![plan_id, patch.idempotency_key],
                |r| r.get(0),
            )
            .optional())?;
        if let Some(old) = previous {
            let value: Value = decode(&old)?;
            if value["fingerprint"] != fingerprint {
                return Err(error("Idempotency key reused for different patch"));
            }
            return self.plan(plan_id);
        }
        let graph = self.preview_patch(&plan, patch, &registry)?;
        let tx = sql(self
            .db
            .transaction_with_behavior(TransactionBehavior::Immediate))?;
        let mut current: Plan = decode(&sql(tx.query_row(
            "SELECT payload FROM plans WHERE id=?1",
            [plan_id],
            |r| r.get::<_, String>(0),
        ))?)?;
        if current.graph_version != patch.base_graph_version {
            return Err(error("Concurrent graph patch committed; rebase required"));
        }
        commit_graph(&tx, &mut current, patch, graph, actor)?;
        sql(tx.commit())?;
        Ok(current)
    }
}

pub(super) fn commit_graph(
    tx: &rusqlite::Transaction<'_>,
    current: &mut Plan,
    patch: &Patch,
    graph: Value,
    actor: &str,
) -> crate::Result<()> {
    if current.graph_version != patch.base_graph_version {
        return Err(error("Graph proposal is stale; rebase before applying"));
    }
    current.graph_version += 1;
    current.graph = graph;
    current.source_digest = digest(&current.graph);
    save_plan(tx, current)?;
    sql(tx.execute(
        "INSERT INTO operations VALUES(?1,?2,?3)",
        rusqlite::params![
            current.id,
            patch.idempotency_key,
            encode(&json!({"fingerprint":digest(patch),"version":current.graph_version}))?
        ],
    ))?;
    sql(tx.execute(
        "INSERT INTO outbox(plan_id,kind,payload) VALUES(?1,'graph',?2)",
        rusqlite::params![current.id, encode(current)?],
    ))?;
    event(
        tx,
        &current.id,
        "graph.patch.committed",
        &json!({"actor":actor,"patch":patch,"graph":current.graph,"version":current.graph_version}),
    )?;
    let candidates: Vec<Decision> = {
        let mut stmt = sql(tx.prepare("SELECT payload FROM requests WHERE plan_id=?1"))?;
        let rows = sql(stmt.query_map([&current.id], |r| r.get::<_, String>(0)))?;
        rows.map(|r| decode(&sql(r)?))
            .collect::<crate::Result<_>>()?
    };
    for mut decision in candidates {
        if decision.status != "pending_human" {
            continue;
        }
        let changed = decision.request.task_id != "*"
            && task(&current.graph, &decision.request.task_id).is_ok_and(|node| {
                node.get("decision_disposition").is_some()
                    || digest(&contract(node)) != decision.subject_digest
            });
        let stale_patch = decision
            .request
            .proposed_patch
            .as_ref()
            .is_some_and(|p| p.base_graph_version != current.graph_version);
        if changed || stale_patch {
            decision.version += 1;
            decision.status = "superseded".into();
            current.decision_epoch += 1;
            sql(tx.execute(
                "UPDATE requests SET payload=?2 WHERE id=?1",
                rusqlite::params![decision.id, encode(&decision)?],
            ))?;
            event(
                tx,
                &current.id,
                "decision.superseded",
                &json!({"actor":actor,"reason":"Graph changed the request subject or invalidated its patch; refresh the request instead of answering stale work","decision":decision}),
            )?;
        }
    }
    save_plan(tx, current)?;
    Ok(())
}
