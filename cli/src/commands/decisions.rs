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
        #[arg(long)]
        file: Option<PathBuf>,
        #[arg(long)]
        attestation_file: Option<PathBuf>,
        #[arg(long)]
        challenge: bool,
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
        #[arg(long)]
        apply_patch: bool,
        #[arg(long)]
        fields_file: Option<PathBuf>,
        #[arg(long)]
        attestation_file: Option<PathBuf>,
        #[arg(long)]
        challenge: bool,
        #[arg(long, default_value = "answer")]
        outcome: String,
    },
    Transport {
        #[command(subcommand)]
        action: TransportAction,
    },
    Investigate {
        id: String,
        #[arg(long)]
        version: u64,
        #[arg(long)]
        facts_file: PathBuf,
    },
    GateAttestation {
        #[arg(long)]
        task: String,
        #[arg(long)]
        record_file: PathBuf,
        #[arg(long)]
        attestation_file: Option<PathBuf>,
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
    Operations {
        #[command(subcommand)]
        action: OperationAction,
    },
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
pub enum TransportAction {
    Sign {
        #[arg(long)]
        key: PathBuf,
        #[arg(long)]
        principal: String,
        #[arg(long)]
        file: PathBuf,
        #[arg(long)]
        git_note: bool,
    },
    Import {
        #[arg(long)]
        file: Option<PathBuf>,
        #[arg(long)]
        git_note: Option<String>,
        #[arg(long)]
        allowed_signers: PathBuf,
        #[arg(long)]
        enroll_project: bool,
    },
}
#[derive(Subcommand)]
pub enum OperationAction {
    List,
    Execute {
        id: String,
    },
    Probe {
        operation: String,
        #[arg(long)]
        version: u64,
    },
    Reconcile {
        operation: String,
        #[arg(long)]
        version: u64,
        #[arg(long)]
        state: String,
        #[arg(long)]
        evidence: String,
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
        #[arg(long)]
        attestation_file: Option<PathBuf>,
    },
    Reconcile {
        file: PathBuf,
        #[arg(long)]
        source_digest: String,
        #[arg(long)]
        attestation_file: Option<PathBuf>,
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
    if let Action::Config {
        enabled,
        mode,
        file,
        attestation_file,
        challenge,
    } = action
    {
        let mut policy = dm::policy(root)?;
        if enabled.is_some() || mode.is_some() || file.is_some() {
            operator(root)?;
            let current = policy.clone();
            if let Some(file) = file {
                policy = read(file, 262144)?;
            }
            if let Some(enabled) = enabled {
                policy.enabled = *enabled;
            }
            if let Some(mode) = mode {
                policy.human_question_min_risk =
                    serde_json::from_value(json!(mode)).map_err(|_| {
                        dm::error("Mode must be low, medium, high, critical or never_ask")
                    })?;
            }
            if *challenge {
                return Ok(dm::policy_challenge(&current, &policy));
            }
            let proof = attestation_file
                .as_ref()
                .map(|file| read(file, 65536))
                .transpose()?;
            policy = dm::configure_attested(root, policy, proof.as_ref())?;
        }
        return Ok(json!(policy));
    }
    if matches!(action, Action::List) {
        return dm::snapshot(root);
    }
    let p = dm::policy(root)?;
    if let Action::Transport {
        action:
            TransportAction::Import {
                file,
                git_note,
                allowed_signers,
                enroll_project,
            },
    } = action
    {
        operator(root)?;
        let mut store = dm::Store::open(root)?;
        let bundle = match (file, git_note) {
            (Some(file), None) => read(file, 4 * 1024 * 1024)?,
            (None, Some(object)) => store.read_note(object)?,
            _ => {
                return Err(dm::error(
                    "Select exactly one signed file or Git note object",
                ))
            }
        };
        store.import_archive(&bundle, allowed_signers, *enroll_project)?;
        return Ok(json!({"imported":true,"project_id":store.project_id}));
    }
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
        Action::Transport { action } => {
            operator(root)?;
            match action {
                TransportAction::Sign {
                    key,
                    principal,
                    file,
                    git_note,
                } => {
                    let bundle = store.sign_archive(key, principal)?;
                    std::fs::write(
                        file,
                        serde_json::to_vec_pretty(&bundle).map_err(|e| dm::error(e.to_string()))?,
                    )
                    .map_err(|e| dm::error(e.to_string()))?;
                    let object = if *git_note {
                        Some(store.publish_note(&bundle)?)
                    } else {
                        None
                    };
                    Ok(
                        json!({"exported":file,"git_note_object":object,"notes_ref":"refs/notes/macc-decisions","network_sync":"explicit Git fetch/push required"}),
                    )
                }
                TransportAction::Import {
                    file,
                    git_note,
                    allowed_signers,
                    enroll_project,
                } => {
                    let bundle = match (file, git_note) {
                        (Some(file), None) => read(file, 4 * 1024 * 1024)?,
                        (None, Some(object)) => store.read_note(object)?,
                        _ => {
                            return Err(dm::error(
                                "Select exactly one signed file or Git note object",
                            ))
                        }
                    };
                    store.import_archive(&bundle, allowed_signers, *enroll_project)?;
                    Ok(json!({"imported":true,"project_id":store.project_id}))
                }
            }
        }
        Action::Operations { action } => match action {
            OperationAction::List => Ok(json!(store.operations(&plan.id)?)),
            OperationAction::Execute { id } => {
                store.decision_in_scope(&plan, id)?;
                if std::env::var_os("MACC_DECISION_ATTEMPT").is_some() {
                    let c = store.read_context(
                        &env("MACC_DECISION_PLAN")?,
                        &env("MACC_DECISION_ATTEMPT")?,
                        &env("MACC_DECISION_PHASE")?,
                    )?;
                    let d = store.decision(id)?;
                    if c.plan_id != d.plan_id
                        || c.task_id != d.request.task_id
                        || store
                            .context(&plan, &c.task_id, &c.attempt_id, &c.phase, &p)?
                            .context_digest
                            != c.context_digest
                    {
                        return Err(dm::error(
                            "Operation is outside the current pinned invocation",
                        ));
                    }
                } else {
                    operator(root)?;
                }
                Ok(json!(store.execute_operation(id)?))
            }
            OperationAction::Probe { operation, version } => {
                operator(root)?;
                Ok(json!(store.probe_operation(&plan.id, operation, *version)?))
            }
            OperationAction::Reconcile {
                operation,
                version,
                state,
                evidence,
            } => {
                let actor = operator(root)?;
                let state = serde_json::from_value(json!(state))
                    .map_err(|_| dm::error("State must be succeeded, failed or reserved"))?;
                Ok(json!(store.reconcile_operation(
                    &plan.id, operation, *version, state, evidence, &actor
                )?))
            }
        },
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
            apply_patch,
            fields_file,
            attestation_file,
            challenge,
            outcome,
        } => {
            let actor = operator(root)?;
            let fields = if let Some(path) = fields_file {
                read(path, p.max_request_bytes)?
            } else {
                Default::default()
            };
            let outcome = serde_json::from_value(json!(outcome))
                .map_err(|_| dm::error("Outcome must be answer, reject or request_changes"))?;
            let attestation = attestation_file
                .as_ref()
                .map(|path| read(path, p.max_request_bytes))
                .transpose()?;
            let input = dm::AnswerInput {
                attestation,
                text: text.clone(),
                option: option.clone(),
                advisory: *advisory,
                apply_patch: *apply_patch,
                fields,
                outcome,
            };
            if *challenge {
                return Ok(dm::answer_challenge(&store.decision(id)?, &input, &p));
            }
            return Ok(json!(
                store.answer_details(id, *version, &input, &actor, &p)?
            ));
        }
        Action::Investigate {
            id,
            version,
            facts_file,
        } => {
            let d = store.decision_in_scope(&plan, id)?;
            let actor = if std::env::var_os("MACC_DECISION_ATTEMPT").is_some() {
                let c = store.read_context(
                    &env("MACC_DECISION_PLAN")?,
                    &env("MACC_DECISION_ATTEMPT")?,
                    &env("MACC_DECISION_PHASE")?,
                )?;
                if c.plan_id != d.plan_id || c.task_id != d.request.task_id {
                    return Err(dm::error("Investigation is outside the pinned task"));
                }
                format!("agent:{}", c.task_id)
            } else {
                operator(root)?
            };
            let facts = read(facts_file, p.max_request_bytes)?;
            Ok(json!(store.investigate(id, *version, &facts, &actor, &p)?))
        }
        Action::GateAttestation {
            task,
            record_file,
            attestation_file,
        } => {
            operator(root)?;
            let record = read(record_file, p.max_request_bytes)?;
            let proof = attestation_file
                .as_ref()
                .map(|path| read(path, p.max_request_bytes))
                .transpose()?;
            if proof.is_none() {
                return dm::approval_challenge(&plan, task, &record, &p);
            }
            store.record_approval_attested(root, &plan, task, &record, proof.as_ref())?;
            Ok(json!({"recorded":true}))
        }
        Action::Feedback {
            text,
            task,
            advisory,
        } => {
            let actor = operator(root)?;
            Ok(json!(
                store.feedback(&plan, task, text, &actor, *advisory, &p)?
            ))
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
                | GraphAction::Apply { file, .. }
                | GraphAction::Reconcile { file, .. } => file,
                _ => unreachable!(),
            };
            let patch: dm::Patch = read(file, 262144)?;
            let registry = dm::registry(root)?;
            if matches!(action, GraphAction::Preview { .. }) {
                return store.graph_preview(&plan, &patch);
            }
            let actor = operator(root)?;
            let applied = if let GraphAction::Reconcile {
                source_digest,
                attestation_file,
                ..
            } = action
            {
                let proof = attestation_file
                    .as_ref()
                    .map(|file| read(file, p.max_request_bytes))
                    .transpose()?;
                store.reconcile_projection_attested(
                    root,
                    &patch,
                    source_digest,
                    &actor,
                    proof.as_ref(),
                )?
            } else {
                let proof = if let GraphAction::Apply {
                    attestation_file: Some(file),
                    ..
                } = action
                {
                    Some(read(file, p.max_request_bytes)?)
                } else {
                    None
                };
                store.apply_patch_attested(&plan.id, &patch, &registry, &actor, proof.as_ref())?
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
