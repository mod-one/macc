use crate::commands::AppContext;
use crate::commands::Command;
use clap::{Subcommand, ValueEnum};
use macc_core::process_ownership::{
    ClientIdentity, ClientKind, OwnershipRecord, OwnershipStatus, ProcessHandle, ProcessKind,
};
use macc_core::{MaccError, Result};

pub struct ProcessCommand<'a> {
    app: AppContext,
    command: &'a ProcessCommands,
}

impl<'a> ProcessCommand<'a> {
    pub fn new(app: AppContext, command: &'a ProcessCommands) -> Self {
        Self { app, command }
    }
}

impl<'a> Command for ProcessCommand<'a> {
    fn run(&self) -> Result<()> {
        let paths = self.app.project_paths()?;
        match self.command {
            ProcessCommands::List => {
                let records = self.app.engine.process_list_running(&paths.root)?;
                print_records_table(&records);
                Ok(())
            }
            ProcessCommands::Ownership { kind, pid } => {
                let handle = build_handle(&paths.root, *kind, *pid);
                let record = self
                    .app
                    .engine
                    .process_ownership_status(&paths.root, &handle)?;
                let rendered = serde_json::to_string_pretty(&record).map_err(|e| {
                    MaccError::Validation(format!(
                        "Failed to serialize process ownership output: {}",
                        e
                    ))
                })?;
                println!("{}", rendered);
                Ok(())
            }
            ProcessCommands::Claim { kind, pid } => {
                let handle = build_handle(&paths.root, *kind, *pid);
                let identity = cli_identity();
                let (status, _owner_guard, _viewer_guard) = self
                    .app
                    .engine
                    .process_ownership_claim(&paths.root, handle, identity)?;
                println!("{}", format_status(status));
                Ok(())
            }
            ProcessCommands::Release {
                kind,
                pid,
                client_id,
            } => {
                let handle = build_handle(&paths.root, *kind, *pid);
                self.app
                    .engine
                    .process_ownership_release(&paths.root, &handle, client_id)?;
                println!(
                    "Released ownership for {} {}",
                    format_process_handle(*kind, *pid),
                    client_id
                );
                Ok(())
            }
            ProcessCommands::Takeover { takeover_command } => match takeover_command {
                TakeoverCommands::Request { kind, pid } => {
                    let handle = build_handle(&paths.root, *kind, *pid);
                    let identity = cli_identity();
                    let request_id = self.app.engine.process_ownership_request_takeover(
                        &paths.root,
                        &handle,
                        identity,
                    )?;
                    println!("Takeover requested. request_id: {request_id}");
                    Ok(())
                }
                TakeoverCommands::Accept {
                    kind,
                    pid,
                    owner_client_id,
                    request_id,
                } => {
                    let handle = build_handle(&paths.root, *kind, *pid);
                    self.app.engine.process_ownership_respond_takeover(
                        &paths.root,
                        &handle,
                        owner_client_id,
                        request_id,
                        true,
                    )?;
                    println!("Takeover accepted.");
                    Ok(())
                }
                TakeoverCommands::Reject {
                    kind,
                    pid,
                    owner_client_id,
                    request_id,
                } => {
                    let handle = build_handle(&paths.root, *kind, *pid);
                    self.app.engine.process_ownership_respond_takeover(
                        &paths.root,
                        &handle,
                        owner_client_id,
                        request_id,
                        false,
                    )?;
                    println!("Takeover rejected.");
                    Ok(())
                }
            },
        }
    }
}

#[derive(Subcommand, Debug)]
pub enum ProcessCommands {
    /// List tracked process ownership records
    List,
    /// Show the full ownership record for one process
    Ownership {
        #[arg(long, value_enum)]
        kind: ProcessKindArg,
        #[arg(long)]
        pid: i32,
    },
    /// Claim ownership for a process as this CLI client
    Claim {
        #[arg(long, value_enum)]
        kind: ProcessKindArg,
        #[arg(long)]
        pid: i32,
    },
    /// Release ownership for a specific client ID
    Release {
        #[arg(long, value_enum)]
        kind: ProcessKindArg,
        #[arg(long)]
        pid: i32,
        #[arg(long)]
        client_id: String,
    },
    /// Request, accept, or reject a process takeover.
    Takeover {
        #[command(subcommand)]
        takeover_command: TakeoverCommands,
    },
}

#[derive(Subcommand, Debug)]
pub enum TakeoverCommands {
    /// Request to become the owner of a process currently owned by another client.
    Request {
        #[arg(long, value_enum)]
        kind: ProcessKindArg,
        #[arg(long)]
        pid: i32,
    },
    /// Accept a pending takeover request (only the current owner may accept).
    Accept {
        #[arg(long, value_enum)]
        kind: ProcessKindArg,
        #[arg(long)]
        pid: i32,
        #[arg(long)]
        owner_client_id: String,
        #[arg(long)]
        request_id: String,
    },
    /// Reject a pending takeover request (only the current owner may reject).
    Reject {
        #[arg(long, value_enum)]
        kind: ProcessKindArg,
        #[arg(long)]
        pid: i32,
        #[arg(long)]
        owner_client_id: String,
        #[arg(long)]
        request_id: String,
    },
}

#[derive(ValueEnum, Clone, Copy, Debug, PartialEq, Eq)]
pub enum ProcessKindArg {
    Coordinator,
    Supervisor,
    WebServer,
    TerminalSession,
    Project,
}

