use crate::commands::{AppContext, Command};
use crate::confirm_yes_no;
use macc_core::ops_motif::{apply_preset_to_config, calculate_trust_summary};
use macc_core::Result;
use std::path::PathBuf;

pub struct StartCommand {
    app: AppContext,
    intent: Option<String>,
    dry_run: bool,
    web: bool,
    tui: bool,
    profile: Option<String>,
    preset: Option<String>,
    locked: bool,
}

impl StartCommand {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        app: AppContext,
        intent: Option<String>,
        dry_run: bool,
        web: bool,
        tui: bool,
        profile: Option<String>,
        preset: Option<String>,
        locked: bool,
    ) -> Self {
        Self {
            app,
            intent,
            dry_run,
            web,
            tui,
            profile,
            preset,
            locked,
        }
    }
}

impl Command for StartCommand {
    fn run(&self) -> Result<()> {
        println!("MACC Cockpit guided entry point [macc start]");

        let paths = self.app.project_paths()?;
        if self.locked {
            let lock_path = paths.macc_dir.join("macc.lock.yaml");
            if !lock_path.exists() {
                return Err(macc_core::MaccError::Validation(
                    "Lock file not found. Cannot proceed under --locked.".to_string(),
                ));
            }
            let lock_str =
                std::fs::read_to_string(&lock_path).map_err(|e| macc_core::MaccError::Io {
                    path: lock_path.to_string_lossy().into(),
                    action: "read macc.lock.yaml".into(),
                    source: e,
                })?;
            let lock: macc_core::ops_motif::LockManifest = serde_yaml::from_str(&lock_str)
                .map_err(|e| macc_core::MaccError::Config {
                    path: lock_path.to_string_lossy().into(),
                    source: e,
                })?;
            let report = macc_core::ops_motif::verify_lock_manifest(&paths, &lock)?;
            if !report.matches {
                eprintln!("Lock verification: DRIFT DETECTED.");
                for d in report.drift {
                    eprintln!("  - {}", d);
                }
                return Err(macc_core::MaccError::Validation(
                    "Lock file check failed due to drift under --locked constraint.".to_string(),
                ));
            }
            println!("Lock verification: SUCCESS. Environment matches lock perfectly.");
        }

        let mut planned_writes = Vec::new();

        // =====================================================================
        // 1. DETECT
        // =====================================================================
        println!("\n=== [1/7] DETECT ===");
        println!("Detecting repository files and configuration state...");

        let git_exists = paths.macc_dir.join("../.git").exists();
        let config_exists = paths.config_path.exists();
        let lock_path = paths.macc_dir.join("macc.lock.yaml");
        let lock_exists = lock_path.exists();

        // Active tool scanner using ToolSpecLoader and Doctor system checks
        println!("Scanning PATH for installed AI helper commands...");
        let loader = macc_core::tool::loader::ToolSpecLoader::new(
            macc_core::tool::loader::ToolSpecLoader::default_search_paths(&paths.root),
        );
        let (specs, _) = loader.load_all_with_embedded();
        let mut checks = macc_core::doctor::checks_for_enabled_tools(&specs);
        macc_core::doctor::run_checks(&mut checks);

        let mut detected_tools = Vec::new();
        for check in &checks {
            if check.status == macc_core::doctor::ToolStatus::Installed {
                if let Some(ref tool_id) = check.tool_id {
                    detected_tools.push(tool_id.clone());
                }
            }
        }
        detected_tools.sort();
        detected_tools.dedup();

        println!(
            "- Git repository: {}",
            if git_exists { "detected" } else { "none" }
        );
        println!(
            "- MACC configuration: {}",
            if config_exists { "detected" } else { "none" }
        );
        println!(
            "- Lockfile: {}",
            if lock_exists { "detected" } else { "none" }
        );
        println!(
            "- Detected installed AI tools: {}",
            if detected_tools.is_empty() {
                "none".to_string()
            } else {
                detected_tools.join(", ")
            }
        );

        // =====================================================================
        // 2. DIAGNOSE
        // =====================================================================
        println!("\n=== [2/7] DIAGNOSE ===");
        let mut warnings = Vec::new();
        if !git_exists {
            warnings.push(
                "Not inside a Git repository. Certain worktree commands may fail.".to_string(),
            );
        }
        if detected_tools.is_empty() {
            warnings.push(
                "No local AI developer tools found on PATH. Install assistants/tools first."
                    .to_string(),
            );
        }
        let git_identity_missing = macc_core::git::missing_git_identity_fields(&paths.root);
        if !git_identity_missing.is_empty() {
            warnings.push(format!(
                "Git identity fields ({}) are unconfigured. Commits will fail.",
                git_identity_missing.join(", ")
            ));
        }

        if warnings.is_empty() {
            println!("No configuration or dependency issues diagnosed.");
        } else {
            for w in &warnings {
                println!("  [WARNING] {}", w);
            }
        }

        // =====================================================================
        // 3. RESOLVE
        // =====================================================================
        println!("\n=== [3/7] RESOLVE ===");
        let mut config = if config_exists {
            macc_core::load_canonical_config(&paths.config_path)?
        } else {
            println!("No configuration found. Planning initialization of default config...");
            planned_writes.push(format!(
                "Initialize default canonical configuration at {}",
                paths.config_path.display()
            ));
            macc_core::init(&paths, false)?;
            macc_core::load_canonical_config(&paths.config_path)?
        };

        // Determine user-configured PRD path
        let prd_filename = config
            .automation
            .coordinator
            .as_ref()
            .and_then(|c| c.prd_file.clone())
            .unwrap_or_else(|| "prd.json".to_string());
        let prd_path = paths.root.join(&prd_filename);
        let prd_exists = prd_path.exists();
        println!("- Resolved PRD path: {}", prd_path.display());

        // Restore profile if specified
        if let Some(ref profile_name) = self.profile {
            println!("Applying config profile '{}'...", profile_name);
            let mgr = macc_core::profile::ProfileManager::new()?;
            config = mgr.restore(profile_name, &config, None)?;
            planned_writes.push(format!(
                "Merge profile '{}' into configuration",
                profile_name
            ));
        }

        // Apply preset if specified
        if let Some(ref preset_name) = self.preset {
            println!("Applying preset: {}...", preset_name);
            apply_preset_to_config(&mut config, preset_name)?;
            planned_writes.push(format!(
                "Apply preset '{}' overrides to coordinator config",
                preset_name
            ));
        }

        if self.preset.is_some() || self.profile.is_some() {
            planned_writes.push(format!(
                "Save updated configuration changes to {}",
                paths.config_path.display()
            ));
        }

        // 4. Guided Intent Selector
        let intent_str = match &self.intent {
            Some(i) => i.clone(),
            None => {
                println!("\nWhat do you want to do?");
                println!("[1] Configure tools (detect assistants, catalog templates)");
                println!("[2] Run one task (select/run task in worktree)");
                println!("[3] Run a batch (validate PRD, run multiple tasks)");
                println!("[4] Inspect existing project (status, logs, diagnostics)");

                let selection = if self.tui {
                    "4".to_string()
                } else {
                    use std::io::{self, Write};
                    print!("Select intent [1-4]: ");
                    let _ = io::stdout().flush();
                    let mut input = String::new();
                    if io::stdin().read_line(&mut input).is_ok() {
                        input.trim().to_string()
                    } else {
                        "4".to_string()
                    }
                };
                match selection.as_str() {
                    "1" => "configure-tools".to_string(),
                    "2" => "run-one-task".to_string(),
                    "3" => "run-batch".to_string(),
                    _ => "inspect-project".to_string(),
                }
            }
        };
        println!("Selected intent: {}", intent_str);

        // PRD Setup Option
        let mut prd_selection = "3".to_string();
        let mut import_path_to_use: Option<PathBuf> = None;

        if !prd_exists {
            println!("\nNo PRD task spec found at {}.", prd_path.display());
            println!("How would you like to handle PRD task setup?");
            println!("[1] Create a minimal PRD (placeholder task list)");
            println!("[2] Import an existing PRD from file path");
            println!("[3] Skip PRD setup (configuration-only)");

            prd_selection = if self.tui {
                "3".to_string()
            } else {
                use std::io::{self, Write};
                print!("Select setup option [1-3]: ");
                let _ = io::stdout().flush();
                let mut input = String::new();
                if io::stdin().read_line(&mut input).is_ok() {
                    input.trim().to_string()
                } else {
                    "3".to_string()
                }
            };

            match prd_selection.as_str() {
                "1" => {
                    planned_writes.push(format!(
                        "Create a minimal PRD task specification at {}",
                        prd_path.display()
                    ));
                }
                "2" => {
                    if self.tui {
                        println!("Skipping import in non-interactive/TUI mode.");
                    } else {
                        use std::io::{self, Write};
                        print!("Enter path of the PRD file to import: ");
                        let _ = io::stdout().flush();
                        let mut import_path_str = String::new();
                        if io::stdin().read_line(&mut import_path_str).is_ok() {
                            let import_path = std::path::Path::new(import_path_str.trim());
                            if import_path.exists() {
                                planned_writes.push(format!(
                                    "Import PRD from {} to {}",
                                    import_path.display(),
                                    prd_path.display()
                                ));
                                import_path_to_use = Some(import_path.to_path_buf());
                            } else {
                                println!("Import path does not exist. Skipping PRD import.");
                            }
                        }
                    }
                }
                _ => {}
            }
        } else {
            println!("Accepting existing PRD task specification.");
        }

        // Establish reproducibility baseline lock file if missing
        if !lock_exists {
            planned_writes.push(format!(
                "Create environment lock manifest at {}",
                lock_path.display()
            ));
        }

        // =====================================================================
        // 4. PREVIEW
        // =====================================================================
        println!("\n=== [4/7] PREVIEW ===");
        let trust = calculate_trust_summary(&paths, &config);
        println!("Trust State     : {:?}", trust.state);
        println!(
            "Local only      : {}",
            if trust.local_only { "yes" } else { "no" }
        );
        println!(
            "Terminal access : {}",
            if trust.terminal_enabled {
                "enabled"
            } else {
                "disabled"
            }
        );
        println!(
            "Backups         : {}",
            if trust.backups_ready {
                "ready"
            } else {
                "missing"
            }
        );
        println!(
            "Catalog         : {}",
            if trust.catalog_pinned {
                "pinned"
            } else {
                "unpinned"
            }
        );

        println!("\nPlanned writes & actions:");
        if planned_writes.is_empty() {
            println!("  (none - cockpit will boot in read-only mode)");
        } else {
            for action in &planned_writes {
                println!("  - {}", action);
            }
        }

        // =====================================================================
        // 5. CONFIRM
        // =====================================================================
        println!("\n=== [5/7] CONFIRM ===");
        if self.dry_run {
            println!("Dry-run mode: no changes will be applied. Exiting.");
            return Ok(());
        }

        let proceed = confirm_yes_no("Proceed with startup and apply planned changes [y/N]? ")?;
        if !proceed {
            println!("Startup cancelled by user.");
            return Ok(());
        }

        // =====================================================================
        // 6. APPLY
        // =====================================================================
        println!("\n=== [6/7] APPLY ===");
        if self.preset.is_some() || self.profile.is_some() {
            println!("Saving config updates to disk...");
            let serialized = serde_yaml::to_string(&config).map_err(|e| {
                macc_core::MaccError::Validation(format!("serialize config: {}", e))
            })?;
            std::fs::write(&paths.config_path, serialized).map_err(|e| {
                macc_core::MaccError::Io {
                    path: paths.config_path.to_string_lossy().into(),
                    action: "write updated config".into(),
                    source: e,
                }
            })?;
        }

        // Generate or Import PRD
        if !prd_exists {
            match prd_selection.as_str() {
                "1" => {
                    let minimal_prd = r#"{
  "tasks": [
    {
      "id": "TASK-001",
      "description": "Verify MACC environment is healthy",
      "status": "todo"
    }
  ]
}"#;
                    std::fs::write(&prd_path, minimal_prd).map_err(|e| {
                        macc_core::MaccError::Io {
                            path: prd_path.to_string_lossy().into(),
                            action: "create minimal prd".into(),
                            source: e,
                        }
                    })?;
                    println!("Created minimal PRD at {}", prd_path.display());
                }
                "2" => {
                    if let Some(import_from) = &import_path_to_use {
                        std::fs::copy(import_from, &prd_path).map_err(|e| {
                            macc_core::MaccError::Io {
                                path: prd_path.to_string_lossy().into(),
                                action: "import prd file".into(),
                                source: e,
                            }
                        })?;
                        println!(
                            "Imported PRD from {} to {}",
                            import_from.display(),
                            prd_path.display()
                        );
                    }
                }
                _ => {}
            }
        }

