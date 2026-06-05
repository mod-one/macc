use crate::commands::{AppContext, Command};
use macc_core::ops_motif::{apply_preset_to_config, get_setting_descriptors, SettingCategory};
use macc_core::Result;
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
        /// Reference profile to trace values from
        #[arg(long)]
        profile: Option<String>,
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
            SettingsCommands::Show {
                advanced,
                admin,
                profile,
            } => {
                let max_category = if *admin {
                    SettingCategory::Admin
                } else if *advanced {
                    SettingCategory::Advanced
                } else {
                    SettingCategory::Basic
                };

                // Read raw yaml to check which keys are explicitly in the project config
                let raw_yaml: Option<serde_yaml::Value> = if paths.config_path.exists() {
                    if let Ok(content) = fs::read_to_string(&paths.config_path) {
                        serde_yaml::from_str(&content).ok()
                    } else {
                        None
                    }
                } else {
                    None
                };

                // Read raw profile yaml if a profile name is supplied
                let raw_profile_yaml: Option<serde_yaml::Value> = if let Some(ref p) = profile {
                    let mgr = macc_core::profile::ProfileManager::new().ok();
                    if let Some(m) = mgr {
                        let path = m.profile_path(p);
                        if path.exists() {
                            if let Ok(content) = fs::read_to_string(&path) {
                                serde_yaml::from_str::<serde_yaml::Value>(&content)
                                    .ok()
                                    .and_then(|v| v.get("config").cloned())
                            } else {
                                None
                            }
                        } else {
                            None
                        }
                    } else {
                        None
                    }
                } else {
                    None
                };

                // Restore profile in-memory to resolve its values
                if let Some(ref p) = profile {
                    let mgr = macc_core::profile::ProfileManager::new()?;
                    config = mgr.restore(p, &config, None)?;
                }

                // Resolve the config to avoid hardcoded fallbacks
                let resolved = macc_core::config::CoordinatorConfigResolved::resolve(
                    config.automation.coordinator.as_ref(),
                );

                println!("====================================================");
                println!("                MACC SETTINGS                       ");
                println!("====================================================");

                let descriptors = get_setting_descriptors();
                for desc in descriptors {
                    // Check categories
                    let show = match desc.category {
                        SettingCategory::Basic => true,
                        SettingCategory::Advanced => {
                            max_category == SettingCategory::Advanced
                                || max_category == SettingCategory::Admin
                        }
                        SettingCategory::Admin => max_category == SettingCategory::Admin,
                    };

                    if show {
                        let val = match desc.name.as_str() {
                            "quiet" => config.settings.quiet.to_string(),
                            "offline" => config.settings.offline.to_string(),
                            "web_port" => config.settings.web_port.unwrap_or(3450).to_string(),
                            "coordinator_tool" => resolved
                                .coordinator_tool
                                .clone()
                                .unwrap_or_else(|| "Auto-select".to_string()),
                            "reference_branch" => resolved.reference_branch.clone(),
                            "max_parallel" => resolved.max_parallel.to_string(),
                            "timeout_seconds" => resolved.timeout_seconds.to_string(),
                            "prd_file" => resolved
                                .prd_file
                                .clone()
                                .unwrap_or_else(|| "prd.json".to_string()),
                            "max_dispatch" => resolved.max_dispatch.to_string(),
                            "phase_runner_max_attempts" => {
                                resolved.phase_runner_max_attempts.to_string()
                            }
                            "merge_ai_fix" => resolved.merge_ai_fix.to_string(),
                            "safety_policy" => resolved.safety_policy.clone(),
                            "destructive_actions" => resolved.destructive_actions.clone(),
                            "storage_mode" => resolved.storage_mode.clone(),
                            "task_registry_file" => {
                                resolved.task_registry_file.clone().unwrap_or_else(|| {
                                    ".macc/automation/task/task_registry.json".to_string()
                                })
                            }
                            _ => desc.default_value.clone(),
                        };

                        // Determine source of value
                        let source = {
                            let name = desc.name.as_str();
                            let overrides = &self.app.overrides;
                            if (name == "quiet" && overrides.quiet.is_some())
                                || (name == "offline" && overrides.offline.is_some())
                            {
                                "CLI override"
                            } else if raw_profile_yaml
                                .as_ref()
                                .map(|py| {
                                    if name == "quiet" || name == "offline" || name == "web_port" {
                                        py.get("settings").and_then(|s| s.get(name)).is_some()
                                    } else {
                                        py.get("automation")
                                            .and_then(|a| a.get("coordinator"))
                                            .and_then(|c| c.get(name))
                                            .is_some()
                                    }
                                })
                                .unwrap_or(false)
                            {
                                "profile"
                            } else if raw_yaml
                                .as_ref()
                                .map(|y| {
                                    if name == "quiet" || name == "offline" || name == "web_port" {
                                        y.get("settings").and_then(|s| s.get(name)).is_some()
                                    } else {
                                        y.get("automation")
                                            .and_then(|a| a.get("coordinator"))
                                            .and_then(|c| c.get(name))
                                            .is_some()
                                    }
                                })
                                .unwrap_or(false)
                            {
                                "project config"
                            } else {
                                "default"
                            }
                        };

                        println!("Name:           {}", desc.name);
                        println!("Category:       {:?}", desc.category);
                        println!("Value:          {}", val);
                        println!("Source:         {}", source);
                        println!("Description:    {}", desc.description);
                        println!("Impact:         {}", desc.impact_summary);
                        println!("Restart Req:    {}", desc.restart_required);
                        println!("Examples:       {}", desc.examples.join(", "));
                        println!("----------------------------------------------------");
                    }
                }
                println!("====================================================");
            }
            SettingsCommands::Preset { name } => {
                println!("Applying preset: {}...", name);
                apply_preset_to_config(&mut config, name)?;

                let serialized = serde_yaml::to_string(&config).map_err(|e| {
                    macc_core::MaccError::Validation(format!("serialize config: {}", e))
                })?;
                fs::write(&paths.config_path, serialized).map_err(|e| {
                    macc_core::MaccError::Io {
                        path: paths.config_path.to_string_lossy().into(),
                        action: "write updated config with preset".into(),
                        source: e,
                    }
                })?;
                println!("Preset successfully applied and saved to config.");
            }
        }

        Ok(())
    }
}
