use super::{store::*, *};
use crate::{config::CanonicalConfig, coordinator::model::TaskRegistry, ProjectPaths, Result};
use rusqlite::{OptionalExtension, TransactionBehavior};
use serde_json::{json, Value};
use std::{
    collections::{BTreeMap, BTreeSet},
    path::Path,
};

pub fn policy(root: &Path) -> Result<Policy> {
    let path = ProjectPaths::from_root(root).config_path;
    let config = if path.exists() {
        crate::load_canonical_config(&path)?
    } else {
        CanonicalConfig::default()
    };
    let policy = config
        .automation
        .coordinator
        .and_then(|c| c.decisions)
        .unwrap_or_default();
    policy.validate()?;
    Ok(policy)
}
pub fn now() -> String {
    chrono::Utc::now().to_rfc3339()
}
pub fn registry(root: &Path) -> Result<TaskRegistry> {
    TaskRegistry::from_value(&crate::coordinator::state::coordinator_state_registry_load(
        root,
        &BTreeMap::new(),
    )?)
}
pub fn active_plan(root: &Path) -> Result<(Store, Plan)> {
    let config = crate::load_canonical_config(&ProjectPaths::from_root(root).config_path)?;
    if !config
        .automation
        .coordinator
        .as_ref()
        .and_then(|c| c.decisions.as_ref())
        .is_some_and(|p| p.enabled)
    {
        return Err(error("Decision management is disabled; enable automation.coordinator.decisions.enabled first"));
    }
    let c = config.automation.coordinator.as_ref();
    let mut store = Store::open(root)?;
    store.project(root)?;
    let source = crate::prd_queue::active_path(root, c, None)?;
    let reference = crate::config::CoordinatorConfigResolved::resolve(c).reference_branch;
    let plan = store.ensure_plan(root, &source, &reference, &registry(root)?)?;
    store.import_legacy(root, &plan)?;
    store.import_planner_requests(&plan, &policy(root)?)?;
    let plan = store.plan(&plan.id)?;
    Ok((store, plan))
}

/// Inspection remains available when a projection has drifted or the queue
/// completed; dispatch still uses active_plan's strict validation.
pub fn inspect_plan(root: &Path) -> Result<(Store, Plan)> {
    let cfg = crate::load_canonical_config(&ProjectPaths::from_root(root).config_path)?;
    let c = cfg.automation.coordinator.as_ref();
    let reference = crate::config::CoordinatorConfigResolved::resolve(c).reference_branch;
    let source = if let Some(c) = c.filter(|c| !c.prd_files.is_empty()) {
        let progress = crate::prd_queue::load(root)?;
        c.prd_files
            .get(progress.next)
            .or_else(|| c.prd_files.last())
            .cloned()
            .unwrap()
    } else {
        c.and_then(|c| c.prd_file.clone())
            .unwrap_or_else(|| "prd.json".into())
    };
    let store = Store::open(root)?;
    if let Some(plan) = store
        .plans()?
        .into_iter()
        .find(|p| p.source == source && p.reference_branch == reference)
    {
        return Ok((store, plan));
    }
    active_plan(root)
}

pub fn configure(root: &Path, updated: Policy) -> Result<Policy> {
    updated.validate()?;
    let paths = ProjectPaths::from_root(root);
    let mut config = crate::load_canonical_config(&paths.config_path)?;
    let was_enabled = config
        .automation
        .coordinator
        .as_ref()
        .and_then(|c| c.decisions.as_ref())
        .is_some_and(|p| p.enabled);
    let _stopped_guard = if was_enabled && !updated.enabled {
        Some(controller_lock(root).map_err(|_|error("Stop the coordinator before disabling decisions; active invocations must retain their context and merge barriers"))?.ok_or_else(||error("Cannot establish decision controller ownership"))?)
    } else {
        None
    };
    if !was_enabled && updated.enabled {
        crate::session_store::require_stopped(root)?;
    }
    let mut control = if was_enabled || updated.enabled {
        Some(Store::open(root)?)
    } else {
        None
    };
    let _publication_fence = control.as_ref().map(Store::mutation_lock).transpose()?;
    config
        .automation
        .coordinator
        .get_or_insert_with(Default::default)
        .decisions = Some(updated.clone());
    crate::save_canonical_config(&paths, &config)?;
    if let Some(store) = control.as_mut() {
        let tx = sql(store
            .db
            .transaction_with_behavior(TransactionBehavior::Immediate))?;
        event(
            &tx,
            "project",
            "decision.policy.changed",
            &json!({"actor":crate::service::coordinator_workflow::operator_identity(root),"origin":"local_operator_configuration","policy":updated}),
        )?;
        sql(tx.commit())?;
    }
    Ok(updated)
}
pub fn snapshot(root: &Path) -> Result<Value> {
    let p = policy(root)?;
    if !p.enabled {
        return Ok(json!({"enabled":false,"policy":p,"decisions":[],"events":[],"plan":null}));
    }
    let (store, plan) = inspect_plan(root)?;
    let projection_error = active_plan(root).err().map(|e| e.to_string());
    Ok(
        json!({"enabled":true,"policy":p,"projection_error":projection_error,"plan":plan,"decisions":store.applicable_decisions(&plan)?,"events":store.scoped_history(&plan)?}),
    )
}

