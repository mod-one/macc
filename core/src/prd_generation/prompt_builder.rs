use crate::prd_generation::{
    request::{ModelRoutingMode, ModelSelection},
    PRD_GENERATION_INTERNAL_SKILL,
};
use std::path::Path;

/// Planning rule shared by PRD generation and audit: a task consuming another
/// module's endpoint must be checked against that endpoint's declared audience
/// and contract while planning. Found at run time instead, the mismatch stops
/// the consumer with a precondition whose fix lies outside its change scope.
pub const CROSS_MODULE_CONTRACT_CHECK: &str = "## Cross-module contract check\n\n\
For every task that consumes an endpoint, route, event or shared contract owned by another module \
(for example a Web/UI task calling a backend or BFF route):\n\
- Find the endpoint in its contract source (OpenAPI, API catalogue, route declarations) and check that its \
declared audience or consumers (for example `x-audience`, public/internal/BFF/web, required permission or role) \
include the consuming surface, and that the method, path and payload the consumer needs exist.\n\
- If the audience or the contract does not serve the consumer, plan a contract-resolution task in the owning \
module, make the consuming task depend on it, and keep that fix out of the consumer's `change_scope`.\n\
- Give each consuming task a `change_scope` limited to the module where its own work lives; a precondition \
whose fix lies outside it blocks the task at run time.\n\n";

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

    prompt.push_str(CROSS_MODULE_CONTRACT_CHECK);
    prompt.push_str("## Decision governance\n\nInvestigate repository contracts and existing decisions before planning. Do not invent required human roles or approvals: derive gates from explicit project governance and cite governance_ref/approval_trigger. Routine reversible implementation choices remain autonomous. For an important unresolved choice, include a top-level decision_requests array of structured proposals (idempotency_key, task_id, kind, question, context, investigated, recommendation, options with IDs, recommended_option_id, affected_tasks, risk, impact with evidence). Questions are proposals for the coordinator policy, never pre-approved human consent. Keep every original task ID and retain delivered history; use follow-up corrective tasks for delivered work. Runtime changes use versioned graph patches, never direct edits of the active PRD.\n\n");

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

#[cfg(test)]
mod contract_check_tests {
    use super::*;

    #[test]
    fn generation_prompt_asks_to_check_consumed_endpoint_audiences() {
        let prompt = build_generate_prompt("brief", Path::new("out"), None, None, None);
        assert!(prompt.contains("## Cross-module contract check"));
        assert!(prompt.contains("declared audience"));
        assert!(prompt.contains("contract-resolution task in the owning module"));
    }
}
