use crate::commands::AppContext;
use crate::commands::Command;
use macc_core::prd_generation::{
    audit::PrdAuditRequest,
    metadata::PRD_GENERATION_DEFAULT_TARGET_DIR,
    promotion::PromoteOptions,
    prompt_builder::{resolve_instructions, resolve_tool},
    request::{ModelRoutingMode, ModelSelection},
    target_dir::resolve_target_dir,
    ValidationResult,
};
use macc_core::{find_project_root, load_canonical_config, Result};
use std::path::PathBuf;

pub enum PrdSubcommand {
    Generate {
        from_path: PathBuf,
        tool: Option<String>,
        model_routing: Option<String>,
        model: Option<String>,
        instructions: Option<String>,
        instructions_file: Option<PathBuf>,
        target_dir: Option<PathBuf>,
        update_path: Option<PathBuf>,
        dry_run: bool,
        promote: bool,
        yes: bool,
        json: bool,
    },
    Audit {
        prd_path: PathBuf,
        tool: Option<String>,
        model_routing: Option<String>,
        model: Option<String>,
        instructions: Option<String>,
        instructions_file: Option<PathBuf>,
        reference_branch: Option<String>,
        diff_stat: bool,
        dry_run: bool,
        json: bool,
    },
    Promote {
        source_path: PathBuf,
        dest_path: Option<PathBuf>,
        yes: bool,
        json: bool,
    },
    Validate {
        prd_path: PathBuf,
        json: bool,
    },
    Runs {
        json: bool,
    },
    ShowRun {
        run_id: String,
        json: bool,
    },
}

pub struct PrdCommand {
    pub app: AppContext,
    pub subcommand: PrdSubcommand,
}

impl PrdCommand {
    pub fn new(app: AppContext, subcommand: PrdSubcommand) -> Self {
        Self { app, subcommand }
    }
}

impl Command for PrdCommand {
    fn run(&self) -> Result<()> {
        let paths = find_project_root(&self.app.cwd)?;
        match &self.subcommand {
            PrdSubcommand::Generate {
                from_path,
                tool,
                model_routing,
                model,
                instructions,
                instructions_file,
                target_dir,
                update_path,
                dry_run,
                promote,
                yes,
                json,
            } => run_generate(
                &paths,
                from_path,
                tool.as_deref(),
                model_routing.as_deref(),
                model.as_deref(),
                instructions.as_deref(),
                instructions_file.as_deref(),
                target_dir.as_deref(),
                update_path.as_deref(),
                *dry_run,
                *promote,
                *yes,
                *json,
                &*self.app.engine,
            ),
            PrdSubcommand::Audit {
                prd_path,
                tool,
                model_routing,
                model,
                instructions,
                instructions_file,
                reference_branch,
                diff_stat,
                dry_run,
                json,
            } => run_audit(
                &paths,
                prd_path,
                tool.as_deref(),
                model_routing.as_deref(),
                model.as_deref(),
                instructions.as_deref(),
                instructions_file.as_deref(),
                reference_branch.as_deref(),
                *diff_stat,
                *dry_run,
                *json,
                &*self.app.engine,
            ),
            PrdSubcommand::Promote {
                source_path,
                dest_path,
                yes,
                json,
            } => run_promote(
                &paths,
                source_path,
                dest_path.as_deref(),
                *yes,
                *json,
                &*self.app.engine,
            ),
            PrdSubcommand::Validate { prd_path, json } => {
                run_validate(prd_path, *json, &*self.app.engine)
            }
            PrdSubcommand::Runs { json } => run_list_runs(&paths, *json, &*self.app.engine),
            PrdSubcommand::ShowRun { run_id, json } => run_show_run(&paths, run_id, *json),
        }
    }
}

// ── Generate ─────────────────────────────────────────────────────────────────

