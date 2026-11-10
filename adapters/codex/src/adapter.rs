use crate::emit::{config_toml, rules};
use crate::map::CodexConfig;
use macc_core::plan::builders as plan_builders;
use macc_core::plan::ActionPlan;
use macc_core::resolve::{PlanningContext, SelectionKind};
use macc_core::ToolAdapter;

pub struct CodexAdapter;

impl ToolAdapter for CodexAdapter {
    fn native_output_hook(&self) -> Option<macc_core::hooks::native::Spec> {
        Some(macc_core::hooks::native::Spec {
            tool: "codex".into(),
            config: ".codex/hooks.json".into(),
            launcher: ".codex/hooks/macc-output.sh".into(),
            protocol: macc_core::hooks::native::Protocol::Feedback,
        })
    }
    fn id(&self) -> String {
        "codex".to_string()
    }

    fn context_file_target(&self) -> Option<String> {
        Some("AGENTS.md".to_string())
    }

    fn context_file_fallback(&self) -> Option<String> {
        Some("AGENTS.md is not generated when codex.rules_enabled is false.\n".to_string())
    }

    fn context_reference_files(&self) -> Vec<String> {
        vec![".codex/config.toml".into()]
    }

    fn plan(&self, ctx: &PlanningContext) -> macc_core::Result<ActionPlan> {
        let config = CodexConfig::from_resolved(ctx.resolved);
        let mut plan = ActionPlan::new();

        plan_builders::write_text(
            &mut plan,
            ".codex/config.toml",
            &config_toml::render_config_toml(&config.tool_config),
        );

        install_remote_skills(&mut plan, ctx)?;
        if config.tool_config.rules_enabled.unwrap_or(false) {
            plan_builders::write_text(
                &mut plan,
                ".codex/rules/default.rules",
                &rules::render_default_rules(),
            );
        }

        Ok(plan)
    }

    fn context_file_targets(&self, ctx: &PlanningContext) -> macc_core::Result<Vec<String>> {
        if !CodexConfig::from_resolved(ctx.resolved)
            .tool_config
            .rules_enabled
            .unwrap_or(false)
        {
            return Ok(Vec::new());
        }
        macc_core::project_context::targets(ctx, "codex", "AGENTS.md")
    }
}

fn install_remote_skills(plan: &mut ActionPlan, ctx: &PlanningContext) -> macc_core::Result<()> {
    for unit in ctx.materialized_units {
        for selection in &unit.selections {
            if selection.kind == SelectionKind::Skill
                && (selection.tools.is_empty()
                    || selection.tools.iter().any(|tool| tool == "codex"))
            {
                plan_builders::plan_skill_install(
                    plan,
                    "codex",
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
