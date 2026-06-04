use crate::prd_generation::{
    request::{ModelRoutingMode, ModelSelection},
    PRD_GENERATION_INTERNAL_SKILL,
};
use std::path::Path;

/// Build the fixed macc-prd-planner generation prompt.
///
/// Assembles the full prompt in the order specified by spec §19:
/// 1. Role + fixed skill instruction
/// 2. Brief content
/// 3. Target output directory
/// 4. Repository context (minimal)
/// 5. Existing PRD content when update_path is set
/// 6. Client-provided instructions
/// 7. Execution constraints + output contract
pub fn build_generate_prompt(
    brief_content: &str,
    target_dir: &Path,
    existing_prd: Option<&str>,
    instructions: Option<&str>,
    model_selection: Option<&ModelSelection>,
) -> String {
    let mut prompt = String::with_capacity(4096);

    // 1. Role + fixed skill instruction
    prompt.push_str("You are running MACC's built-in PRD generation flow.\n\n");
    prompt.push_str("Use the MACC PRD Planner skill:\n");
    prompt.push_str(&format!("- `{}`\n\n", PRD_GENERATION_INTERNAL_SKILL));

    // Model routing info (informational, does not change tool behavior)
    if let Some(ms) = model_selection {
        match ms.mode {
            ModelRoutingMode::Auto => {
                prompt.push_str(
                    "Model routing: auto (MACC selects the lightest sufficient model)\n\n",
                );
            }
            ModelRoutingMode::Manual => {
                let model_label = ms.model.as_deref().unwrap_or("(unspecified)");
                prompt.push_str(&format!(
                    "Model routing: manual — use model: {}\n\n",
                    model_label
                ));
            }
        }
    }

    // 2. Goal
    prompt.push_str("## Goal\n\nGenerate or update a MACC-compatible PRD file.\n\n");

    // 3. Brief
    prompt.push_str("## Input brief\n\n");
    prompt.push_str(brief_content);
    if !brief_content.ends_with('\n') {
        prompt.push('\n');
    }
    prompt.push('\n');

    // 4. Target output directory
    prompt.push_str("## Target output directory\n\n");
    prompt.push_str(&format!("{}\n\n", target_dir.display()));

    // 5. Existing PRD (when updating)
    if let Some(prd) = existing_prd {
        prompt.push_str("## Existing PRD to update\n\n```json\n");
        prompt.push_str(prd);
        if !prd.ends_with('\n') {
            prompt.push('\n');
        }
        prompt.push_str("```\n\n");
    }

    // 6. Client instructions
    if let Some(instr) = instructions {
        if !instr.trim().is_empty() {
            prompt.push_str("## Client instructions\n\n");
            prompt.push_str(instr);
            if !instr.ends_with('\n') {
                prompt.push('\n');
            }
            prompt.push('\n');
        }
    }

    // 7. Execution constraints + output contract
    prompt.push_str("## Execution constraints\n\n");
    prompt.push_str("- Write output files only inside the target output directory above.\n");
    prompt.push_str("- Do not edit source files, run commands, or execute remote code.\n");
    prompt.push_str("- Do not include concrete provider model names in routing_hints.\n");
    prompt.push_str("  Allowed routing_hints keys: execution_mode, reasoning_depth, context_scope, risk_level, validation_profile.\n");
    prompt.push_str(
        "- Task IDs must be unique and stable (never rename or delete existing IDs).\n\n",
    );

    prompt.push_str("## Output contract\n\n");
    prompt
        .push_str("Write `prd.json` (and optionally `prd.summary.md`) to the target directory.\n");
    prompt.push_str("At the end, print a short status line (e.g. `prd.json written`).\n");

    prompt
}

/// Resolve instructions: prefer inline, fall back to file content.
pub fn resolve_instructions(
    inline: Option<&str>,
    file_path: Option<&Path>,
) -> crate::Result<Option<String>> {
    if let Some(text) = inline {
        if !text.trim().is_empty() {
            return Ok(Some(text.to_string()));
        }
    }
    if let Some(path) = file_path {
        let content = std::fs::read_to_string(path).map_err(|e| crate::MaccError::Io {
            path: path.to_string_lossy().into(),
            action: "read instructions file".into(),
            source: e,
        })?;
        if !content.trim().is_empty() {
            return Ok(Some(content));
        }
    }
    Ok(None)
}

/// Attempt to resolve the tool to use for PRD generation.
/// Resolution order: explicit argument → config default → first enabled tool.
pub fn resolve_tool(
    explicit: Option<&str>,
    config_default: Option<&str>,
    coordinator_tool: Option<&str>,
    enabled_tools: &[String],
) -> Option<String> {
    if let Some(t) = explicit {
        return Some(t.to_string());
    }
    if let Some(t) = config_default {
        return Some(t.to_string());
    }
    if let Some(t) = coordinator_tool {
        return Some(t.to_string());
    }
    enabled_tools.first().cloned()
}
