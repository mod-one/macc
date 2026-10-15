//! CLI transport for the common ownership service; no provider-specific syntax.
use super::{AppContext, Command};
use macc_core::{session_store as store, MaccError, Result};
use serde::de::DeserializeOwned;
use std::io::Read;
#[derive(clap::Subcommand, Clone, Debug)]
pub enum SessionStoreCommands {
    Status,
    Backup,
    Events {
        id: String,
    },
    Migrate {
        #[arg(long)]
        apply: bool,
        #[arg(long)]
        expected_sha256: Option<String>,
    },
    Rollback {
        #[arg(long)]
        apply: bool,
        #[arg(long)]
        expected_sha256: Option<String>,
    },
    CheckpointPrepare,
    CheckpointFinish,
    Checkpoint {
        id: String,
    },
    Reconcile {
        id: String,
        #[arg(long)]
        expected_sha256: Option<String>,
    },
    Upgrade {
        #[arg(long)]
        apply: bool,
        #[arg(long)]
        expected_sha256: Option<String>,
    },
    Acquire,
    Bind,
    Heartbeat,
    Release,
    Recover,
    Close {
        id: String,
    },
    Policy {
        #[arg(long)]
        revision: String,
    },
}
pub struct SessionStoreCommand {
    app: AppContext,
    command: SessionStoreCommands,
}
impl SessionStoreCommand {
    pub fn new(app: AppContext, command: SessionStoreCommands) -> Self {
        Self { app, command }
    }
}
fn input<T: DeserializeOwned>() -> Result<T> {
    let mut bytes = Vec::new();
    std::io::stdin()
        .take(1024 * 1024 + 1)
        .read_to_end(&mut bytes)
        .map_err(|e| MaccError::Validation(e.to_string()))?;
    if bytes.len() > 1024 * 1024 {
        return Err(MaccError::Validation("Session request too large".into()));
    }
    serde_json::from_slice(&bytes)
        .map_err(|e| MaccError::Validation(format!("Invalid session request: {e}")))
}
fn emit(value: impl serde::Serialize) -> Result<()> {
    println!(
        "{}",
        serde_json::to_string(&value).map_err(|e| MaccError::Validation(e.to_string()))?
    );
    Ok(())
}
#[derive(serde::Deserialize)]
struct Binding {
    #[serde(flatten)]
    key: store::LeaseKey,
    provider_session_id: String,
}
#[derive(serde::Deserialize)]
struct Release {
    #[serde(flatten)]
    key: store::LeaseKey,
    clean: bool,
}
impl Command for SessionStoreCommand {
    fn run(&self) -> Result<()> {
        let root = &self.app.project_paths()?.root;
        match &self.command {
            SessionStoreCommands::Status => emit(store::inspect(root)?),
            SessionStoreCommands::Backup => {
                super::gate_cli_mutation(root)?;
                emit(store::backup(root)?)
            }
            SessionStoreCommands::Events { id } => emit(store::events(root, id)?),
            SessionStoreCommands::Migrate {
                apply,
                expected_sha256,
            } => {
                if *apply {
                    super::gate_cli_mutation(root)?;
                }
                emit(store::migrate(root, *apply, expected_sha256.as_deref())?)
            }
            SessionStoreCommands::Rollback {
                apply,
                expected_sha256,
            } => {
                if *apply {
                    super::gate_cli_mutation(root)?;
                }
                emit(store::rollback(root, *apply, expected_sha256.as_deref())?)
            }
            SessionStoreCommands::CheckpointPrepare => {
                emit(store::checkpoints::prepare(root, &input()?)?)
            }
            SessionStoreCommands::CheckpointFinish => {
                emit(store::checkpoints::finish(root, &input()?)?)
            }
            SessionStoreCommands::Checkpoint { id } => emit(store::checkpoints::get(root, id)?),
            SessionStoreCommands::Reconcile {
                id,
                expected_sha256,
            } => {
                if expected_sha256.is_some() {
                    super::gate_cli_mutation(root)?;
                }
                emit(store::checkpoints::reconcile(
                    root,
                    id,
                    expected_sha256.as_deref(),
                )?)
            }
            SessionStoreCommands::Upgrade {
                apply,
                expected_sha256,
            } => {
                if *apply {
                    super::gate_cli_mutation(root)?;
                }
                emit(store::checkpoints::upgrade(
                    root,
                    *apply,
                    expected_sha256.as_deref(),
                )?)
            }
            SessionStoreCommands::Acquire => emit(store::acquire(root, &input()?)?),
            SessionStoreCommands::Bind => {
                let r: Binding = input()?;
                store::bind(root, &r.key, &r.provider_session_id)?;
                emit(serde_json::json!({"ok":true}))
            }
            SessionStoreCommands::Heartbeat => {
                store::heartbeat(root, &input()?)?;
                emit(serde_json::json!({"ok":true}))
            }
            SessionStoreCommands::Release => {
                let r: Release = input()?;
                store::release(root, &r.key, r.clean)?;
                emit(serde_json::json!({"ok":true}))
            }
            SessionStoreCommands::Recover => {
                super::gate_cli_mutation(root)?;
                emit(store::recover(root)?)
            }
            SessionStoreCommands::Close { id } => {
                super::gate_cli_mutation(root)?;
                store::close(root, id)?;
                emit(serde_json::json!({"ok":true}))
            }
            SessionStoreCommands::Policy { revision } => {
                super::gate_cli_mutation(root)?;
                let policy: Option<store::SessionPolicy> = input()?;
                emit(match policy {
                    Some(p) => store::update_policy(root, revision, &p)?,
                    None => store::remove_policy_override(root, revision)?,
                })
            }
        }
    }
}
