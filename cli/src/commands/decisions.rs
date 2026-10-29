use super::AppContext;
use clap::Subcommand;
use macc_core::{coordinator::decisions as dm, Result};
use serde_json::{json, Value};
use std::path::{Path, PathBuf};

#[derive(Subcommand)]
pub enum Action {
    /// Show current policy, pending requests and durable resolutions (JSON)
    List,
    Show {
        id: String,
    },
    /// Configure opt-in decision management and the minimum question risk
    Config {
        #[arg(long)]
        enabled: Option<bool>,
        #[arg(long)]
        mode: Option<String>,
    },
    /// Submit an investigated question/recommendation from the current task
    Request {
        #[arg(long)]
        file: PathBuf,
    },
    /// Answer a current request, preserving the exact human text
    Answer {
        id: String,
        #[arg(long)]
        version: u64,
        #[arg(long)]
        text: String,
        #[arg(long)]
        option: Option<String>,
        #[arg(long)]
        advisory: bool,
    },
    /// Record new scoped human direction without a pending gate
    Feedback {
        #[arg(long)]
        text: String,
        #[arg(long, default_value = "*")]
        task: String,
        #[arg(long)]
        advisory: bool,
    },
    Revoke {
        id: String,
        #[arg(long)]
        version: u64,
        #[arg(long)]
        reason: String,
    },
    /// Reserve the exact authorized operation; retry only with verified idempotency
    Authorize {
        id: String,
        #[arg(long)]
        operation: String,
    },
    /// Inspect exact contexts issued to a task, independently of model acknowledgement
    TaskContext {
        task: String,
    },
    History,
    Verify,
    Recover,
    Graph {
        #[command(subcommand)]
        action: GraphAction,
    },
    /// Print the pinned context manifest or its complete rendered instructions
    Context {
        #[arg(long)]
        render: bool,
    },
    /// Trusted wrapper: verify the actual prompt includes the pinned context
    Receipt {
        #[arg(long)]
        prompt_file: PathBuf,
    },
    /// Trusted wrapper: verify context freshness, acknowledgement and compliance
    Complete {
        #[arg(long)]
        output_file: PathBuf,
    },
    Export {
        #[arg(long)]
        file: PathBuf,
    },
}
#[derive(Subcommand)]
pub enum GraphAction {
    Show,
    Preview {
        file: PathBuf,
    },
    Apply {
        file: PathBuf,
    },
    Reconcile {
        file: PathBuf,
        #[arg(long)]
        source_digest: String,
    },
}