#[allow(clippy::too_many_arguments)]
fn run_generate(
    paths: &macc_core::ProjectPaths,
    from_path: &std::path::Path,
    tool: Option<&str>,
    model_routing: Option<&str>,
    model: Option<&str>,
    instructions: Option<&str>,
    instructions_file: Option<&std::path::Path>,
    target_dir_override: Option<&std::path::Path>,
    update_path: Option<&std::path::Path>,
    dry_run: bool,
    promote: bool,
    yes: bool,
    json: bool,
    engine: &dyn macc_core::Engine,
) -> Result<()> {
    use macc_core::MaccError;

    if !from_path.exists() {
        return Err(MaccError::Validation(format!(
            "PRD-GEN-INPUT-MISSING: brief file '{}' does not exist",
            from_path.display()
        )));
    }

    let brief_content = std::fs::read_to_string(from_path).map_err(|e| MaccError::Io {
        path: from_path.to_string_lossy().into(),
        action: "read brief file".into(),
        source: e,
    })?;

    // Resolve model selection
    let model_selection = build_model_selection(model_routing, model);

    // Resolve instructions
    let resolved_instructions = resolve_instructions(instructions, instructions_file)?;

    // Resolve run id and target dir
    let run_metadata = macc_core::prd_generation::metadata::GenerationRunMetadata::new(
        &from_path.to_string_lossy(),
        tool.map(|s| s.to_string()),
        dry_run,
    );
    let run_dir = resolve_target_dir(&paths.root, target_dir_override, &run_metadata.run_id)
        .map_err(|e| MaccError::Validation(format!("PRD-GEN-PROMPT-FAILED: {}", e)))?;

    // Existing PRD when --update
    let existing_prd = if let Some(upd) = update_path {
        if !upd.exists() {
            return Err(MaccError::Validation(format!(
                "PRD-GEN-INPUT-MISSING: update path '{}' does not exist",
                upd.display()
            )));
        }
        Some(std::fs::read_to_string(upd).map_err(|e| MaccError::Io {
            path: upd.to_string_lossy().into(),
            action: "read existing PRD".into(),
            source: e,
        })?)
    } else {
        None
    };

    let prompt = engine.prd_build_generate_prompt(
        &brief_content,
        &run_dir,
        existing_prd.as_deref(),
        resolved_instructions.as_deref(),
        model_selection.as_ref(),
    );

    if dry_run {
        if json {
            let out = serde_json::json!({
                "status": "dry_run",
                "run_id": run_metadata.run_id,
                "target_dir": run_dir.to_string_lossy(),
                "tool": tool,
                "model_selection": model_selection.as_ref().map(|ms| serde_json::json!({
                    "mode": format!("{:?}", ms.mode).to_lowercase(),
                    "model": ms.model,
                })),
                "prompt": prompt,
            });
            println!("{}", serde_json::to_string_pretty(&out).unwrap());
        } else {
            println!("=== DRY RUN — Prompt (not invoking tool) ===");
            println!("Run ID:     {}", run_metadata.run_id);
            println!("Target dir: {}", run_dir.display());
            if let Some(t) = tool {
                println!("Tool:       {}", t);
            }
            println!();
            println!("{}", prompt);
        }
        return Ok(());
    }

    // Resolve tool
    let canonical = load_canonical_config(&paths.config_path)?;
    let prd_cfg = canonical.prd_generation.as_ref();
    let coordinator_tool = canonical
        .automation
        .coordinator
        .as_ref()
        .and_then(|c| c.coordinator_tool.as_deref());

    let resolved_tool = resolve_tool(
        tool,
        prd_cfg.and_then(|c| c.default_tool.as_deref()),
        coordinator_tool,
        &canonical.tools.enabled,
    );

    match resolved_tool {
        Some(t) => {
            println!(
                "PRD generation: invoking tool '{}' with macc-prd-planner prompt.",
                t
            );
            println!("Output directory: {}", run_dir.display());
            println!("(Tool will write prd.json to the target directory above.)");

            // Create the run directory so the tool has somewhere to write.
            std::fs::create_dir_all(&run_dir).map_err(|e| MaccError::Io {
                path: run_dir.to_string_lossy().into(),
                action: "create run directory".into(),
                source: e,
            })?;

            // Invoke tool via the Engine facade (never call service layer directly).
            engine.prd_invoke_tool(paths, &t, &prompt)?;

            // Validate the generated output.
            let generated_prd = run_dir.join("prd.json");
            let validation = engine.prd_validate(&generated_prd, Some(&run_dir));
            print_validation_result(&validation, json);

            // Optionally promote.
            if promote && validation.ok {
                let dest = paths.root.join(
                    prd_cfg
                        .and_then(|c| c.promotion.as_ref())
                        .map(|p| p.default_output_path.as_str())
                        .unwrap_or("prd.json"),
                );
                let options = PromoteOptions {
                    source_path: generated_prd.clone(),
                    dest_path: dest,
                    yes,
                    backup_dir: Some(paths.backups_dir.clone()),
                };
                let result = engine.prd_promote(&options)?;
                if json {
                    println!("{}", serde_json::to_string_pretty(&result).unwrap());
                } else {
                    println!("Promoted: {} -> {}", result.source, result.destination);
                    if let Some(b) = result.backup_created {
                        println!("Backup:   {}", b);
                    }
                }
            }

            if json {
                let out = serde_json::json!({
                    "status": "generated",
                    "run_id": run_metadata.run_id,
                    "target_dir": run_dir.to_string_lossy(),
                    "tool": t,
                    "validation": {
                        "status": if validation.ok { "ok" } else { "failed" },
                        "warnings": validation.warnings,
                        "errors": validation.errors,
                    },
                    "promoted": promote && validation.ok,
                });
                println!("{}", serde_json::to_string_pretty(&out).unwrap());
            }
        }
        None => {
            return Err(MaccError::Validation(
                "PRD-GEN-TOOL-UNAVAILABLE: no tool configured. \
                 Add a tool with 'macc apply' or use --tool <id>."
                    .to_string(),
            ));
        }
    }

    Ok(())
}

