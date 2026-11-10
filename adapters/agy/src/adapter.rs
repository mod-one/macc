use crate::emit::{mcp_config_json, settings_json};
use crate::map::AgyConfig;
use macc_core::plan::builders as plan_builders;
use macc_core::plan::ActionPlan;
use macc_core::resolve::{PlanningContext, SelectionKind};
use macc_core::ToolAdapter;

pub struct AgyAdapter;

impl ToolAdapter for AgyAdapter {
    fn id(&self) -> String {
        "agy".to_string()
    }

    fn skill_install_directory(&self) -> String {
        ".agents/skills".into()
    }

    fn context_file_target(&self) -> Option<String> {
        Some("GEMINI.md".to_string())
    }

    fn context_file_fallback(&self) -> Option<String> {
        Some("GEMINI.md preview unavailable.\n".to_string())
    }

    fn context_reference_files(&self) -> Vec<String> {
        vec![
            ".agents/settings.json".into(),
            ".agents/mcp_config.json".into(),
        ]
    }

    fn plan(&self, ctx: &PlanningContext) -> macc_core::Result<ActionPlan> {
        let config = AgyConfig::from_resolved(ctx.resolved);
        let mut plan = ActionPlan::new();

        // Generate native Antigravity settings.
        plan_builders::write_text(
            &mut plan,
            ".agents/settings.json",
            &settings_json::render_settings_json(&config),
        );

        // Include MCP configuration when servers are selected.
        if !config.mcp_servers.is_empty() {
            plan_builders::write_text(
                &mut plan,
                ".agents/mcp_config.json",
                &mcp_config_json::render_mcp_config_json(&config),
            );
        }

        // Install real selected workspace skills.
        install_remote_skills(&mut plan, ctx)?;

        Ok(plan)
    }
}

fn install_remote_skills(plan: &mut ActionPlan, ctx: &PlanningContext) -> macc_core::Result<()> {
    for unit in ctx.materialized_units {
        for selection in &unit.selections {
            if selection.kind == SelectionKind::Skill
                && (selection.tools.is_empty() || selection.tools.iter().any(|tool| tool == "agy"))
            {
                plan_builders::plan_skill_install(
                    plan,
                    "agents",
                    &selection.id,
                    &unit.source_root_path,
                    &selection.subpath,
                )
                .map_err(macc_core::MaccError::Validation)?;
            }
        }
    }
    Ok(())
}
