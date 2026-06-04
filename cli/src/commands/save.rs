use crate::commands::AppContext;
use crate::commands::Command;
use macc_core::{MaccError, Result};

pub enum SaveAction {
    Create {
        name: String,
        overwrite: bool,
        description: Option<String>,
        only: Option<String>,
        no_sessions: bool,
        include_logs: bool,
        log_max_size: String,
        log_since: String,
        redact_logs: bool,
        dry_run: bool,
    },
    List {
        matching: bool,
    },
    Show {
        name: String,
    },
    Delete {
        name: String,
        yes: bool,
    },
}

pub struct SaveCommand {
    app: AppContext,
    action: SaveAction,
}

impl SaveCommand {
    pub fn new(app: AppContext, action: SaveAction) -> Self {
        Self { app, action }
    }
}

impl Command for SaveCommand {
    fn run(&self) -> Result<()> {
        let paths = self.app.project_paths()?;

        match &self.action {
            SaveAction::Create {
                name,
                overwrite,
                description,
                only,
                no_sessions,
                include_logs,
                log_max_size,
                log_since,
                redact_logs,
                dry_run,
            } => {
                let mut name = name.clone();
                let mut overwrite = *overwrite;

                let saves_dir = macc_core::save::user_saves_dir();
                if let Some(ref dir) = saves_dir {
                    let target_save_dir = dir.join(&name);
                    if target_save_dir.exists() && !overwrite {
                        use std::io::IsTerminal;
                        let term_interactive = std::io::stdin().is_terminal()
                            && std::env::var("CARGO_MANIFEST_DIR").is_err();
                        if term_interactive {
                            println!("Save \"{}\" already exists.\n", name);
                            println!("Choose:");
                            println!("  [O] Overwrite");
                            println!("  [N] Create a new save name");
                            println!("  [A] Abort");

                            use std::io::{self, Write};
                            print!("> ");
                            io::stdout().flush().ok();
                            let mut input = String::new();
                            io::stdin().read_line(&mut input).ok();
                            let choice = input.trim().to_lowercase();

                            if choice == "o" || choice == "overwrite" {
                                overwrite = true;
                            } else if choice == "n" || choice == "new" {
                                print!("Enter new save name: ");
                                io::stdout().flush().ok();
                                let mut new_name = String::new();
                                io::stdin().read_line(&mut new_name).ok();
                                let trimmed = new_name.trim().to_string();
                                if !trimmed.is_empty() {
                                    name = trimmed;
                                } else {
                                    println!("Save aborted: Empty name provided.");
                                    return Ok(());
                                }
                            } else {
                                println!("Save aborted.");
                                return Ok(());
                            }
                        }
                    }
                }

                let opts = macc_core::save::SaveOptions {
                    description: description.clone(),
                    overwrite,
                    only: only.clone(),
                    no_sessions: *no_sessions,
                    include_logs: *include_logs,
                    log_max_size: log_max_size.clone(),
                    log_since: log_since.clone(),
                    redact_logs: *redact_logs,
                    dry_run: *dry_run,
                    include_prd: false,
                    include_state: false,
                    handle_secrets: None,
                };

                let manifest =
                    crate::commands::create_save_bundle_interactive(&paths, &name, opts)?;
                println!("Saved MACC bundle \"{}\".\n", manifest.name);
                println!("Included:");
                println!(
                    "  {} config",
                    if manifest.includes.config { "✓" } else { "-" }
                );
                println!(
                    "  {} coordinator sessions",
                    if manifest.includes.coordinator_sessions {
                        "✓"
                    } else {
                        "-"
                    }
                );
                println!(
                    "  {} catalogs",
                    if manifest.includes.catalogs {
                        "✓"
                    } else {
                        "-"
                    }
                );
                if manifest.includes.logs {
                    println!("  ✓ logs, redacted");
                }

                println!("\nExcluded:");
                println!("  - worktrees");
                println!("  - cache");
                println!("  - generated files");
                if !manifest.includes.logs {
                    println!("  - logs");
                }

                if !dry_run {
                    if let Some(saves_dir) = macc_core::save::user_saves_dir() {
                        println!("\nLocation:\n  {}", saves_dir.join(name).display());
                    }
                }
            }
            SaveAction::List { matching } => {
                let list = if *matching {
                    let matches = macc_core::save::detect_matching_saves(&paths)?;
                    matches.into_iter().map(|m| m.0).collect()
                } else {
                    macc_core::save::list_save_bundles()?
                };

                if list.is_empty() {
                    println!("No saves found.");
                } else {
                    println!("MACC saves:");
                    for m in list {
                        println!("\n  {}", m.name);
                        println!("    Saved: {}", m.created_at);
                        println!("    Repo: {}", m.repository.root_name);
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
                        if m.includes.logs {
                            incls.push("logs");
                        }
                        println!("    Includes: {}", incls.join(", "));
                    }
                }
            }
            SaveAction::Show { name } => {
                let saves = macc_core::save::list_save_bundles()?;
                let manifest = saves.iter().find(|m| m.name == *name).ok_or_else(|| {
                    MaccError::Validation(format!("Save bundle '{}' not found.", name))
                })?;

                let current_identity = macc_core::save::get_repository_identity(&paths.root);
                let match_strength = macc_core::save::compute_match_strength(
                    &current_identity,
                    &manifest.repository,
                );

                println!("Save: {}", manifest.name);
                if let Some(desc) = &manifest.description {
                    println!("Description: {}", desc);
                }
                println!("Created: {}", manifest.created_at);
                println!("Repository match: {}", match_strength.as_str());
                println!("Includes:");
                println!(
                    "  {} config",
                    if manifest.includes.config { "✓" } else { "-" }
                );
                println!(
                    "  {} coordinator sessions",
                    if manifest.includes.coordinator_sessions {
                        "✓"
                    } else {
                        "-"
                    }
                );
                println!(
                    "  {} catalogs",
                    if manifest.includes.catalogs {
                        "✓"
                    } else {
                        "-"
                    }
                );
                println!("  {} logs", if manifest.includes.logs { "✓" } else { "-" });

                println!("\nExplicitly excluded:");
                println!("  - worktrees");
                println!("  - cache");
                println!("  - generated files");
                println!("  - secrets");

                println!("\nRecommended restore:");
                println!("  macc restore {} --apply", manifest.name);
            }
            SaveAction::Delete { name, yes } => {
                if !*yes {
                    let prompt = format!(
                        "Are you sure you want to delete save bundle '{}' [y/N]? ",
                        name
                    );
                    if !crate::confirm_yes_no(&prompt)? {
                        println!("Delete cancelled.");
                        return Ok(());
                    }
                }
                macc_core::save::delete_save_bundle(name)?;
                println!("Save bundle '{}' deleted.", name);
            }
        }

        Ok(())
    }
}
