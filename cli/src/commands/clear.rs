use crate::commands::AppContext;
use crate::commands::Command;
use crate::services::interaction::CliInteraction;
use macc_core::{MaccError, Result};
use std::io::IsTerminal;
use chrono::Utc;

pub struct ClearCommand {
    app: AppContext,
    save: Option<String>,
    include_logs: bool,
    no_save_prompt: bool,
    force: bool,
    dry_run: bool,
}

impl ClearCommand {
    pub fn new(
        app: AppContext,
        save: Option<String>,
        include_logs: bool,
        no_save_prompt: bool,
        force: bool,
        dry_run: bool,
    ) -> Self {
        Self {
            app,
            save,
            include_logs,
            no_save_prompt,
            force,
            dry_run,
        }
    }
}

impl Command for ClearCommand {
    fn run(&self) -> Result<()> {
        let paths = self.app.project_paths()?;
        
        if !self.dry_run {
            crate::commands::gate_cli_mutation(&paths.root)?;
        }

        let is_unsaved = macc_core::save::is_macc_state_unsaved(&paths)?;

        if is_unsaved && !self.no_save_prompt {
            if let Some(ref save_name) = self.save {
                let opts = macc_core::save::SaveOptions {
                    description: Some("Auto-saved before clear".to_string()),
                    overwrite: true,
                    only: None,
                    no_sessions: false,
                    include_logs: self.include_logs,
                    log_max_size: "50MB".to_string(),
                    log_since: "7d".to_string(),
                    redact_logs: true,
                    dry_run: self.dry_run,
                    include_prd: false,
                    include_state: false,
                };
                println!("Saving current MACC setup to \"{}\" before clear...", save_name);
                macc_core::save::create_save_bundle(&paths, save_name, &opts)?;
            } else {
                let term_interactive = std::io::stdin().is_terminal() && std::env::var("CARGO_MANIFEST_DIR").is_err();
                if !term_interactive {
                    if !self.force {
                        return Err(MaccError::Validation(
                            "MACC-CLEAR-4000: Unsaved MACC state detected in non-interactive mode. Save first or run with --no-save-prompt.".to_string()
                        ));
                    }
                } else {
                    println!("Unsaved MACC setup detected.\n");
                    println!("Save before clearing?");
                    println!("  [Y] Save now");
                    println!("  [N] Continue without saving");
                    println!("  [A] Abort clear");

                    use std::io::{self, Write};
                    print!("> ");
                    io::stdout().flush().ok();
                    let mut input = String::new();
                    io::stdin().read_line(&mut input).ok();
                    let choice = input.trim().to_lowercase();

                    if choice.is_empty() || choice == "y" || choice == "yes" {
                        print!("Save name [default: before-clear]: ");
                        io::stdout().flush().ok();
                        let mut name_input = String::new();
                        io::stdin().read_line(&mut name_input).ok();
                        let mut save_name = name_input.trim().to_string();
                        if save_name.is_empty() {
                            save_name = format!("before-clear-{}", Utc::now().timestamp());
                        }

                        let include_logs = crate::confirm_yes_no("Include logs? [y/N] ")?;

                        let opts = macc_core::save::SaveOptions {
                            description: Some("Saved before clear".to_string()),
                            overwrite: true,
                            only: None,
                            no_sessions: false,
                            include_logs,
                            log_max_size: "50MB".to_string(),
                            log_since: "7d".to_string(),
                            redact_logs: true,
                            dry_run: self.dry_run,
                            include_prd: false,
                            include_state: false,
                        };
                        macc_core::save::create_save_bundle(&paths, &save_name, &opts)?;
                        println!("Setup saved successfully.");
                    } else if choice == "a" || choice == "abort" {
                        println!("Clear aborted.");
                        return Ok(());
                    }
                }
            }
        }

        let _ = self
            .app
            .engine
            .clear_project(&paths, self.force, self.dry_run, &CliInteraction)?;
        println!("Project MACC files cleared successfully.");
        Ok(())
    }
}
