use super::{AppContext, Command};
use macc_core::skills_runner::{SkillResolver, SkillRisk, SkillRunRequest, SkillRunner};
use macc_core::{MaccError, Result};
use std::collections::HashMap;
use std::io::{self, Write};

pub struct RunCommand {
    pub app: AppContext,
    pub skill: String,
    pub tool: Option<String>,
    pub agent: Option<String>,
    pub task_id: Option<String>,
    pub scope: Option<String>,
    pub feature: Option<String>,
    pub dry_run: bool,
    pub watch: bool,
    pub json: bool,
    pub yes: bool,
}

impl Command for RunCommand {
    fn run(&self) -> Result<()> {
        let paths = self.app.project_paths()?;

        let skill = SkillResolver::resolve(&self.skill, &paths.macc_dir).ok_or_else(|| {
            MaccError::Validation(format!(
                "Skill '{}' not found. Run 'macc skills list' to see available skills.",
                self.skill
            ))
        })?;

        if self.dry_run {
            let log_path = paths
                .macc_dir
                .join("log")
                .join("run")
                .join(format!("<ts>-{}.jsonl", skill.id));
            let preview = macc_core::skills_runner::SkillDryRunPreview {
                skill_id: skill.id.clone(),
                title: skill.title.clone(),
                kind: skill.kind.as_str().to_string(),
                tool: self.tool.clone(),
                risk: skill.risk.as_str().to_string(),
                commands: skill
                    .steps
                    .iter()
                    .filter_map(|s| s.run.clone())
                    .collect(),
                writes: Vec::new(),
                context_estimate: None,
                logs_path: log_path.display().to_string(),
            };

            if self.json {
                let json = serde_json::to_string_pretty(&preview).map_err(|e| {
                    MaccError::Validation(format!("serialize error: {}", e))
                })?;
                println!("{}", json);
            } else {
                println!("Skill:   {}", preview.skill_id);
                println!("Title:   {}", preview.title);
                println!("Kind:    {}", preview.kind);
                println!("Tool:    {}", preview.tool.as_deref().unwrap_or("none"));
                println!("Risk:    {}", preview.risk);
                println!();
                if !preview.commands.is_empty() {
                    println!("Commands:");
                    for cmd in &preview.commands {
                        println!("  {}", cmd);
                    }
                } else {
                    println!("Commands: (prompt/agent skill — no local commands)");
                }
                println!();
                println!("Logs:    {}", preview.logs_path);
            }
            return Ok(());
        }

        // Risk gate
        if !self.yes {
            match skill.risk {
                SkillRisk::Caution => {
                    print!(
                        "Skill '{}' is classified as 'caution'. Proceed? [y/N] ",
                        skill.id
                    );
                    io::stdout().flush().ok();
                    let mut line = String::new();
                    io::stdin().read_line(&mut line).ok();
                    if !line.trim().eq_ignore_ascii_case("y") {
                        println!("Aborted.");
                        return Ok(());
                    }
                }
                SkillRisk::Dangerous => {
                    print!(
                        "Skill '{}' is classified as 'dangerous'. Type 'YES' to confirm: ",
                        skill.id
                    );
                    io::stdout().flush().ok();
                    let mut line = String::new();
                    io::stdin().read_line(&mut line).ok();
                    if line.trim() != "YES" {
                        println!("Aborted.");
                        return Ok(());
                    }
                }
                SkillRisk::Safe => {}
            }
        }

        let log_dir = paths.macc_dir.join("log").join("run");
        let request = SkillRunRequest {
            skill_id: skill.id.clone(),
            tool_id: self.tool.clone(),
            cwd: paths.root.clone(),
            task_id: self.task_id.clone(),
            scope: self.scope.as_ref().map(|s| vec![s.clone()]),
            inputs: HashMap::new(),
            dry_run: false,
            watch: self.watch,
            yes: self.yes,
        };

        let result = SkillRunner::run(&skill, &request, &log_dir)?;

        if self.json {
            let json = serde_json::to_string_pretty(&result).map_err(|e| {
                MaccError::Validation(format!("serialize error: {}", e))
            })?;
            println!("{}", json);
        } else {
            println!("Skill:    {}", result.skill_id);
            println!("Status:   {}", result.status);
            println!("Duration: {}ms", result.duration_ms);
            if let Some(log_path) = &result.log_path {
                println!("Log:      {}", log_path.display());
            }
            if !result.stdout.is_empty() {
                println!("\n--- stdout ---");
                print!("{}", result.stdout);
            }
            if !result.stderr.is_empty() {
                println!("\n--- stderr ---");
                print!("{}", result.stderr);
            }
        }

        Ok(())
    }
}