impl From<ProcessKindArg> for ProcessKind {
    fn from(value: ProcessKindArg) -> Self {
        match value {
            ProcessKindArg::Coordinator => Self::Coordinator,
            ProcessKindArg::Supervisor => Self::Supervisor,
            ProcessKindArg::WebServer => Self::WebServer,
            ProcessKindArg::TerminalSession => Self::TerminalSession,
            ProcessKindArg::Project => Self::Project,
        }
    }
}

fn build_handle(project_root: &std::path::Path, kind: ProcessKindArg, pid: i32) -> ProcessHandle {
    ProcessHandle {
        kind: kind.into(),
        project_root: project_root.to_path_buf(),
        pid: Some(pid),
    }
}

fn cli_identity() -> ClientIdentity {
    let connected_at = chrono::Utc::now().to_rfc3339();
    ClientIdentity {
        client_id: format!("cli-{}", std::process::id()),
        kind: ClientKind::Cli,
        connected_at: connected_at.clone(),
        last_heartbeat: connected_at,
    }
}

fn format_status(status: OwnershipStatus) -> &'static str {
    match status {
        OwnershipStatus::Owner => "Owner",
        OwnershipStatus::Viewer => "Viewer",
        OwnershipStatus::Unregistered => "Unregistered",
    }
}

fn print_records_table(records: &[OwnershipRecord]) {
    println!(
        "{:<18} {:<8} {:<24} {:<7}",
        "PROCESS_KIND", "PID", "OWNER", "VIEWERS"
    );
    println!("{:-<18} {:-<8} {:-<24} {:-<7}", "", "", "", "");
    for record in records {
        let owner = record
            .owner
            .as_ref()
            .map(|owner| owner.client_id.as_str())
            .unwrap_or("-");
        let pid = record
            .process
            .pid
            .map(|pid| pid.to_string())
            .unwrap_or_else(|| "-".into());
        println!(
            "{:<18} {:<8} {:<24} {:<7}",
            format_process_kind(&record.process.kind),
            pid,
            truncate_cell(owner, 24),
            record.viewers.len()
        );
    }
}

fn format_process_kind(kind: &ProcessKind) -> &'static str {
    match kind {
        ProcessKind::Coordinator => "Coordinator",
        ProcessKind::Supervisor => "Supervisor",
        ProcessKind::WebServer => "WebServer",
        ProcessKind::TerminalSession => "TerminalSession",
        ProcessKind::Project => "Project",
    }
}

fn format_process_handle(kind: ProcessKindArg, pid: i32) -> String {
    format!("{}:{}", format_process_kind(&ProcessKind::from(kind)), pid)
}

fn truncate_cell(value: &str, width: usize) -> String {
    if value.chars().count() <= width {
        return value.to_string();
    }

    let truncated: String = value.chars().take(width.saturating_sub(1)).collect();
    format!("{}~", truncated)
}

#[cfg(test)]
mod tests {
    use super::{
        build_handle, cli_identity, format_status, print_records_table, ProcessCommands,
        ProcessKindArg,
    };
    use clap::Subcommand;
    use macc_core::process_ownership::{
        ClientIdentity, ClientKind, OwnershipRecord, OwnershipStatus, ProcessHandle, ProcessKind,
    };

    #[test]
    fn process_subcommands_are_registered() {
        assert!(ProcessCommands::has_subcommand("list"));
        assert!(ProcessCommands::has_subcommand("ownership"));
        assert!(ProcessCommands::has_subcommand("claim"));
        assert!(ProcessCommands::has_subcommand("release"));
    }

    #[test]
    fn build_handle_uses_project_root_and_pid() {
        let root = std::path::Path::new("/tmp/project");
        let handle = build_handle(root, ProcessKindArg::Coordinator, 1234);
        assert_eq!(
            handle,
            ProcessHandle {
                kind: ProcessKind::Coordinator,
                project_root: root.to_path_buf(),
                pid: Some(1234),
            }
        );
    }

    #[test]
    fn cli_identity_uses_cli_kind() {
        let identity = cli_identity();
        assert_eq!(identity.kind, ClientKind::Cli);
        assert!(identity.client_id.starts_with("cli-"));
        assert_eq!(identity.connected_at, identity.last_heartbeat);
    }

    #[test]
    fn format_status_matches_cli_output() {
        assert_eq!(format_status(OwnershipStatus::Owner), "Owner");
        assert_eq!(format_status(OwnershipStatus::Viewer), "Viewer");
        assert_eq!(format_status(OwnershipStatus::Unregistered), "Unregistered");
    }

    #[test]
    fn list_table_handles_empty_and_owned_records() {
        print_records_table(&[]);
        let record = OwnershipRecord {
            process: ProcessHandle {
                kind: ProcessKind::Supervisor,
                project_root: std::path::PathBuf::from("/tmp/project"),
                pid: Some(42),
            },
            owner: Some(ClientIdentity {
                client_id: "cli-42".into(),
                kind: ClientKind::Cli,
                connected_at: "2026-05-21T00:00:00Z".into(),
                last_heartbeat: "2026-05-21T00:00:00Z".into(),
            }),
            viewers: vec![ClientIdentity {
                client_id: "tui-9".into(),
                kind: ClientKind::Tui,
                connected_at: "2026-05-21T00:00:00Z".into(),
                last_heartbeat: "2026-05-21T00:00:00Z".into(),
            }],
            takeover_request: None,
            started_at: "2026-05-21T00:00:00Z".into(),
        };
        print_records_table(&[record]);
    }
}