        // Create lock file if missing
        if !lock_exists {
            println!("Establishing reproducibility lock baseline...");
            let lock = macc_core::ops_motif::generate_lock_manifest(&paths, &config)?;
            let serialized_lock = serde_yaml::to_string(&lock).map_err(|e| {
                macc_core::MaccError::Validation(format!("serialize lock manifest: {}", e))
            })?;
            std::fs::write(&lock_path, serialized_lock).map_err(|e| macc_core::MaccError::Io {
                path: lock_path.to_string_lossy().into(),
                action: "write baseline lock manifest".into(),
                source: e,
            })?;
        }

        // =====================================================================
        // 7. LAUNCH
        // =====================================================================
        println!("\n=== [7/7] LAUNCH ===");
        if self.web {
            println!("Launching Web dashboard...");
            let web_cmd = crate::commands::web::WebCommand::new(
                self.app.clone(),
                "127.0.0.1".to_string(),
                None,
                None,
                false,
            );
            web_cmd.run()?;
        } else if self.tui {
            println!("Launching TUI dashboard...");
            macc_tui::run_tui().map_err(|e| macc_core::MaccError::Io {
                path: "tui".into(),
                action: "run_tui".into(),
                source: std::io::Error::other(e.to_string()),
            })?;
        } else {
            println!("Cockpit initialization completed. Open Web or TUI client to monitor.");
        }

        Ok(())
    }
}