// ── Audit ─────────────────────────────────────────────────────────────────────

#[allow(clippy::too_many_arguments)]
fn run_audit(
    paths: &macc_core::ProjectPaths,
    prd_path: &std::path::Path,
    tool: Option<&str>,
    model_routing: Option<&str>,
    model: Option<&str>,
    instructions: Option<&str>,
    instructions_file: Option<&std::path::Path>,
    reference_branch: Option<&str>,
    diff_stat: bool,
    dry_run: bool,
    json: bool,
    engine: &dyn macc_core::Engine,
) -> Result<()> {
    // Resolve the PRD path (default: prd.json at project root).
    let resolved_prd = if prd_path.as_os_str().is_empty() || prd_path == std::path::Path::new("") {
        paths.root.join("prd.json")
    } else if prd_path.is_absolute() {
        prd_path.to_path_buf()
    } else {
        paths.root.join(prd_path)
    };

    // Resolve reference branch.
    let canonical = load_canonical_config(&paths.config_path)?;
    let ref_branch = reference_branch
        .map(|s| s.to_string())
        .or_else(|| {
            canonical
                .automation
                .coordinator
                .as_ref()
                .and_then(|c| c.reference_branch.clone())
        })
        .unwrap_or_else(|| "main".to_string());

    let model_selection = build_model_selection(model_routing, model);

    let request = PrdAuditRequest {
        prd_path: resolved_prd,
        tool: tool.map(|s| s.to_string()),
        model_selection,
        instructions: instructions.map(|s| s.to_string()),
        instructions_file: instructions_file.map(|p| p.to_path_buf()),
        reference_branch: ref_branch,
        include_diff_stat: diff_stat,
        dry_run,
    };

    let result = engine.prd_audit(paths, &request)?;

    if json {
        println!("{}", serde_json::to_string_pretty(&result).unwrap());
        return Ok(());
    }

    if !result.prompt_generated {
        println!("No tasks to audit (no completed tasks with commit context or todo tasks).");
        return Ok(());
    }

    println!(
        "Audit PRD: {} completed task(s) with context, {} todo task(s).",
        result.completed_with_context, result.todo_tasks
    );

    if dry_run || !result.dispatched {
        if let Some(ref prompt) = result.prompt {
            println!("--- BEGIN AUDIT PROMPT ---");
            println!("{}", prompt);
            println!("--- END AUDIT PROMPT ---");
        }
        if tool.is_none() {
            println!("Hint: use --tool <id> to invoke an AI tool with this prompt.");
        }
    } else {
        println!(
            "Audit prompt sent to tool '{}'.",
            tool.unwrap_or("(unknown)")
        );
    }

    Ok(())
}

// ── Promote ───────────────────────────────────────────────────────────────────

fn run_promote(
    paths: &macc_core::ProjectPaths,
    source_path: &std::path::Path,
    dest_path: Option<&std::path::Path>,
    yes: bool,
    json: bool,
    engine: &dyn macc_core::Engine,
) -> Result<()> {
    let canonical = load_canonical_config(&paths.config_path).ok();
    let default_dest = canonical
        .as_ref()
        .and_then(|c| c.prd_generation.as_ref())
        .and_then(|p| p.promotion.as_ref())
        .map(|pr| pr.default_output_path.as_str())
        .unwrap_or("prd.json");

    let dest = dest_path
        .map(|p| p.to_path_buf())
        .unwrap_or_else(|| paths.root.join(default_dest));

    let source = if source_path.is_absolute() {
        source_path.to_path_buf()
    } else {
        paths.root.join(source_path)
    };

    let options = PromoteOptions {
        source_path: source,
        dest_path: dest,
        yes,
        backup_dir: Some(paths.backups_dir.clone()),
    };

    let result = engine.prd_promote(&options)?;

    if json {
        println!("{}", serde_json::to_string_pretty(&result).unwrap());
    } else {
        println!("Promoted: {} -> {}", result.source, result.destination);
        if let Some(b) = result.backup_created {
            println!("Backup:   {}", b);
        }
    }

    Ok(())
}

