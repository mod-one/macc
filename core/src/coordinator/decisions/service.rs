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
    configure_attested(root, updated, None)
}
pub fn policy_challenge(current: &Policy, updated: &Policy) -> Value {
    json!({"action":"policy.configure","previous_policy_digest":digest(current),"updated_policy":updated})
}
pub fn configure_attested(
    root: &Path,
    updated: Policy,
    proof: Option<&Attestation>,
) -> Result<Policy> {
    updated.validate()?;
    let current = policy(root)?;
    let verified_actor = if current.trust_mode == "secure_approval" && updated != current {
        Some(super::secure::verify_attestation(root,&current,proof.ok_or_else(||error("Changing a secure decision policy requires an isolated attestation; prepare a policy challenge first"))?,"policy.configure",&digest(&policy_challenge(&current,&updated)),chrono::Utc::now().timestamp())?)
    } else {
        None
    };
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
            &json!({"actor":verified_actor.unwrap_or_else(||crate::service::coordinator_workflow::operator_identity(root)),"attestation":proof,"origin":"operator_configuration","policy":updated}),
        )?;
        sql(tx.commit())?;
    }
    if updated.enabled {
        if let Some(store) = control.as_mut() {
            for plan in store.plans()? {
                store.reevaluate_pending_locked(&plan, &updated)?;
            }
            store.project(root)?;
        }
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
        json!({"enabled":true,"policy":p,"projection_error":projection_error,"plan":plan,"decisions":store.applicable_decisions(&plan)?,"events":store.scoped_history(&plan)?,"operations":store.operations(&plan.id)?,"delivery":store.delivery_status(&plan,&p)?,"approval_broker_available":super::secure::broker_available(&p),"approval_tickets":store.approval_tickets()?.into_iter().filter(|t|store.operator_in_scope(&plan,&t.intent).is_ok()).collect::<Vec<_>>(),"pending_approvals":crate::coordinator::approval::current_pending_approvals(root,&registry(root)?),"operator_identity":crate::service::coordinator_workflow::operator_identity(root),"analytics":store.decision_analytics(&plan)?,"similar_requests":store.similar_requests(&plan,&p)?}),
    )
}

