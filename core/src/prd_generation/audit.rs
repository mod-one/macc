/// `macc prd audit` — enriches an existing PRD from commit history.
///
/// This module is pure business logic: it builds the audit prompt and returns it.
/// Tool invocation is the caller's responsibility (Engine facade or CLI layer).
/// This mirrors the design of `prd_auditor::prepare_audit`, which also never invokes.
use crate::coordinator::prd_auditor;
use crate::prd_generation::request::ModelSelection;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

pub struct PrdAuditRequest {
    /// Path to the PRD file to audit (absolute or relative to project root).
    pub prd_path: PathBuf,
    /// Tool to invoke. When `None` or `dry_run=true`, the prompt is returned without invoking.
    pub tool: Option<String>,
    pub model_selection: Option<ModelSelection>,
    /// Inline client instructions appended to the audit prompt.
    pub instructions: Option<String>,
    pub instructions_file: Option<PathBuf>,
    /// Reference branch to gather commit history from.
    pub reference_branch: String,
    /// Include `git diff --stat` per commit.
    pub include_diff_stat: bool,
    pub dry_run: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PrdAuditResult {
    pub completed_with_context: usize,
    pub todo_tasks: usize,
    pub prompt_generated: bool,
    pub prompt: Option<String>,
    /// True when the prompt was delivered to a tool (not dry-run).
    pub dispatched: bool,
}

/// PRD files an audit run covers, in audit order.
///
/// `explicit` is a PRD file, or a directory whose `*.json` files holding a
/// top-level `tasks` array are audited in name order. Without it, the
/// coordinator PRD queue (`prd_files`) is used, then `prd_file`, then `prd.json`.
pub fn resolve_audit_targets(
    repo_root: &Path,
    explicit: Option<&Path>,
    coordinator: Option<&crate::config::CoordinatorConfig>,
) -> crate::Result<Vec<PathBuf>> {
    use crate::MaccError;
    let Some(path) = explicit else {
        if let Some(config) = coordinator.filter(|c| !c.prd_files.is_empty()) {
            return Ok(config.prd_files.iter().map(|p| repo_root.join(p)).collect());
        }
        return Ok(vec![repo_root.join(
            coordinator
                .and_then(|c| c.prd_file.as_deref())
                .unwrap_or("prd.json"),
        )]);
    };
    let path = repo_root.join(path);
    if !path.is_dir() {
        return Ok(vec![path]);
    }
    let io = |e| MaccError::Io {
        path: path.to_string_lossy().into(),
        action: "read PRD directory".into(),
        source: e,
    };
    let mut files = Vec::new();
    for entry in std::fs::read_dir(&path).map_err(io)? {
        let file = entry.map_err(io)?.path();
        let is_prd = file.extension().is_some_and(|e| e == "json")
            && file.is_file()
            && std::fs::read_to_string(&file)
                .ok()
                .and_then(|text| serde_json::from_str::<serde_json::Value>(&text).ok())
                .is_some_and(|value| value.get("tasks").is_some_and(|t| t.is_array()));
        if is_prd {
            files.push(file);
        }
    }
    if files.is_empty() {
        return Err(MaccError::Validation(format!(
            "PRD-GEN-INPUT-MISSING: directory '{}' contains no PRD file (a .json file with a top-level tasks array)",
            path.display()
        )));
    }
    files.sort();
    Ok(files)
}

/// Build the PRD audit prompt from commit history.
///
/// Always returns `dispatched: false` — this module never invokes a tool.
/// The caller (Engine facade) decides whether to forward the prompt to a tool.
pub fn run_prd_audit(repo_root: &Path, request: &PrdAuditRequest) -> crate::Result<PrdAuditResult> {
    use crate::MaccError;

    if !request.prd_path.exists() {
        return Err(MaccError::Validation(format!(
            "PRD-GEN-INPUT-MISSING: PRD file '{}' does not exist",
            request.prd_path.display()
        )));
    }

    let prd_json = std::fs::read_to_string(&request.prd_path).map_err(|e| MaccError::Io {
        path: request.prd_path.to_string_lossy().into(),
        action: "read PRD file".into(),
        source: e,
    })?;

    let prd_path_str = request
        .prd_path
        .strip_prefix(repo_root)
        .unwrap_or(&request.prd_path)
        .to_string_lossy()
        .to_string();

    let registry = parse_task_registry(&prd_json, &prd_path_str)?;

    // Resolve optional client instructions
    let extra_instructions = resolve_instructions(
        request.instructions.as_deref(),
        request.instructions_file.as_deref(),
    )?;

    let report = prd_auditor::prepare_audit(
        repo_root,
        &prd_json,
        &prd_path_str,
        &registry,
        &request.reference_branch,
        request.include_diff_stat,
        None, // no sync summary — audit is standalone here
    )?;

    if !report.prompt_generated {
        return Ok(PrdAuditResult {
            completed_with_context: 0,
            todo_tasks: 0,
            prompt_generated: false,
            prompt: None,
            dispatched: false,
        });
    }

    // Append client instructions to the generated prompt if provided.
    let final_prompt = if let Some(instr) = extra_instructions {
        let mut p = report.prompt.clone().unwrap_or_default();
        p.push_str("\n\n## Client instructions\n\n");
        p.push_str(&instr);
        p
    } else {
        report.prompt.clone().unwrap_or_default()
    };

    // This function never dispatches — the Engine facade handles tool invocation.
    Ok(PrdAuditResult {
        completed_with_context: report.completed_with_context,
        todo_tasks: report.todo_tasks,
        prompt_generated: true,
        prompt: Some(final_prompt),
        dispatched: false,
    })
}

fn resolve_instructions(
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

fn parse_task_registry(
    prd_json: &str,
    prd_path: &str,
) -> crate::Result<crate::coordinator::model::TaskRegistry> {
    // Try full TaskRegistry shape first.
    if let Ok(registry) = serde_json::from_str::<crate::coordinator::model::TaskRegistry>(prd_json)
    {
        return Ok(registry);
    }

    // Fall back to minimal wrapper with a top-level `tasks` array.
    #[derive(serde::Deserialize)]
    struct PrdShape {
        tasks: Option<Vec<crate::coordinator::model::Task>>,
    }

    let shape: PrdShape = serde_json::from_str(prd_json).map_err(|e| {
        crate::MaccError::Validation(format!(
            "PRD-GEN-VALIDATION-FAILED: cannot parse '{}' as a task registry: {}",
            prd_path, e
        ))
    })?;

    Ok(crate::coordinator::model::TaskRegistry {
        tasks: shape.tasks.unwrap_or_default(),
        ..Default::default()
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn coordinator(yaml: &str) -> crate::config::CoordinatorConfig {
        serde_yaml::from_str(yaml).unwrap()
    }

    #[test]
    fn targets_default_to_the_prd_queue_then_prd_file_then_prd_json() {
        let root = Path::new("/project");
        let queue = coordinator("prd_file: legacy.json\nprd_files: [docs/b.json, docs/a.json]");
        assert_eq!(
            resolve_audit_targets(root, None, Some(&queue)).unwrap(),
            vec![root.join("docs/b.json"), root.join("docs/a.json")]
        );
        let single = coordinator("prd_file: plan/prd.json");
        assert_eq!(
            resolve_audit_targets(root, None, Some(&single)).unwrap(),
            vec![root.join("plan/prd.json")]
        );
        assert_eq!(
            resolve_audit_targets(root, None, None).unwrap(),
            vec![root.join("prd.json")]
        );
    }

    #[test]
    fn explicit_target_is_a_file_or_the_prd_files_of_a_directory() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path();
        let dir = root.join("docs/prd");
        std::fs::create_dir_all(dir.join("nested.json")).unwrap();
        std::fs::write(dir.join("02-b.prd.json"), r#"{"tasks":[]}"#).unwrap();
        std::fs::write(dir.join("01-a.prd.json"), r#"{"tasks":[]}"#).unwrap();
        std::fs::write(dir.join("schema.json"), r#"{"type":"object"}"#).unwrap();
        std::fs::write(dir.join("notes.md"), "tasks").unwrap();
        let queue = coordinator("prd_files: [other.json]");
        assert_eq!(
            resolve_audit_targets(root, Some(Path::new("docs/prd")), Some(&queue)).unwrap(),
            vec![dir.join("01-a.prd.json"), dir.join("02-b.prd.json")]
        );
        assert_eq!(
            resolve_audit_targets(root, Some(Path::new("docs/prd/01-a.prd.json")), None).unwrap(),
            vec![dir.join("01-a.prd.json")]
        );
        std::fs::create_dir_all(root.join("empty")).unwrap();
        let error = resolve_audit_targets(root, Some(Path::new("empty")), None).unwrap_err();
        assert!(error.to_string().contains("PRD-GEN-INPUT-MISSING"));
    }
}
