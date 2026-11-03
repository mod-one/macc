//! Apply the recorded answer's concrete graph effects before implementation.
use super::{store::*, *};
use serde_json::{json, Value};
pub fn effects_challenge(d: &Decision, patch: &Patch, p: &Policy) -> Value {
    super::secure::mutation_challenge(
        "decision.effects.apply",
        json!({"id":d.id,"version":d.version,"resolution":d.resolution,"patch":patch}),
        p,
    )
}
impl Store {
    pub fn apply_effects_attested(
        &mut self,
        id: &str,
        version: u64,
        patch: &Patch,
        actor: &str,
        proof: Option<&Attestation>,
    ) -> crate::Result<Decision> {
        let _lock = self.mutation_lock()?;
        let d = self.decision(id)?;
        let p = policy(&self.root)?;
        let actor = super::secure::mutation_authority(
            &self.root,
            "decision.effects.apply",
            &effects_challenge(&d, patch, &p),
            actor,
            proof,
            &p,
        )?;
        self.commit_effects(d, version, patch, &actor, proof, None)
    }
    /// Only a native issued planning attempt may complete mechanical graph effects.
    pub fn propose_effects(
        &mut self,
        id: &str,
        version: u64,
        patch: &Patch,
        attempt: &str,
        phase: &str,
    ) -> crate::Result<Decision> {
        let _lock = self.mutation_lock()?;
        let d = self.decision(id)?;
        let p = policy(&self.root)?;
        self.verify_human_binding(&d, &p)?;
        let c = self.read_context(&d.plan_id, attempt, phase)?;
        let planning = c
            .planning
            .as_ref()
            .ok_or_else(|| error("This invocation is not authorized for decision planning"))?;
        if planning.purpose == "investigation" {
            return Err(error("Investigation does not authorize graph effects"));
        }
        if c.task_id != d.request.task_id
            || !planning
                .requests
                .iter()
                .any(|r| r.id == id && r.version == version)
        {
            return Err(error(
                "Effects proposal is outside the pinned planning scope",
            ));
        }
        let receipt: Option<String> = sql(self.db.query_row(
            "SELECT receipt FROM contexts WHERE plan_id=?1 AND attempt_id=?2 AND phase=?3",
            rusqlite::params![d.plan_id, attempt, phase],
            |r| r.get(0),
        ))?;
        if receipt.is_none() {
            return Err(error(
                "Planning context was not delivered through a native runner",
            ));
        }
        if d.status == "resolved"
            && d.version == version + 1
            && d.effects_commit
                .as_ref()
                .is_some_and(|e| e.patch_digest == digest(patch))
        {
            return Ok(d);
        }
        if planning.artifact_digest.as_deref()
            != Some(
                crate::session_store::checkpoints::decision_planning_evidence(
                    std::path::Path::new(&planning.worktree),
                )?
                .as_str(),
            )
        {
            return Err(error("Planning modified retained source or Git state; preserve work and revalidate before applying effects"));
        }
        let plan = self.plan(&d.plan_id)?;
        self.validate_mechanical_effects(&plan, &d, patch)?;
        self.commit_effects(
            d,
            version,
            patch,
            &format!("agent:{}:decision-planning", c.task_id),
            None,
            Some(c.task_id.as_str()),
        )
    }
    fn validate_mechanical_effects(
        &self,
        plan: &Plan,
        d: &Decision,
        patch: &Patch,
    ) -> crate::Result<()> {
        if !patch.metadata_changes.is_empty() {
            return Err(error("Planning cannot rewrite project governance"));
        }
        for op in &patch.operations {
            if let Operation::AddTask { task } = op {
                if task.get("gate").is_some() {
                    return Err(error(
                        "A new approval policy needs an explicit governed proposal",
                    ));
                }
                // New work inherits the parent boundary; it cannot request unrestricted paths.
                let parent = super::graph::task(&plan.graph, &d.request.task_id)?;
                let allowed = parent
                    .pointer("/change_scope/allowed_paths")
                    .and_then(Value::as_array);
                let proposed = task
                    .pointer("/change_scope/allowed_paths")
                    .and_then(Value::as_array);
                if allowed.is_some() || !d.request.affected_paths.is_empty() {
                    let proposed = proposed.ok_or_else(|| {
                        error("New planning tasks must retain the authorized path boundary")
                    })?;
                    for path in proposed {
                        let path = path
                            .as_str()
                            .ok_or_else(|| error("Invalid planning path"))?;
                        if allowed.is_some_and(|set| {
                            !set.iter()
                                .filter_map(Value::as_str)
                                .any(|boundary| within(path, boundary))
                        }) || !d.request.affected_paths.is_empty()
                            && !d
                                .request
                                .affected_paths
                                .iter()
                                .any(|boundary| within(path, boundary))
                        {
                            return Err(error(
                                "Planning cannot widen its authorized source boundary",
                            ));
                        }
                    }
                }
                if proposed.is_some_and(|paths| {
                    paths
                        .iter()
                        .any(|p| p.as_str().is_none_or(|p| !relative(p)))
                }) {
                    return Err(error("Planning paths must remain inside the project"));
                }
                continue;
            }
            let id = match op {
                Operation::UpdateTask { task_id, .. }
                | Operation::AddDependency { task_id, .. }
                | Operation::RemoveDependency { task_id, .. }
                | Operation::DisableTask { task_id }
                | Operation::SupersedeTask { task_id, .. }
                | Operation::BindDecision { task_id, .. } => task_id,
                _ => unreachable!(),
            };
            let new_task = patch
                .operations
                .iter()
                .any(|op| matches!(op,Operation::AddTask{task} if task["id"]==*id));
            if new_task {
                if let Operation::BindDecision { decision_id, .. } = op {
                    if decision_id == &d.id {
                        continue;
                    }
                }
                return Err(error(
                    "Create a complete new task contract rather than mutating an unissued task",
                ));
            }
            if let Operation::BindDecision { decision_id, .. } = op {
                if decision_id != &d.id {
                    return Err(error(
                        "Planning cannot introduce another decision authority",
                    ));
                }
            }
            if !self.decision_applies(plan, d, id)? {
                return Err(error("Planning cannot change an unrelated task"));
            }
            if super::graph::task(&plan.graph, id)?.get("gate").is_some() {
                return Err(error("Planning cannot alter an approval gate"));
            }
            if let Operation::UpdateTask { changes, .. } = op {
                if changes.keys().any(|k| {
                    matches!(
                        k.as_str(),
                        "gate" | "governance_ref" | "dependencies" | "change_scope"
                    )
                }) {
                    return Err(error(
                        "Planning cannot weaken governance or widen writable scope",
                    ));
                }
            }
            if let Operation::RemoveDependency { depends_on, .. } = op {
                if super::graph::task(&plan.graph, depends_on)?
                    .get("gate")
                    .is_some()
                {
                    return Err(error("Planning cannot remove an approval dependency"));
                }
            }
        }
        Ok(())
    }
    fn commit_effects(
        &mut self,
        mut d: Decision,
        version: u64,
        patch: &Patch,
        actor: &str,
        proof: Option<&Attestation>,
        planning_task: Option<&str>,
    ) -> crate::Result<Decision> {
        if d.status == "resolved"
            && d.effects_commit
                .as_ref()
                .is_some_and(|e| e.patch_digest == digest(patch))
            && d.version == version + 1
        {
            return Ok(d);
        }
        if d.version != version
            || d.status != "awaiting_effects"
            || !d.resolution.as_ref().is_some_and(|r| !r.advisory)
        {
            return Err(error("Answer changed or no longer awaits graph effects"));
        }
        let mut plan = self.plan(&d.plan_id)?;
        if patch.plan_id != plan.id {
            return Err(error("Effects belong to a different plan"));
        }
        let mut registry = registry(&self.root)?;
        // The sole exception to active-contract editing is the verified source-unchanged
        // planning attempt itself; it is fenced from completing implementation below.
        if let Some(task) = planning_task {
            if let Some(t) = registry.find_task_mut(task) {
                t.task_runtime.pid = None;
            }
        }
        let mut graph = self.preview_patch(&plan, patch, &registry)?;
        let previous_ids = super::graph::tasks(&plan.graph)?
            .iter()
            .filter_map(|t| t["id"].as_str())
            .collect::<std::collections::BTreeSet<_>>();
        if let Some(tasks) = graph["tasks"].as_array_mut() {
            for task in tasks {
                if task["id"]
                    .as_str()
                    .is_some_and(|id| !previous_ids.contains(id))
                {
                    let refs = task
                        .as_object_mut()
                        .unwrap()
                        .entry("decision_refs")
                        .or_insert(json!([]))
                        .as_array_mut()
                        .ok_or_else(|| error("New task decision references must be an array"))?;
                    if !refs.contains(&json!(d.id)) {
                        refs.push(json!(d.id));
                    }
                }
            }
        }

        let tx = sql(self
            .db
            .transaction_with_behavior(rusqlite::TransactionBehavior::Immediate))?;
        let current: Decision = decode(&sql(tx.query_row(
            "SELECT payload FROM requests WHERE id=?1",
            [&d.id],
            |r| r.get::<_, String>(0),
        ))?)?;
        if current.version != version || current.status != "awaiting_effects" {
            return Err(error("Decision changed while its effects were prepared"));
        }
        super::graph::commit_graph(&tx, &mut plan, patch, graph, actor)?;
        d.version += 1;
        d.status = "resolved".into();
        d.effects_commit = Some(EffectCommit {
            patch_digest: digest(patch),
            graph_version: plan.graph_version,
            actor: actor.into(),
            recorded_at: now(),
            attestation: proof.cloned(),
        });
        sql(tx.execute(
            "UPDATE requests SET payload=?2 WHERE id=?1",
            rusqlite::params![d.id, encode(&d)?],
        ))?;
        plan.decision_epoch += 1;
        Self::supersede_in_transaction(&tx, &d, &mut plan)?;
        save_plan(&tx, &plan)?;
        event(&tx, &plan.id, "decision.resolved", &d)?;
        sql(tx.commit())?;
        Ok(d)
    }
    pub fn planning_attempts(&self, id: &str) -> crate::Result<usize> {
        use rusqlite::OptionalExtension;
        Ok(sql(self
            .db
            .query_row(
                "SELECT value FROM metadata WHERE key=?1",
                [format!("planning:{id}")],
                |r| r.get::<_, String>(0),
            )
            .optional())?
        .and_then(|s| s.parse().ok())
        .unwrap_or(0))
    }
}

fn relative(path: &str) -> bool {
    !path.trim().is_empty()
        && !std::path::Path::new(path).is_absolute()
        && !std::path::Path::new(path)
            .components()
            .any(|c| matches!(c, std::path::Component::ParentDir))
}
fn within(path: &str, boundary: &str) -> bool {
    relative(path)
        && (path == boundary
            || boundary == "**"
            || boundary == "*"
            || boundary
                .strip_suffix("/**")
                .is_some_and(|base| path.starts_with(&format!("{base}/"))))
}