// ── Validate ──────────────────────────────────────────────────────────────────

fn run_validate(
    prd_path: &std::path::Path,
    json: bool,
    engine: &dyn macc_core::Engine,
) -> Result<()> {
    let result = engine.prd_validate(prd_path, None);
    if json {
        println!("{}", serde_json::to_string_pretty(&result).unwrap());
    } else {
        print_validation_result(&result, false);
    }
    if result.ok {
        Ok(())
    } else {
        Err(macc_core::MaccError::Validation(
            "PRD-GEN-VALIDATION-FAILED: validation found errors — see output above.".to_string(),
        ))
    }
}

// ── Runs ──────────────────────────────────────────────────────────────────────

fn run_list_runs(
    paths: &macc_core::ProjectPaths,
    json: bool,
    engine: &dyn macc_core::Engine,
) -> Result<()> {
    let runs = engine.prd_list_runs(paths);
    if json {
        let items: Vec<serde_json::Value> = runs
            .iter()
            .map(|(id, path)| serde_json::json!({ "run_id": id, "path": path.to_string_lossy() }))
            .collect();
        println!("{}", serde_json::to_string_pretty(&items).unwrap());
    } else if runs.is_empty() {
        println!(
            "No generation runs found under '{}'.",
            PRD_GENERATION_DEFAULT_TARGET_DIR
        );
    } else {
        println!("{:<25} PATH", "RUN ID");
        println!("{}", "-".repeat(60));
        for (id, path) in &runs {
            println!("{:<25} {}", id, path.display());
        }
    }
    Ok(())
}

// ── Show run ──────────────────────────────────────────────────────────────────

fn run_show_run(paths: &macc_core::ProjectPaths, run_id: &str, json: bool) -> Result<()> {
    use macc_core::MaccError;

    let run_dir = paths
        .root
        .join(PRD_GENERATION_DEFAULT_TARGET_DIR)
        .join(run_id);

    if !run_dir.exists() {
        return Err(MaccError::Validation(format!(
            "Run '{}' not found under '{}'.",
            run_id, PRD_GENERATION_DEFAULT_TARGET_DIR
        )));
    }

    let metadata_path = run_dir.join("generation-metadata.json");
    let files: Vec<String> = std::fs::read_dir(&run_dir)
        .ok()
        .into_iter()
        .flatten()
        .flatten()
        .filter_map(|e| {
            let p = e.path();
            if p.is_file() {
                Some(e.file_name().to_string_lossy().to_string())
            } else {
                None
            }
        })
        .collect();

    if json {
        let meta: serde_json::Value = if metadata_path.exists() {
            let raw = std::fs::read_to_string(&metadata_path).unwrap_or_default();
            serde_json::from_str(&raw).unwrap_or(serde_json::json!({}))
        } else {
            serde_json::json!({})
        };
        let out = serde_json::json!({
            "run_id": run_id,
            "path": run_dir.to_string_lossy(),
            "files": files,
            "metadata": meta,
        });
        println!("{}", serde_json::to_string_pretty(&out).unwrap());
    } else {
        println!("Run ID: {}", run_id);
        println!("Path:   {}", run_dir.display());
        println!("Files:");
        for f in &files {
            println!("  {}", f);
        }
    }

    Ok(())
}

// ── Helpers ───────────────────────────────────────────────────────────────────

fn build_model_selection(
    model_routing: Option<&str>,
    model: Option<&str>,
) -> Option<ModelSelection> {
    match model_routing {
        Some("manual") => Some(ModelSelection {
            mode: ModelRoutingMode::Manual,
            model: model.map(|s| s.to_string()),
        }),
        Some("auto") => Some(ModelSelection {
            mode: ModelRoutingMode::Auto,
            model: None,
        }),
        _ => None,
    }
}

fn print_validation_result(result: &ValidationResult, json: bool) {
    if json {
        println!("{}", serde_json::to_string_pretty(result).unwrap());
        return;
    }
    if result.ok {
        println!("Validation: OK");
    } else {
        println!("Validation: FAILED");
    }
    for e in &result.errors {
        println!("  ERROR: {}", e);
    }
    for w in &result.warnings {
        println!("  WARN:  {}", w);
    }
}
