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
    Usage,
    UsageBegin,
    CompactionOptions,
    McpOptions,
    UsageEnd,
    UsageUpgrade {
        #[arg(long)]
        apply: bool,
        #[arg(long)]
        expected_sha256: Option<String>,
    },
    MergeRepairPrepare,
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
    json: bool,
}
impl SessionStoreCommand {
    pub fn new(app: AppContext, command: SessionStoreCommands, json: bool) -> Self {
        Self { app, command, json }
    }
}
fn input<T: DeserializeOwned>() -> Result<T> {
    let mut bytes = Vec::new();
    std::io::stdin()
        .take(32 * 1024 * 1024 + 1)
        .read_to_end(&mut bytes)
        .map_err(|e| MaccError::Validation(e.to_string()))?;
    if bytes.len() > 32 * 1024 * 1024 {
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
        let out = |kind: &str, val: serde_json::Value| {
            super::o2_format::emit_session_store(kind, val, self.json)
        };
        match &self.command {
            SessionStoreCommands::Usage => {
                super::o2_format::emit_session_store("usage", store::usage::inspect(root)?, self.json)
            }
            SessionStoreCommands::CompactionOptions => emit(
                macc_core::o2::compaction::execution_options(root, &input()?)?,
            ),
            SessionStoreCommands::UsageBegin => emit(store::usage::begin(root, &input()?)?),
            SessionStoreCommands::UsageEnd => emit(store::usage::end(root, &input()?)?),
            SessionStoreCommands::UsageUpgrade {
                apply,
                expected_sha256,
            } => {
                if *apply {
                    super::gate_cli_mutation(root)?;
                }
                super::o2_format::emit_session_store(
                    "usage_upgrade",
                    store::usage::upgrade(root, *apply, expected_sha256.as_deref())?,
                    self.json,
                )
            }
            SessionStoreCommands::McpOptions => {
                emit(macc_core::o2::agent::options(root, &input()?)?)
            }
            SessionStoreCommands::Status => {
                super::o2_format::emit_session_store("status", store::inspect(root)?, self.json)
            }
            SessionStoreCommands::Backup => {
                super::gate_cli_mutation(root)?;
                super::o2_format::emit_session_store("backup", store::backup(root)?, self.json)
            }
            SessionStoreCommands::Events { id } => {
                super::o2_format::emit_session_store("events", store::events(root, id)?, self.json)
            }
            SessionStoreCommands::Migrate {
                apply,
                expected_sha256,
            } => {
                if *apply {
                    super::gate_cli_mutation(root)?;
                }
                super::o2_format::emit_session_store(
                    "migrate",
                    store::migrate(root, *apply, expected_sha256.as_deref())?,
                    self.json,
                )
            }
            SessionStoreCommands::Rollback {
                apply,
                expected_sha256,
            } => {
                if *apply {
                    super::gate_cli_mutation(root)?;
                }
                super::o2_format::emit_session_store(
                    "rollback",
                    store::rollback(root, *apply, expected_sha256.as_deref())?,
                    self.json,
                )
            }
            SessionStoreCommands::MergeRepairPrepare => {
                emit(store::checkpoints::merge_repair::capture(root, &input()?)?)
            }
            SessionStoreCommands::CheckpointPrepare => {
                emit(store::checkpoints::prepare(root, &input()?)?)
            }
            SessionStoreCommands::CheckpointFinish => {
                emit(store::checkpoints::finish(root, &input()?)?)
            }
            SessionStoreCommands::Checkpoint { id } => super::o2_format::emit_session_store(
                "checkpoint",
                store::checkpoints::get(root, id)?,
                self.json,
            ),
            SessionStoreCommands::Reconcile {
                id,
                expected_sha256,
            } => {
                if expected_sha256.is_some() {
                    super::gate_cli_mutation(root)?;
                }
                out(
                    "reconcile",
                    store::checkpoints::reconcile(root, id, expected_sha256.as_deref())?,
                )
            }
            SessionStoreCommands::Upgrade {
                apply,
                expected_sha256,
            } => {
                if *apply {
                    super::gate_cli_mutation(root)?;
                }
                super::o2_format::emit_session_store(
                    "upgrade",
                    store::checkpoints::upgrade(root, *apply, expected_sha256.as_deref())?,
                    self.json,
                )
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
                super::o2_format::emit_session_store("recover", store::recover(root)?, self.json)
            }
            SessionStoreCommands::Close { id } => {
                super::gate_cli_mutation(root)?;
                store::close(root, id)?;
                out("close", serde_json::json!({"ok":true}))
            }
            SessionStoreCommands::Policy { revision } => {
                super::gate_cli_mutation(root)?;
                let policy: Option<store::SessionPolicy> = input()?;
                super::o2_format::emit_session_store(
                    "policy",
                    match policy {
                        Some(p) => store::update_policy(root, revision, &p)?,
                        None => store::remove_policy_override(root, revision)?,
                    },
                    self.json,
                )
            }
        }
    }
}
