use super::{errors::ApiError, WebState};
use axum::{extract::State, http::HeaderMap, Json};
use macc_core::coordinator::decisions as dm;
use serde::Deserialize;
use serde_json::{json, Value};

#[derive(Deserialize)]
#[serde(tag = "action", rename_all = "snake_case", deny_unknown_fields)]
pub(super) enum Mutation {
    Configure {
        policy: dm::Policy,
    },
    Answer {
        id: String,
        version: u64,
        text: String,
        option: Option<String>,
        #[serde(default)]
        advisory: bool,
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
    },
    PatchPreview {
        patch: dm::Patch,
    },
    PatchApply {
        patch: dm::Patch,
    },
    PatchReconcile {
        patch: dm::Patch,
        source_digest: String,
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
    tokio::task::spawn_blocking(move||{
        let root=&state.paths.root;
        let actor=macc_core::service::coordinator_workflow::operator_identity(root);
        if actor.is_empty(){return Err(ApiError::validation("Configure a local operator identity before recording decisions"));}
        if let Mutation::Configure{policy}=action{return dm::configure(root,policy).map(|p|Json(json!(p))).map_err(ApiError::from);}
        let p=dm::policy(root)?;
        let(mut store,plan)=if matches!(action,Mutation::PatchPreview{..}|Mutation::PatchApply{..}|Mutation::PatchReconcile{..}|Mutation::Recover){dm::inspect_plan(root)?}else{dm::active_plan(root)?};
        if let Mutation::Answer{id,..}|Mutation::Revoke{id,..}=&action{store.decision_in_scope(&plan,id)?;}
        let value=match action {
            Mutation::Answer{id,version,text,option,advisory}=>json!(store.answer(&id,version,&text,option,&actor,advisory,&p)?),
            Mutation::Feedback{text,task,advisory}=>{
                let task=task.unwrap_or_else(||"*".into());
                let request:dm::Request=serde_json::from_value(json!({"idempotency_key":format!("feedback-{}",chrono::Utc::now().timestamp_nanos_opt().unwrap_or_default()),"task_id":task,"scope":if task=="*"{"project"}else{"task"},"kind":"feedback","question":"Operator guidance","context":text,"recommendation":text,"investigated":["Direct operator direction"],"impact":{"evidence":["Local operator"]}})).map_err(|e|ApiError::validation(e.to_string()))?;
                let mut human=p.clone();human.human_question_min_risk=dm::Consultation::Low;
                let d=store.submit(&plan,request,&actor,"human",&human)?;
                json!(store.answer(&d.id,d.version,&text,None,&actor,advisory,&p)?)
            }
            Mutation::Revoke{id,version,reason}=>json!(store.revoke(&id,version,&actor,&reason)?),
            Mutation::PatchPreview{patch}=>json!({"proposed":store.preview_patch(&plan,&patch,&dm::registry(root)?)?,"base":plan.graph_version,"projection":store.projection_diff(root,&plan)?}),
            Mutation::PatchApply{patch}=>{let applied=store.apply_patch(&plan.id,&patch,&dm::registry(root)?,&actor)?;store.project(root)?;json!(applied)},
            Mutation::PatchReconcile{patch,source_digest}=>{let applied=store.reconcile_projection(root,&patch,&source_digest,&actor)?;store.project(root)?;json!(applied)},
            Mutation::Recover=>{store.verify()?;dm::recover(root)?;json!({"recovered":true})},
            Mutation::Configure{..}=>unreachable!(),
        };Ok(Json(value))
    }).await.map_err(|e|ApiError::validation(e.to_string()))?
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
