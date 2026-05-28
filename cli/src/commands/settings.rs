use crate::commands::{AppContext, Command};
use macc_core::Result;
use macc_core::ops_motif::{get_setting_descriptors, SettingCategory, apply_preset_to_config};
use std::fs;

pub struct SettingsCommand {
    app: AppContext,
    subcommand: SettingsCommands,
}

#[derive(clap::Subcommand, Clone, Debug)]
pub enum SettingsCommands {
    /// Show settings and their descriptions
    Show {
        /// Show advanced settings
        #[arg(long)]
        advanced: bool,
        /// Show administrative settings
        #[arg(long)]
        admin: bool,
    },
    /// Apply preset config
    Preset {
        /// Preset name (conservative, balanced, throughput)
        name: String,
    },
}

impl SettingsCommand {
    pub fn new(app: AppContext, subcommand: SettingsCommands) -> Self {
        Self { app, subcommand }
    }
}

impl Command for SettingsCommand {
    fn run(&self) -> Result<()> {
        let paths = self.app.project_paths()?;
        let mut config = self.app.canonical_config()?;

        match &self.subcommand {
            SettingsCommands::Show { advanced, admin } => {
                let max_category = if *admin {
                    SettingCategory::Admin
                } else if *advanced {
                    SettingCategory::Advanced
                } else {
                    SettingCategory::Basic
                };

                println!("====================================================");
                println!("                MACC SETTINGS                       ");
                println!("====================================================");

                let descriptors = get_setting_descriptors();
                for desc in descriptors {
                    // Check categories
                    let show = match desc.category {
                        SettingCategory::Basic => true,
                        SettingCategory::Advanced => max_category == SettingCategory::Advanced || max_category == SettingCategory::Admin,
                        SettingCategory::Admin => max_category == SettingCategory::Admin,
                    };

                    if show {
                        let val = match desc.name.as_str() {
                            "quiet" => config.settings.quiet.to_string(),
                            "offline" => config.settings.offline.to_string(),
                            "web_port" => config.settings.web_port.unwrap_or(3450).to_string(),
                            "coordinator_tool" => config.automation.coordinator.as_ref()
                                .and_then(|c| c.coordinator_tool.clone())
                                .unwrap_or_else(|| "Auto-select".to_string()),
                            "reference_branch" => config.automation.coordinator.as_ref()
                                .and_then(|c| c.reference_branch.clone())
                                .unwrap_or_else(|| "master".to_string()),
                            "max_parallel" => config.automation.coordinator.as_ref()
                                .and_then(|c| c.max_parallel)
                                .unwrap_or(3).to_string(),
                            "timeout_seconds" => config.automation.coordinator.as_ref()
                                .and_then(|c| c.timeout_seconds)
                                .unwrap_or(0).to_string(),
                            "prd_file" => config.automation.coordinator.as_ref()
                                .and_then(|c| c.prd_file.clone())
                                .unwrap_or_else(|| "prd.json".to_string()),
                            "max_dispatch" => config.automation.coordinator.as_ref()
                                .and_then(|c| c.max_dispatch)
                                .unwrap_or(10).to_string(),
                            "phase_runner_max_attempts" => config.automation.coordinator.as_ref()
                                .and_then(|c| c.phase_runner_max_attempts)
                                .unwrap_or(1).to_string(),
                            "merge_ai_fix" => config.automation.coordinator.as_ref()
                                .and_then(|c| c.merge_ai_fix)
                                .unwrap_or(false).to_string(),
                            "storage_mode" => config.automation.coordinator.as_ref()
                                .and_then(|c| c.storage_mode.clone())
                                .unwrap_or_else(|| "json".to_string()),
                            "task_registry_file" => config.automation.coordinator.as_ref()
                                .and_then(|c| c.task_registry_file.clone())
                                .unwrap_or_else(|| ".macc/automation/task/task_registry.json".to_string()),
                            _ => desc.default_value.clone(),
                        };

                        println!("Name:           {}", desc.name);
                        println!("Category:       {:?}", desc.category);
                        println!("Value:          {}", val);
                        println!("Description:    {}", desc.description);
                        println!("Impact:         {}", desc.impact_summary);
                        println!("Restart Req:    {}", desc.restart_required);
                        println!("----------------------------------------------------");
                    }
                }
                println!("====================================================");
            }
            SettingsCommands::Preset { name } => {
                println!("Applying preset: {}...", name);
                apply_preset_to_config(&mut config, name)?;

                let serialized = serde_yaml::to_string(&config)
                    .map_err(|e| macc_core::MaccError::Validation(format!("serialize config: {}", e)))?;
                fs::write(&paths.config_path, serialized)
                    .map_err(|e| macc_core::MaccError::Io {
                        path: paths.config_path.to_string_lossy().into(),
                        action: "write updated config with preset".into(),
                        source: e,
                    })?;
                println!("Preset successfully applied and saved to config.");
            }
        }

        Ok(())
    }
}
