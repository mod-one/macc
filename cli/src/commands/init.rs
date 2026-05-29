use crate::commands::AppContext;
use crate::commands::Command;
use macc_core::{MaccError, Result};
use std::io::IsTerminal;

pub struct InitCommand {
    app: AppContext,
    force: bool,
    wizard: bool,
    fresh: bool,
    restore: Option<String>,
    no_restore_prompt: bool,
    apply: bool,
}

impl InitCommand {
    pub fn new(
        app: AppContext,
        force: bool,
        wizard: bool,
        fresh: bool,
        restore: Option<String>,
        no_restore_prompt: bool,
        apply: bool,
    ) -> Self {
        Self {
            app,
            force,
            wizard,
            fresh,
            restore,
            no_restore_prompt,
            apply,
        }
    }
}

impl Command for InitCommand {
    fn run(&self) -> Result<()> {
        let root = std::path::PathBuf::from(&self.app.cwd);
        let root = root.canonicalize().unwrap_or(root);
        let paths = macc_core::ProjectPaths::from_root(root);

        // If --fresh is passed, do a fresh init immediately
        if self.fresh {
            return crate::commands::lifecycle_support::init(&self.app, self.force, self.wizard);
        }

        // Check if --restore was explicitly requested (with or without a name)
        if let Some(ref restore_val) = self.restore {
            let name = if restore_val.is_empty() {
                // Find best match
                let matches = macc_core::save::detect_matching_saves(&paths)?;
                if matches.is_empty() {
                    return Err(MaccError::Validation("No matching saves found to restore.".to_string()));
                }
                matches[0].0.name.clone()
            } else {
                restore_val.clone()
            };

            let opts = macc_core::save::RestoreOptions {
                apply: self.apply,
                config_only: false,
                sessions: true,
                no_sessions: false,
                include_logs: false,
                dry_run: false,
                yes: true,
            };

            println!("Restoring matching MACC save \"{}\"...", name);
            macc_core::save::restore_save_bundle(&paths, &name, &opts)?;
            println!("Restore completed successfully.");

            if self.apply {
                println!("Running macc apply...");
                crate::commands::lifecycle_support::apply(
                    &self.app,
                    None,
                    false, // dry_run
                    false, // allow_user_scope
                    false, // json
                    false, // explain
                )?;
            }
            return Ok(());
        }

        // Interactive checking for matching saves
        if !self.no_restore_prompt {
            let matches = macc_core::save::detect_matching_saves(&paths)?;
            if !matches.is_empty() {
                let best_match = &matches[0].0;
                println!("A previous MACC save was found for this repository:\n");
                println!("  {}", best_match.name);
                println!("  Saved: {}", best_match.created_at);
                
                let mut incls = Vec::new();
                if best_match.includes.config { incls.push("config"); }
                if best_match.includes.coordinator_sessions { incls.push("sessions"); }
                if best_match.includes.catalogs { incls.push("catalogs"); }
                println!("  Includes: {}", incls.join(", "));
                println!("  Excludes: worktrees, cache, generated files, logs\n");

                println!("Restore it now?");
                println!("  [Y] Restore this save");
                println!("  [N] Start fresh");
                println!("  [L] List all matching saves");
                println!("  [A] Abort");

                // In tests/non-interactive environments, default to yes/restore or fresh depending on flag
                // We reuse crate::confirm_yes_no logic or standard terminal check
                let term_interactive = std::io::stdin().is_terminal() && std::env::var("CARGO_MANIFEST_DIR").is_err();
                let choice = if !term_interactive {
                    println!("(Auto-confirmed Start fresh in test/non-interactive environment)");
                    "n".to_string()
                } else {
                    use std::io::{self, Write};
                    print!("> ");
                    io::stdout().flush().ok();
                    let mut input = String::new();
                    io::stdin().read_line(&mut input).ok();
                    input.trim().to_lowercase()
                };

                if choice.is_empty() || choice == "y" || choice == "yes" {
                    let opts = macc_core::save::RestoreOptions {
                        apply: self.apply,
                        config_only: false,
                        sessions: true,
                        no_sessions: false,
                        include_logs: false,
                        dry_run: false,
                        yes: true,
                    };
                    macc_core::save::restore_save_bundle(&paths, &best_match.name, &opts)?;
                    println!("Restore completed successfully.");
                    
                    let run_apply = if self.apply {
                        true
                    } else if !term_interactive {
                        false
                    } else {
                        crate::confirm_yes_no("Run `macc apply` now? ")?
                    };

                    if run_apply {
                        crate::commands::lifecycle_support::apply(
                            &self.app,
                            None,
                            false,
                            false,
                            false,
                            false,
                        )?;
                    }
                    return Ok(());
                } else if choice == "l" {
                    println!("\nMatching saves:");
                    for (idx, (m, strength)) in matches.iter().enumerate() {
                        println!("  {}. {} (Match: {})", idx + 1, m.name, strength.as_str());
                    }
                    println!("\nRun 'macc restore <name>' to restore a specific save.");
                    return Ok(());
                } else if choice == "a" || choice == "abort" {
                    println!("Init aborted.");
                    return Ok(());
                }
                // 'n' continues to fresh init below
            }
        }

        crate::commands::lifecycle_support::init(&self.app, self.force, self.wizard)
    }
}
