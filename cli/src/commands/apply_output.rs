//! Terminal presentation only; preparation decisions remain in shared services.
use super::lifecycle_support::CliLifecycleUi;
use macc_core::{MaccError, Result};
use std::io::{IsTerminal, Write};
#[derive(Default)]
pub(crate) struct Options {
    pub json: bool,
    pub verbose: bool,
    pub quiet: bool,
    pub yes: bool,
    pub progress: std::sync::Mutex<Progress>,
}
#[derive(Default)]
pub(crate) struct Progress {
    stage: String,
    output: String,
    open: bool,
}
impl CliLifecycleUi {
    pub(super) fn apply_confirm(&self, prompt: &str) -> Result<bool> {
        let options = self.output.as_ref().unwrap();
        if options.yes && prompt == "Continue? [y/N] " {
            return Ok(true);
        }
        if !std::io::stdin().is_terminal() {
            return Err(MaccError::Validation("Apply needs explicit consent. Use --yes for the initial reviewed plan; interactive preparation questions still require a terminal.".into()));
        }
        self.finish_progress();
        eprint!("{prompt}");
        std::io::stderr()
            .flush()
            .map_err(|e| MaccError::Validation(e.to_string()))?;
        let mut input = String::new();
        std::io::stdin()
            .read_line(&mut input)
            .map_err(|e| MaccError::Validation(e.to_string()))?;
        Ok(matches!(
            input.trim().to_ascii_lowercase().as_str(),
            "y" | "yes"
        ))
    }
    pub(super) fn user_scope(
        &self,
        paths: &macc_core::ProjectPaths,
        ops: &[macc_core::plan::PlannedOp],
    ) -> Result<()> {
        let selected: Vec<_> = ops
            .iter()
            .filter(|op| op.scope == macc_core::plan::Scope::User)
            .collect();
        if selected.is_empty() {
            return Ok(());
        }
        self.finish_progress();
        eprintln!(
            "User-level changes: {} operations. Separate consent is required.",
            selected.len()
        );
        for op in selected.iter().take(12) {
            eprintln!("  {}", op.path);
        }
        eprintln!("Project backups: {}", paths.backups_dir.display());
        if self.apply_confirm("Proceed with user-level changes [y/N]? ")? {
            Ok(())
        } else {
            Err(MaccError::Validation(
                "Apply cancelled before user-level changes".into(),
            ))
        }
    }
    pub(super) fn finish_progress(&self) {
        if let Some(options) = &self.output {
            if let Ok(mut state) = options.progress.lock() {
                if state.open {
                    eprintln!();
                    state.open = false;
                }
            }
        }
    }
    pub(super) fn preparation_progress(&self, report: &macc_core::service::preparation::Report) {
        let Some(options) = &self.output else {
            return;
        };
        let Ok(mut state) = options.progress.lock() else {
            return;
        };
        if report.status != "preparing" {
            if state.open {
                eprintln!();
                state.open = false;
            }
            return;
        }
        let stage = report
            .current
            .as_deref()
            .unwrap_or("Preparing enabled features");
        if state.stage != stage {
            if !options.quiet && !options.json {
                if std::io::stderr().is_terminal() {
                    if state.open {
                        eprintln!();
                    }
                    eprint!("[RUN] {stage}");
                    let _ = std::io::stderr().flush();
                    state.open = true;
                } else {
                    eprintln!("[RUN] {stage}");
                }
            }
            state.stage = stage.into();
            state.output.clear();
        }
        if (options.verbose && !options.quiet)
            || report.output_kind.as_deref() == Some("interaction")
        {
            let output = report.live_output.as_deref().unwrap_or("");
            let delta = output.strip_prefix(&state.output).unwrap_or(output);
            if !delta.is_empty() {
                if state.open {
                    eprintln!();
                    state.open = false;
                }
                eprint!(
                    "{}",
                    delta
                        .chars()
                        .filter(|c| !c.is_control() || matches!(c, '\n' | '\t'))
                        .collect::<String>()
                );
                if !delta.ends_with('\n') {
                    eprintln!();
                }
            }
            state.output = output.into();
        }
    }
    pub(super) fn review(
        &self,
        paths: &macc_core::ProjectPaths,
        plan: &macc_core::plan::ActionPlan,
        ops: &[macc_core::plan::PlannedOp],
    ) -> Result<()> {
        let options = self.output.as_ref().unwrap();
        self.finish_progress();
        let review = macc_core::service::apply_presentation::review(paths, plan, ops);
        if !options.quiet || !options.yes {
            eprintln!("{review}");
        } else {
            for warning in review.lines().filter(|line| line.starts_with("WARN ")) {
                eprintln!("{warning}");
            }
        }
        if options.verbose && !options.quiet {
            for step in plan.preparation.iter().flat_map(|p| &p.steps) {
                eprintln!("Prepare: {step}");
            }
        }
        Ok(())
    }
    pub(super) fn result(&self, report: &macc_core::ApplyReport) -> Result<()> {
        let options = self.output.as_ref().unwrap();
        self.finish_progress();
        if options.json {
            println!(
                "{}",
                serde_json::to_string(&report.json())
                    .map_err(|e| MaccError::Validation(e.to_string()))?
            );
        } else if options.quiet {
            for warning in &report.warnings {
                eprintln!("WARN {warning}");
            }
        } else {
            println!(
                "{}",
                if options.verbose {
                    report.render_detailed()
                } else {
                    report.render_cli()
                }
            );
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use macc_core::service::interaction::InteractionHandler;
    #[test]
    fn explicit_yes_only_answers_the_initial_apply_confirmation() {
        let ui = CliLifecycleUi {
            output: Some(Options {
                yes: true,
                ..Default::default()
            }),
        };
        assert!(ui.confirm_yes_no("Continue? [y/N] ").unwrap());
        if !std::io::stdin().is_terminal() {
            assert!(ui.confirm_yes_no("Approve benchmark?").is_err());
        }
    }
    #[test]
    fn noninteractive_json_does_not_grant_consent() {
        if !std::io::stdin().is_terminal() {
            let ui = CliLifecycleUi {
                output: Some(Options {
                    json: true,
                    ..Default::default()
                }),
            };
            assert!(ui.confirm_yes_no("Continue? [y/N] ").is_err());
        }
    }
}
