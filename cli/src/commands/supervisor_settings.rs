//! Supervisor configuration commands; no coordinator or AI side effects.
use super::AppContext;
use macc_core::supervisor::{settings, validation};
use macc_core::{MaccError, Result};

#[derive(clap::Subcommand, Clone, Debug)]
pub enum ConfigCommand {
    /// Show effective values, available fields and help
    Show,
    /// Set one canonical field (empty value restores optional overrides)
    Set { key: String, value: String },
    /// Check configuration, ToolSpec resolution and validation discovery without running AI or tests
    Check,
}
pub fn run(app: &AppContext, command: &ConfigCommand) -> Result<()> {
    let paths = app.project_paths()?;
    let canonical = app.canonical_config()?;
    let config = canonical.automation.supervisor.clone().unwrap_or_default();
    match command {
        ConfigCommand::Show => {
            println!(
                "{}",
                serde_yaml::to_string(&config).map_err(|e| MaccError::Validation(e.to_string()))?
            );
            for f in settings::FIELDS {
                println!("{}: {}", f.key, f.help);
            }
        }
        ConfigCommand::Set { key, value } => {
            macc_core::config::transaction::update(&paths, None, |canonical| {
                let config = canonical
                    .automation
                    .supervisor
                    .get_or_insert_with(Default::default);
                settings::set(config, key, value)
            })?;
            println!("Saved automation.supervisor.{key}");
        }
        ConfigCommand::Check => {
            config.validate()?;
            let tool =
                super::supervisor_runtime::configured_tool(&paths.root, &canonical, &config)?;
            println!("AI tool: {tool}");
            if config.repair_enabled {
                let commands = validation::commands(&paths.root, &config)?;
                for command in commands {
                    println!("Check: {command}");
                }
            } else {
                println!("Diagnosis only: automatic repair disabled");
            }
            println!(
                "tests_enabled={} ai_verification_enabled={} auto_restart={}",
                config.tests_enabled, config.ai_verification_enabled, config.auto_restart
            );
            println!("Configuration ready; commands were not executed.");
        }
    }
    Ok(())
}
