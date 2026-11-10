use crate::emit::{geminiignore, settings_json, styleguide_md};
use crate::map::GeminiConfig;
use crate::user_mcp_merge::plan_user_mcp_merge;
use macc_core::plan::builders as plan_builders;
use macc_core::plan::ActionPlan;
use macc_core::resolve::{PlanningContext, SelectionKind};
use macc_core::ToolAdapter;
use serde_json::Value as JsonValue;
use std::collections::BTreeMap;

pub struct GeminiAdapter;

impl ToolAdapter for GeminiAdapter {
    fn id(&self) -> String {
        "gemini".to_string()
    }

    fn context_file_target(&self) -> Option<String> {
        Some("GEMINI.md".to_string()) // macc:allow-tool-name
    }

    fn context_file_fallback(&self) -> Option<String> {
        Some("GEMINI.md preview unavailable.\n".to_string()) // macc:allow-tool-name
    }

    fn context_reference_files(&self) -> Vec<String> {
        vec![".gemini/styleguide.md".into()]
    }

    fn plan(&self, ctx: &PlanningContext) -> macc_core::Result<ActionPlan> {
        let mut config = GeminiConfig::from_resolved(ctx.resolved);
        if config
            .standards_path
            .as_ref()
            .is_some_and(|path| !ctx.paths.root.join(path).is_file())
        {
            config.standards_path = None;
        }
        let mut plan = ActionPlan::new();

        let mcp_servers = collect_mcp_servers(ctx)?;
        config.mcp_servers.extend(mcp_servers.clone());

        plan_builders::write_text(
            &mut plan,
            ".gemini/settings.json",
            &settings_json::render_settings_json(&config),
        );
        plan_builders::write_text(
            &mut plan,
            ".gemini/styleguide.md",
            &styleguide_md::render_styleguide_md(&config),
        );
        plan_builders::write_text(
            &mut plan,
            ".geminiignore",
            &geminiignore::render_geminiignore(),
        );

        install_remote_skills(&mut plan, ctx)?;

        if config.user_mcp_merge {
            plan_user_mcp_merge(&mut plan, &mcp_servers)?;
        }

        Ok(plan)
    }
}

fn install_remote_skills(plan: &mut ActionPlan, ctx: &PlanningContext) -> macc_core::Result<()> {
    for unit in ctx.materialized_units {
        for selection in &unit.selections {
            if selection.kind == SelectionKind::Skill
                && (selection.tools.is_empty()
                    || selection.tools.iter().any(|tool| tool == "gemini"))
            {
                plan_builders::plan_skill_install(
                    plan,
                    "gemini",
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

fn collect_mcp_servers(ctx: &PlanningContext) -> macc_core::Result<BTreeMap<String, JsonValue>> {
    let mut mcp_servers = BTreeMap::new();

    for unit in ctx.materialized_units {
        for selection in &unit.selections {
            if selection.kind != SelectionKind::Mcp {
                continue;
            }

            let mcp_path = if selection.subpath.is_empty() || selection.subpath == "." {
                unit.source_root_path.clone()
            } else {
                unit.source_root_path.join(&selection.subpath)
            };

            let manifest = macc_core::packages::validate_mcp_folder(&mcp_path, &selection.id)
                .map_err(macc_core::MaccError::Validation)?;

            mcp_servers
                .entry(selection.id.clone())
                .or_insert_with(|| manifest.mcp.server.clone());
        }
    }

    Ok(mcp_servers)
}
