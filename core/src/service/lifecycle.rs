use crate::config::CanonicalConfig;
use crate::engine::Engine;
use crate::resolve::{
    resolve, resolve_fetch_units, CliOverrides, FetchUnit, MaterializedFetchUnit,
};
use crate::service::interaction::InteractionHandler;
use crate::{load_canonical_config, MaccError, ProjectPaths, Result, ToolDescriptor};
use std::path::Path;

pub trait LifecycleFetchMaterializer {
    fn materialize_fetch_units(
        &self,
        paths: &ProjectPaths,
        units: Vec<FetchUnit>,
        quiet: bool,
        offline: bool,
    ) -> Result<Vec<MaterializedFetchUnit>>;
}

pub trait LifecycleUi: InteractionHandler {
    fn print_checks(&self, checks: &[crate::doctor::ToolCheck]);
    fn render_plan_preview(
        &self,
        paths: &ProjectPaths,
        plan: &crate::plan::ActionPlan,
        ops: &[crate::plan::PlannedOp],
        json: bool,
        explain: bool,
    ) -> Result<()>;
    fn print_pre_apply_summary(
        &self,
        paths: &ProjectPaths,
        plan: &crate::plan::ActionPlan,
        ops: &[crate::plan::PlannedOp],
    );
    fn print_pre_apply_explanations(&self, ops: &[crate::plan::PlannedOp]);
    fn confirm_user_scope_apply(
        &self,
        paths: &ProjectPaths,
        ops: &[crate::plan::PlannedOp],
    ) -> Result<()>;
    fn mark_apply_completed(&self, paths: &ProjectPaths) -> Result<()>;
    fn run_tui(&self) -> Result<()>;
    fn set_current_dir(&self, path: &Path) -> Result<()>;
    fn prompt_line(&self, prompt: &str) -> Result<String>;
    fn is_command_available(&self, command: &str) -> bool;
    /// Spawn the coordinator in the background (detached from this process).
    /// The default implementation does nothing — CLI overrides this.
    fn start_coordinator_background(&self, _paths: &ProjectPaths) -> Result<()> {
        Ok(())
    }
}

pub fn init(
    cwd: &Path,
    engine: &dyn Engine,
    force: bool,
    wizard: bool,
    ui: &dyn LifecycleUi,
) -> Result<()> {
    let paths = crate::find_project_root(cwd).unwrap_or_else(|_| ProjectPaths::from_root(cwd));
    crate::init(&paths, force)?;
    if wizard {
        run_init_wizard(&paths, engine, ui)?;
    }
    offer_saved_config_restore(&paths, ui);
    offer_saved_session_restore(&paths, ui);
    let checks = engine.doctor(&paths);
    ui.print_checks(&checks);
    Ok(())
}

pub fn plan(
    cwd: &Path,
    engine: &dyn Engine,
    overrides: CliOverrides,
    json: bool,
    explain: bool,
    ui: &dyn LifecycleUi,
    fetch_materializer: &dyn LifecycleFetchMaterializer,
) -> Result<()> {
    let project_ctx = load_project_context(cwd, engine)?;
    let paths = project_ctx.paths.clone();
    let canonical = project_ctx.canonical.clone();
    let descriptors = project_ctx.descriptors.clone();
    crate::service::project::report_diagnostics(&project_ctx.diagnostics, ui);
    let allowed_tools = project_ctx.allowed_tools;

    let migration = crate::migrate::migrate_with_known_tools(canonical.clone(), &allowed_tools);
    if !migration.warnings.is_empty() {
        ui.warn(
            "Warning: Legacy configuration detected. Run 'macc migrate' to update your config.",
        );
    }

    let resolved = resolve(&canonical, &overrides);
    let enabled_titles = enabled_titles(&descriptors, &resolved.tools.enabled);
    if !json {
        ui.info(&format!(
            "Core: Planning in {} with tools: {:?}",
            paths.root.display(),
            enabled_titles
        ));
    }

    let fetch_units = resolve_fetch_units(&paths, &resolved)?;
    let materialized_units = fetch_materializer.materialize_fetch_units(
        &paths,
        fetch_units,
        resolved.settings.quiet,
        resolved.settings.offline,
    )?;
    let plan = engine.plan(&paths, &canonical, &materialized_units, &overrides)?;
    let ops = engine.plan_operations(&paths, &plan);
    ui.render_plan_preview(&paths, &plan, &ops, json, explain)
}

