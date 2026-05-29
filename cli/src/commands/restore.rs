use crate::commands::AppContext;
use crate::commands::Command;
use macc_core::service::interaction::InteractionHandler;
use macc_core::{MaccError, Result};

struct CliBackupsUi;

impl InteractionHandler for CliBackupsUi {
    fn info(&self, message: &str) {
        println!("{}", message);
    }

    fn warn(&self, message: &str) {
        eprintln!("{}", message);
    }

    fn error(&self, message: &str) {
        eprintln!("{}", message);
    }

    fn confirm_yes_no(&self, prompt: &str) -> Result<bool> {
        crate::confirm_yes_no(prompt)
    }
}

impl macc_core::service::backups::BackupsUi for CliBackupsUi {
    fn open_in_editor(&self, path: &std::path::Path, command: &str) -> Result<()> {
        macc_core::service::task_runner::open_in_editor(path, command)
    }
}

pub struct RestoreCommand {
    app: AppContext,
    name: Option<String>,
    latest: bool,
    user: bool,
    backup: Option<String>,
    dry_run: bool,
    yes: bool,
    apply: bool,
    config_only: bool,
    sessions: bool,
    no_sessions: bool,
    include_logs: bool,
}

impl RestoreCommand {
    pub fn new(
        app: AppContext,
        name: Option<String>,
        latest: bool,
        user: bool,
        backup: Option<String>,
        dry_run: bool,
        yes: bool,
        apply: bool,
        config_only: bool,
        sessions: bool,
        no_sessions: bool,
        include_logs: bool,
    ) -> Self {
        Self {
            app,
            name,
            latest,
            user,
            backup,
            dry_run,
            yes,
            apply,
            config_only,
            sessions,
            no_sessions,
            include_logs,
        }
    }
}

impl Command for RestoreCommand {
    fn run(&self) -> Result<()> {
        let paths = self.app.project_paths()?;

        if !self.dry_run {
            crate::commands::gate_cli_mutation(&paths.root)?;
        }

        // Determine if this is a save bundle restore
        let has_save_bundle_args = self.name.is_some()
            || self.apply
            || self.config_only
            || self.sessions
            || self.no_sessions
            || self.include_logs;

        // Also check if --latest refers to a save bundle (we prefer save bundle if there are saves)
        let is_save_bundle_restore = has_save_bundle_args || (self.latest && {
            let matches = macc_core::save::detect_matching_saves(&paths).unwrap_or_default();
            !matches.is_empty() && self.backup.is_none() && !self.user
        });

        if is_save_bundle_restore {
            let name = match &self.name {
                Some(n) => n.clone(),
                None => {
                    let matches = macc_core::save::detect_matching_saves(&paths)?;
                    if matches.is_empty() {
                        return Err(MaccError::Validation("No matching MACC saves found for this repository.".to_string()));
                    }
                    matches[0].0.name.clone()
                }
            };

            let opts = macc_core::save::RestoreOptions {
                apply: self.apply,
                config_only: self.config_only,
                sessions: self.sessions,
                no_sessions: self.no_sessions,
                include_logs: self.include_logs,
                dry_run: self.dry_run,
                yes: self.yes,
            };

            if !self.yes {
                println!("Restoring MACC save \"{}\"...", name);
                if !crate::confirm_yes_no("Proceed with restore [y/N]? ")? {
                    println!("Restore cancelled.");
                    return Ok(());
                }
            }

            macc_core::save::restore_save_bundle(&paths, &name, &opts)?;
            println!("Restore completed successfully.");

            if opts.apply {
                println!("Running macc apply...");
                crate::commands::lifecycle_support::apply(
                    &self.app,
                    None,
                    false, // dry_run
                    false, // allow_user_scope
                    false, // json
                    false, // explain
                )?;
            } else {
                println!("Next recommended command:\n  macc apply");
            }
            return Ok(());
        }

        // If no name is provided and not latest/backup, print matching saves list
        if !self.latest && self.backup.is_none() {
            println!("No save name provided.\n");
            let matches = macc_core::save::detect_matching_saves(&paths)?;
            if !matches.is_empty() {
                println!("Matching saves:");
                for (idx, (m, strength)) in matches.iter().enumerate() {
                    println!("  {}. {} (Match: {})", idx + 1, m.name, strength.as_str());
                }
                println!("\nRun:\n  macc restore <name>");
            } else {
                println!("No matching MACC saves found for this repository.");
            }
            return Ok(());
        }

        // Otherwise fallback to safety backup restore
        self.app.engine.backups_restore(
            &paths,
            self.user,
            self.backup.as_deref(),
            self.latest,
            self.dry_run,
            self.yes,
            &CliBackupsUi,
        )
    }
}
