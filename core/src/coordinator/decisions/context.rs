use super::{store::*, *};
use rusqlite::{OptionalExtension, TransactionBehavior};
use serde_json::{json, Value};
use std::{
    collections::BTreeSet,
    path::{Path, PathBuf},
};

pub(super) fn applies(plan: &Plan, decision: &Decision, task_id: &str) -> crate::Result<bool> {
    if decision.request.scope == "project"
        || decision.request.affected_tasks.iter().any(|id| id == "*")
    {
        return Ok(true);
    }
    // A replacement binds the union of prior and new affected scopes.
    // Superseded records remain available for this applicability calculation.
    let node = super::graph::task(&plan.graph, task_id)?;
    if decision.request.scope == "requirements" {
        for field in ["spec_refs", "requirement_refs", "requirements"] {
            if let Some(refs) = node[field].as_array() {
                if decision.request.requirement_refs.iter().any(|id| {
                    refs.iter().any(|r| {
                        r.as_str() == Some(id)
                            || ["id", "ref", "requirement_id", "path"]
                                .iter()
                                .any(|key| r[*key].as_str() == Some(id))
                    })
                }) {
                    return Ok(true);
                }
            }
        }
    }
    if decision.request.scope == "paths" {
        if let Some(paths) = node
            .pointer("/change_scope/allowed_paths")
            .and_then(Value::as_array)
        {
            if decision.request.affected_paths.iter().any(|scope| {
                paths.iter().filter_map(Value::as_str).any(|p| {
                    let scope_prefix = scope.split(['*', '?', '[']).next().unwrap_or_default();
                    let path_prefix = p.split(['*', '?', '[']).next().unwrap_or_default();
                    path_prefix.starts_with(scope_prefix) || scope_prefix.starts_with(path_prefix)
                })
            }) {
                return Ok(true);
            }
        }
    }
    if node["decision_refs"]
        .as_array()
        .is_some_and(|ids| ids.iter().any(|id| id.as_str() == Some(&decision.id)))
    {
        return Ok(true);
    }
    let mut seen: BTreeSet<String> = decision.request.affected_tasks.iter().cloned().collect();
    loop {
        let before = seen.len();
        for task in super::graph::tasks(&plan.graph)? {
            if task["id"].as_str().is_some_and(|id| seen.contains(id)) {
                if let Some(replacement) = task["replacement_task_id"].as_str() {
                    seen.insert(replacement.into());
                }
            }
            if task["dependencies"].as_array().is_some_and(|deps| {
                deps.iter()
                    .filter_map(Value::as_str)
                    .any(|d| seen.contains(d))
            }) {
                if let Some(id) = task["id"].as_str() {
                    seen.insert(id.into());
                }
            }
        }
        if before == seen.len() {
            break;
        }
    }
    Ok(seen.contains(task_id))
}