#[allow(clippy::too_many_arguments)]
pub fn apply(
    cwd: &Path,
    engine: &dyn Engine,
    overrides: CliOverrides,
    dry_run: bool,
    allow_user_scope: bool,
    json: bool,
    explain: bool,
    ui: &dyn LifecycleUi,
    fetch_materializer: &dyn LifecycleFetchMaterializer,
) -> Result<()> {
    let project_ctx = load_project_context(cwd, engine)?;
    let paths = project_ctx.paths.clone();
    let canonical = project_ctx.canonical.clone();
    let descriptors = project_ctx.descriptors.clone();
    crate::service::project::report_diagnostics(&project_ctx.diagnostics, ui);
    let allowed_tools = project_ctx.allowed_tools;

    let migration = crate::migrate::migrate_with_known_tools(canonical.clone(), &allowed_tools);
    if !migration.warnings.is_empty() {
        ui.warn(
            "Warning: Legacy configuration detected. Run 'macc migrate' to update your config.",
        );
    }

    let resolved = resolve(&canonical, &overrides);
    let enabled_titles = enabled_titles(&descriptors, &resolved.tools.enabled);
    let fetch_units = resolve_fetch_units(&paths, &resolved)?;
    let materialized_units = fetch_materializer.materialize_fetch_units(
        &paths,
        fetch_units,
        resolved.settings.quiet,
        resolved.settings.offline,
    )?;

    if dry_run {
        if !json {
            ui.info(&format!(
                "Core: Dry-run apply (planning) in {} with tools: {:?}",
                paths.root.display(),
                enabled_titles
            ));
        }
        let plan = engine.plan(&paths, &canonical, &materialized_units, &overrides)?;
        let ops = engine.plan_operations(&paths, &plan);
        return ui.render_plan_preview(&paths, &plan, &ops, json, explain);
    }

    ui.info(&format!(
        "Core: Applying in {} with tools: {:?}",
        paths.root.display(),
        enabled_titles
    ));
    let mut plan = engine.plan(&paths, &canonical, &materialized_units, &overrides)?;
    let ops = engine.plan_operations(&paths, &plan);
    if !json {
        crate::ops_motif::print_trust_review_card(&paths, &plan, allow_user_scope);
        if !ui.confirm_yes_no("Proceed with apply [y/N]? ")? {
            return Err(crate::MaccError::Validation("Apply cancelled by user".to_string()));
        }
        ui.print_pre_apply_summary(&paths, &plan, &ops);
        if explain {
            ui.print_pre_apply_explanations(&ops);
        }
    }
    if allow_user_scope {
        ui.confirm_user_scope_apply(&paths, &ops)?;
    }
    let report = engine.apply(&paths, &mut plan, allow_user_scope)?;
    ui.info(&report.render_cli());
    // Auto-save configuration to user-level profiles so 'macc init' can offer it
    // on the next fresh checkout. Failure is non-fatal.
    if let Ok(mgr) = crate::profile::ProfileManager::new() {
        if let Err(e) = mgr.save_auto(&paths.root, &canonical) {
            ui.warn(&format!("Note: configuration auto-save failed: {}", e));
        }
    }
    ui.mark_apply_completed(&paths)?;
    Ok(())
}