fn read<T: serde::de::DeserializeOwned>(file: &Path, limit: usize) -> Result<T> {
    let metadata = std::fs::metadata(file).map_err(|e| dm::error(e.to_string()))?;
    if metadata.len() > limit as u64 {
        return Err(dm::error("Input exceeds the decision budget"));
    }
    serde_json::from_slice(&std::fs::read(file).map_err(|e| dm::error(e.to_string()))?)
        .map_err(|e| dm::error(e.to_string()))
}
fn env(name: &str) -> Result<String> {
    std::env::var(name).map_err(|_| {
        dm::error(format!(
            "Missing inherited {name}; this command requires a pinned task invocation"
        ))
    })
}
fn operator(root: &Path) -> Result<String> {
    if std::env::var_os("MACC_DECISION_ATTEMPT").is_some() {
        return Err(dm::error(
            "Agent requests cannot masquerade as human answers or change autonomy policy",
        ));
    }
    super::gate_cli_mutation(root)?;
    let actor = macc_core::service::coordinator_workflow::operator_identity(root);
    if actor.is_empty() {
        return Err(dm::error(
            "Configure Git user.name/user.email to attribute local human decisions",
        ));
    }
    Ok(actor)
}
pub fn run(app: &AppContext, action: &Action) -> Result<()> {
    let root = std::env::var_os("MACC_DECISION_ROOT")
        .map(PathBuf::from)
        .unwrap_or(app.project_paths()?.root);
    let output = execute(&root, action)?;
    println!(
        "{}",
        serde_json::to_string_pretty(&output).map_err(|e| dm::error(e.to_string()))?
    );
    Ok(())
}
fn execute(root: &Path, action: &Action) -> Result<Value> {
    if let Action::Config { enabled, mode } = action {
        let mut policy = dm::policy(root)?;
        if enabled.is_some() || mode.is_some() {
            operator(root)?;
            if let Some(enabled) = enabled {
                policy.enabled = *enabled;
            }
            if let Some(mode) = mode {
                policy.human_question_min_risk =
                    serde_json::from_value(json!(mode)).map_err(|_| {
                        dm::error("Mode must be low, medium, high, critical or never_ask")
                    })?;
            }
            policy = dm::configure(root, policy)?;
        }
        return Ok(json!(policy));
    }
    if matches!(action, Action::List) {
        return dm::snapshot(root);
    }
    let p = dm::policy(root)?;
    let (mut store, plan) = if matches!(
        action,
        Action::Show { .. }
            | Action::TaskContext { .. }
            | Action::History
            | Action::Verify
            | Action::Recover
            | Action::Export { .. }
            | Action::Graph { .. }
    ) {
        dm::inspect_plan(root)?
    } else {
        dm::active_plan(root)?
    };
    if let Action::Show { id }
    | Action::Answer { id, .. }
    | Action::Revoke { id, .. }
    | Action::Authorize { id, .. } = action
    {
        store.decision_in_scope(&plan, id)?;
    }
    match action {
        Action::Authorize { id, operation } => {
            let decision = store.decision(id)?;
            if std::env::var_os("MACC_DECISION_ATTEMPT").is_some() {
                let context = store.read_context(
                    &env("MACC_DECISION_PLAN")?,
                    &env("MACC_DECISION_ATTEMPT")?,
                    &env("MACC_DECISION_PHASE")?,
                )?;
                if context.plan_id != decision.plan_id
                    || context.task_id != decision.request.task_id
                {
                    return Err(dm::error(
                        "Authorization is outside this invocation's scope",
                    ));
                }
                let current = store.context(
                    &plan,
                    &context.task_id,
                    &context.attempt_id,
                    &context.phase,
                    &p,
                )?;
                if current.context_digest != context.context_digest {
                    return Err(dm::error(
                        "Refresh the invocation before consuming an authorization",
                    ));
                }
            } else {
                operator(root)?;
            }
            store.consume_authorization(id, operation)?;
            Ok(
                json!({"reserved":true,"decision_id":id,"operation_id":operation,"retry_requires_verified_idempotency":true}),
            )
        }
        Action::Show { id } => Ok(json!(store.decision(id)?)),
        Action::TaskContext { task } => Ok(json!(store.context_history(&plan.id, task)?)),
        Action::History => Ok(json!(store.scoped_history(&plan)?)),
        Action::Verify => {
            store.verify()?;
            Ok(json!({"verified":true,"project_id":store.project_id,"plan":plan.id}))
        }
        Action::Recover => {
            operator(root)?;
            dm::recover(root)?;
            Ok(json!({"recovered":true,"plan":store.plan(&plan.id)?}))
        }
        Action::Request { file } => {
            let mut request: dm::Request = read(file, p.max_request_bytes)?;
            let (actor, origin) = if std::env::var_os("MACC_DECISION_ATTEMPT").is_some() {
                let pinned = store.read_context(
                    &env("MACC_DECISION_PLAN")?,
                    &env("MACC_DECISION_ATTEMPT")?,
                    &env("MACC_DECISION_PHASE")?,
                )?;
                if pinned.plan_id != plan.id || pinned.task_id != request.task_id {
                    return Err(dm::error("Request does not match its pinned task/plan"));
                }
                let current = store.context(
                    &plan,
                    &pinned.task_id,
                    &pinned.attempt_id,
                    &pinned.phase,
                    &p,
                )?;
                if current.contract_digest != pinned.contract_digest {
                    return Err(dm::error(
                        "Task contract changed; request needs a fresh attempt",
                    ));
                }
                request.attempt_id = pinned.attempt_id.clone();
                (
                    format!("agent:{}:{}", request.task_id, pinned.phase),
                    "agent",
                )
            } else {
                (operator(root)?, "human")
            };
            Ok(json!(store.submit(&plan, request, &actor, origin, &p)?))
        }
        Action::Answer {
            id,
            version,
            text,
            option,
            advisory,
        } => {
            let actor = operator(root)?;
            Ok(json!(store.answer(
                id,
                *version,
                text,
                option.clone(),
                &actor,
                *advisory,
                &p
            )?))
        }
        Action::Feedback {
            text,
            task,
            advisory,
        } => {
            let actor = operator(root)?;
            let request:dm::Request=serde_json::from_value(json!({"idempotency_key":uuid_key(),"task_id":task,"kind":"feedback","scope":if task=="*"{"project"}else{"task"},"question":"New operator direction","recommendation":text,"context":text,"investigated":["Direct operator instruction"],"risk":"high","impact":{"evidence":["Operator input"]}})).map_err(|e|dm::error(e.to_string()))?;
            let mut human = p.clone();
            human.human_question_min_risk = dm::Consultation::Low;
            let decision = store.submit(&plan, request, &actor, "human", &human)?;
            Ok(json!(store.answer(
                &decision.id,
                decision.version,
                text,
                None,
                &actor,
                *advisory,
                &p
            )?))
        }
        Action::Revoke {
            id,
            version,
            reason,
        } => Ok(json!(store.revoke(
            id,
            *version,
            &operator(root)?,
            reason
        )?)),
        Action::Graph { action } => {
            if matches!(action, GraphAction::Show) {
                return Ok(json!(plan));
            }
            let file = match action {
                GraphAction::Preview { file }
                | GraphAction::Apply { file }
                | GraphAction::Reconcile { file, .. } => file,
                _ => unreachable!(),
            };
            let patch: dm::Patch = read(file, 262144)?;
            let registry = dm::registry(root)?;
            if matches!(action, GraphAction::Preview { .. }) {
                return Ok(
                    json!({"base":plan.graph_version,"projection":store.projection_diff(root,&plan)?,"proposed":store.preview_patch(&plan,&patch,&registry)?}),
                );
            }
            let actor = operator(root)?;
            let applied = if let GraphAction::Reconcile { source_digest, .. } = action {
                store.reconcile_projection(root, &patch, source_digest, &actor)?
            } else {
                store.apply_patch(&plan.id, &patch, &registry, &actor)?
            };
            store.project(root)?;
            Ok(json!(applied))
        }
        Action::Context { render } => {
            let context = store.read_context(
                &env("MACC_DECISION_PLAN")?,
                &env("MACC_DECISION_ATTEMPT")?,
                &env("MACC_DECISION_PHASE")?,
            )?;
            if *render {
                Ok(json!({"rendered":context.render(),"context_digest":context.context_digest}))
            } else {
                Ok(json!(context))
            }
        }
        Action::Receipt { prompt_file } => {
            let prompt =
                std::fs::read_to_string(prompt_file).map_err(|e| dm::error(e.to_string()))?;
            Ok(json!(store.issued(
                &env("MACC_DECISION_PLAN")?,
                &env("MACC_DECISION_ATTEMPT")?,
                &env("MACC_DECISION_PHASE")?,
                &prompt,
                &p
            )?))
        }
        Action::Complete { output_file } => {
            let output =
                std::fs::read_to_string(output_file).map_err(|e| dm::error(e.to_string()))?;
            Ok(json!(store.complete(
                &env("MACC_DECISION_PLAN")?,
                &env("MACC_DECISION_ATTEMPT")?,
                &env("MACC_DECISION_PHASE")?,
                &output,
                &p
            )?))
        }
        Action::Export { file } => {
            let bundle = json!({"schema_version":1,"trust":"local_trust_evidence_only","project_id":store.project_id,"plans":store.plans()?,"decisions":store.decisions(&plan.id)?,"events":store.events(&plan.id)?});
            std::fs::write(
                file,
                serde_json::to_vec_pretty(&bundle).map_err(|e| dm::error(e.to_string()))?,
            )
            .map_err(|e| dm::error(e.to_string()))?;
            Ok(json!({"exported":file,"digest":dm::digest(&bundle)}))
        }
        Action::Config { .. } | Action::List => unreachable!(),
    }
}
fn uuid_key() -> String {
    format!(
        "operator-feedback-{}",
        chrono::Utc::now().timestamp_nanos_opt().unwrap_or_default()
    )
}
