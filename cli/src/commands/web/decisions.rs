use super::{errors::ApiError, WebState};
use axum::{extract::State, http::HeaderMap, Json};
use macc_core::coordinator::decisions as dm;
use serde::Deserialize;
use serde_json::{json, Value};

#[derive(Deserialize)]
#[serde(tag = "action", rename_all = "snake_case", deny_unknown_fields)]
pub(super) enum Mutation {
    Configure {
        #[serde(default)]
        attestation: Option<dm::Attestation>,
        #[serde(default)]
        challenge: bool,
        policy: dm::Policy,
    },
    Answer {
        id: String,
        version: u64,
        text: String,
        option: Option<String>,
        #[serde(default)]
        advisory: bool,
        #[serde(default)]
        apply_patch: bool,
        #[serde(default)]
        fields: std::collections::BTreeMap<String, String>,
        #[serde(default)]
        outcome: dm::AnswerOutcome,
        #[serde(default)]
        effects: dm::AnswerEffects,
        #[serde(default)]
        patch: Option<dm::Patch>,
        #[serde(default)]
        attestation: Option<dm::Attestation>,
        #[serde(default)]
        challenge: bool,
    },
    Feedback {
        text: String,
        #[serde(default)]
        task: Option<String>,
        #[serde(default)]
        advisory: bool,
    },
    Revoke {
        id: String,
        version: u64,
        reason: String,
        #[serde(default)]
        attestation: Option<dm::Attestation>,
    },
    PatchPreview {
        patch: dm::Patch,
    },
    PatchApply {
        #[serde(default)]
        attestation: Option<dm::Attestation>,
        patch: dm::Patch,
    },
    PatchReconcile {
        #[serde(default)]
        attestation: Option<dm::Attestation>,
        patch: dm::Patch,
        source_digest: String,
    },
    OperationExecute {
        id: String,
    },
    OperationProbe {
        operation: String,
        version: u64,
    },
    OperationReconcile {
        operation: String,
        version: u64,
        state: dm::OperationState,
        evidence: String,
        #[serde(default)]
        attestation: Option<dm::Attestation>,
    },
    Operator {
        intent: dm::OperatorIntent,
        #[serde(default)]
        attestation: Option<dm::Attestation>,
        #[serde(default)]
        challenge: bool,
    },
    ApprovalBegin {
        intent: dm::OperatorIntent,
    },
    ApprovalPoll {
        id: String,
    },
    ApprovalApply {
        id: String,
    },
    Recover,
}
pub(super) async fn snapshot(State(state): State<WebState>) -> Result<Json<Value>, ApiError> {
    tokio::task::spawn_blocking(move || {
        dm::snapshot(&state.paths.root)
            .map(Json)
            .map_err(ApiError::from)
    })
    .await
    .map_err(|e| ApiError::validation(e.to_string()))?
}
pub(super) async fn mutate(
    State(state): State<WebState>,
    headers: HeaderMap,
    Json(action): Json<Mutation>,
) -> Result<Json<Value>, ApiError> {
    super::mutation_gate::require_project_owner(&state, &headers)?;
    tokio::task::spawn_blocking(move || {
        let root = &state.paths.root;
        let actor = macc_core::service::coordinator_workflow::operator_identity(root);
        if actor.is_empty() {
            return Err(ApiError::validation(
                "Configure a local operator identity before recording decisions",
            ));
        }
        if let Mutation::Configure {
            policy,
            attestation,
            challenge,
        } = action
        {
            if challenge {
                return Ok(Json(dm::policy_challenge(&dm::policy(root)?, &policy)));
            }
            return dm::configure_attested(root, policy, attestation.as_ref())
                .map(|p| Json(json!(p)))
                .map_err(ApiError::from);
        }
        let p = dm::policy(root)?;
        let (mut store, plan) = if matches!(
            action,
            Mutation::PatchPreview { .. }
                | Mutation::PatchApply { .. }
                | Mutation::PatchReconcile { .. }
                | Mutation::Recover
                | Mutation::Operator { .. }
                | Mutation::ApprovalBegin { .. }
                | Mutation::ApprovalPoll { .. }
                | Mutation::ApprovalApply { .. }
        ) {
            dm::inspect_plan(root)?
        } else {
            dm::active_plan(root)?
        };
        if let Mutation::Answer { id, .. } | Mutation::Revoke { id, .. } = &action {
            store.decision_in_scope(&plan, id)?;
        }
        let value = match action {
            Mutation::Operator {
                intent,
                attestation,
                challenge,
            } => {
                store.operator_in_scope(&plan, &intent)?;
                if challenge {
                    store.operator_challenge(&intent)?
                } else {
                    store.apply_operator(&intent, attestation.as_ref(), &actor)?
                }
            }
            Mutation::ApprovalBegin { intent } => {
                store.operator_in_scope(&plan, &intent)?;
                json!(store.begin_approval(intent, &actor)?)
            }
            Mutation::ApprovalPoll { id } => {
                store.operator_in_scope(&plan, &store.approval_ticket(&id)?.intent)?;
                json!(store.poll_approval(&id, &actor)?)
            }
            Mutation::ApprovalApply { id } => {
                store.operator_in_scope(&plan, &store.approval_ticket(&id)?.intent)?;
                json!(store.apply_approval(&id, &actor)?)
            }
            Mutation::Answer {
                id,
                version,
                text,
                option,
                advisory,
                apply_patch,
                fields,
                outcome,
                effects,
                patch,
                attestation,
                challenge,
            } => {
                let input = dm::AnswerInput {
                    effects,
                    patch,
                    attestation,
                    text,
                    option,
                    advisory,
                    apply_patch,
                    fields,
                    outcome,
                };
                if challenge {
                    dm::answer_challenge(&store.decision(&id)?, &input, &p)
                } else {
                    json!(store.answer_details(&id, version, &input, &actor, &p)?)
                }
            }
            Mutation::Feedback {
                text,
                task,
                advisory,
            } => {
                let task = task.unwrap_or_else(|| "*".into());
                json!(store.feedback(&plan, &task, &text, &actor, advisory, &p)?)
            }
            Mutation::Revoke {
                id,
                version,
                reason,
                attestation,
            } => {
                json!(store.revoke_attested(&id, version, &actor, &reason, attestation.as_ref())?)
            }
            Mutation::PatchPreview { patch } => store.graph_preview(&plan, &patch)?,
            Mutation::PatchApply { patch, attestation } => {
                let applied = store.apply_patch_attested(
                    &plan.id,
                    &patch,
                    &dm::registry(root)?,
                    &actor,
                    attestation.as_ref(),
                )?;
                store.project(root)?;
                json!(applied)
            }
            Mutation::PatchReconcile {
                patch,
                source_digest,
                attestation,
            } => {
                let applied = store.reconcile_projection_attested(
                    root,
                    &patch,
                    &source_digest,
                    &actor,
                    attestation.as_ref(),
                )?;
                store.project(root)?;
                json!(applied)
            }
            Mutation::OperationExecute { id } => {
                store.decision_in_scope(&plan, &id)?;
                json!(store.execute_operation(&id)?)
            }
            Mutation::OperationProbe { operation, version } => {
                json!(store.probe_operation(&plan.id, &operation, version)?)
            }
            Mutation::OperationReconcile {
                operation,
                version,
                state,
                evidence,
                attestation,
            } => json!(store.reconcile_operation_attested(
                &plan.id,
                &operation,
                version,
                state,
                &evidence,
                &actor,
                attestation.as_ref()
            )?),
            Mutation::Recover => {
                store.verify()?;
                dm::recover(root)?;
                json!({"recovered":true})
            }
            Mutation::Configure { .. } => unreachable!(),
        };
        Ok(Json(value))
    })
    .await
    .map_err(|e| ApiError::validation(e.to_string()))?
}

pub(super) async fn contexts(
    State(state): State<WebState>,
    axum::extract::Path(task): axum::extract::Path<String>,
) -> Result<Json<Value>, ApiError> {
    tokio::task::spawn_blocking(move || {
        let (store, plan) = dm::inspect_plan(&state.paths.root)?;
        Ok(Json(json!(store.context_history(&plan.id, &task)?)))
    })
    .await
    .map_err(|e| ApiError::validation(e.to_string()))?
}