#[allow(clippy::too_many_arguments)]
pub fn quickstart(
    cwd: &Path,
    engine: &dyn Engine,
    overrides: CliOverrides,
    assume_yes: bool,
    apply: bool,
    no_tui: bool,
    ui: &dyn LifecycleUi,
    fetch_materializer: &dyn LifecycleFetchMaterializer,
) -> Result<()> {
    let paths = crate::find_project_root(cwd).unwrap_or_else(|_| ProjectPaths::from_root(cwd));

    let mut missing = Vec::new();
    for cmd in ["git", "curl", "jq"] {
        if !ui.is_command_available(cmd) {
            missing.push(cmd);
        }
    }
    if !missing.is_empty() {
        return Err(MaccError::Validation(format!(
            "Missing required commands: {}",
            missing.join(", ")
        )));
    }

    if !paths.root.join(".git").exists() {
        ui.info(&format!(
            "No .git directory found in {}.",
            paths.root.display()
        ));
        if !assume_yes && !ui.confirm("Continue anyway [y/N]? ")? {
            return Err(MaccError::Validation("Quickstart cancelled.".into()));
        }
    }

    if !paths.macc_dir.exists() && !assume_yes {
        ui.info(".macc/ was not found in this project.");
        if !ui.confirm("Run 'macc init' now [y/N]? ")? {
            return Err(MaccError::Validation(
                "Quickstart requires initialization. Cancelled.".into(),
            ));
        }
    }

    crate::init(&paths, false)?;
    ui.info(&format!(
        "Quickstart: initialized project at {}",
        paths.root.display()
    ));

    if apply {
        run_plan_then_optional_apply(
            engine,
            &paths,
            overrides,
            assume_yes,
            ui,
            fetch_materializer,
        )?;
        return Ok(());
    }

    if no_tui {
        ui.info("Quickstart complete.");
        ui.info("Next: run 'macc plan' then 'macc apply'.");
        return Ok(());
    }

    ui.info("Quickstart complete. Opening TUI...");
    ui.set_current_dir(&paths.root)?;
    ui.run_tui()
}

fn run_plan_then_optional_apply(
    engine: &dyn Engine,
    paths: &ProjectPaths,
    overrides: CliOverrides,
    assume_yes: bool,
    ui: &dyn LifecycleUi,
    fetch_materializer: &dyn LifecycleFetchMaterializer,
) -> Result<()> {
    let canonical = load_canonical_config(&paths.config_path)?;
    let (_descriptors, diagnostics) = engine.list_tools(paths);
    crate::service::project::report_diagnostics(&diagnostics, ui);
    let resolved = resolve(&canonical, &overrides);
    let fetch_units = resolve_fetch_units(paths, &resolved)?;
    let materialized_units = fetch_materializer.materialize_fetch_units(
        paths,
        fetch_units,
        resolved.settings.quiet,
        resolved.settings.offline,
    )?;
    let plan = engine.plan(paths, &canonical, &materialized_units, &overrides)?;
    crate::preview_plan(&plan, paths)?;
    ui.info(&format!(
        "Core: Total actions planned: {}",
        plan.actions.len()
    ));

    if !assume_yes && !ui.confirm("Apply this plan now [y/N]? ")? {
        ui.info("Plan generated only. Run 'macc apply' when ready.");
        return Ok(());
    }

    let resolved = resolve(&canonical, &overrides);
    let fetch_units = resolve_fetch_units(paths, &resolved)?;
    let materialized_units = fetch_materializer.materialize_fetch_units(
        paths,
        fetch_units,
        resolved.settings.quiet,
        resolved.settings.offline,
    )?;
    let mut apply_plan = engine.plan(paths, &canonical, &materialized_units, &overrides)?;
    let report = engine.apply(paths, &mut apply_plan, false)?;
    ui.info(&report.render_cli());
    ui.mark_apply_completed(paths)?;
    Ok(())
}

/// If saved configurations exist for this project, list them and interactively
/// offer to restore one. All errors and rejections are silently swallowed —
/// this is a convenience enhancement, not a required step.
fn offer_saved_config_restore(paths: &ProjectPaths, ui: &dyn LifecycleUi) {
    let mgr = match crate::profile::ProfileManager::new() {
        Ok(m) => m,
        Err(_) => return,
    };
    let profiles = match mgr.list_for_project(&paths.root) {
        Ok(p) if !p.is_empty() => p,
        _ => return,
    };
    ui.info(&format!(
        "Found {} saved configuration(s) for this project:",
        profiles.len()
    ));
    for (i, p) in profiles.iter().enumerate() {
        let date = p
            .created_at
            .as_deref()
            .map(|d| &d[..10.min(d.len())])
            .unwrap_or("-");
        let desc_part = match p.description.as_deref() {
            Some(d) if !d.is_empty() => format!(" — {}", d),
            _ => String::new(),
        };
        ui.info(&format!("  {}. {} ({}){}", i + 1, p.name, date, desc_part));
    }
    let answer = match ui
        .prompt_line("Restore a saved configuration? Enter number or press Enter to skip: ")
    {
        Ok(a) => a,
        Err(_) => return,
    };
    let answer = answer.trim().to_string();
    if answer.is_empty() {
        return;
    }
    let idx = match answer
        .parse::<usize>()
        .ok()
        .and_then(|n| n.checked_sub(1))
        .filter(|&i| i < profiles.len())
    {
        Some(i) => i,
        None => {
            ui.warn(&format!("Invalid selection '{}', skipping.", answer));
            return;
        }
    };
    let name = &profiles[idx].name;
    let current_config = match crate::load_canonical_config(&paths.config_path) {
        Ok(c) => c,
        Err(e) => {
            ui.warn(&format!("Could not read current config: {}", e));
            return;
        }
    };
    let merged = match mgr.restore(name, &current_config, None) {
        Ok(m) => m,
        Err(e) => {
            ui.warn(&format!(
                "Could not restore configuration '{}': {}",
                name, e
            ));
            return;
        }
    };
    let yaml = match merged.to_yaml() {
        Ok(y) => y,
        Err(e) => {
            ui.warn(&format!(
                "Could not serialize restored configuration: {}",
                e
            ));
            return;
        }
    };
    if let Err(e) = std::fs::write(&paths.config_path, yaml.as_bytes()) {
        ui.warn(&format!("Could not write restored configuration: {}", e));
        return;
    }
    ui.info(&format!("Configuration '{}' restored.", name));
}