impl Store {
    /// Shared operator feedback path. Exact retries reuse the recorded human
    /// direction; they must not try to answer an already resolved question.
    pub fn feedback(
        &mut self,
        plan: &Plan,
        task: &str,
        text: &str,
        actor: &str,
        advisory: bool,
        policy: &Policy,
    ) -> Result<Decision> {
        self.feedback_attested(plan, task, text, actor, advisory, policy, None)
    }
    pub fn feedback_attested(
        &mut self,
        plan: &Plan,
        task: &str,
        text: &str,
        actor: &str,
        advisory: bool,
        policy: &Policy,
        proof: Option<&Attestation>,
    ) -> Result<Decision> {
        if text.trim().is_empty() {
            return Err(error("Operator guidance must not be empty"));
        }
        let actor = super::secure::mutation_authority(
            &self.root,
            "decision.feedback",
            &super::secure::feedback_challenge(plan, task, text, advisory, policy)?,
            actor,
            proof,
            policy,
        )?;
        let actor = actor.as_str();
        let request:Request=serde_json::from_value(json!({"idempotency_key":format!("feedback-{}",uuid::Uuid::new_v4()),"task_id":task,"kind":"feedback","scope":if task=="*"{"project"}else{"task"},"question":"Operator guidance","context":text,"recommendation":text,"investigated":["Direct operator instruction"],"impact":{"evidence":["Local operator"]}})).map_err(|e|error(e.to_string()))?;
        let mut human = policy.clone();
        human.human_question_min_risk = Consultation::Low;
        let decision = self.submit(plan, request, actor, "human", &human)?;
        if decision.status == "resolved" {
            if decision
                .resolution
                .as_ref()
                .is_some_and(|r| r.text == text && r.advisory == advisory)
            {
                return Ok(decision);
            }
            return Err(error("This guidance already has a different binding status; revoke or explicitly revise it instead of silently changing its authority"));
        }
        self.answer_details(
            &decision.id,
            decision.version,
            &AnswerInput {
                text: text.into(),
                advisory,
                attestation: proof.cloned(),
                ..Default::default()
            },
            actor,
            policy,
        )
    }
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
                if request.has_effects() {
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
        request: Request,
        actor: &str,
        origin: &str,
        policy: &Policy,
    ) -> Result<Decision> {
        let _fence = self.mutation_lock()?;
        self.submit_locked(plan, request, actor, origin, policy, None, None)
    }
    pub(super) fn submit_locked(
        &mut self,
        plan: &Plan,
        mut request: Request,
        actor: &str,
        origin: &str,
        policy: &Policy,
        previous: Option<&Decision>,
        revision_audit: Option<&Value>,
    ) -> Result<Decision> {
        policy.validate()?;
        if previous.is_none() && request.revision_of.is_some() {
            return Err(error("Use the versioned revision API to replace a request"));
        }
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
        if request.kind == "approval" {
            let revision = self.approval_subject_revision(plan, &request)?;
            if request
                .subject_revision
                .as_ref()
                .is_some_and(|r| r != &revision)
            {
                return Err(error(
                    "Approval proposal names an obsolete subject revision",
                ));
            }
            request.subject_revision = Some(revision);
        }
        let semantic = |r: &Request| {
            let mut r = r.clone();
            r.idempotency_key.clear();
            r.attempt_id.clear();
            digest(&r)
        };
        let submission_digest = semantic(&request);
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
            if old
                .submission_digest
                .clone()
                .unwrap_or_else(|| semantic(&old.request))
                != submission_digest
                || old.subject_digest != subject
            {
                return Err(error(
                    "Idempotency key reused for changed question or subject",
                ));
            }
            return Ok(old);
        }
        // Compare the submitted payload, independently of later native reconciliation.
        if let Some(old) = self.decisions(&plan.id)?.into_iter().find(|d| {
            d.submission_digest
                .clone()
                .unwrap_or_else(|| semantic(&d.request))
                == submission_digest
                && d.subject_digest == subject
                && !matches!(d.status.as_str(), "revoked" | "superseded")
        }) {
            return Ok(old);
        }
        let triage = self.triage(plan, &mut request, policy)?;
        if request.response_fields.len() > 32
            || request.supersedes.len() > 32
            || request.topic.as_ref().is_some_and(|t| t.len() > 256)
        {
            return Err(error(
                "Decision fields, topic or supersession list exceeds its structured budget",
            ));
        }
        let mut fields = BTreeSet::new();
        for field in &request.response_fields {
            if field.id.is_empty()
                || field.id.starts_with("__")
                || !fields.insert(&field.id)
                || field.label.trim().is_empty()
            {
                return Err(error(
                    "Response fields need unique nonempty IDs and readable labels",
                ));
            }
        }
        if request.impact.shared_contract
            && triage.domain.is_none()
            && policy.human_question_min_risk == Consultation::NeverAsk
        {
            return Err(error("Shared-contract choices require a stable topic, requirement reference or affected path for cross-performer arbitration"));
        }
        for patch in request.proposed_patch.iter().chain(
            request
                .options
                .iter()
                .filter_map(|o| o.proposed_patch.as_ref()),
        ) {
            if origin != "human" {
                if !patch.metadata_changes.is_empty() {
                    return Err(error(
                        "Agent proposals cannot rewrite plan governance metadata",
                    ));
                }
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
            self.preview_patch(plan, patch, &registry(&self.root)?)?;
        }
        let proposed_graph = request
            .patch_for(request.recommended_option_id.as_deref())
            .map(|patch| self.preview_patch(plan, patch, &registry(&self.root)?))
            .transpose()?;
        let requires_human_renewal =
            previous.is_some_and(|d| d.status == "rejected" || d.requires_human_renewal);
        let expired = if let Some(at) = &request.expires_at {
            chrono::DateTime::parse_from_rfc3339(at)
                .map_err(|_| error("Request expiry must be RFC3339"))?
                .timestamp()
                <= chrono::Utc::now().timestamp()
        } else {
            false
        };
        let unresolved =
            !triage.report.missing_sources.is_empty() || !request.missing_facts.is_empty();
        let automatic = !unresolved
            && !requires_human_renewal
            && !expired
            && (policy.human_question_min_risk == Consultation::NeverAsk
                || triage.reuse.is_some()
                || triage.standing.is_some()
                || (!triage.mandatory && !policy.human_question_min_risk.asks(triage.risk)));
        let reuse = triage.reuse.as_ref();
        let selected = if let Some(old) = reuse {
            old.resolution
                .as_ref()
                .and_then(|r| r.selected_option_id.clone())
                .filter(|id| request.options.iter().any(|o| o.id == *id))
        } else if let Some((_, choice)) = &triage.standing {
            choice
                .clone()
                .filter(|id| request.options.iter().any(|o| o.id == *id))
        } else {
            request.recommended_option_id.clone()
        };
        let apply_graph = automatic
            && reuse.is_none()
            && (triage.standing.is_none()
                || selected.is_some() && selected == request.recommended_option_id);
        let resolution = automatic.then(|| Resolution {
            answered_version: None,
            applied_patch: None,
            effects: String::new(),
            text: reuse
                .and_then(|d| d.resolution.as_ref())
                .map(|r| r.text.clone())
                .or_else(|| triage.standing.as_ref().map(|s| s.0.clone()))
                .unwrap_or_else(|| request.recommendation.clone()),
            selected_option_id: selected,
            actor: if reuse.is_some() || triage.standing.is_some() {
                "coordinator".into()
            } else {
                actor.into()
            },
            origin: if reuse.is_some() {
                "coordinator_reconciliation"
            } else if triage.standing.is_some() {
                "verified_existing_policy"
            } else {
                "delegated_autonomous"
            }
            .into(),
            policy_digest: digest(policy),
            recorded_at: now(),
            advisory: false,
            source_revision: request.subject_revision.clone(),
            attestation: None,
            fields: Default::default(),
            outcome: String::new(),
            patch_applied: apply_graph && proposed_graph.is_some(),
        });
        let decision = Decision {
            effects_commit: None,
            requester_actor: actor.into(),
            requester_origin: origin.into(),
            id: uuid::Uuid::new_v4().to_string(),
            plan_id: plan.id.clone(),
            version: 1,
            status: if automatic && reuse.is_some_and(|d| d.status == "awaiting_effects") {
                "waiting_dependency"
            } else if automatic
                && request
                    .options
                    .iter()
                    .find(|o| Some(&o.id) == request.recommended_option_id.as_ref())
                    .is_some_and(|o| o.requires_replan)
                && proposed_graph.is_none()
            {
                "awaiting_effects"
            } else if expired {
                "expired"
            } else if requires_human_renewal
                && policy.human_question_min_risk == Consultation::NeverAsk
            {
                "investigating"
            } else if automatic {
                "resolved"
            } else if unresolved
                && (policy.human_question_min_risk == Consultation::NeverAsk
                    || !policy.human_question_min_risk.asks(triage.risk))
            {
                "investigating"
            } else {
                "pending_human"
            }
            .into(),
            request,
            effective_risk: triage.risk,
            resolution,
            created_at: now(),
            requires_human_renewal,
            subject_digest: subject,
            submission_digest: Some(submission_digest),
            domain: triage.domain,
            policy_reason: triage.reason,
            reused_from: reuse.map(|d| DecisionRef {
                id: d.id.clone(),
                version: d.version,
            }),
            investigation: Some(triage.report),
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
        if let Some(old) = previous {
            if old.status != "resolved" {
                let mut retired: Decision = decode(&sql(tx.query_row(
                    "SELECT payload FROM requests WHERE id=?1",
                    [&old.id],
                    |r| r.get::<_, String>(0),
                ))?)?;
                if retired.version != old.version || retired.status != old.status {
                    return Err(error("Request changed during revision"));
                }
                retired.version += 1;
                retired.status = "superseded".into();
                sql(tx.execute(
                    "UPDATE requests SET payload=?2 WHERE id=?1",
                    rusqlite::params![retired.id, encode(&retired)?],
                ))?;
                event(
                    &tx,
                    &retired.plan_id,
                    "decision.lifecycle",
                    &json!({"actor":actor,"action":"revised","replacement":decision.id,"decision":retired}),
                )?;
            }
        }
        if let Some(audit) = revision_audit {
            let mut audit = audit.clone();
            audit["result_id"] = json!(decision.id);
            audit["result_version"] = json!(decision.version);
            event(&tx, &plan.id, "decision.mutation", &audit)?;
        }
        if decision.status == "resolved" {
            Self::supersede_in_transaction(&tx, &decision, &mut current)?;
        }
        if apply_graph {
            if let (Some(patch), Some(graph)) = (
                decision.request.patch_for(
                    decision
                        .resolution
                        .as_ref()
                        .and_then(|r| r.selected_option_id.as_deref()),
                ),
                proposed_graph,
            ) {
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
        self.answer_details(
            id,
            version,
            &AnswerInput {
                text: text.into(),
                option,
                advisory,
                ..Default::default()
            },
            actor,
            policy,
        )
    }
    pub fn answer_details(
        &mut self,
        id: &str,
        version: u64,
        input: &AnswerInput,
        actor: &str,
        policy: &Policy,
    ) -> Result<Decision> {
        let text = input.text.as_str();
        let option = input.option.clone();
        let advisory = input.advisory;
        let _fence = self.mutation_lock()?;
        if actor.trim().is_empty() || text.trim().is_empty() {
            return Err(error("An attributed nonempty answer is required"));
        }
        if encode(input)?.len() > policy.max_request_bytes {
            return Err(error("Answer exceeds configured decision budget"));
        }
        let pending = self.decision(id)?;
        if matches!(
            pending.status.as_str(),
            "resolved" | "rejected" | "awaiting_effects"
        ) {
            if pending.version == version + 1
                && pending.resolution.as_ref().is_some_and(|r| {
                    r.actor
                        == input
                            .attestation
                            .as_ref()
                            .map(|a| a.body.principal.as_str())
                            .unwrap_or(actor)
                        && r.attestation == input.attestation
                        && r.text == text
                        && r.selected_option_id == option
                        && r.advisory == advisory
                        && r.fields == input.fields
                        && (if r.outcome.is_empty() {
                            "answer"
                        } else {
                            r.outcome.as_str()
                        }) == input.outcome.as_str()
                        && r.patch_applied == input.apply_patch
                        && r.applied_patch == input.patch
                        && (if r.effects.is_empty() {
                            "auto"
                        } else {
                            r.effects.as_str()
                        }) == input.effects.as_str()
                })
            {
                return Ok(pending);
            }
        }
        let feedback_proof = pending.request.kind == "feedback"
            && pending.request.context == text
            && input
                .attestation
                .as_ref()
                .is_some_and(|proof| proof.body.action == "decision.feedback");
        let secure_actor = if policy.trust_mode == "secure_approval" {
            let challenge = if feedback_proof {
                super::secure::feedback_challenge(
                    &self.plan(&pending.plan_id)?,
                    &pending.request.task_id,
                    text,
                    advisory,
                    policy,
                )?
            } else {
                super::secure::answer_challenge(&pending, input, policy)
            };
            Some(super::secure::mutation_authority(
                &self.root,
                challenge["action"].as_str().unwrap(),
                &challenge,
                actor,
                input.attestation.as_ref(),
                policy,
            )?)
        } else {
            None
        };
        let actor = secure_actor.as_deref().unwrap_or(actor);
        if pending.request.kind == "approval"
            && pending.request.subject_revision.as_ref()
                != Some(
                    &self.approval_subject_revision(
                        &self.plan(&pending.plan_id)?,
                        &pending.request,
                    )?,
                )
        {
            return Err(error(
                "Approval artifact changed; obtain a fresh subject-bound request",
            ));
        }
        if pending.request.expires_at.as_ref().is_some_and(|at| {
            chrono::DateTime::parse_from_rfc3339(at)
                .is_ok_and(|at| at.timestamp() <= chrono::Utc::now().timestamp())
        }) {
            return Err(error(
                "The request expired; submit a reviewed revision before answering",
            ));
        }
        let mut current_request = pending.request.clone();
        self.triage(&self.plan(&pending.plan_id)?, &mut current_request, policy)?;
        for field in &current_request.response_fields {
            if field.required
                && input
                    .fields
                    .get(&field.id)
                    .is_none_or(|value| value.trim().is_empty())
            {
                return Err(error(format!("Required response field: {}", field.label)));
            }
            if let Some(value) = input.fields.get(&field.id) {
                if !field.choices.is_empty() && !field.choices.contains(value) {
                    return Err(error(format!(
                        "Invalid value for response field {}",
                        field.label
                    )));
                }
            }
        }
        if matches!(pending.request.kind.as_str(), "approval" | "authorization")
            && input.outcome == AnswerOutcome::Answer
            && input
                .fields
                .get("evidence")
                .is_none_or(|v| v.trim().is_empty())
        {
            return Err(error(
                "An approval/authorization answer requires explicit evidence",
            ));
        }
        if encode(&input.fields)?.len() + text.len() > policy.max_request_bytes {
            return Err(error("Response fields exceed the decision budget"));
        }
        let selected_patch = input
            .patch
            .as_ref()
            .or_else(|| pending.request.patch_for(option.as_deref()));
        if input.apply_patch && selected_patch.is_none() {
            return Err(error("There is no displayed patch for this choice"));
        }
        if input.outcome == AnswerOutcome::Answer && !advisory {
            if let Some(domain) = &pending.domain {
                for existing in self.applicable_decisions(&self.plan(&pending.plan_id)?)? {
                    if existing.id != pending.id
                        && existing.domain.as_ref() == Some(domain)
                        && existing.status == "resolved"
                        && !existing.resolution.as_ref().is_some_and(|r| r.advisory)
                        && !pending
                            .request
                            .supersedes
                            .iter()
                            .any(|r| r.id == existing.id && r.version == existing.version)
                        && existing
                            .resolution
                            .as_ref()
                            .is_some_and(|r| r.text != text || r.selected_option_id != option)
                    {
                        return Err(error(format!("Decision {} v{} already governs this topic. Submit a versioned supersession request before replacing its binding",existing.id,existing.version)));
                    }
                }
            }
        }
        if input.apply_patch
            && (advisory
                || input.outcome != AnswerOutcome::Answer
                || input.effects != AnswerEffects::Auto)
        {
            return Err(error(
                "Patch acceptance requires a binding answer and an unambiguous graph effect",
            ));
        }
        let proposed_graph = if input.apply_patch {
            Some(self.preview_patch(
                &self.plan(&pending.plan_id)?,
                selected_patch.unwrap(),
                &registry(&self.root)?,
            )?)
        } else {
            None
        };
        let awaiting = input.outcome == AnswerOutcome::Answer
            && !advisory
            && !input.apply_patch
            && (input.effects == AnswerEffects::Replan
                || input.effects == AnswerEffects::Auto
                    && (pending.request.has_effects() || input.patch.is_some()));
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
        decision.status = if awaiting {
            "awaiting_effects"
        } else if input.outcome == AnswerOutcome::Answer {
            "resolved"
        } else {
            "rejected"
        }
        .into();
        decision.resolution = Some(Resolution {
            answered_version: Some(version),
            applied_patch: input.patch.clone(),
            effects: input.effects.as_str().into(),
            text: text.into(),
            selected_option_id: option,
            actor: actor.into(),
            origin: if secure_actor.is_some() && feedback_proof {
                "human_feedback_secure"
            } else if secure_actor.is_some() {
                "human_secure_approval"
            } else {
                "human_local_trust"
            }
            .into(),
            policy_digest: digest(policy),
            recorded_at: now(),
            advisory,
            source_revision: decision.request.subject_revision.clone(),
            attestation: input.attestation.clone(),
            fields: input.fields.clone(),
            outcome: input.outcome.as_str().into(),
            patch_applied: input.apply_patch,
        });
        sql(tx.execute(
            "UPDATE requests SET payload=?2 WHERE id=?1",
            rusqlite::params![id, encode(&decision)?],
        ))?;
        plan.decision_epoch += 1;
        if decision.status == "resolved" {
            Self::supersede_in_transaction(&tx, &decision, &mut plan)?;
        }
        if let (Some(patch), Some(graph)) = (selected_patch, proposed_graph) {
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
        self.revoke_attested(id, version, actor, reason, None)
    }
    pub fn revoke_attested(
        &mut self,
        id: &str,
        version: u64,
        actor: &str,
        reason: &str,
        proof: Option<&Attestation>,
    ) -> Result<Decision> {
        let _fence = self.mutation_lock()?;
        if reason.trim().is_empty() {
            return Err(error("Revocation needs a reason"));
        }
        if let Some(d) =
            self.mutation_replay(id, version, "revoke", &json!(reason), actor, proof)?
        {
            return Ok(d);
        }
        let subject = self.decision(id)?;
        let p = policy(&self.root)?;
        let actor = super::secure::mutation_authority(
            &self.root,
            "decision.revoke",
            &super::secure::revoke_challenge(&subject, reason, &p),
            actor,
            proof,
            &p,
        )?;
        let actor = actor.as_str();
        let audit = Self::mutation_audit(id, version, "revoke", &json!(reason), actor, proof);
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
            &json!({"actor":actor,"reason":reason,"attestation":proof,"decision":decision}),
        )?;
        let mut audit = audit;
        audit["result_id"] = json!(decision.id);
        audit["result_version"] = json!(decision.version);
        event(&tx, &plan.id, "decision.mutation", &audit)?;
        let mut descendants = std::collections::BTreeSet::from([decision.id.clone()]);
        let mut aliases: Vec<Decision> = {
            let mut statement = sql(tx.prepare("SELECT payload FROM requests"))?;
            let rows = sql(statement.query_map([], |r| r.get::<_, String>(0)))?;
            rows.map(|row| decode(&sql(row)?)).collect::<Result<_>>()?
        };
        loop {
            let mut changed = false;
            for alias in &mut aliases {
                if matches!(alias.status.as_str(), "resolved" | "waiting_dependency")
                    && alias
                        .reused_from
                        .as_ref()
                        .is_some_and(|r| descendants.contains(&r.id))
                {
                    alias.version += 1;
                    alias.status = "revoked".into();
                    descendants.insert(alias.id.clone());
                    changed = true;
                    sql(tx.execute(
                        "UPDATE requests SET payload=?2 WHERE id=?1",
                        rusqlite::params![alias.id, encode(alias)?],
                    ))?;
                    let mut owner: Plan = decode(&sql(tx.query_row(
                        "SELECT payload FROM plans WHERE id=?1",
                        [&alias.plan_id],
                        |r| r.get::<_, String>(0),
                    ))?)?;
                    owner.decision_epoch += 1;
                    save_plan(&tx, &owner)?;
                    event(
                        &tx,
                        &alias.plan_id,
                        "decision.revoked",
                        &json!({"actor":actor,"reason":"Originating decision revoked","root":decision.id,"decision":alias}),
                    )?;
                }
            }
            if !changed {
                break;
            }
        }
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
