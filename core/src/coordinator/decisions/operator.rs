//! Shared typed operator actions for CLI, Web, TUI and isolated approval flows.
use super::*;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "action", rename_all = "snake_case", deny_unknown_fields)]
pub enum OperatorIntent {
    EvidencePermission {
        id: String,
        plan_id: String,
        version: u64,
        share: bool,
    },
    Feedback {
        plan_id: String,
        task: String,
        text: String,
        advisory: bool,
    },
    Graph {
        patch: Patch,
        #[serde(default)]
        source_digest: Option<String>,
    },
    Answer {
        id: String,
        version: u64,
        input: AnswerInput,
    },
    Revoke {
        id: String,
        version: u64,
        reason: String,
    },
    Withdraw {
        id: String,
        version: u64,
        reason: String,
    },
    Revise {
        id: String,
        version: u64,
        request: Request,
    },
    Effects {
        id: String,
        version: u64,
        patch: Patch,
    },
    Reconcile {
        plan_id: String,
        operation: String,
        version: u64,
        state: OperationState,
        evidence: String,
    },
    Configure {
        policy: Policy,
    },
    ConditionalGateVote {
        plan_id: String,
        task: String,
        record: crate::coordinator::model::ApprovalRecord,
        conditions: Vec<ApprovalCondition>,
    },
    GateVote {
        plan_id: String,
        task: String,
        record: crate::coordinator::model::ApprovalRecord,
    },
}
impl OperatorIntent {
    pub fn decision_id(&self) -> Option<&str> {
        match self {
            Self::Answer { id, .. }
            | Self::Revoke { id, .. }
            | Self::Withdraw { id, .. }
            | Self::Revise { id, .. }
            | Self::Effects { id, .. } => Some(id),
            _ => None,
        }
    }
}
impl Store {
    /// Client boundary: an explicit typed plan never silently crosses the
    /// selected reference branch or borrows a colliding task identity.
    pub fn operator_in_scope(&self, plan: &Plan, intent: &OperatorIntent) -> crate::Result<()> {
        if let Some(id) = intent.decision_id() {
            self.decision_in_scope(plan, id)?;
        }
        let requested = match intent {
            OperatorIntent::EvidencePermission { plan_id, .. }
            | OperatorIntent::Feedback { plan_id, .. }
            | OperatorIntent::Reconcile { plan_id, .. }
            | OperatorIntent::ConditionalGateVote { plan_id, .. }
            | OperatorIntent::GateVote { plan_id, .. } => Some(plan_id.as_str()),
            OperatorIntent::Graph { patch, .. } => Some(patch.plan_id.as_str()),
            _ => None,
        };
        if requested.is_some_and(|id| id != plan.id) {
            return Err(error("This action belongs to another plan/reference branch; select that plan before submitting"));
        }
        Ok(())
    }
    pub fn operator_challenge(&self, intent: &OperatorIntent) -> crate::Result<Value> {
        let p = policy(&self.root)?;
        let decision = |id: &str, version: u64| -> crate::Result<Decision> {
            let d = self.decision(id)?;
            if d.version != version {
                return Err(error("Request changed; refresh before preparing approval"));
            }
            Ok(d)
        };
        match intent {
            OperatorIntent::EvidencePermission {
                id,
                plan_id,
                version,
                share,
            } => {
                if self.artifact(id)?.plan_id != *plan_id {
                    return Err(error("Evidence consent belongs to another plan"));
                }
                self.evidence_permission_challenge(id, *version, *share)
            }
            OperatorIntent::Feedback {
                plan_id,
                task,
                text,
                advisory,
            } => super::secure::feedback_challenge(&self.plan(plan_id)?, task, text, *advisory, &p),
            OperatorIntent::Graph {
                patch,
                source_digest,
            } => Ok(graph_challenge(
                &self.plan(&patch.plan_id)?,
                patch,
                source_digest.as_deref(),
                &p,
            )),
            OperatorIntent::Answer { id, version, input } => {
                Ok(answer_challenge(&decision(id, *version)?, input, &p))
            }
            OperatorIntent::Revoke {
                id,
                version,
                reason,
            } => Ok(revoke_challenge(&decision(id, *version)?, reason, &p)),
            OperatorIntent::Withdraw {
                id,
                version,
                reason,
            } => Ok(withdraw_challenge(&decision(id, *version)?, reason, &p)),
            OperatorIntent::Revise {
                id,
                version,
                request,
            } => Ok(revision_challenge(&decision(id, *version)?, request, &p)),
            OperatorIntent::Effects { id, version, patch } => {
                Ok(effects_challenge(&decision(id, *version)?, patch, &p))
            }
            OperatorIntent::Reconcile {
                plan_id,
                operation,
                version,
                state,
                evidence,
            } => {
                self.plan(plan_id)?;
                let op = self
                    .operation(plan_id, operation)?
                    .ok_or_else(|| error("Unknown operation"))?;
                if op.version != *version {
                    return Err(error(
                        "Operation changed; refresh before preparing approval",
                    ));
                }
                Ok(operation_challenge(&op, *state, evidence, &p))
            }
            OperatorIntent::Configure { policy: updated } => Ok(policy_challenge(&p, updated)),
            OperatorIntent::ConditionalGateVote {
                plan_id,
                task,
                record,
                conditions,
            } => {
                self.validate_operator_vote(plan_id, task, record)?;
                super::approvals::approval_conditions_challenge(
                    &self.plan(plan_id)?,
                    task,
                    record,
                    conditions,
                    &p,
                )
            }
            OperatorIntent::GateVote {
                plan_id,
                task,
                record,
            } => {
                self.validate_operator_vote(plan_id, task, record)?;
                approval_challenge(&self.plan(plan_id)?, task, record, &p)
            }
        }
    }
    pub fn apply_operator(
        &mut self,
        intent: &OperatorIntent,
        proof: Option<&Attestation>,
        actor: &str,
    ) -> crate::Result<Value> {
        let p = policy(&self.root)?;
        match intent {
            OperatorIntent::EvidencePermission {
                id,
                plan_id,
                version,
                share,
            } => {
                if self.artifact(id)?.plan_id != *plan_id {
                    return Err(error("Evidence consent belongs to another plan"));
                }
                Ok(json!(
                    self.share_evidence(id, *version, *share, actor, proof)?
                ))
            }
            OperatorIntent::Feedback {
                plan_id,
                task,
                text,
                advisory,
            } => Ok(json!(self.feedback_attested(
                &self.plan(plan_id)?,
                task,
                text,
                actor,
                *advisory,
                &p,
                proof
            )?)),
            OperatorIntent::Graph {
                patch,
                source_digest,
            } => {
                let plan = if let Some(source) = source_digest {
                    self.reconcile_projection_attested(
                        &self.root.clone(),
                        patch,
                        source,
                        actor,
                        proof,
                    )?
                } else {
                    self.apply_patch_attested(
                        &patch.plan_id,
                        patch,
                        &registry(&self.root)?,
                        actor,
                        proof,
                    )?
                };
                self.project(&self.root.clone())?;
                Ok(json!(plan))
            }
            OperatorIntent::Answer { id, version, input } => {
                let mut input = input.clone();
                if let Some(proof) = proof {
                    input.attestation = Some(proof.clone());
                }
                Ok(json!(self.answer_details(id, *version, &input, actor, &p)?))
            }
            OperatorIntent::Revoke {
                id,
                version,
                reason,
            } => Ok(json!(
                self.revoke_attested(id, *version, actor, reason, proof)?
            )),
            OperatorIntent::Withdraw {
                id,
                version,
                reason,
            } => Ok(json!(
                self.withdraw_request(id, *version, reason, actor, proof)?
            )),
            OperatorIntent::Revise {
                id,
                version,
                request,
            } => Ok(json!(self.revise_request(
                id,
                *version,
                request.clone(),
                actor,
                "human",
                &p,
                proof
            )?)),
            OperatorIntent::Effects { id, version, patch } => {
                let d = self.apply_effects_attested(id, *version, patch, actor, proof)?;
                self.project(&self.root.clone())?;
                Ok(json!(d))
            }
            OperatorIntent::Reconcile {
                plan_id,
                operation,
                version,
                state,
                evidence,
            } => Ok(json!(self.reconcile_operation_attested(
                plan_id, operation, *version, *state, evidence, actor, proof
            )?)),
            OperatorIntent::Configure { policy: updated } => Ok(json!(configure_attested(
                &self.root,
                updated.clone(),
                proof
            )?)),
            OperatorIntent::ConditionalGateVote {
                plan_id,
                task,
                record,
                conditions,
            } => {
                self.validate_operator_vote(plan_id, task, record)?;
                self.record_gate_conditions_attested(
                    &self.root.clone(),
                    &self.plan(plan_id)?,
                    task,
                    record,
                    conditions,
                    proof,
                )?;
                Ok(json!({"recorded":true,"task":task,"conditional":true}))
            }
            OperatorIntent::GateVote {
                plan_id,
                task,
                record,
            } => {
                self.validate_operator_vote(plan_id, task, record)?;
                self.record_approval_attested(
                    &self.root.clone(),
                    &self.plan(plan_id)?,
                    task,
                    record,
                    proof,
                )?;
                Ok(json!({"recorded":true,"task":task}))
            }
        }
    }
    fn validate_operator_vote(
        &self,
        plan_id: &str,
        task: &str,
        record: &crate::coordinator::model::ApprovalRecord,
    ) -> crate::Result<()> {
        let plan = self.plan(plan_id)?;
        let mut registry = registry(&self.root)?;
        crate::coordinator::approval::reconcile_human_gates_for_project(
            &self.root,
            &mut registry,
            &now(),
        )?;
        let subject = super::graph::task(&plan.graph, task)?
            .pointer("/gate/subject_task")
            .and_then(Value::as_str)
            .ok_or_else(|| error("This task is not a subject-bound approval gate"))?;
        let revision = crate::coordinator::approval::subject_revision(
            &self.root,
            &plan.reference_branch,
            subject,
        )
        .ok_or_else(|| error("No verified current approval subject"))?;
        let request = crate::coordinator::approval::DecisionRequest {
            task_id: task.into(),
            decision: record.decision,
            role: record.role.clone(),
            actor: record.actor.clone(),
            revision: record.revision.clone(),
            evidence: record.evidence.clone(),
            reason: record.reason.clone(),
        };
        let validated = crate::coordinator::approval::validate_decision(
            &registry,
            &request,
            Some(&revision),
            &record.recorded_at,
        )?;
        if validated != *record {
            return Err(error(
                "Approval form changed during validation; review exact current fields and subject",
            ));
        }
        Ok(())
    }
    /// Recover an already committed action without replaying it after a client crash.
    pub(super) fn operator_result(
        &self,
        intent: &OperatorIntent,
        proof: &Attestation,
    ) -> crate::Result<Option<Value>> {
        match intent {
            OperatorIntent::EvidencePermission {
                id, version, share, ..
            } => {
                if let Some(p) = self.evidence_permission(id)? {
                    if p.version == version + 1
                        && p.share == *share
                        && p.attestation.as_ref() == Some(proof)
                    {
                        return Ok(Some(json!(p)));
                    }
                }
            }
            OperatorIntent::Feedback {
                plan_id,
                task,
                text,
                advisory,
            } => {
                for d in self.decisions(plan_id)? {
                    if d.request.task_id == *task
                        && d.resolution.as_ref().is_some_and(|r| {
                            r.text == *text
                                && r.advisory == *advisory
                                && r.attestation.as_ref() == Some(proof)
                        })
                    {
                        return Ok(Some(json!(d)));
                    }
                }
            }
            OperatorIntent::Graph { patch, .. } => {
                use rusqlite::OptionalExtension;
                let record: Option<String> = super::store::sql(
                    self.db
                        .query_row(
                            "SELECT payload FROM operations WHERE plan_id=?1 AND key=?2",
                            rusqlite::params![patch.plan_id, patch.idempotency_key],
                            |r| r.get(0),
                        )
                        .optional(),
                )?;
                if record.as_ref().is_some_and(|r| {
                    serde_json::from_str::<Value>(r)
                        .is_ok_and(|v| v["fingerprint"] == digest(patch))
                }) {
                    return Ok(Some(json!(self.plan(&patch.plan_id)?)));
                }
            }
            OperatorIntent::Answer { id, version, input } => {
                let d = self.decision(id)?;
                if d.resolution.as_ref().is_some_and(|r| {
                    r.answered_version == Some(*version)
                        && r.text == input.text
                        && r.fields == input.fields
                        && r.attestation.as_ref() == Some(proof)
                }) {
                    return Ok(Some(json!(d)));
                }
            }
            OperatorIntent::Effects { id, version, patch } => {
                let d = self.decision(id)?;
                if d.version >= version + 1
                    && d.effects_commit.as_ref().is_some_and(|e| {
                        e.patch_digest == digest(patch) && e.attestation.as_ref() == Some(proof)
                    })
                {
                    return Ok(Some(json!(d)));
                }
            }
            OperatorIntent::Configure { policy: updated } => {
                if policy(&self.root)? == *updated {
                    return Ok(Some(json!(updated)));
                }
            }
            OperatorIntent::Revise {
                id,
                version,
                request,
            } => {
                if let Some(d) = self.mutation_replay(
                    id,
                    *version,
                    "revise",
                    &json!(request),
                    &proof.body.principal,
                    Some(proof),
                )? {
                    return Ok(Some(json!(d)));
                }
            }
            OperatorIntent::Revoke { id, version, .. }
            | OperatorIntent::Withdraw { id, version, .. } => {
                let d = self.decision(id)?;
                let target = if matches!(intent, OperatorIntent::Revoke { .. }) {
                    "revoked"
                } else {
                    "withdrawn"
                };
                if d.version >= version + 1 && d.status == target {
                    for event in self.events(&d.plan_id)? {
                        let v = &event["payload"];
                        if v["attestation"] == json!(proof)
                            && v["decision"]["id"] == *id
                            && v["decision"]["version"] == version + 1
                        {
                            return Ok(Some(json!(d)));
                        }
                    }
                }
            }
            OperatorIntent::Reconcile {
                plan_id,
                operation,
                version,
                state,
                evidence,
            } => {
                if let Some(op) = self.operation(plan_id, operation)? {
                    if op.version == version + 1 && op.state == *state && op.evidence == *evidence {
                        for event in self.events(plan_id)? {
                            let v = &event["payload"];
                            if v["authority"]["attestation"] == json!(proof)
                                && v["operation"]["operation_id"] == *operation
                                && v["operation"]["version"] == version + 1
                            {
                                return Ok(Some(json!(op)));
                            }
                        }
                    }
                }
            }
            OperatorIntent::ConditionalGateVote {
                plan_id,
                task,
                record,
                ..
            } => {
                for event in self.events(plan_id)? {
                    let v = &event["payload"];
                    if v["task_id"] == *task
                        && v["attestation"] == json!(proof)
                        && v["actor"] == record.actor
                        && v["recorded_at"] == record.recorded_at
                    {
                        return Ok(Some(
                            json!({"recorded":true,"task":task,"conditional":true}),
                        ));
                    }
                }
            }
            OperatorIntent::GateVote {
                plan_id,
                task,
                record,
            } => {
                for event in self.events(plan_id)? {
                    let v = &event["payload"];
                    if v["task_id"] == *task
                        && v["attestation"] == json!(proof)
                        && v["actor"] == record.actor
                        && v["recorded_at"] == record.recorded_at
                    {
                        return Ok(Some(json!({"recorded":true,"task":task})));
                    }
                }
            }
        }
        Ok(None)
    }
}