/// If saved session snapshots exist for this project, list them and
/// interactively offer to restore one. All errors are non-fatal.
fn offer_saved_session_restore(paths: &ProjectPaths, ui: &dyn LifecycleUi) {
    let snapshots = match crate::coordinator::session_manager::list_saved_sessions(&paths.root) {
        Ok(s) if !s.is_empty() => s,
        _ => return,
    };
    ui.info(&format!(
        "Found {} saved session snapshot(s) for this project:",
        snapshots.len()
    ));
    for (i, s) in snapshots.iter().enumerate() {
        let date = &s.saved_at[..10.min(s.saved_at.len())];
        ui.info(&format!(
            "  {}. {} ({}, {} session(s) across {} tool(s))",
            i + 1,
            s.name,
            date,
            s.active_session_count,
            s.tool_count
        ));
    }
    let answer =
        match ui.prompt_line("Restore a session snapshot? Enter number or press Enter to skip: ") {
            Ok(a) => a,
            Err(_) => return,
        };
    let answer = answer.trim().to_string();
    if answer.is_empty() {
        return;
    }
    let idx = match answer
        .parse::<usize>()
        .ok()
        .and_then(|n| n.checked_sub(1))
        .filter(|&i| i < snapshots.len())
    {
        Some(i) => i,
        None => {
            ui.warn(&format!("Invalid selection '{}', skipping.", answer));
            return;
        }
    };
    let name = &snapshots[idx].name;
    match crate::coordinator::session_manager::restore_sessions(&paths.root, name, false) {
        Ok(_) => ui.info(&format!("Session snapshot '{}' restored.", name)),
        Err(e) => ui.warn(&format!(
            "Could not restore session snapshot '{}': {}",
            name, e
        )),
    }
}

