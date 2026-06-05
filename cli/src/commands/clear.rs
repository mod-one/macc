use crate::commands::AppContext;
use crate::commands::Command;
use crate::services::interaction::CliInteraction;
use chrono::Utc;
use macc_core::{MaccError, Result};
use std::io::IsTerminal;

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
                    handle_secrets: None,
                };
                println!(
                    "Saving current MACC setup to \"{}\" before clear...",
                    save_name
                );
                crate::commands::create_save_bundle_interactive(&paths, save_name, opts)?;
            } else {
                let term_interactive =
                    std::io::stdin().is_terminal() && std::env::var("CARGO_MANIFEST_DIR").is_err();
                if !term_interactive {
                    if !self.force {
                        return Err(MaccError::Validation(
                            "MACC-CLEAR-4000: Unsaved MACC state detected in non-interactive mode. Save first or run with --no-save-prompt.".to_string()
                        ));
                    }
                } else {
                    let unsaved_report = macc_core::save::get_unsaved_state_report(&paths)?;
                    println!("Unsaved MACC setup detected.\n");
                    println!("The following state has changed since the last save:");
                    if unsaved_report.config_changed {
                        println!("  - config");
                    }
                    if unsaved_report.sessions_changed {
                        println!("  - coordinator sessions");
                    }
                    println!("\nSave before clearing?");
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
                        let saves = macc_core::save::detect_matching_saves(&paths)?;
                        let (save_name, overwrite) = if !saves.is_empty() {
                            println!("Existing saves found for this repository:\n");
                            for (idx, (m, _)) in saves.iter().enumerate() {
                                println!("  {}. {}", idx + 1, m.name);
                                println!("     Saved: {}", m.created_at);
                                let mut incls = Vec::new();
                                if m.includes.config {
                                    incls.push("config");
                                }
                                if m.includes.coordinator_sessions {
                                    incls.push("sessions");
                                }
                                if m.includes.catalogs {
                                    incls.push("catalogs");
                                }
                                println!("     Includes: {}", incls.join(", "));
                            }
                            println!("\nChoose:");
                            println!("  [1-{}] Overwrite an existing save", saves.len());
                            println!("  [N]   Create a new save");
                            println!("  [S]   Skip saving");
                            println!("  [A]   Abort clear");

                            print!("> ");
                            io::stdout().flush().ok();
                            let mut save_choice = String::new();
                            io::stdin().read_line(&mut save_choice).ok();
                            let choice_str = save_choice.trim().to_lowercase();

                            if choice_str == "s" || choice_str == "skip" {
                                (None, false)
                            } else if choice_str == "a" || choice_str == "abort" {
                                println!("Clear aborted.");
                                return Ok(());
                            } else if choice_str == "n" || choice_str == "new" {
                                print!("Save name [default: before-clear]: ");
                                io::stdout().flush().ok();
                                let mut name_input = String::new();
                                io::stdin().read_line(&mut name_input).ok();
                                let mut save_name = name_input.trim().to_string();
                                if save_name.is_empty() {
                                    save_name = format!("before-clear-{}", Utc::now().timestamp());
                                }
                                (Some(save_name), true)
                            } else if let Ok(idx) = choice_str.parse::<usize>() {
                                if idx >= 1 && idx <= saves.len() {
                                    (Some(saves[idx - 1].0.name.clone()), true)
                                } else {
                                    println!("Invalid choice. Clear aborted.");
                                    return Ok(());
                                }
                            } else {
                                println!("Invalid choice. Clear aborted.");
                                return Ok(());
                            }
                        } else {
                            print!("Save name [default: before-clear]: ");
                            io::stdout().flush().ok();
                            let mut name_input = String::new();
                            io::stdin().read_line(&mut name_input).ok();
                            let mut save_name = name_input.trim().to_string();
                            if save_name.is_empty() {
                                save_name = format!("before-clear-{}", Utc::now().timestamp());
                            }
                            (Some(save_name), true)
                        };

                        if let Some(name) = save_name {
                            let include_logs = crate::confirm_yes_no("Include logs? [y/N] ")?;

                            let opts = macc_core::save::SaveOptions {
                                description: Some("Saved before clear".to_string()),
                                overwrite,
                                only: None,
                                no_sessions: false,
                                include_logs,
                                log_max_size: "50MB".to_string(),
                                log_since: "7d".to_string(),
                                redact_logs: true,
                                dry_run: self.dry_run,
                                include_prd: false,
                                include_state: false,
                                handle_secrets: None,
                            };
                            crate::commands::create_save_bundle_interactive(&paths, &name, opts)?;
                            println!("Setup saved successfully.");
                        }
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
