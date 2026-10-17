//! Nested CLI transport keeps experiment parsing separate from the O2 command builder.
use super::o2::stdin;
use clap::Subcommand;
use macc_core::{o2::actions::Action, MaccError, Result};
#[derive(Subcommand, Clone, Debug)]
pub enum TuningCommand {
    /// Inspect all trials, or one pre-registered trial and its evidence.
    Status {
        id: Option<String>,
    },
    /// Read a typed trial plan from stdin; does not change policy or dispatch tasks.
    Create {
        #[arg(long)]
        confirm: bool,
    },
    /// Read one paired task quality observation from stdin; costs come from SQLite.
    Observe {
        trial_id: String,
        #[arg(long)]
        confirm: bool,
    },
    Close {
        trial_id: String,
        #[arg(long)]
        cancel: bool,
        #[arg(long)]
        confirm: bool,
    },
    Preview {
        report_id: String,
        checksum: String,
    },
    Apply {
        report_id: String,
        checksum: String,
        revision: String,
        #[arg(long)]
        confirm: bool,
    },
}
impl TuningCommand {
    pub(super) fn action(&self) -> Result<Action> {
        Ok(match self {
            Self::Status { id } => Action::TuningStatus { id: id.clone() },
            Self::Create { confirm } => Action::TuningCreate {
                plan: serde_json::from_str(&stdin()?)
                    .map_err(|e| MaccError::Validation(e.to_string()))?,
                confirmed: *confirm,
            },
            Self::Observe { trial_id, confirm } => Action::TuningObserve {
                trial_id: trial_id.clone(),
                observation: serde_json::from_str(&stdin()?)
                    .map_err(|e| MaccError::Validation(e.to_string()))?,
                confirmed: *confirm,
            },
            Self::Close {
                trial_id,
                cancel,
                confirm,
            } => Action::TuningClose {
                trial_id: trial_id.clone(),
                cancel: *cancel,
                confirmed: *confirm,
            },
            Self::Preview {
                report_id,
                checksum,
            } => Action::TuningPreview {
                report_id: report_id.clone(),
                checksum: checksum.clone(),
            },
            Self::Apply {
                report_id,
                checksum,
                revision,
                confirm,
            } => Action::TuningApply {
                report_id: report_id.clone(),
                checksum: checksum.clone(),
                revision: revision.clone(),
                confirmed: *confirm,
            },
        })
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    use clap::Parser;
    #[derive(Parser)]
    struct Transport {
        #[command(subcommand)]
        command: TuningCommand,
    }
    #[test]
    fn explicit_tuning_confirmation_and_revision_reach_shared_actions() {
        let cli = Transport::try_parse_from([
            "tuning",
            "apply",
            "report",
            "sha",
            "revision",
            "--confirm",
        ])
        .unwrap();
        assert!(
            matches!(cli.command.action().unwrap(),Action::TuningApply{report_id,checksum,revision,confirmed:true} if report_id=="report"&&checksum=="sha"&&revision=="revision")
        );
        let cli = Transport::try_parse_from(["tuning", "close", "trial", "--cancel"]).unwrap();
        assert!(matches!(
            cli.command.action().unwrap(),
            Action::TuningClose {
                cancel: true,
                confirmed: false,
                ..
            }
        ));
    }
}
