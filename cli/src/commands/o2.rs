//! Thin command transport; every operation is shared with both interactive clients.
pub use super::o2_parser::O2Command;
use super::{AppContext, Command};
use clap::Subcommand;
use macc_core::{
    o2::{
        actions::{execute, Action},
        memory,
    },
    MaccError, Result,
};
use std::io::Read;
#[derive(Subcommand, Clone, Debug)]
pub enum ExistingCommand {
    Status,
    /// Serve scoped O2 MCP over stdio using runner-provided capability environment.
    Mcp,
    AgentStatus,
    SemanticStatus {
        #[arg(long)]
        task: Option<String>,
        #[arg(long)]
        tool: Option<String>,
        #[arg(long)]
        worktree: Option<String>,
        #[arg(long)]
        feature: Option<String>,
        #[arg(long)]
        user: Option<String>,
    },
    SemanticBenchmark,
    SemanticGate {
        report_id: String,
        checksum: String,
        #[arg(long)]
        revoke: bool,
        #[arg(long)]
        confirm: bool,
    },
    MemoryExtract {
        checkpoint_id: String,
    },
    Upgrade {
        #[arg(long)]
        apply: bool,
        #[arg(long)]
        expected_sha256: Option<String>,
    },
    /// Execute a typed O2 operation from bounded JSON on stdin (all client operations).
    Action,
    MemorySearch {
        #[arg(default_value = "")]
        query: String,
        #[arg(long)]
        task: Option<String>,
        #[arg(long)]
        tool: Option<String>,
        #[arg(long)]
        worktree: Option<String>,
        #[arg(long)]
        all: bool,
        #[arg(long)]
        full: bool,
        /// Explain scoped exclusions without changing facts; operator output is not token-budgeted.
        #[arg(long, conflicts_with_all = ["full", "cursor"])]
        explain: bool,
        #[arg(long, default_value_t = 0)]
        cursor: usize,
        #[arg(long)]
        feature: Option<String>,
        #[arg(long)]
        user: Option<String>,
    },
    /// Preview a scoped transfer request (JSON on stdin).
    MemoryExportPreview,
    /// Export the exact reviewed request (JSON on stdin).
    MemoryExport {
        checksum: String,
        #[arg(long)]
        confirm: bool,
    },
    /// Preview an import request containing document, format and access (JSON on stdin).
    MemoryImportPreview,
    /// Import the exact reviewed request as proposals (JSON on stdin).
    MemoryImport {
        checksum: String,
        #[arg(long)]
        confirm: bool,
    },
    MemoryPropose,
    MemoryReview,
    MemoryGet {
        id: String,
        #[arg(long)]
        full: bool,
        #[arg(long, default_value_t = 0)]
        offset: usize,
        #[arg(long)]
        feature: Option<String>,
        #[arg(long)]
        tool: Option<String>,
        #[arg(long)]
        worktree: Option<String>,
        #[arg(long)]
        user: Option<String>,
        #[arg(long)]
        task: Option<String>,
    },
    MemoryEvidence {
        path: String,
        #[arg(long)]
        worktree: Option<String>,
    },
    MemoryEvidenceSources,
    MemorySymbolEvidence {
        path: String,
        symbol: String,
        #[arg(long, default_value = "native")]
        provider: String,
        #[arg(long)]
        worktree: Option<String>,
    },
    MemoryToolEvidence {
        tool_id: String,
    },
    MemoryConflicts,
    MemoryResolveConflict {
        id: String,
        #[arg(long)]
        reason: String,
        #[arg(long, default_value="both_stale", value_parser=["both_stale","keep_left","keep_right","dismiss"])]
        resolution: String,
        #[arg(long)]
        checksum: Option<String>,
        #[arg(long)]
        confirm: bool,
    },
    FtsRebuild,
    ContextList,
    ContextGet {
        id: String,
    },
    Reduce {
        #[arg(value_parser=["test","lint","trace","diff","log"])]
        bundle: String,
    },
    Providers,
    ProviderPlan {
        provider: String,
        operation: String,
    },
    ProviderApply {
        plan_id: String,
        checksum: String,
    },
    CodeQuery {
        text: String,
        #[arg(long, default_value = "file_search")]
        kind: String,
        #[arg(long)]
        worktree: Option<String>,
        #[arg(long)]
        path: Option<String>,
    },
    ProviderOperation {
        id: String,
    },
    CodeCompare {
        provider: String,
        text: String,
        #[arg(long, default_value = "references")]
        kind: String,
        #[arg(long)]
        worktree: Option<String>,
        #[arg(long)]
        path: Option<String>,
    },
    CompactionList,
    CompactionProbe {
        tool_id: String,
    },
}
pub(super) fn stdin() -> Result<String> {
    let mut value = String::new();
    std::io::stdin()
        .take(8 * 1024 * 1024 + 1)
        .read_to_string(&mut value)
        .map_err(|e| MaccError::Validation(e.to_string()))?;
    if value.len() > 8 * 1024 * 1024 {
        return Err(MaccError::Validation(
            "O2 request exceeds transport bound".into(),
        ));
    }
    Ok(value)
}
pub struct O2 {
    app: AppContext,
    command: O2Command,
}
impl O2 {
    pub fn new(app: AppContext, command: O2Command) -> Self {
        Self { app, command }
    }
}
impl Command for O2 {
    fn run(&self) -> Result<()> {
        if self.command.is_mcp() {
            return super::o2_mcp::run();
        }
        if let O2Command::Policy(command) = &self.command {
            return super::o2_policy::run(&self.app, command);
        }
        let action = match &self.command {
            O2Command::Policy(_) => unreachable!("policy transport handled above"),
            O2Command::Tuning(command) => command.action()?,
            O2Command::Existing(command) => match command {
                ExistingCommand::Mcp => unreachable!(),
                ExistingCommand::AgentStatus => Action::AgentStatus,
                ExistingCommand::SemanticStatus {
                    task,
                    tool,
                    worktree,
                    feature,
                    user,
                } => Action::SemanticStatus {
                    access: memory::Access {
                        task_id: task.clone(),
                        tool_id: tool.clone(),
                        worktree: worktree.clone(),
                        feature: feature.clone(),
                        user_id: user.clone(),
                    },
                },
                ExistingCommand::SemanticBenchmark => Action::SemanticBenchmark {
                    suite: serde_json::from_str(&stdin()?)
                        .map_err(|e| MaccError::Validation(e.to_string()))?,
                },
                ExistingCommand::SemanticGate {
                    report_id,
                    checksum,
                    revoke,
                    confirm,
                } => Action::SemanticGate {
                    report_id: report_id.clone(),
                    checksum: checksum.clone(),
                    enabled: !*revoke,
                    confirmed: *confirm,
                },
                ExistingCommand::MemoryExtract { checkpoint_id } => Action::MemoryExtract {
                    checkpoint_id: checkpoint_id.clone(),
                },
                ExistingCommand::Status => Action::Status,
                ExistingCommand::Upgrade {
                    apply,
                    expected_sha256,
                } => {
                    if *apply {
                        Action::UpgradeApply {
                            source_sha256: expected_sha256.clone().ok_or_else(|| {
                                MaccError::Validation(
                                    "Preview upgrade and supply --expected-sha256".into(),
                                )
                            })?,
                        }
                    } else {
                        Action::UpgradePlan
                    }
                }
                ExistingCommand::MemoryExportPreview => Action::MemoryExportPreview {
                    request: serde_json::from_str(&stdin()?)
                        .map_err(|e| MaccError::Validation(e.to_string()))?,
                },
                ExistingCommand::MemoryExport { checksum, confirm } => Action::MemoryExport {
                    request: serde_json::from_str(&stdin()?)
                        .map_err(|e| MaccError::Validation(e.to_string()))?,
                    checksum: checksum.clone(),
                    confirmed: *confirm,
                },
                ExistingCommand::MemoryImportPreview => Action::MemoryImportPreview {
                    request: serde_json::from_str(&stdin()?)
                        .map_err(|e| MaccError::Validation(e.to_string()))?,
                },
                ExistingCommand::MemoryImport { checksum, confirm } => Action::MemoryImport {
                    request: serde_json::from_str(&stdin()?)
                        .map_err(|e| MaccError::Validation(e.to_string()))?,
                    checksum: checksum.clone(),
                    confirmed: *confirm,
                },
                ExistingCommand::Action => serde_json::from_str(&stdin()?)
                    .map_err(|e| MaccError::Validation(e.to_string()))?,
                ExistingCommand::MemorySearch {
                    query,
                    task,
                    tool,
                    worktree,
                    all,
                    full,
                    explain,
                    cursor,
                    feature,
                    user,
                } => {
                    let search = memory::Search {
                        query: query.clone(),
                        access: memory::Access {
                            task_id: task.clone(),
                            tool_id: tool.clone(),
                            worktree: worktree.clone(),
                            feature: feature.clone(),
                            user_id: user.clone(),
                        },
                        include_inactive: *all,
                    };
                    if *explain {
                        Action::MemoryExplain { search }
                    } else if *full {
                        Action::MemorySearch { search }
                    } else {
                        Action::MemoryPull {
                            search,
                            cursor: *cursor,
                        }
                    }
                }
                ExistingCommand::MemoryPropose => Action::MemoryPropose {
                    proposal: serde_json::from_str(&stdin()?)
                        .map_err(|e| MaccError::Validation(e.to_string()))?,
                },
                ExistingCommand::MemoryReview => Action::MemoryReview {
                    review: serde_json::from_str(&stdin()?)
                        .map_err(|e| MaccError::Validation(e.to_string()))?,
                },
                ExistingCommand::MemoryGet {
                    id,
                    task,
                    full,
                    offset,
                    feature,
                    tool,
                    worktree,
                    user,
                } => {
                    let access = memory::Access {
                        task_id: task.clone(),
                        tool_id: tool.clone(),
                        worktree: worktree.clone(),
                        feature: feature.clone(),
                        user_id: user.clone(),
                    };
                    if *full {
                        Action::MemoryGet {
                            id: id.clone(),
                            access,
                        }
                    } else {
                        Action::MemoryRead {
                            id: id.clone(),
                            access,
                            offset: *offset,
                        }
                    }
                }
                ExistingCommand::MemoryEvidence { path, worktree } => Action::MemoryEvidence {
                    path: path.clone(),
                    worktree: worktree.clone(),
                },
                ExistingCommand::MemoryEvidenceSources => Action::MemoryEvidenceSources,
                ExistingCommand::MemorySymbolEvidence {
                    path,
                    symbol,
                    provider,
                    worktree,
                } => Action::MemorySymbolEvidence {
                    path: path.clone(),
                    symbol: symbol.clone(),
                    provider: provider.clone(),
                    worktree: worktree.clone(),
                },
                ExistingCommand::MemoryToolEvidence { tool_id } => Action::MemoryToolEvidence {
                    tool_id: tool_id.clone(),
                },
                ExistingCommand::MemoryConflicts => Action::MemoryConflicts,
                ExistingCommand::MemoryResolveConflict {
                    id,
                    reason,
                    resolution,
                    checksum,
                    confirm,
                } => Action::MemoryResolveConflict {
                    id: id.clone(),
                    reason: reason.clone(),
                    resolution: serde_json::from_value(serde_json::json!(resolution))
                        .map_err(|e| MaccError::Validation(e.to_string()))?,
                    expected_checksum: checksum.clone(),
                    confirmed: *confirm,
                },
                ExistingCommand::FtsRebuild => Action::FtsRebuild,
                ExistingCommand::ContextList => Action::ContextList,
                ExistingCommand::ContextGet { id } => Action::ContextGet { id: id.clone() },
                ExistingCommand::Reduce { bundle } => Action::Reduce {
                    kind: bundle.clone(),
                    raw: stdin()?,
                },
                ExistingCommand::Providers => Action::Providers,
                ExistingCommand::ProviderPlan {
                    provider,
                    operation,
                } => Action::ProviderPlan {
                    provider: provider.clone(),
                    operation: operation.clone(),
                },
                ExistingCommand::ProviderApply { plan_id, checksum } => Action::ProviderApply {
                    plan_id: plan_id.clone(),
                    checksum: checksum.clone(),
                },
                ExistingCommand::CodeQuery {
                    text,
                    kind,
                    worktree,
                    path,
                } => Action::CodeQuery {
                    query: macc_core::o2::providers::Query {
                        path: path.clone(),
                        text: text.clone(),
                        kind: kind.clone(),
                        worktree: worktree.clone(),
                    },
                },
                ExistingCommand::ProviderOperation { id } => {
                    Action::ProviderOperation { id: id.clone() }
                }
                ExistingCommand::CodeCompare {
                    provider,
                    text,
                    kind,
                    worktree,
                    path,
                } => Action::CodeCompare {
                    provider: provider.clone(),
                    query: macc_core::o2::providers::Query {
                        path: path.clone(),
                        text: text.clone(),
                        kind: kind.clone(),
                        worktree: worktree.clone(),
                    },
                },
                ExistingCommand::CompactionList => Action::CompactionList,
                ExistingCommand::CompactionProbe { tool_id } => Action::CompactionProbe {
                    tool_id: tool_id.clone(),
                },
            },
        };
        let paths = self.app.project_paths()?;
        if action.is_mutation() {
            super::gate_cli_mutation(&paths.root)?;
        }
        let result = execute(&paths.root, action)?;
        print!("{}", macc_core::context::payload::json_line(&result)?);
        Ok(())
    }
}
