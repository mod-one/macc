//! User-facing single-field policy edits; domain validation lives in core.
use super::AppContext;
use clap::Subcommand;
use macc_core::{
    o2::actions::{execute, Action},
    MaccError, Result,
};
use std::io::{self, IsTerminal, Write};

#[derive(Subcommand, Clone, Debug)]
pub enum PolicyCommand {
    /// List effective settings and the shared field catalog (types, ranges, choices).
    List,
    /// Inspect one field's current value and configuration revision.
    Get { path: String },
    /// Preview one typed edit; --apply requests confirmation before writing.
    Set {
        path: String,
        value: String,
        #[arg(long)]
        apply: bool,
        /// Confirm a reviewed edit non-interactively; requires --apply and --revision.
        #[arg(long, requires_all = ["apply", "revision"])]
        confirm: bool,
        /// Bind the edit to the revision returned by the preview.
        #[arg(long)]
        revision: Option<String>,
    },
}
fn output(value: &serde_json::Value) -> Result<()> {
    print!("{}", macc_core::context::payload::json_line(value)?);
    Ok(())
}
pub(super) fn run(app: &AppContext, command: &PolicyCommand) -> Result<()> {
    let paths = app.project_paths()?;
    let action = match command {
        PolicyCommand::List => Action::Status,
        PolicyCommand::Get { path } => Action::PolicyFieldGet { path: path.clone() },
        PolicyCommand::Set {
            path,
            value,
            apply,
            confirm,
            revision,
        } => {
            let preview = execute(
                &paths.root,
                Action::PolicyFieldPreview {
                    path: path.clone(),
                    value: value.clone(),
                    revision: revision.clone(),
                },
            )?;
            if !apply {
                return output(&preview);
            }
            if *confirm && revision.is_none() {
                return Err(MaccError::Validation(
                    "--confirm requires the reviewed --revision".into(),
                ));
            }
            if !confirm {
                if !io::stdin().is_terminal() {
                    return Err(MaccError::Validation("Preview with `macc o2 policy set <field> <value>`, then use --apply --confirm --revision <reviewed-revision> in a non-interactive client".into()));
                }
                eprintln!(
                    "{}: {} -> {}\nActivation: {}",
                    path, preview["before"], preview["after"], preview["activation"]
                );
                eprint!("Apply this O2 setting? [y/N] ");
                io::stderr()
                    .flush()
                    .map_err(|e| MaccError::Validation(e.to_string()))?;
                let mut answer = String::new();
                io::stdin()
                    .read_line(&mut answer)
                    .map_err(|e| MaccError::Validation(e.to_string()))?;
                if !matches!(answer.trim().to_ascii_lowercase().as_str(), "y" | "yes") {
                    return output(
                        &serde_json::json!({"applied":false,"cancelled":true,"preview":preview}),
                    );
                }
            }
            super::gate_cli_mutation(&paths.root)?;
            Action::PolicyFieldApply {
                path: path.clone(),
                value: value.clone(),
                revision: preview["revision"].as_str().unwrap_or_default().into(),
                confirmed: true,
            }
        }
    };
    output(&execute(&paths.root, action)?)
}
