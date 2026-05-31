use super::{AppContext, Command};
use macc_core::engine::Engine;
use macc_core::skills_runner::{SkillRisk, SkillRunRequest};
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

        let skill = self
            .app
            .engine
            .resolve_skill(&paths, &self.skill)
            .ok_or_else(|| {
                MaccError::Validation(format!(
                    "Skill '{}' not found. Run 'macc skills list' to see available skills.",
                    self.skill
                ))
            })?;

        // Spec §3.5: 8-step tool selection algorithm.
        let resolved_tool = self
            .app
            .engine
            .resolve_skill_tool(&paths, &skill, self.tool.as_deref());

        if self.dry_run {
            let preview = self
                .app
                .engine
                .dry_run_skill(&paths, &skill, resolved_tool.clone());

            if self.json {
                let json = serde_json::to_string_pretty(&preview)
                    .map_err(|e| MaccError::Validation(format!("serialize error: {}", e)))?;
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

        // Spec §3.6: if a tool is required and missing, warn clearly.
        if resolved_tool.is_none() {
            use macc_core::skills_runner::SkillKind;
            if !matches!(skill.kind, SkillKind::LocalCommand) {
                return Err(MaccError::Validation(format!(
                    "Skill '{}' is a prompt/hybrid skill but no tool adapter is available. \
                     Specify one with --tool or set skills.run_policy.default_tool in .macc/macc.yaml.",
                    skill.id
                )));
            }
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

        // Spec §3.11: worktree-aware execution — if --task is given and a matching
        // active worktree exists, run the skill there rather than in paths.root.
        let cwd = self
            .task_id
            .as_deref()
            .and_then(|tid| self.app.engine.find_task_worktree(&paths, tid))
            .unwrap_or_else(|| paths.root.clone());

        if let Some(tid) = &self.task_id {
            if cwd != paths.root {
                println!("Running in worktree for task {} ({})", tid, cwd.display());
            } else {
                println!(
                    "Note: no active worktree found for task '{}' — running in project root.",
                    tid
                );
            }
        }

        let request = SkillRunRequest {
            skill_id: skill.id.clone(),
            tool_id: resolved_tool,
            cwd,
            task_id: self.task_id.clone(),
            scope: self.scope.as_ref().map(|s| vec![s.clone()]),
            inputs: HashMap::new(),
            dry_run: false,
            watch: self.watch,
            yes: self.yes,
        };

        let result = self.app.engine.run_skill(&paths, &skill, &request)?;

        if self.json {
            let json = serde_json::to_string_pretty(&result)
                .map_err(|e| MaccError::Validation(format!("serialize error: {}", e)))?;
            println!("{}", json);
        } else {
            println!("Skill:    {}", result.skill_id);
            println!("Status:   {}", result.status);
            println!("Duration: {}ms", result.duration_ms);
            if let Some(log_path) = &result.log_path {
                println!("Log:      {}", log_path.display());
            }
            // Spec §5.6: show summary metadata when output was processed.
            if let Some(meta) = &result.summary {
                println!();
                println!("Output summarized");
                println!("  Raw size:     {} chars", meta.raw_size_chars);
                println!("  Summary size: {} chars", meta.summary_size_chars);
                if !meta.bundles_applied.is_empty() {
                    println!("  Policy:       {}", meta.bundles_applied.join(" + "));
                }
                if meta.was_truncated {
                    println!("  Note:         output truncated to fit token budget");
                }
            }
            if !result.stdout.is_empty() && !self.watch {
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
