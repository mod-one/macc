//! Thin command transport; every operation is shared with both interactive clients.
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
pub enum O2Command {
    Status,
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
        #[arg(long, default_value_t = 0)]
        cursor: usize,
        #[arg(long)]
        feature: Option<String>,
        #[arg(long)]
        user: Option<String>,
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
    },
    CompactionList,
    CompactionProbe {
        tool_id: String,
    },
}
fn stdin() -> Result<String> {
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
        let action = match &self.command {
            O2Command::Status => Action::Status,
            O2Command::Upgrade {
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
            O2Command::Action => {
                serde_json::from_str(&stdin()?).map_err(|e| MaccError::Validation(e.to_string()))?
            }
            O2Command::MemorySearch {
                query,
                task,
                tool,
                worktree,
                all,
                full,
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
                if *full {
                    Action::MemorySearch { search }
                } else {
                    Action::MemoryPull {
                        search,
                        cursor: *cursor,
                    }
                }
            }
            O2Command::MemoryPropose => Action::MemoryPropose {
                proposal: serde_json::from_str(&stdin()?)
                    .map_err(|e| MaccError::Validation(e.to_string()))?,
            },
            O2Command::MemoryReview => Action::MemoryReview {
                review: serde_json::from_str(&stdin()?)
                    .map_err(|e| MaccError::Validation(e.to_string()))?,
            },
            O2Command::MemoryGet {
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
            O2Command::MemoryEvidence { path, worktree } => Action::MemoryEvidence {
                path: path.clone(),
                worktree: worktree.clone(),
            },
            O2Command::FtsRebuild => Action::FtsRebuild,
            O2Command::ContextList => Action::ContextList,
            O2Command::ContextGet { id } => Action::ContextGet { id: id.clone() },
            O2Command::Reduce { bundle } => Action::Reduce {
                kind: bundle.clone(),
                raw: stdin()?,
            },
            O2Command::Providers => Action::Providers,
            O2Command::ProviderPlan {
                provider,
                operation,
            } => Action::ProviderPlan {
                provider: provider.clone(),
                operation: operation.clone(),
            },
            O2Command::ProviderApply { plan_id, checksum } => Action::ProviderApply {
                plan_id: plan_id.clone(),
                checksum: checksum.clone(),
            },
            O2Command::CodeQuery {
                text,
                kind,
                worktree,
            } => Action::CodeQuery {
                query: macc_core::o2::providers::Query {
                    text: text.clone(),
                    kind: kind.clone(),
                    worktree: worktree.clone(),
                },
            },
            O2Command::CompactionList => Action::CompactionList,
            O2Command::CompactionProbe { tool_id } => Action::CompactionProbe {
                tool_id: tool_id.clone(),
            },
        };
        let paths = self.app.project_paths()?;
        if action.is_mutation() {
            super::gate_cli_mutation(&paths.root)?;
        }
        let result = execute(&paths.root, action)?;
        println!(
            "{}",
            serde_json::to_string_pretty(&result)
                .map_err(|e| MaccError::Validation(e.to_string()))?
        );
        Ok(())
    }
}
