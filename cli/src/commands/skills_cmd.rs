use super::{AppContext, Command};
use macc_core::skills_runner::SkillKind;
use macc_core::Result;

pub struct SkillsCmdCommand {
    pub app: AppContext,
    pub subcommand: SkillsSubcommand,
}

pub enum SkillsSubcommand {
    List { tool: Option<String> },
    Show { skill: String },
    Explain { skill: String },
    Doctor,
}

impl Command for SkillsCmdCommand {
    fn run(&self) -> Result<()> {
        let paths = self.app.project_paths()?;
        match &self.subcommand {
            SkillsSubcommand::List { tool } => {
                let skills = self.app.engine.list_skills(&paths);
                if skills.is_empty() {
                    println!("No skills found. Add skill YAML files to .macc/skills/");
                    return Ok(());
                }
                println!("{:<20} {:<12} {:<10} {}", "ID", "KIND", "RISK", "TITLE");
                println!("{:-<20} {:-<12} {:-<10} {:-<30}", "", "", "", "");
                for skill in &skills {
                    if let Some(tool_filter) = tool {
                        if !skill.targets.contains_key(tool_filter.as_str()) {
                            continue;
                        }
                    }
                    println!(
                        "{:<20} {:<12} {:<10} {}",
                        skill.id,
                        skill.kind.as_str(),
                        skill.risk.as_str(),
                        skill.title
                    );
                }
            }
            SkillsSubcommand::Show { skill } => {
                match self.app.engine.resolve_skill(&paths, skill) {
                    Some(def) => {
                        println!("ID:          {}", def.id);
                        println!("Title:       {}", def.title);
                        println!("Kind:        {}", def.kind.as_str());
                        println!("Risk:        {}", def.risk.as_str());
                        println!("Description: {}", def.description);
                        if !def.steps.is_empty() {
                            println!("Steps:");
                            for (i, step) in def.steps.iter().enumerate() {
                                if let Some(cmd) = &step.run {
                                    println!("  {}. run: {}", i + 1, cmd);
                                } else if let Some(prompt) = &step.prompt {
                                    let excerpt = if prompt.len() > 60 {
                                        format!("{}…", &prompt[..60])
                                    } else {
                                        prompt.clone()
                                    };
                                    println!("  {}. prompt: {}", i + 1, excerpt);
                                }
                            }
                        }
                        if !def.targets.is_empty() {
                            println!("Targets:");
                            for (tool, target) in &def.targets {
                                println!("  {}: strategy={}", tool, target.strategy);
                            }
                        }
                    }
                    None => {
                        println!(
                            "Skill '{}' not found. Run 'macc skills list' to see available skills.",
                            skill
                        );
                    }
                }
            }
            SkillsSubcommand::Explain { skill } => {
                match self.app.engine.resolve_skill(&paths, skill) {
                    Some(def) => {
                        println!("Skill:       {}", def.id);
                        println!("Title:       {}", def.title);
                        println!("Kind:        {}", def.kind.as_str());
                        println!("Risk:        {}", def.risk.as_str());
                        if !def.description.is_empty() {
                            println!("Description: {}", def.description);
                        }
                        println!();
                        // Plain-English explanation of the execution path.
                        match def.kind {
                            macc_core::skills_runner::SkillKind::LocalCommand => {
                                println!(
                                    "Execution: local commands only — no AI tool required."
                                );
                                if !def.steps.is_empty() {
                                    println!("Steps:");
                                    for (i, step) in def.steps.iter().enumerate() {
                                        if let Some(cmd) = &step.run {
                                            println!("  {}. $ {}", i + 1, cmd);
                                        }
                                    }
                                }
                            }
                            macc_core::skills_runner::SkillKind::Prompt => {
                                println!(
                                    "Execution: prompt sent to the selected tool adapter."
                                );
                                if !def.steps.is_empty() {
                                    if let Some(prompt) = def.steps.first().and_then(|s| s.prompt.as_deref()) {
                                        let excerpt = if prompt.len() > 200 {
                                            format!("{}…", &prompt[..200])
                                        } else {
                                            prompt.to_string()
                                        };
                                        println!("Prompt excerpt:\n  {}", excerpt);
                                    }
                                }
                            }
                            macc_core::skills_runner::SkillKind::Hybrid => {
                                println!(
                                    "Execution: local commands first, then output summarized \
                                     and sent to the selected tool adapter."
                                );
                            }
                            macc_core::skills_runner::SkillKind::Agent => {
                                println!(
                                    "Execution: routed to a specific agent persona."
                                );
                            }
                            macc_core::skills_runner::SkillKind::Coordinator => {
                                println!(
                                    "Execution: acts on PRD, task registry, or coordinator state."
                                );
                            }
                        }
                        if !def.targets.is_empty() {
                            println!();
                            println!("Adapter strategies:");
                            for (tool, target) in &def.targets {
                                println!("  {}: {}", tool, target.strategy);
                            }
                        }
                        println!();
                        println!(
                            "Run:      macc run {}",
                            def.id
                        );
                        println!(
                            "Dry-run:  macc run {} --dry-run",
                            def.id
                        );
                    }
                    None => {
                        println!(
                            "Skill '{}' not found. Run 'macc skills list' to see available skills.",
                            skill
                        );
                    }
                }
            }
            SkillsSubcommand::Doctor => {
                let skills = self.app.engine.list_skills(&paths);
                println!("Skills Doctor");
                println!("=============");
                println!("Skills directory: {}", paths.macc_dir.join("skills").display());
                println!("Skills found: {}", skills.len());
                println!();
                let local: Vec<_> = skills
                    .iter()
                    .filter(|s| !matches!(s.kind, SkillKind::Prompt) || !s.steps.is_empty())
                    .collect();
                let prompt_only: Vec<_> = skills
                    .iter()
                    .filter(|s| matches!(s.kind, SkillKind::Prompt) && s.steps.is_empty())
                    .collect();
                println!("Local-command skills: {}", local.len());
                println!("Prompt-only skills:   {}", prompt_only.len());
                println!();
                if skills.is_empty() {
                    println!(
                        "No skills found. Create skill YAML files in {}",
                        paths.macc_dir.join("skills").display()
                    );
                } else {
                    println!("OK — skills are available.");
                }
            }
        }
        Ok(())
    }
}