impl Store {
    pub fn import_planner_requests(&mut self, plan: &Plan, policy: &Policy) -> Result<()> {
        if let Some(proposals) = plan.graph.get("decision_requests") {
            let requests: Vec<Request> = serde_json::from_value(proposals.clone())
                .map_err(|e| error(format!("Invalid planner decision_requests: {e}")))?;
            if requests.len() > 256 {
                return Err(error("Planner decision intake exceeds 256 proposals"));
            }
            for request in requests {
                if self
                    .decisions(&plan.id)?
                    .iter()
                    .any(|d| d.request.idempotency_key == request.idempotency_key)
                {
                    continue;
                }
                if request.proposed_patch.is_some() {
                    return Err(error("Initial planner intake cannot apply runtime patches before its plan identity is issued"));
                }
                self.submit(plan, request, "planner baseline proposal", "agent", policy)?;
            }
        }
        Ok(())
    }
    pub fn submit(
        &mut self,
        plan: &Plan,
        mut request: Request,
        actor: &str,
        origin: &str,
        policy: &Policy,
    ) -> Result<Decision> {
        let _fence = self.mutation_lock()?;
        policy.validate()?;
        if actor.trim().is_empty() || !matches!(origin, "human" | "agent" | "coordinator") {
            return Err(error(
                "Decision intake requires an attributed requester and recognized origin",
            ));
        }
        if self.plan(&plan.id)?.graph_version != plan.graph_version {
            return Err(error(
                "Task graph changed before the request was committed; refresh context",
            ));
        }
        if encode(&request)?.len() > policy.max_request_bytes {
            return Err(error("Decision request exceeds budget; reduce auxiliary evidence, not the question/constraints"));
        }
        if request.idempotency_key.trim().is_empty()
            || request.question.trim().is_empty()
            || request.recommendation.trim().is_empty()
            || request.investigated.is_empty()
        {
            return Err(error("A material question needs an idempotency key, prior investigation, concrete question and recommendation"));
        }
        if !matches!(
            request.kind.as_str(),
            "decision" | "clarification" | "approval" | "authorization" | "feedback"
        ) || !matches!(
            request.scope.as_str(),
            "task" | "project" | "paths" | "requirements"
        ) {
            return Err(error("Unknown request kind/scope"));
        }
        if request.kind == "authorization"
            && request
                .operation_id
                .as_deref()
                .is_none_or(|id| id.trim().is_empty())
        {
            return Err(error("An authorization requires a stable operation_id; it cannot authorize an unspecified future execution"));
        }
        if request.scope == "requirements" && request.requirement_refs.is_empty() {
            return Err(error("Requirements scope needs explicit requirement_refs"));
        }
        if request.scope == "paths"
            && (request.affected_paths.is_empty()
                || request.affected_paths.iter().any(|path| {
                    path.trim().is_empty()
                        || Path::new(path).is_absolute()
                        || Path::new(path)
                            .components()
                            .any(|c| matches!(c, std::path::Component::ParentDir))
                }))
        {
            return Err(error(
                "Path scope needs explicit project-relative affected_paths",
            ));
        }
        let subject = if request.task_id == "*" && origin == "human" {
            digest(&json!(plan.id))
        } else {
            digest(&super::graph::contract(super::graph::task(
                &plan.graph,
                &request.task_id,
            )?))
        };
        if request.affected_tasks.is_empty() {
            request.affected_tasks.push(request.task_id.clone());
        }
        for id in &request.affected_tasks {
            if id != "*" {
                super::graph::task(&plan.graph, id)?;
            }
        }
        let mut ids = BTreeSet::new();
        for option in &request.options {
            if option.id.trim().is_empty() || !ids.insert(&option.id) {
                return Err(error("Option IDs must be distinct and nonempty"));
            }
        }
        if request
            .recommended_option_id
            .as_ref()
            .is_some_and(|id| !ids.contains(id))
        {
            return Err(error("Recommended option does not exist"));
        }
        let old: Option<String> = sql(self
            .db
            .query_row(
                "SELECT payload FROM requests WHERE plan_id=?1 AND dedupe=?2",
                rusqlite::params![plan.id, request.idempotency_key],
                |r| r.get(0),
            )
            .optional())?;
        if let Some(old) = old {
            let old: Decision = decode(&old)?;
            if {
                let mut prior = old.request.clone();
                prior.attempt_id = request.attempt_id.clone();
                prior != request
            } || old.subject_digest != subject
            {
                return Err(error(
                    "Idempotency key reused for changed question or subject",
                ));
            }
            return Ok(old);
        }
        // Exact scoped deduplication only: no semantic model may merge consent.
        let semantic = |r: &Request| {
            let mut r = r.clone();
            r.idempotency_key.clear();
            r.attempt_id.clear();
            digest(&r)
        };
        if let Some(old) = self.decisions(&plan.id)?.into_iter().find(|d| {
            semantic(&d.request) == semantic(&request)
                && d.subject_digest == subject
                && !matches!(d.status.as_str(), "revoked" | "superseded")
        }) {
            return Ok(old);
        }
        let proposed_graph = if let Some(patch) = &request.proposed_patch {
            if origin != "human" {
                for op in &patch.operations {
                    match op {
                        Operation::UpdateTask { changes, .. }
                            if changes.keys().any(|key| {
                                matches!(key.as_str(), "gate" | "governance_ref" | "dependencies")
                            }) =>
                        {
                            return Err(error("An agent graph patch cannot change governance"))
                        }
                        Operation::RemoveDependency { depends_on, .. }
                            if super::graph::task(&plan.graph, depends_on)?
                                .get("gate")
                                .is_some() =>
                        {
                            return Err(error(
                                "An agent cannot remove a required approval dependency",
                            ))
                        }
                        Operation::DisableTask { task_id }
                        | Operation::SupersedeTask { task_id, .. }
                        | Operation::RemoveDependency { task_id, .. }
                            if super::graph::task(&plan.graph, task_id)?
                                .get("gate")
                                .is_some() =>
                        {
                            return Err(error(
                                "An agent graph patch cannot remove an approval requirement",
                            ))
                        }
                        _ => {}
                    }
                }
            }
            Some(self.preview_patch(plan, patch, &registry(&self.root)?)?)
        } else {
            None
        };
        let mut risk = request.risk;
        if proposed_graph.is_some() {
            risk = risk.max(Risk::High);
        }
        if request.impact.shared_contract
            || request.impact.product_scope
            || request.impact.security_privacy
            || request.scope == "project"
            || request.impact.evidence.is_empty()
        {
            risk = risk.max(Risk::High);
        }
        if request.impact.irreversible || request.impact.production {
            risk = Risk::Critical;
        }
        if let Ok(task) = super::graph::task(&plan.graph, &request.task_id) {
            if let Some(label) = task
                .pointer("/routing_hints/risk_level")
                .and_then(Value::as_str)
            {
                if let Ok(declared) = serde_json::from_value::<Risk>(json!(label)) {
                    risk = risk.max(declared);
                }
            }
            if task.get("gate").is_some() {
                risk = risk.max(Risk::High);
            }
        }
        let mandatory = (request.kind == "approval" || request.kind == "authorization")
            || (request.impact.production
                && policy
                    .mandatory_human_triggers
                    .iter()
                    .any(|t| t == "production"))
            || (request.impact.irreversible
                && policy
                    .mandatory_human_triggers
                    .iter()
                    .any(|t| t == "irreversible"));
        let automatic = policy.human_question_min_risk == Consultation::NeverAsk
            || (!mandatory && !policy.human_question_min_risk.asks(risk));
        let resolution = automatic.then(|| Resolution {
            text: request.recommendation.clone(),
            selected_option_id: request.recommended_option_id.clone(),
            actor: actor.into(),
            origin: "delegated_autonomous".into(),
            policy_digest: digest(policy),
            recorded_at: now(),
            advisory: false,
            source_revision: None,
        });
        let decision = Decision {
            requester_actor: actor.into(),
            requester_origin: origin.into(),
            id: uuid::Uuid::new_v4().to_string(),
            plan_id: plan.id.clone(),
            version: 1,
            status: if automatic {
                "resolved"
            } else {
                "pending_human"
            }
            .into(),
            request,
            effective_risk: risk,
            resolution,
            created_at: now(),
            subject_digest: subject,
        };
        let tx = sql(self
            .db
            .transaction_with_behavior(TransactionBehavior::Immediate))?;
        sql(tx.execute(
            "INSERT INTO requests VALUES(?1,?2,?3,?4)",
            rusqlite::params![
                decision.id,
                plan.id,
                decision.request.idempotency_key,
                encode(&decision)?
            ],
        ))?;
        let mut current: Plan = decode(&sql(tx.query_row(
            "SELECT payload FROM plans WHERE id=?1",
            [&plan.id],
            |r| r.get::<_, String>(0),
        ))?)?;
        current.decision_epoch += 1;
        if automatic {
            if let (Some(patch), Some(graph)) = (&decision.request.proposed_patch, proposed_graph) {
                super::graph::commit_graph(&tx, &mut current, patch, graph, actor)?;
            }
        }
        save_plan(&tx, &current)?;
        event(
            &tx,
            &plan.id,
            if automatic {
                "decision.resolved"
            } else {
                "decision.requested"
            },
            &decision,
        )?;
        sql(tx.commit())?;
        Ok(decision)
    }
    pub fn answer(
        &mut self,
        id: &str,
        version: u64,
        text: &str,
        option: Option<String>,
        actor: &str,
        advisory: bool,
        policy: &Policy,
    ) -> Result<Decision> {
        let _fence = self.mutation_lock()?;
        if actor.trim().is_empty() || text.trim().is_empty() {
            return Err(error("An attributed nonempty answer is required"));
        }
        if text.len() > policy.max_request_bytes {
            return Err(error("Answer exceeds configured decision budget"));
        }
        let pending = self.decision(id)?;
        let proposed_graph = if option.is_some() && option == pending.request.recommended_option_id
        {
            if let Some(patch) = &pending.request.proposed_patch {
                Some(self.preview_patch(
                    &self.plan(&pending.plan_id)?,
                    patch,
                    &registry(&self.root)?,
                )?)
            } else {
                None
            }
        } else {
            None
        };
        let tx = sql(self
            .db
            .transaction_with_behavior(TransactionBehavior::Immediate))?;
        let mut decision: Decision = decode(&sql(tx.query_row(
            "SELECT payload FROM requests WHERE id=?1",
            [id],
            |r| r.get::<_, String>(0),
        ))?)?;
        if decision.version != version || decision.status != "pending_human" {
            return Err(error(
                "Request changed or was answered elsewhere; refresh before submitting",
            ));
        }
        if option
            .as_ref()
            .is_some_and(|id| !decision.request.options.iter().any(|o| o.id == *id))
        {
            return Err(error("Selected option is not in the question"));
        }
        let mut plan: Plan = decode(&sql(tx.query_row(
            "SELECT payload FROM plans WHERE id=?1",
            [&decision.plan_id],
            |r| r.get::<_, String>(0),
        ))?)?;
        if decision.request.task_id != "*"
            && digest(&super::graph::contract(super::graph::task(
                &plan.graph,
                &decision.request.task_id,
            )?)) != decision.subject_digest
        {
            return Err(error(
                "Subject changed; reconcile the request rather than approving stale work",
            ));
        }
        decision.version += 1;
        decision.status = "resolved".into();
        decision.resolution = Some(Resolution {
            text: text.into(),
            selected_option_id: option,
            actor: actor.into(),
            origin: "human_local_trust".into(),
            policy_digest: digest(policy),
            recorded_at: now(),
            advisory,
            source_revision: None,
        });
        sql(tx.execute(
            "UPDATE requests SET payload=?2 WHERE id=?1",
            rusqlite::params![id, encode(&decision)?],
        ))?;
        plan.decision_epoch += 1;
        if let (Some(patch), Some(graph)) = (&decision.request.proposed_patch, proposed_graph) {
            super::graph::commit_graph(&tx, &mut plan, patch, graph, actor)?;
        }
        save_plan(&tx, &plan)?;
        event(&tx, &plan.id, "decision.resolved", &decision)?;
        sql(tx.commit())?;
        Ok(decision)
    }
    pub fn revoke(
        &mut self,
        id: &str,
        version: u64,
        actor: &str,
        reason: &str,
    ) -> Result<Decision> {
        let _fence = self.mutation_lock()?;
        if reason.trim().is_empty() {
            return Err(error("Revocation needs a reason"));
        }
        let tx = sql(self
            .db
            .transaction_with_behavior(TransactionBehavior::Immediate))?;
        let mut decision: Decision = decode(&sql(tx.query_row(
            "SELECT payload FROM requests WHERE id=?1",
            [id],
            |r| r.get::<_, String>(0),
        ))?)?;
        if decision.version != version {
            return Err(error("Stale decision version"));
        }
        decision.version += 1;
        decision.status = "revoked".into();
        sql(tx.execute(
            "UPDATE requests SET payload=?2 WHERE id=?1",
            rusqlite::params![id, encode(&decision)?],
        ))?;
        let mut plan: Plan = decode(&sql(tx.query_row(
            "SELECT payload FROM plans WHERE id=?1",
            [&decision.plan_id],
            |r| r.get::<_, String>(0),
        ))?)?;
        plan.decision_epoch += 1;
        save_plan(&tx, &plan)?;
        event(
            &tx,
            &plan.id,
            "decision.revoked",
            &json!({"actor":actor,"reason":reason,"decision":decision}),
        )?;
        sql(tx.commit())?;
        Ok(decision)
    }
    pub fn project(&mut self, root: &Path) -> Result<()> {
        let cfg = crate::load_canonical_config(&ProjectPaths::from_root(root).config_path)?;
        let reference =
            crate::config::CoordinatorConfigResolved::resolve(cfg.automation.coordinator.as_ref())
                .reference_branch;
        // A missing exported PRD is recoverable even when the original outbox
        // was already acknowledged before clear/crash.
        for plan in self.plans()? {
            if plan.reference_branch == reference && !root.join(&plan.source).exists() {
                sql(self.db.execute("INSERT INTO outbox(plan_id,kind,payload) SELECT ?1,'graph',?2 WHERE NOT EXISTS(SELECT 1 FROM outbox WHERE plan_id=?1 AND applied=0)",rusqlite::params![plan.id,encode(&plan)?]))?;
            }
        }
        let pending: Vec<(i64, String)> = {
            let mut stmt = sql(self
                .db
                .prepare("SELECT id,plan_id FROM outbox WHERE applied=0 ORDER BY id"))?;
            let rows = sql(stmt.query_map([], |r| Ok((r.get(0)?, r.get(1)?))))?;
            rows.map(sql).collect::<Result<_>>()?
        };
        for (id, plan_id) in pending {
            let tx = sql(self
                .db
                .transaction_with_behavior(TransactionBehavior::Immediate))?;
            let mut plan: Plan = decode(&sql(tx.query_row(
                "SELECT payload FROM plans WHERE id=?1",
                [&plan_id],
                |r| r.get::<_, String>(0),
            ))?)?;
            if plan.reference_branch != reference {
                continue;
            }
            let paths = ProjectPaths::from_root(root);
            let target = super::graph::projection_path(root, &plan.source)?;
            if let Ok(bytes) = std::fs::read(&target) {
                let disk: Value =
                    serde_json::from_slice(&bytes).map_err(|e| error(e.to_string()))?;
                if digest(&disk) != plan.source_digest {
                    // Only replace the previous accepted projection, never a
                    // hand-edited PRD competing with an outbox replay.
                    let mut stmt=sql(tx.prepare("SELECT payload FROM events WHERE plan_id=?1 AND kind IN ('plan.imported','graph.patch.committed','graph.source.reconciled') ORDER BY seq"))?;
                    let rows = sql(stmt.query_map([&plan.id], |r| r.get::<_, String>(0)))?;
                    let mut known = false;
                    for row in rows {
                        let v: Value = decode(&sql(row)?)?;
                        if v.get("graph").is_some_and(|g| digest(g) == digest(&disk)) {
                            known = true;
                        }
                    }
                    if !known {
                        return Err(error(
                            "PRD projection has out-of-band edits; recovery did not overwrite it",
                        ));
                    }
                }
            }
            crate::atomic_write(
                &paths,
                &target,
                serde_json::to_string_pretty(&plan.graph)
                    .map_err(|e| error(e.to_string()))?
                    .as_bytes(),
            )?;
            let mut queue = crate::prd_queue::load(root)?;
            if let Some(entry) = queue.entries.iter_mut().find(|e| e.path == plan.source) {
                use sha2::Digest;
                entry.fingerprint = format!(
                    "{:x}",
                    sha2::Sha256::digest(std::fs::read(&target).map_err(|e| error(e.to_string()))?)
                );
                entry.task_ids = super::graph::tasks(&plan.graph)?
                    .iter()
                    .filter_map(|t| t["id"].as_str().map(str::to_owned))
                    .collect();
                crate::prd_queue::save(root, &queue)?;
            }
            plan.projected_version = plan.graph_version;
            save_plan(&tx, &plan)?;
            sql(tx.execute("UPDATE outbox SET applied=1 WHERE id=?1", [id]))?;
            event(
                &tx,
                &plan.id,
                "graph.projection.rebuilt",
                &json!({"version":plan.graph_version}),
            )?;
            sql(tx.commit())?;
        }
        Ok(())
    }
    pub fn import_legacy(&mut self, root: &Path, plan: &Plan) -> Result<()> {
        let file = root.join(".macc/state/coordinator.sqlite");
        if !file.exists() {
            return Ok(());
        }
        let source = sql(rusqlite::Connection::open_with_flags(
            &file,
            rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY,
        ))?;
        let exists:bool=sql(source.query_row("SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type='table' AND name='gate_decisions')",[],|r|r.get(0)))?;
        if !exists {
            return Ok(());
        }
        let mut stmt=sql(source.prepare("SELECT id,task_id,decision,role,actor,revision,evidence,reason,recorded_at FROM gate_decisions ORDER BY id"))?;
        let rows = sql(stmt.query_map([], |r| {
            Ok((
                r.get::<_, i64>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, String>(2)?,
                r.get::<_, String>(3)?,
                r.get::<_, String>(4)?,
                r.get::<_, Option<String>>(5)?,
                r.get::<_, Option<String>>(6)?,
                r.get::<_, Option<String>>(7)?,
                r.get::<_, String>(8)?,
            ))
        }))?;
        let known: Vec<Value> = {
            let mut stmt = sql(self
                .db
                .prepare("SELECT payload FROM legacy_approvals WHERE plan_id=?1"))?;
            let rows = sql(stmt.query_map([&plan.id], |r| r.get::<_, String>(0)))?;
            rows.map(|r| decode(&sql(r)?)).collect::<Result<_>>()?
        };
        let tx = sql(self
            .db
            .transaction_with_behavior(TransactionBehavior::Immediate))?;
        for row in rows {
            let (id, task, decision, role, actor, revision, evidence, reason, at) = sql(row)?;
            // Never relabel old evidence under a newly configured policy.
            if known.iter().any(|v| {
                v["task_id"] == task
                    && v["actor"] == actor
                    && v["role"] == role
                    && v["recorded_at"] == at
                    && v["decision"] == decision
            }) {
                continue;
            }
            if super::graph::task(&plan.graph, &task).is_err() {
                continue;
            }
            let payload = json!({"task_id":task,"decision":decision,"role":role,"actor":actor,"revision":revision,"evidence":evidence,"reason":reason,"recorded_at":at,"trust":"legacy_local_trust","binding_verified":false,"binding_note":"Historical evidence retained in this plan; the original policy/plan binding is not proven. Observed digests do not authorize reuse.","gate_digest":super::graph::task(&plan.graph,&task).ok().map(|t|digest(&super::graph::contract(t))),"policy_digest":digest(&policy(root)?),"manifest":super::approvals::approval_manifest(root,plan,&task,revision.as_deref()).ok()});
            let key = format!(
                "coordinator:{id}:{}",
                digest(&json!([
                    task, decision, role, actor, revision, evidence, reason, at
                ]))
            );
            let changed = sql(tx.execute(
                "INSERT OR IGNORE INTO legacy_approvals VALUES(?1,?2,?3)",
                rusqlite::params![plan.id, key, encode(&payload)?],
            ))?;
            if changed > 0 {
                event(&tx, &plan.id, "approval.legacy_imported", &payload)?;
            }
        }
        sql(tx.commit())?;
        Ok(())
    }
}