fn run_init_wizard(paths: &ProjectPaths, engine: &dyn Engine, ui: &dyn LifecycleUi) -> Result<()> {
    ui.info("Init wizard (3 questions)");
    let mut config = load_canonical_config(&paths.config_path)?;
    let (descriptors, diagnostics) = engine.list_tools(paths);
    crate::service::project::report_diagnostics(&diagnostics, ui);
    let tool_ids: Vec<String> = descriptors.iter().map(|d| d.id.clone()).collect();
    if !tool_ids.is_empty() {
        ui.info(&format!("Available tools: {}", tool_ids.join(", ")));
    }

    let tools_answer = ui.prompt_line("Q1/3 - Enabled tools (CSV, empty keeps current): ")?;
    if !tools_answer.is_empty() {
        let selected = parse_csv(&tools_answer);
        if selected.is_empty() {
            return Err(MaccError::Validation(
                "Wizard: at least one tool is required when tools are provided.".into(),
            ));
        }
        let unknown: Vec<String> = selected
            .iter()
            .filter(|id| !tool_ids.iter().any(|known| known == *id))
            .cloned()
            .collect();
        if !unknown.is_empty() {
            return Err(MaccError::Validation(format!(
                "Wizard: unknown tools: {}",
                unknown.join(", ")
            )));
        }
        config.tools.enabled = selected;
    }

    ui.info("Standards presets: minimal | strict | none");
    let preset = ui.prompt_line("Q2/3 - Standards preset [minimal]: ")?;
    apply_standards_preset(
        &mut config,
        if preset.is_empty() {
            "minimal"
        } else {
            &preset
        },
    )?;

    let mcp_answer =
        ui.prompt_line("Q3/3 - Enable default MCP templates in selections? [y/N]: ")?;
    let enable_mcp = matches!(mcp_answer.trim().to_ascii_lowercase().as_str(), "y" | "yes");
    if enable_mcp {
        let ids: Vec<String> = config.mcp_templates.iter().map(|t| t.id.clone()).collect();
        let mut selections = config.selections.unwrap_or_default();
        selections.mcp = ids;
        config.selections = Some(selections);
    } else if let Some(selections) = config.selections.as_mut() {
        selections.mcp.clear();
    }

    let yaml = config
        .to_yaml()
        .map_err(|e| MaccError::Validation(format!("Failed to serialize wizard config: {}", e)))?;
    crate::atomic_write(paths, &paths.config_path, yaml.as_bytes())?;
    ui.info(&format!("Wizard saved: {}", paths.config_path.display()));
    Ok(())
}

fn apply_standards_preset(config: &mut CanonicalConfig, preset: &str) -> Result<()> {
    config.standards.path = None;
    config.standards.inline.clear();

    match preset.trim().to_ascii_lowercase().as_str() {
        "minimal" => {
            config
                .standards
                .inline
                .insert("language".into(), "English".into());
            config
                .standards
                .inline
                .insert("package_manager".into(), "pnpm".into());
        }
        "strict" => {
            config
                .standards
                .inline
                .insert("language".into(), "English".into());
            config
                .standards
                .inline
                .insert("package_manager".into(), "pnpm".into());
            config
                .standards
                .inline
                .insert("typescript".into(), "strict".into());
            config
                .standards
                .inline
                .insert("imports".into(), "absolute:@/".into());
        }
        "none" => {}
        other => {
            return Err(MaccError::Validation(format!(
                "Wizard: unknown standards preset '{}'. Use minimal|strict|none.",
                other
            )));
        }
    }
    Ok(())
}

fn parse_csv(value: &str) -> Vec<String> {
    value
        .split(',')
        .map(|s| s.trim())
        .filter(|s| !s.is_empty())
        .map(|s| s.to_string())
        .collect()
}

struct LoadedProjectContext {
    paths: ProjectPaths,
    canonical: CanonicalConfig,
    descriptors: Vec<ToolDescriptor>,
    diagnostics: Vec<crate::tool::ToolDiagnostic>,
    allowed_tools: Vec<String>,
}

fn load_project_context(cwd: &Path, engine: &dyn Engine) -> Result<LoadedProjectContext> {
    let paths = crate::find_project_root(cwd)?;
    let canonical = load_canonical_config(&paths.config_path)?;
    let (descriptors, diagnostics) = engine.list_tools(&paths);
    let allowed_tools: Vec<String> = descriptors.iter().map(|d| d.id.clone()).collect();
    Ok(LoadedProjectContext {
        paths,
        canonical,
        descriptors,
        diagnostics,
        allowed_tools,
    })
}

