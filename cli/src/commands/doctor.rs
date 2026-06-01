use crate::commands::AppContext;
use crate::commands::Command;
use macc_core::doctor::{collect_all_findings, fix_git_identity, DiagnosticSeverity};
use macc_core::Result;

pub struct DoctorCommand {
    app: AppContext,
    fix: bool,
    json: bool,
    git_name: Option<String>,
    git_email: Option<String>,
    coordinator_only: bool,
}

impl DoctorCommand {
    pub fn new(
        app: AppContext,
        fix: bool,
        json: bool,
        git_name: Option<String>,
        git_email: Option<String>,
        coordinator_only: bool,
    ) -> Self {
        Self {
            app,
            fix,
            json,
            git_name,
            git_email,
            coordinator_only,
        }
    }
}

impl Command for DoctorCommand {
    fn run(&self) -> Result<()> {
        let paths = self.app.project_paths()?;

        if self.fix {
            crate::commands::gate_cli_mutation(&paths.root)?;
        }

        // Apply git identity fix if requested.
        if self.fix {
            if let (Some(name), Some(email)) = (&self.git_name, &self.git_email) {
                match fix_git_identity(&paths.root, name, email) {
                    Ok(()) => {
                        if !self.json {
                            println!("Fixed: Git identity set (user.name={}, user.email={}).", name, email);
                        }
                    }
                    Err(e) => {
                        if !self.json {
                            eprintln!("Failed to fix Git identity: {}", e);
                        }
                    }
                }
            }
        }

        // Run the legacy tool checks (existing behavior).
        let tool_check_result = self.app.engine.project_run_doctor(
            &paths,
            self.fix,
            &crate::services::interaction::CliInteraction,
        );

        // Run new extended diagnostic findings (spec §5.3).
        let max_parallel = resolve_max_parallel(&paths);
        let mut findings = collect_all_findings(&paths, max_parallel);

        if self.coordinator_only {
            findings.retain(|f| f.category == "coordinator");
        }

        if self.json {
            let blocking = findings.iter().any(|f| f.is_blocking());
            let output = serde_json::json!({
                "ready": !blocking,
                "findings": findings,
            });
            println!("{}", serde_json::to_string_pretty(&output).unwrap_or_default());
            if blocking {
                return Err(macc_core::MaccError::Validation(
                    "Doctor checks found blocking issues.".into(),
                ));
            }
            return Ok(());
        }

        // Human-readable extended output.
        if !findings.is_empty() {
            println!();
            println!("Readiness");
            for f in &findings {
                let symbol = match f.severity {
                    DiagnosticSeverity::Ok => "  ✅",
                    DiagnosticSeverity::Info => "  ℹ️",
                    DiagnosticSeverity::Warning => "  ⚠️",
                    DiagnosticSeverity::Error => "  ❌",
                };
                if f.severity == DiagnosticSeverity::Ok {
                    println!("{} {}", symbol, f.title);
                } else {
                    println!("{} {}", symbol, f.title);
                    println!("     {}", f.message);
                    if let Some(action) = &f.recommended_action {
                        for line in action.lines() {
                            println!("     {}", line);
                        }
                    }
                }
            }
            println!();

            let blocking: Vec<_> = findings.iter().filter(|f| f.is_blocking()).collect();
            if blocking.is_empty() {
                println!("  ✅ Ready to dispatch a task");
            } else {
                println!("  ❌ Not ready to dispatch a task");
                println!("     Blocking issues:");
                for (i, f) in blocking.iter().enumerate() {
                    println!("       {}. {}", i + 1, f.title);
                }
            }
        }

        tool_check_result
    }
}

fn resolve_max_parallel(paths: &macc_core::ProjectPaths) -> u32 {
    let config_path = paths.macc_dir.join("macc.yaml");
    if let Ok(content) = std::fs::read_to_string(config_path) {
        if let Ok(value) = serde_yaml::from_str::<serde_yaml::Value>(&content) {
            if let Some(n) = value
                .get("coordinator")
                .and_then(|c| c.get("max_parallel"))
                .and_then(|v| v.as_u64())
            {
                return n as u32;
            }
        }
    }
    2 // conservative default
}
