//! Compose command groups sequentially: generated debug builders otherwise retain
//! large stack frames for every nested enum while building unrelated CLI commands.
use super::o2_policy::PolicyCommand;
use super::{o2::ExistingCommand, o2_tuning::TuningCommand};
use clap::{ArgMatches, Command, Error, FromArgMatches, Subcommand};
#[derive(Clone, Debug)]
pub enum O2Command {
    Existing(ExistingCommand),
    Tuning(TuningCommand),
    Policy(PolicyCommand),
}
impl O2Command {
    pub fn is_mcp(&self) -> bool {
        matches!(self, Self::Existing(ExistingCommand::Mcp))
    }
}
impl FromArgMatches for O2Command {
    fn from_arg_matches(matches: &ArgMatches) -> Result<Self, Error> {
        if let Some(("policy", args)) = matches.subcommand() {
            PolicyCommand::from_arg_matches(args).map(Self::Policy)
        } else if let Some(("tuning", args)) = matches.subcommand() {
            TuningCommand::from_arg_matches(args).map(Self::Tuning)
        } else {
            ExistingCommand::from_arg_matches(matches).map(Self::Existing)
        }
    }
    fn update_from_arg_matches(&mut self, matches: &ArgMatches) -> Result<(), Error> {
        *self = Self::from_arg_matches(matches)?;
        Ok(())
    }
}
impl Subcommand for O2Command {
    fn augment_subcommands(cmd: Command) -> Command {
        let cmd = ExistingCommand::augment_subcommands(cmd);
        let cmd = cmd.subcommand(
            PolicyCommand::augment_subcommands(
                Command::new("policy")
                    .about("Inspect and edit individual O2 settings with reviewed confirmation"),
            )
            .subcommand_required(true)
            .arg_required_else_help(true),
        );
        cmd.subcommand(
            TuningCommand::augment_subcommands(
                Command::new("tuning")
                    .about("Reviewed task-quality and measured-cost policy trials"),
            )
            .subcommand_required(true)
            .arg_required_else_help(true),
        )
    }
    fn augment_subcommands_for_update(cmd: Command) -> Command {
        let cmd = ExistingCommand::augment_subcommands_for_update(cmd);
        let cmd = cmd.subcommand(PolicyCommand::augment_subcommands_for_update(Command::new(
            "policy",
        )));
        cmd.subcommand(TuningCommand::augment_subcommands_for_update(Command::new(
            "tuning",
        )))
    }
    fn has_subcommand(name: &str) -> bool {
        name == "policy" || name == "tuning" || ExistingCommand::has_subcommand(name)
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    use clap::Parser;
    #[test]
    fn root_parser_supports_trials_and_legacy_stdio_without_extra_stack() {
        let cli = crate::Cli::try_parse_from([
            "macc",
            "o2",
            "tuning",
            "apply",
            "report",
            "sha",
            "revision",
            "--confirm",
        ])
        .unwrap();
        assert!(matches!(
            cli.command,
            Some(crate::Commands::O2 {
                command: O2Command::Tuning(TuningCommand::Apply { confirm: true, .. }),
                ..
            })
        ));
        let cli = crate::Cli::try_parse_from(["macc", "o2", "mcp"]).unwrap();
        assert!(matches!(cli.command,Some(crate::Commands::O2{command, ..}) if command.is_mcp()));
        assert!(crate::Cli::try_parse_from(["macc", "o2", "tuning"]).is_err());
    }
}
