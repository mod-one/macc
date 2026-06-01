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

        // Human-readable extended output — grouped by category (spec §5.2).
        if !findings.is_empty() {
            print_grouped_findings(&findings);
        }

        tool_check_result
    }
}

/// Print findings grouped by category, matching spec §5.2 output format.
fn print_grouped_findings(findings: &[macc_core::doctor::DiagnosticFinding]) {
    // Category display order and labels (spec §5.2).
    let groups: &[(&str, &str)] = &[
        ("project", "Project"),
        ("git", "Git"),
        ("worktrees", "Worktrees"),
        ("coordinator", "Coordinator"),
        ("tools", "Tools"),
        ("tasks", "Tasks"),
    ];

    for (category, label) in groups {
        let group: Vec<_> = findings.iter().filter(|f| f.category == *category).collect();
        if group.is_empty() {
            continue;
        }
        println!();
        println!("{}", label);
        for f in &group {
            let symbol = match f.severity {
                DiagnosticSeverity::Ok => "  ✅",
                DiagnosticSeverity::Info => "  ℹ️",
                DiagnosticSeverity::Warning => "  ⚠️",
                DiagnosticSeverity::Error => "  ❌",
            };
            println!("{} {}", symbol, f.title);
            if !matches!(f.severity, DiagnosticSeverity::Ok) && !f.message.is_empty() {
                println!("     {}", f.message);
                if let Some(action) = &f.recommended_action {
                    for line in action.lines() {
                        println!("     {}", line);
                    }
                }
            }
        }
    }

    // Uncategorised findings
    let other: Vec<_> = findings
        .iter()
        .filter(|f| !groups.iter().any(|(cat, _)| f.category == *cat))
        .collect();
    if !other.is_empty() {
        println!();
        println!("Other");
        for f in &other {
            let symbol = match f.severity {
                DiagnosticSeverity::Ok => "  ✅",
                DiagnosticSeverity::Info => "  ℹ️",
                DiagnosticSeverity::Warning => "  ⚠️",
                DiagnosticSeverity::Error => "  ❌",
            };
            println!("{} {}", symbol, f.title);
        }
    }

    // Readiness summary (spec §5.2 last block).
    println!();
    println!("Readiness");
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
