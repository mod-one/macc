//! Hook commands share the same application operations as TUI and Web.
use super::{AppContext, Command};
use clap::Subcommand;
use macc_core::{
    hooks::{Action, Hook},
    Result,
};
use std::path::PathBuf;
#[derive(Debug, Clone, Subcommand)]
pub enum HooksCommand {
    /// Discover effective hooks, owners and adapter integration.
    List {
        #[arg(long)]
        json: bool,
    },
    Enable {
        id: String,
        #[arg(long)]
        revision: Option<String>,
    },
    Disable {
        id: String,
        #[arg(long)]
        revision: Option<String>,
    },
    /// Release the manual override, preserving feature requirements.
    Reset {
        id: String,
        #[arg(long)]
        revision: Option<String>,
    },
    /// Import a reviewed, local hook.json snapshot (no network or shell execution).
    Import {
        manifest: PathBuf,
        #[arg(long)]
        sha256: String,
        #[arg(long)]
        revision: Option<String>,
    },
    Remove {
        id: String,
        #[arg(long)]
        revision: Option<String>,
    },
    /// Print the canonical manifest checksum for review before import.
    Inspect { manifest: PathBuf },
}
pub struct Hooks(pub AppContext, pub HooksCommand);
fn read(path: &std::path::Path) -> Result<Hook> {
    use std::io::Read;
    let mut bytes = Vec::new();
    std::fs::File::open(path)
        .map_err(|e| {
            macc_core::MaccError::Validation(format!("Read hook manifest {}: {e}", path.display()))
        })?
        .take(262145)
        .read_to_end(&mut bytes)
        .map_err(|e| macc_core::MaccError::Validation(e.to_string()))?;
    if bytes.len() > 262144 {
        return Err(macc_core::MaccError::Validation(
            "Hook manifest exceeds 256 KiB".into(),
        ));
    }
    serde_json::from_slice(&bytes)
        .map_err(|e| macc_core::MaccError::Validation(format!("Invalid hook manifest: {e}")))
}
impl Command for Hooks {
    fn run(&self) -> Result<()> {
        if let HooksCommand::Inspect { manifest } = &self.1 {
            let hook = macc_core::hooks::review(read(manifest)?)?;
            println!("{}", serde_json::json!(hook));
            return Ok(());
        }
        let paths = self.0.project_paths()?;
        let current = macc_core::hooks::execute(&paths.root, Action::Status)?;
        let revision = |requested: &Option<String>| {
            requested
                .clone()
                .unwrap_or_else(|| current.revision.clone())
        };
        let action = match &self.1 {
            HooksCommand::List { .. } => Action::Status,
            HooksCommand::Enable { id, revision: r } => Action::Set {
                id: id.clone(),
                enabled: Some(true),
                revision: revision(r),
            },
            HooksCommand::Disable { id, revision: r } => Action::Set {
                id: id.clone(),
                enabled: Some(false),
                revision: revision(r),
            },
            HooksCommand::Reset { id, revision: r } => Action::Set {
                id: id.clone(),
                enabled: None,
                revision: revision(r),
            },
            HooksCommand::Import {
                manifest,
                sha256,
                revision: r,
            } => Action::Import {
                definition: read(manifest)?,
                sha256: sha256.clone(),
                revision: revision(r),
            },
            HooksCommand::Remove { id, revision: r } => Action::Remove {
                id: id.clone(),
                revision: revision(r),
            },
            HooksCommand::Inspect { .. } => unreachable!(),
        };
        if action.is_mutation() {
            super::gate_cli_mutation(&paths.root)?;
        }
        let view = macc_core::hooks::execute(&paths.root, action)?;
        if matches!(self.1, HooksCommand::List { json: true }) {
            println!(
                "{}",
                serde_json::to_string_pretty(&view)
                    .map_err(|e| macc_core::MaccError::Validation(e.to_string()))?
            );
        } else {
            println!("MACC Hooks · revision {}", view.revision);
            for h in view.hooks {
                println!(
                    "[{}] {}{}",
                    if h.enabled { "on" } else { "off" },
                    h.hook.id,
                    if h.required_by.is_empty() {
                        String::new()
                    } else {
                        format!(" · required by {}", h.required_by.join(", "))
                    }
                );
                println!(
                    "  {} · {:?} · {}",
                    h.hook.version, h.hook.event, h.integration
                );
            }
        }
        Ok(())
    }
}