/// Enhanced quickstart with spec §6 interactive flow and teaching mode.
#[allow(clippy::too_many_arguments)]
pub fn quickstart_extended(
    cwd: &Path,
    engine: &dyn Engine,
    overrides: CliOverrides,
    assume_yes: bool,
    apply: bool,
    no_tui: bool,
    tool: Option<&str>,
    starter_task: bool,
    start_coordinator: bool,
    check_only: bool,
    json: bool,
    ui: &dyn LifecycleUi,
    fetch_materializer: &dyn LifecycleFetchMaterializer,
) -> Result<()> {
    let paths = crate::find_project_root(cwd).unwrap_or_else(|_| ProjectPaths::from_root(cwd));

    ui.info("Welcome to MACC.");
    ui.info("");

    // Step 1 — detect project
    if !paths.root.join(".git").exists() {
        ui.info(&format!(
            "No .git directory found in {}.",
            paths.root.display()
        ));
        if !assume_yes && !ui.confirm("Continue anyway [y/N]? ")? {
            return Err(MaccError::Validation("Quickstart cancelled.".into()));
        }
    }

    ui.info(&format!(
        "Project detected:\n  Path: {}\n",
        paths.root.display()
    ));

    if check_only {
        if json {
            // Emit structured JSON for CI/scripting (spec §6.3).
            let ladder = engine.readiness_ladder(&paths);
            let findings = engine.collect_diagnostic_findings(&paths, 2);
            let ready = ladder.is_ready();
            let output = serde_json::json!({
                "ready": ready,
                "blocking_count": ladder.blocking_count,
                "steps": ladder.steps.iter().map(|s| {
                    serde_json::json!({
                        "number": s.number,
                        "label": s.label,
                        "status": format!("{:?}", s.status).to_lowercase(),
                        "detail": s.detail,
                    })
                }).collect::<Vec<_>>(),
                "findings": findings.iter().map(|f| {
                    serde_json::json!({
                        "id": f.id,
                        "title": f.title,
                        "severity": f.severity.to_string(),
                        "category": f.category,
                        "message": f.message,
                        "recommended_action": f.recommended_action,
                        "fix_available": f.fix_available,
                    })
                }).collect::<Vec<_>>(),
            });
            println!("{}", serde_json::to_string_pretty(&output).unwrap_or_default());
            return if ready { Ok(()) } else {
                Err(MaccError::Validation("Not ready to dispatch a task.".into()))
            };
        }
        ui.info("Running environment checks (--check-only)...");
        run_readiness_check(&paths, ui);
        return Ok(());
    }

    // Step 2 — initialize MACC
    crate::init(&paths, false)?;

    // Step 3 — select tool adapter
    let selected_tool = if let Some(t) = tool {
        ui.info(&format!("Tool adapter: {} (from --tool flag)", t));
        Some(t.to_string())
    } else {
        interactive_tool_selection(&paths, engine, assume_yes, ui)?
    };

    // Step 4 — apply config
    let mut run_overrides = overrides;
    if let Some(ref t) = selected_tool {
        let (descriptors, _) = engine.list_tools(&paths);
        let allowed: Vec<String> = descriptors.iter().map(|d| d.id.clone()).collect();
        if let Ok(parsed) = CliOverrides::from_tools_csv(t, &allowed) {
            run_overrides.tools = parsed.tools;
        }
    }

    if apply || assume_yes {
        ui.info("\nApplying config...");
        if let Err(e) = crate::service::lifecycle::apply(
            &paths.root,
            engine,
            run_overrides,
            false,
            false,
            false,
            false,
            ui,
            fetch_materializer,
        ) {
            ui.warn(&format!("Apply warning: {}", e));
        }
    }

    // Step 5 — create starter task if needed
    if starter_task {
        create_starter_task_if_needed(&paths, assume_yes, ui)?;
    }

    // Step 7 — run doctor preflight
    ui.info("\nRunning preflight...");
    run_readiness_check(&paths, ui);

    // Step 8 — start coordinator
    if start_coordinator {
        ui.info("\nStarting coordinator (background)...");
        match ui.start_coordinator_background(&paths) {
            Ok(()) => ui.info("  ✅ Coordinator started."),
            Err(e) => {
                ui.warn(&format!("  ⚠️  Could not start coordinator: {}", e));
                ui.info("  Run manually: macc coordinator run");
            }
        }
    }

    // Show teaching mode: equivalent commands
    ui.info("\nEquivalent commands:");
    ui.info("  macc init");
    if let Some(ref t) = selected_tool {
        ui.info(&format!("  macc plan --tools {}", t));
        ui.info(&format!("  macc apply --tools {}", t));
    } else {
        ui.info("  macc plan");
        ui.info("  macc apply");
    }
    if starter_task {
        ui.info("  macc quickstart --starter-task");
    }
    ui.info("  macc doctor");
    if start_coordinator {
        ui.info("  macc coordinator run");
    }
    ui.info("  macc status");

    // Step 10 — show status
    ui.info("\nNext:");
    ui.info("  macc status");
    ui.info("  macc web");

    if !no_tui && !apply && !assume_yes {
        ui.info("\nQuickstart complete. Opening TUI...");
        ui.set_current_dir(&paths.root)?;
        ui.run_tui()?;
    }

    Ok(())
}

