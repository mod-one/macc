use crate::emit::config_toml;
use crate::map::VibeConfig;
use macc_core::mcp_json;
use macc_core::plan::builders as plan_builders;
use macc_core::plan::ActionPlan;
use macc_core::resolve::{PlanningContext, SelectionKind};
use macc_core::tool::ProjectContextSection;
use macc_core::ToolAdapter;
use serde_json::Value as JsonValue;
use std::collections::{BTreeMap, BTreeSet};

pub struct VibeAdapter;

#[allow(dead_code)]
struct InstalledRemoteContent {
    skills: BTreeSet<String>,
    mcp_servers: BTreeMap<String, JsonValue>,
}

impl ToolAdapter for VibeAdapter {
    fn id(&self) -> String {
        "vibe".to_string()
    }

    fn context_file_target(&self) -> Option<String> {
        Some("AGENTS.md".to_string())
    }

    fn context_file_fallback(&self) -> Option<String> {
        Some("AGENTS.md is not generated when this tool is disabled.\n".to_string())
    }

    fn plan(&self, ctx: &PlanningContext) -> macc_core::Result<ActionPlan> {
        let config = VibeConfig::from_resolved(ctx.resolved);
        let mut plan = ActionPlan::new();

        // 1. Write Vibe .vibe/config.toml settings
        let config_toml_content = config_toml::render_config_toml(&config);
        plan_builders::write_text(&mut plan, ".vibe/config.toml", &config_toml_content);

        // 3. Remote skills / MCP server downloads
        let installed = install_remote_content(&mut plan, ctx)?;

        // 4. MCP JSON configuration
        let mut all_mcp_servers = installed.mcp_servers;
        let selection_ids: BTreeSet<String> = ctx.resolved.selections.mcp.iter().cloned().collect();
        for template in &ctx.resolved.mcp_templates {
            if selection_ids.contains(&template.id) {
                all_mcp_servers.insert(template.id.clone(), mcp_json::template_to_value(template));
            }
        }

        if !all_mcp_servers.is_empty() {
            let content = mcp_json::render_mcp_json(&all_mcp_servers);
            plan_builders::write_text(&mut plan, ".mcp.json", &content);
        }

        Ok(plan)
    }

    fn context_file_sections(
        &self,
        _ctx: &PlanningContext,
    ) -> macc_core::Result<Vec<ProjectContextSection>> {
        let sections = vec![ProjectContextSection {
            heading: "Vibe Skills".to_string(),
            content: "- Use `explore` for codebase exploration.\n- Use custom slash commands via the skills system.\n".to_string(),
        }];
        Ok(sections)
    }
}

fn install_remote_content(
    plan: &mut ActionPlan,
    ctx: &PlanningContext,
) -> macc_core::Result<InstalledRemoteContent> {
    let mut installed_skills = BTreeSet::new();
    let mut mcp_servers = BTreeMap::new();
    for unit in ctx.materialized_units {
        for selection in &unit.selections {
            match selection.kind {
                SelectionKind::Skill => {
                    plan_builders::plan_skill_install(
                        plan,
                        "vibe",
                        &selection.id,
                        &unit.source_root_path,
                        &selection.subpath,
                    )
                    .map_err(macc_core::MaccError::Validation)?;
                    installed_skills.insert(selection.id.clone());
                }
                SelectionKind::Mcp => {
                    let manifest = plan_builders::plan_mcp_install(
                        plan,
                        &selection.id,
                        &unit.source_root_path,
                        &selection.subpath,
                    )
                    .map_err(macc_core::MaccError::Validation)?;
                    mcp_servers
                        .entry(selection.id.clone())
                        .or_insert_with(|| manifest.mcp.server.clone());
                }
            }
        }
    }
    Ok(InstalledRemoteContent {
        skills: installed_skills,
        mcp_servers,
    })
}
