use crate::emit::{gemini_md, mcp_config_json, settings_json};
use crate::map::AgyConfig;
use macc_core::plan::builders as plan_builders;
use macc_core::plan::ActionPlan;
use macc_core::resolve::{PlanningContext, SelectionKind};
use macc_core::ToolAdapter;
use std::collections::BTreeSet;

pub struct AgyAdapter;

impl ToolAdapter for AgyAdapter {
    fn id(&self) -> String {
        "agy".to_string()
    }

    fn context_file_target(&self) -> Option<String> {
        Some("GEMINI.md".to_string())
    }

    fn context_file_fallback(&self) -> Option<String> {
        Some("GEMINI.md preview unavailable.\n".to_string())
    }

    fn plan(&self, ctx: &PlanningContext) -> macc_core::Result<ActionPlan> {
        let config = AgyConfig::from_resolved(ctx.resolved);
        let mut plan = ActionPlan::new();

        // 1. Generate GEMINI.md
        plan_builders::write_text(
            &mut plan,
            "GEMINI.md",
            &gemini_md::render_gemini_md(&config),
        );

        // 2. Generate .agents/settings.json
        plan_builders::write_text(
            &mut plan,
            ".agents/settings.json",
            &settings_json::render_settings_json(&config),
        );

        // 3. Generate .agents/mcp_config.json if MCP servers are present
        if !config.mcp_servers.is_empty() {
            plan_builders::write_text(
                &mut plan,
                ".agents/mcp_config.json",
                &mcp_config_json::render_mcp_config_json(&config),
            );
        }

        // 4. Install remote workspace skills
        let installed_skills = install_remote_skills(&mut plan, ctx)?;

        // 5. Add local workspace skills as defined in resolved config
        add_skills(&mut plan, &config, &installed_skills);

        Ok(plan)
    }
}

fn install_remote_skills(
    plan: &mut ActionPlan,
    ctx: &PlanningContext,
) -> macc_core::Result<BTreeSet<String>> {
    let mut installed = BTreeSet::new();
    for unit in ctx.materialized_units {
        for selection in &unit.selections {
            if selection.kind == SelectionKind::Skill {
                plan_builders::plan_skill_install(
                    plan,
                    "agents",
                    &selection.id,
                    &unit.source_root_path,
                    &selection.subpath,
                )
                .map_err(macc_core::MaccError::Validation)?;
                installed.insert(selection.id.clone());
            }
        }
    }
    Ok(installed)
}

fn add_skills(plan: &mut ActionPlan, config: &AgyConfig, cached: &BTreeSet<String>) {
    for skill in &config.skills {
        if cached.contains(skill) {
            continue;
        }
        let content = render_skill_md(skill);
        plan_builders::write_text(plan, format!(".agents/skills/{}/SKILL.md", skill), &content);
    }

    for agent in &config.agents {
        let content = render_skill_md(agent);
        plan_builders::write_text(plan, format!(".agents/skills/{}/SKILL.md", agent), &content);
    }
}

fn render_skill_md(name: &str) -> String {
    let (goal, steps, done) = match name {
        "validate" => (
            "Run the project validation pipeline and report results.",
            "1) Run `pnpm lint`.\n2) Run `pnpm build`.\n3) Run `pnpm test:e2e`.\n4) Summarize failures and propose fixes.",
            "All validation steps pass or remaining failures are clearly explained.",
        ),
        "implement" => (
            "Deliver a change end-to-end with planning, implementation, and validation.",
            "1) Read relevant context (GEMINI.md, styleguide, code).\n2) Propose a short plan.\n3) Implement small, safe changes.\n4) Validate using `/validate`.\n5) Summarize changes and suggest a commit message.",
            "Implementation is complete, validated, and summarized with next steps.",
        ),
        _ => (
            "Execute the workflow for this skill following MACC standards.",
            "1) Clarify inputs and scope.\n2) Plan briefly.\n3) Execute safely.\n4) Summarize outcomes and next steps.",
            "The workflow is completed with a clear summary.",
        ),
    };

    let mut md = String::new();
    md.push_str(&format!(
        "---\nname: {}\ndescription: {}\n---\n\n",
        name, goal
    ));
    md.push_str("# Goal\n");
    md.push_str(goal);
    md.push_str("\n\n# Steps\n");
    md.push_str(steps);
    md.push_str("\n\n# Done when\n");
    md.push_str(done);
    md.push('\n');
    md
}