fn interactive_tool_selection(
    paths: &ProjectPaths,
    engine: &dyn Engine,
    assume_yes: bool,
    ui: &dyn LifecycleUi,
) -> Result<Option<String>> {
    let (descriptors, _) = engine.list_tools(paths);
    if descriptors.is_empty() {
        return Ok(None);
    }

    ui.info("\nDetected tools:");
    for (i, d) in descriptors.iter().enumerate() {
        let check = if ui.is_command_available(&d.id) {
            "✅ installed"
        } else {
            "❌ not found"
        };
        ui.info(&format!("  {}. {}  {}", i + 1, d.title, check));
    }

    if assume_yes {
        // Pick first installed tool automatically.
        for d in &descriptors {
            if ui.is_command_available(&d.id) {
                ui.info(&format!("\nAuto-selected: {} (first installed tool)", d.title));
                return Ok(Some(d.id.clone()));
            }
        }
        return Ok(None);
    }

    let input = ui.prompt_line("\nChoose adapter [1]: ")?;
    let choice: usize = input.trim().parse().unwrap_or(1);
    let selected = descriptors.get(choice.saturating_sub(1));
    Ok(selected.map(|d| d.id.clone()))
}

fn create_starter_task_if_needed(
    paths: &ProjectPaths,
    assume_yes: bool,
    ui: &dyn LifecycleUi,
) -> Result<()> {
    let prd_path = paths.macc_dir.join("prd.json");
    if prd_path.exists() {
        let content = std::fs::read_to_string(&prd_path).unwrap_or_default();
        if let Ok(v) = serde_json::from_str::<serde_json::Value>(&content) {
            if v.get("tasks")
                .and_then(|t| t.as_array())
                .map(|a| !a.is_empty())
                .unwrap_or(false)
            {
                return Ok(());
            }
        }
    }

    if !assume_yes {
        ui.info("\nNo PRD found.");
        if !ui.confirm("Create a starter task? [y/N] ")? {
            return Ok(());
        }
    }

    let starter = serde_json::json!({
        "version": 1,
        "tasks": [{
            "id": "QS-001",
            "title": "Verify MACC setup",
            "state": "todo",
            "description": "Run a minimal validation task to confirm that MACC, the selected tool adapter, worktrees, and coordinator execution are working.",
            "steps": [
                "Read the generated MACC configuration.",
                "Run a lightweight validation command.",
                "Write a short setup confirmation note.",
                "Commit the result using the MACC commit convention."
            ]
        }]
    });

    if let Some(parent) = prd_path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| MaccError::Io {
            path: parent.to_string_lossy().into(),
            action: "create macc dir".into(),
            source: e,
        })?;
    }
    std::fs::write(
        &prd_path,
        serde_json::to_string_pretty(&starter).unwrap_or_default(),
    )
    .map_err(|e| MaccError::Io {
        path: prd_path.to_string_lossy().into(),
        action: "write starter task".into(),
        source: e,
    })?;

    ui.info("\nTask:\n  QS-001 - Verify MACC setup");
    Ok(())
}

fn run_readiness_check(paths: &ProjectPaths, ui: &dyn LifecycleUi) {
    use crate::doctor::{collect_all_findings, DiagnosticSeverity};
    let max_parallel = 2u32;
    let findings = collect_all_findings(paths, max_parallel);
    for f in &findings {
        let symbol = match f.severity {
            DiagnosticSeverity::Ok => "  ✅",
            DiagnosticSeverity::Info => "  ℹ️",
            DiagnosticSeverity::Warning => "  ⚠️",
            DiagnosticSeverity::Error => "  ❌",
        };
        ui.info(&format!("{} {}", symbol, f.title));
        if !f.message.is_empty() && !matches!(f.severity, DiagnosticSeverity::Ok) {
            ui.info(&format!("     {}", f.message));
        }
    }
}

fn enabled_titles(descriptors: &[ToolDescriptor], enabled_ids: &[String]) -> Vec<String> {
    enabled_ids
        .iter()
        .map(|id| {
            descriptors
                .iter()
                .find(|d| &d.id == id)
                .map(|d| d.title.clone())
                .unwrap_or_else(|| id.clone())
        })
        .collect()
}
