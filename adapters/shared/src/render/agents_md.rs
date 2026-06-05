use std::collections::BTreeMap;

pub fn render_shared_agents_md(
    enabled_tools: &[String],
    standards_inline: &BTreeMap<String, String>,
    standards_path: Option<&str>,
) -> String {
    let mut md = String::from("# Project Instructions (MACC)\n\n");

    md.push_str("## Standards\n");
    if standards_inline.is_empty() {
        md.push_str("- No inline standards configured.\n");
    } else {
        for (key, value) in standards_inline {
            md.push_str(&format!("- {}: {}\n", key, value));
        }
    }
    if let Some(path) = standards_path {
        md.push_str(&format!("\nSee additional standards in: {}\n", path));
    }

    md.push_str("\n## Required Workflows\n");
    md.push_str("- Always run tests before committing.\n");
    md.push_str("- Use English for code, docs, and commit messages.\n");

    md.push_str("\n## Skills\n");
    md.push_str("- use `macc-performer` to perform a tasck\n");
    md.push_str("- use `macc-prd-planner` to create or edit a prd file\n");
    md.push_str("- use `macc-reviewer` to perform a review\n");

    if enabled_tools.contains(&"codex".to_string()) {
        md.push_str("\n## Codex Skills\n");
        md.push_str("- Use `validate` to run the standard validation pipeline.\n");
        md.push_str("- Use `implement` for full implementation workflow.\n");
    }

    if enabled_tools.contains(&"vibe".to_string()) {
        md.push_str("\n## Vibe Skills\n");
        md.push_str("- Use `explore` for codebase exploration.\n");
        md.push_str("- Use custom slash commands via the skills system.\n");
    }

    md.push_str("\n## Workflow Chain (BMAD-lite)\n");
    md.push_str("- /brainstorm -> /prd -> /tech-stack -> /implementation-plan -> /implement\n");

    super::format::ensure_trailing_newline(md)
}

pub fn orchestrate_agents_md(
    resolved: &macc_core::resolve::ResolvedConfig,
    sections: &[macc_core::tool::ProjectContextSection],
) -> String {
    let mut md = String::from("# Project Instructions (MACC)\n\n");

    md.push_str("## Standards\n");
    if resolved.standards.inline.is_empty() {
        md.push_str("- No inline standards configured.\n");
    } else {
        for (key, value) in &resolved.standards.inline {
            md.push_str(&format!("- {}: {}\n", key, value));
        }
    }
    if let Some(path) = &resolved.standards.path {
        md.push_str(&format!("\nSee additional standards in: {}\n", path));
    }

    md.push_str("\n## Required Workflows\n");
    md.push_str("- Always run tests before committing.\n");
    md.push_str("- Use English for code, docs, and commit messages.\n");

    md.push_str("\n## Skills\n");
    md.push_str("- use `macc-performer` to perform a tasck\n");
    md.push_str("- use `macc-prd-planner` to create or edit a prd file\n");
    md.push_str("- use `macc-reviewer` to perform a review\n");

    for sec in sections {
        md.push_str(&format!("\n## {}\n", sec.heading));
        md.push_str(&sec.content);
    }

    md.push_str("\n## Workflow Chain (BMAD-lite)\n");
    md.push_str("- /brainstorm -> /prd -> /tech-stack -> /implementation-plan -> /implement\n");

    super::format::ensure_trailing_newline(md)
}