impl Store {
    pub(super) fn decision_applies(
        &self,
        plan: &Plan,
        decision: &Decision,
        task: &str,
    ) -> crate::Result<bool> {
        let mut pending = vec![decision.clone()];
        let mut seen = BTreeSet::new();
        while let Some(d) = pending.pop() {
            if !seen.insert(d.id.clone()) {
                continue;
            }
            if applies(plan, &d, task)? {
                return Ok(true);
            }
            for r in &d.request.supersedes {
                pending.push(self.decision(&r.id)?);
            }
            if let Some(r) = &d.reused_from {
                pending.push(self.decision(&r.id)?);
            }
        }
        Ok(false)
    }
    pub fn context_history(&self, plan: &str, task: &str) -> crate::Result<Vec<Value>> {
        let mut statement = sql(self
            .db
            .prepare("SELECT payload,receipt FROM contexts WHERE plan_id=?1 ORDER BY rowid DESC"))?;
        let rows = sql(statement.query_map([plan], |r| {
            Ok((r.get::<_, String>(0)?, r.get::<_, Option<String>>(1)?))
        }))?;
        let mut result = Vec::new();
        for row in rows {
            let (payload, receipt) = sql(row)?;
            let context: ContextPackage = decode(&payload)?;
            if context.task_id == task {
                result.push(json!({"context":context,"rendered":context.render(),"budget":context.budget(),"receipt":receipt.map(|r|decode::<Value>(&r)).transpose()?}));
                if result.len() >= 50 {
                    break;
                }
            }
        }
        Ok(result)
    }
    pub fn context(
        &self,
        plan: &Plan,
        task: &str,
        attempt: &str,
        phase: &str,
        policy: &Policy,
    ) -> crate::Result<ContextPackage> {
        let context = self.build_context(plan, task, attempt, phase, policy)?;
        context.budget().check()?;
        Ok(context)
    }
    pub(super) fn build_context(
        &self,
        plan: &Plan,
        task: &str,
        attempt: &str,
        phase: &str,
        policy: &Policy,
    ) -> crate::Result<ContextPackage> {
        let node = super::graph::task(&plan.graph, task)?;
        if node.get("decision_disposition").is_some() {
            return Err(error(
                "DM_STALE: task was retired/quarantined by a graph decision; preserve its work",
            ));
        }
        if let Some(refs) = node.get("decision_refs") {
            for id in refs
                .as_array()
                .ok_or_else(|| error("Invalid task decision references"))?
            {
                self.decision(
                    id.as_str()
                        .ok_or_else(|| error("Invalid task decision identity"))?,
                )?;
            }
        }
        let mut bindings = Vec::new();
        let mut planning_requests = Vec::new();
        let mut purpose = String::new();
        for decision in self.applicable_decisions(plan)? {
            if !self.decision_applies(plan, &decision, task)? {
                continue;
            }
            if matches!(
                decision.status.as_str(),
                "awaiting_effects" | "investigating_requested"
            ) && decision.request.task_id == task
                && matches!(phase, "dev" | "fix")
            {
                let next = if decision.status == "investigating_requested" {
                    "investigation"
                } else {
                    "effects"
                };
                if !purpose.is_empty() && purpose != next {
                    return Err(error(
                        "Different planning purposes require separate bounded invocations",
                    ));
                }
                purpose = next.into();
                self.verify_human_binding(&decision, policy)?;
                planning_requests.push(DecisionRef {
                    id: decision.id.clone(),
                    version: decision.version,
                });
                bindings.push(decision);
                continue;
            }
            if matches!(
                decision.status.as_str(),
                "pending_human"
                    | "rejected"
                    | "conflict"
                    | "investigating"
                    | "expired"
                    | "awaiting_effects"
                    | "conditional"
                    | "investigating_requested"
                    | "blocked_external"
                    | "waiting_dependency"
            ) {
                return Err(error(format!(
                    "DM_PENDING: task {task} waits for decision {}",
                    decision.id
                )));
            }
            if decision.status == "resolved" {
                self.verify_artifacts(&decision.plan_id, &decision.request.artifacts)?;
                if let Some(r) = &decision.resolution {
                    self.verify_artifacts(&decision.plan_id, &r.artifacts)?;
                }
                self.check_conditions(&decision, policy)?;
                self.verify_human_binding(&decision, policy)?;
                if decision.request.kind == "approval"
                    && decision.request.subject_revision.as_ref()
                        != Some(&self.approval_subject_revision(
                            &self.plan(&decision.plan_id)?,
                            &decision.request,
                        )?)
                {
                    return Err(error(
                        "DM_STALE: approval artifact changed; renew its subject-bound resolution",
                    ));
                }
                bindings.push(decision);
            }
        }
        let mut dependency_contracts = std::collections::BTreeMap::new();
        let mut pending = node["dependencies"].as_array().cloned().unwrap_or_default();
        while let Some(id) = pending.pop() {
            let Some(id) = id.as_str() else {
                continue;
            };
            if dependency_contracts.contains_key(id) {
                continue;
            }
            if let Ok(dependency) = super::graph::task(&plan.graph, id) {
                dependency_contracts.insert(id.to_owned(), super::graph::contract(dependency));
                pending.extend(
                    dependency["dependencies"]
                        .as_array()
                        .cloned()
                        .unwrap_or_default(),
                );
            }
        }
        let contract_digest = if dependency_contracts.is_empty() {
            digest(&super::graph::contract(node))
        } else {
            digest(&json!([super::graph::contract(node), dependency_contracts]))
        };
        let mut evidence = std::collections::BTreeMap::new();
        for d in &bindings {
            for reference in d
                .request
                .artifacts
                .iter()
                .chain(d.resolution.iter().flat_map(|r| r.artifacts.iter()))
            {
                if self.evidence_deliverable(&reference.id)? {
                    let artifact = self.artifact(&reference.id)?;
                    if artifact.size > policy.max_request_bytes {
                        return Err(error("Shared evidence exceeds the declared prompt evidence budget; use a smaller redacted result"));
                    }
                    evidence.insert(artifact.id.clone(), artifact);
                }
            }
        }
        let mut context = ContextPackage {
            artifacts: evidence.into_values().collect(),
            planning: if planning_requests.is_empty() {
                None
            } else {
                Some(PlanningContext {
                    purpose: if purpose == "effects" {
                        String::new()
                    } else {
                        purpose
                    },
                    requests: planning_requests,
                    worktree: String::new(),
                    artifact_digest: None,
                })
            },
            graph_version: plan.graph_version,
            decision_epoch: plan.decision_epoch,
            schema_version: 3,
            plan_id: plan.id.clone(),
            task_id: task.into(),
            attempt_id: attempt.into(),
            phase: phase.into(),
            contract_digest,
            dependency_contracts,
            policy: policy.clone(),
            bindings,
            context_digest: String::new(),
            plan_context: {
                let mut value = plan.graph.clone();
                value.as_object_mut().unwrap().remove("tasks");
                value.as_object_mut().unwrap().remove("tasks_done");
                if value.as_object().unwrap().is_empty() {
                    None
                } else {
                    Some(value)
                }
            },
        };
        context.context_digest = context_hash(&context);
        Ok(context)
    }
    pub fn issue(
        &mut self,
        plan: &Plan,
        task: &str,
        attempt: &str,
        phase: &str,
        worktree: &Path,
        policy: &Policy,
    ) -> crate::Result<(ContextPackage, PathBuf)> {
        super::validate_context_identity(task, attempt, phase)?;
        let mut context = self.context(plan, task, attempt, phase, policy)?;
        if let Some(planning) = &mut context.planning {
            planning.worktree = worktree.to_string_lossy().into();
            planning.artifact_digest =
                Some(crate::session_store::checkpoints::decision_planning_evidence(worktree)?);
        }
        let tx = sql(self
            .db
            .transaction_with_behavior(TransactionBehavior::Immediate))?;
        let old: Option<String> = sql(tx
            .query_row(
                "SELECT payload FROM contexts WHERE plan_id=?1 AND attempt_id=?2 AND phase=?3",
                rusqlite::params![plan.id, attempt, phase],
                |r| r.get(0),
            )
            .optional())?;
        if let Some(ref payload) = old {
            let prior: ContextPackage = decode(payload)?;
            if prior.context_digest == context.context_digest {
                context = prior;
            }
        }
        if old
            .as_deref()
            .is_some_and(|old| old != encode(&context).unwrap_or_default())
        {
            return Err(error(
                "An attempt's context is immutable; create a new attempt after decision changes",
            ));
        }
        if old.is_none() {
            if let Some(planning) = &context.planning {
                for reference in &planning.requests {
                    let key = format!("planning:{}", reference.id);
                    let count: usize = sql(tx
                        .query_row("SELECT value FROM metadata WHERE key=?1", [&key], |r| {
                            r.get::<_, String>(0)
                        })
                        .optional())?
                    .and_then(|s| s.parse().ok())
                    .unwrap_or(0);
                    if count >= policy.investigation.max_rounds as usize {
                        return Err(error("Decision planning budget exhausted; preserve source and inspect the proposed effects"));
                    }
                    sql(tx.execute("INSERT INTO metadata(key,value) VALUES(?1,?2) ON CONFLICT(key) DO UPDATE SET value=excluded.value",rusqlite::params![key,(count+1).to_string()]))?;
                }
            }
        }
        sql(tx.execute(
            "INSERT OR IGNORE INTO contexts(plan_id,attempt_id,phase,payload) VALUES(?1,?2,?3,?4)",
            rusqlite::params![plan.id, attempt, phase, encode(&context)?],
        ))?;
        event(
            &tx,
            &plan.id,
            "task.context.prepared",
            &json!({"task":task,"attempt":attempt,"phase":phase,"digest":context.context_digest,"context":context}),
        )?;
        sql(tx.commit())?;
        let paths = crate::ProjectPaths::from_root(worktree);
        let dir = worktree.join(".macc/runtime/decisions").join(attempt);
        let file = dir.join(format!("{phase}.json"));
        crate::atomic_write(&paths, &file, encode(&context)?.as_bytes())?;
        crate::atomic_write(
            &paths,
            &dir.join(format!("{phase}.md")),
            context.render().as_bytes(),
        )?;
        Ok((context, file))
    }
    pub fn issued(
        &mut self,
        plan: &str,
        attempt: &str,
        phase: &str,
        prompt: &str,
        policy: &Policy,
    ) -> crate::Result<ContextReceipt> {
        let context = self.read_context(plan, attempt, phase)?;
        let current = self.plan(plan)?;
        let fresh = self.context(&current, &context.task_id, attempt, phase, policy)?;
        if fresh.context_digest != context.context_digest {
            return Err(error("Stale context before provider invocation"));
        }
        if !prompt.contains(&context.render()) {
            return Err(error(
                "Mandatory decision text is missing from the actual runner prompt",
            ));
        }
        let receipt = ContextReceipt {
            context_digest: context.context_digest.clone(),
            prompt_digest: digest(&prompt),
            issued_at: now(),
            acknowledged: false,
            completion_status: "issued".into(),
            reviewed_commit: None,
            verification_error: None,
            compliance: Default::default(),
        };
        sql(self.db.execute(
            "UPDATE contexts SET receipt=?4 WHERE plan_id=?1 AND attempt_id=?2 AND phase=?3",
            rusqlite::params![plan, attempt, phase, encode(&receipt)?],
        ))?;
        Ok(receipt)
    }
    pub fn attest_review_artifact(
        &mut self,
        plan: &str,
        attempt: &str,
        worktree: &Path,
        expected: &str,
    ) -> crate::Result<()> {
        let _fence = self.mutation_lock()?;
        let context = self.read_context(plan, attempt, "review")?;
        let fresh = self.context(
            &self.plan(plan)?,
            &context.task_id,
            attempt,
            "review",
            &policy(&self.root)?,
        )?;
        if fresh.context_digest != context.context_digest {
            return Err(error(
                "DM_STALE: decisions changed before review postchecks completed",
            ));
        }
        if crate::git::is_dirty(worktree)? {
            return Err(error(
                "Review artifact is dirty; retained changes need validation",
            ));
        }
        let commit = crate::git::head_commit(worktree)?;
        if commit != expected {
            return Err(error("DM_STALE: artifact moved after independent review"));
        }
        let raw: String = sql(self.db.query_row(
            "SELECT receipt FROM contexts WHERE plan_id=?1 AND attempt_id=?2 AND phase='review'",
            rusqlite::params![plan, attempt],
            |r| r.get(0),
        ))?;
        let mut receipt: ContextReceipt = decode(&raw)?;
        if receipt.completion_status != "accepted" || !receipt.acknowledged {
            return Err(error("Review context/compliance was not accepted"));
        }
        receipt.reviewed_commit = Some(commit);
        let tx = sql(self
            .db
            .transaction_with_behavior(TransactionBehavior::Immediate))?;
        sql(tx.execute(
            "UPDATE contexts SET receipt=?3 WHERE plan_id=?1 AND attempt_id=?2 AND phase='review'",
            rusqlite::params![plan, attempt, encode(&receipt)?],
        ))?;
        event(
            &tx,
            plan,
            "task.context.received",
            &json!({"task":context.task_id,"attempt":attempt,"phase":"review","receipt":receipt,"postchecks":"clean, unchanged review artifact"}),
        )?;
        sql(tx.commit())?;
        Ok(())
    }
    pub fn read_context(
        &self,
        plan: &str,
        attempt: &str,
        phase: &str,
    ) -> crate::Result<ContextPackage> {
        let context: ContextPackage = decode(&sql(self.db.query_row(
            "SELECT payload FROM contexts WHERE plan_id=?1 AND attempt_id=?2 AND phase=?3",
            rusqlite::params![plan, attempt, phase],
            |r| r.get::<_, String>(0),
        ))?)?;
        if context.context_digest != context_hash(&context) {
            return Err(error("Stored context digest is corrupt"));
        }
        Ok(context)
    }
    pub fn complete(
        &mut self,
        plan: &str,
        attempt: &str,
        phase: &str,
        output: &str,
        policy: &Policy,
    ) -> crate::Result<ContextReceipt> {
        let context = self.read_context(plan, attempt, phase)?;
        let raw: String = sql(self.db.query_row(
            "SELECT receipt FROM contexts WHERE plan_id=?1 AND attempt_id=?2 AND phase=?3",
            rusqlite::params![plan, attempt, phase],
            |r| r.get(0),
        ))?;
        let mut receipt: ContextReceipt = decode(&raw)?;
        receipt.acknowledged = output.lines().any(|line| {
            line.trim()
                .strip_prefix("MACC_DECISION_ACK:")
                .is_some_and(|v| v.trim() == context.context_digest)
        });
        receipt.compliance.clear();
        receipt.reviewed_commit = None;
        let mut verification_error = if !receipt.acknowledged {
            Some("Missing exact MACC_DECISION_ACK; model acknowledgement is required, and is not proof of compliance".to_owned())
        } else {
            None
        };
        receipt.completion_status = if receipt.acknowledged {
            "accepted"
        } else {
            "missing_ack"
        }
        .into();
        match self.context(&self.plan(plan)?, &context.task_id, attempt, phase, policy) {
            Ok(fresh) if fresh.context_digest == context.context_digest => {}
            result => {
                receipt.completion_status = "stale".into();
                verification_error=Some(match result{Err(e)=>format!("DM_STALE: {e}"),_=>"DM_STALE: context changed during execution; preserve work and revalidate against current decisions".into()});
            }
        }
        if context.planning.is_some() {
            receipt.completion_status = "planning_only".into();
            verification_error=Some("DM_STALE: this was a decision-planning invocation, not implementation. Preserve work; the coordinator must re-dispatch with the applied graph.".into());
        }
        for line in output.lines() {
            if let Some(value) = line.trim().strip_prefix("MACC_DECISION_COMPLIANCE:") {
                let checks: std::collections::BTreeMap<String, Compliance> =
                    serde_json::from_str(value.trim()).map_err(|e| error(e.to_string()))?;
                for (id, check) in checks {
                    if !matches!(
                        check.status.as_str(),
                        "pass" | "fail" | "needs_human_review"
                    ) || check.evidence.trim().is_empty()
                    {
                        return Err(error(
                            "Compliance needs a recognized status and concrete evidence",
                        ));
                    }
                    receipt.compliance.insert(id, check);
                }
            }
        }
        if phase == "review" {
            for binding in &context.bindings {
                if !binding.resolution.as_ref().is_some_and(|r| r.advisory)
                    && !receipt
                        .compliance
                        .get(&binding.id)
                        .is_some_and(|c| c.status == "pass")
                {
                    if verification_error.is_none() {
                        receipt.completion_status = "compliance_failed".into();
                        verification_error = Some(format!(
                            "Decision {} has no passing independent compliance evidence",
                            binding.id
                        ));
                    }
                }
            }
        }
        receipt.verification_error = verification_error.clone();
        let tx = sql(self
            .db
            .transaction_with_behavior(TransactionBehavior::Immediate))?;
        sql(tx.execute(
            "UPDATE contexts SET receipt=?4 WHERE plan_id=?1 AND attempt_id=?2 AND phase=?3",
            rusqlite::params![plan, attempt, phase, encode(&receipt)?],
        ))?;
        event(
            &tx,
            plan,
            "task.context.received",
            &json!({"task":context.task_id,"attempt":attempt,"phase":phase,"receipt":receipt}),
        )?;
        sql(tx.commit())?;
        if let Some(message) = verification_error {
            return Err(error(message));
        }
        Ok(receipt)
    }
}
fn context_hash(context: &ContextPackage) -> String {
    let mut v = serde_json::to_value(context).unwrap();
    v.as_object_mut().unwrap().remove("context_digest");
    v.as_object_mut().unwrap().remove("graph_version");
    v.as_object_mut().unwrap().remove("decision_epoch");
    if let Some(planning) = v.get_mut("planning").and_then(Value::as_object_mut) {
        planning.remove("worktree");
        planning.remove("artifact_digest");
    }
    digest(&v)
}
