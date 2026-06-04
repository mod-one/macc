use super::{AppContext, Command};
use macc_core::skills_runner::SkillKind;
use macc_core::Result;

pub struct SkillsCmdCommand {
    pub app: AppContext,
    pub subcommand: SkillsSubcommand,
}

pub enum SkillsSubcommand {
    // ── Run-skill subcommands ───────────────────────────────────────────────
    List { tool: Option<String> },
    Show { skill: String },
    Explain { skill: String },
    Doctor,
    // ── Catalog/package lifecycle (spec §4) ─────────────────────────────────
    Available { tool: Option<String>, source: Option<String>, tag: Option<String>, json: bool },
    CatalogStatus { tool: Option<String>, verbose: bool, json: bool },
    Install { id: String, tool: String, reference: Option<String>, pin: bool, dry_run: bool },
    Update { id: Option<String>, tool: Option<String>, dry_run: bool },
    Verify { tool: Option<String>, json: bool },
    Prune { tool: Option<String>, dry_run: bool },
    Diff { id: Option<String>, tool: Option<String> },
    Uninstall { id: String, tool: Option<String>, all_tools: bool },
}

impl Command for SkillsCmdCommand {
    fn run(&self) -> Result<()> {
        let paths = self.app.project_paths()?;
        match &self.subcommand {
            SkillsSubcommand::List { tool } => {
                let skills = self.app.engine.list_skills(&paths);
                if skills.is_empty() {
                    println!("No skills found. Add skill YAML files to .macc/skills/");
                    return Ok(());
                }
                println!("{:<20} {:<12} {:<10} {}", "ID", "KIND", "RISK", "TITLE");
                println!("{:-<20} {:-<12} {:-<10} {:-<30}", "", "", "", "");
                for skill in &skills {
                    if let Some(tool_filter) = tool {
                        if !skill.targets.contains_key(tool_filter.as_str()) {
                            continue;
                        }
                    }
                    println!(
                        "{:<20} {:<12} {:<10} {}",
                        skill.id,
                        skill.kind.as_str(),
                        skill.risk.as_str(),
                        skill.title
                    );
                }
            }
            SkillsSubcommand::Show { skill } => {
                match self.app.engine.resolve_skill(&paths, skill) {
                    Some(def) => {
                        println!("ID:          {}", def.id);
                        println!("Title:       {}", def.title);
                        println!("Kind:        {}", def.kind.as_str());
                        println!("Risk:        {}", def.risk.as_str());
                        println!("Description: {}", def.description);
                        if !def.steps.is_empty() {
                            println!("Steps:");
                            for (i, step) in def.steps.iter().enumerate() {
                                if let Some(cmd) = &step.run {
                                    println!("  {}. run: {}", i + 1, cmd);
                                } else if let Some(prompt) = &step.prompt {
                                    let excerpt = if prompt.len() > 60 {
                                        format!("{}…", &prompt[..60])
                                    } else {
                                        prompt.clone()
                                    };
                                    println!("  {}. prompt: {}", i + 1, excerpt);
                                }
                            }
                        }
                        if !def.targets.is_empty() {
                            println!("Targets:");
                            for (tool, target) in &def.targets {
                                println!("  {}: strategy={}", tool, target.strategy);
                            }
                        }
                    }
                    None => {
                        println!(
                            "Skill '{}' not found. Run 'macc skills list' to see available skills.",
                            skill
                        );
                    }
                }
            }
            SkillsSubcommand::Explain { skill } => {
                match self.app.engine.resolve_skill(&paths, skill) {
                    Some(def) => {
                        println!("Skill:       {}", def.id);
                        println!("Title:       {}", def.title);
                        println!("Kind:        {}", def.kind.as_str());
                        println!("Risk:        {}", def.risk.as_str());
                        if !def.description.is_empty() {
                            println!("Description: {}", def.description);
                        }
                        println!();
                        // Plain-English explanation of the execution path.
                        match def.kind {
                            macc_core::skills_runner::SkillKind::LocalCommand => {
                                println!(
                                    "Execution: local commands only — no AI tool required."
                                );
                                if !def.steps.is_empty() {
                                    println!("Steps:");
                                    for (i, step) in def.steps.iter().enumerate() {
                                        if let Some(cmd) = &step.run {
                                            println!("  {}. $ {}", i + 1, cmd);
                                        }
                                    }
                                }
                            }
                            macc_core::skills_runner::SkillKind::Prompt => {
                                println!(
                                    "Execution: prompt sent to the selected tool adapter."
                                );
                                if !def.steps.is_empty() {
                                    if let Some(prompt) = def.steps.first().and_then(|s| s.prompt.as_deref()) {
                                        let excerpt = if prompt.len() > 200 {
                                            format!("{}…", &prompt[..200])
                                        } else {
                                            prompt.to_string()
                                        };
                                        println!("Prompt excerpt:\n  {}", excerpt);
                                    }
                                }
                            }
                            macc_core::skills_runner::SkillKind::Hybrid => {
                                println!(
                                    "Execution: local commands first, then output summarized \
                                     and sent to the selected tool adapter."
                                );
                            }
                            macc_core::skills_runner::SkillKind::Agent => {
                                println!(
                                    "Execution: routed to a specific agent persona."
                                );
                            }
                            macc_core::skills_runner::SkillKind::Coordinator => {
                                println!(
                                    "Execution: acts on PRD, task registry, or coordinator state."
                                );
                            }
                        }
                        if !def.targets.is_empty() {
                            println!();
                            println!("Adapter strategies:");
                            for (tool, target) in &def.targets {
                                println!("  {}: {}", tool, target.strategy);
                            }
                        }
                        println!();
                        println!(
                            "Run:      macc run {}",
                            def.id
                        );
                        println!(
                            "Dry-run:  macc run {} --dry-run",
                            def.id
                        );
                    }
                    None => {
                        println!(
                            "Skill '{}' not found. Run 'macc skills list' to see available skills.",
                            skill
                        );
                    }
                }
            }
            SkillsSubcommand::Doctor => {
                let skills = self.app.engine.list_skills(&paths);
                println!("Skills Doctor");
                println!("=============");
                println!("Skills directory: {}", paths.macc_dir.join("skills").display());
                println!("Skills found: {}", skills.len());
                println!();
                let local: Vec<_> = skills
                    .iter()
                    .filter(|s| !matches!(s.kind, SkillKind::Prompt) || !s.steps.is_empty())
                    .collect();
                let prompt_only: Vec<_> = skills
                    .iter()
                    .filter(|s| matches!(s.kind, SkillKind::Prompt) && s.steps.is_empty())
                    .collect();
                println!("Local-command skills: {}", local.len());
                println!("Prompt-only skills:   {}", prompt_only.len());
                println!();
                if skills.is_empty() {
                    println!(
                        "No skills found. Create skill YAML files in {}",
                        paths.macc_dir.join("skills").display()
                    );
                } else {
                    println!("OK — skills are available.");
                }
            }

            // ── Catalog lifecycle ────────────────────────────────────────────

            SkillsSubcommand::Available { tool, source, tag, json } => {

                let entries = self.app.engine.catalog_skills_available(&paths, tool.as_deref());
                let filtered: Vec<_> = entries
                    .into_iter()
                    .filter(|e| {
                        let src_ok = source.as_deref().map_or(true, |s| {
                            e.source.url.contains(s) || e.id.starts_with(s)
                        });
                        let tag_ok = tag.as_deref().map_or(true, |t| e.tags.iter().any(|et| et == t));
                        src_ok && tag_ok
                    })
                    .collect();

                if *json {
                    println!("{}", serde_json::to_string_pretty(&filtered).unwrap_or_default());
                    return Ok(());
                }

                if filtered.is_empty() {
                    println!("No catalog skills found matching the filter.");
                    return Ok(());
                }

                println!("Available skills\n");
                println!("{:<24} {:<18} {:<10} {:<14} {}", "ID", "Tools", "Source", "Ref", "Risk");
                println!("{:-<24} {:-<18} {:-<10} {:-<14} {}", "", "", "", "", "---");
                for e in &filtered {
                    let tools_str = if e.tools.is_empty() {
                        "(any)".to_string()
                    } else {
                        e.tools.join(",")
                    };
                    let src_short = e.source.url.split('/').last().unwrap_or("local");
                    let ref_str = e.recommended_ref.as_deref().unwrap_or("-");
                    let risk = e.risk.as_deref().unwrap_or("-");
                    println!("{:<24} {:<18} {:<10} {:<14} {}", e.id, &tools_str[..tools_str.len().min(17)], src_short, ref_str, risk);
                }
            }

            SkillsSubcommand::CatalogStatus { tool, verbose, json } => {

                let statuses = self.app.engine.skills_status(&paths, tool.as_deref())?;

                if *json {
                    println!("{}", serde_json::to_string_pretty(&statuses).unwrap_or_default());
                    return Ok(());
                }

                if statuses.is_empty() {
                    println!("No installed catalog skills found.");
                    println!("Run 'macc skills install <id> --tool <tool>' to install.");
                    return Ok(());
                }

                println!("Installed skills\n");
                println!("{:<10} {:<20} {:<12} {:<12} {:<12} {}",
                    "Tool", "Skill", "Ref", "Pin", "Status", "Source");
                println!("{:-<10} {:-<20} {:-<12} {:-<12} {:-<12} {:-<30}", "", "", "", "", "", "");
                let mut all_warnings = Vec::new();
                for s in &statuses {
                    let ref_str = s.requested_ref.as_deref().unwrap_or("-");
                    let pin_str = if s.pinned {
                        s.resolved_ref.as_deref().map(|r| &r[..r.len().min(9)]).unwrap_or("pinned").to_string()
                    } else {
                        "unpinned".to_string()
                    };
                    let src = s.source_url.as_deref().unwrap_or("-");
                    println!("{:<10} {:<20} {:<12} {:<12} {:<12} {}",
                        s.tool, s.id, ref_str, pin_str, s.kind.as_str(), src);
                    if *verbose && !s.installed_files.is_empty() {
                        for f in &s.installed_files {
                            println!("             {}", f);
                        }
                    }
                    all_warnings.extend(s.warnings.clone());
                }
                if !all_warnings.is_empty() {
                    println!("\nWarnings:");
                    for w in &all_warnings {
                        println!("- {}", w);
                    }
                }
            }

            SkillsSubcommand::Install { id, tool, reference, pin, dry_run } => {
                use macc_core::skills_catalog::{
                    SkillLockEntry, LockedSource, LockedPackage,
                    CacheRef, InstalledTargets, git_cache_key,
                    detect_conflicts,
                };


                let catalog_entries = self.app.engine.catalog_skills_available(&paths, Some(tool.as_str()));
                let entry = catalog_entries.iter().find(|e| e.id == id.as_str());

                if entry.is_none() {
                    eprintln!("Skill '{}' not found in catalog. Run 'macc skills available' to see available skills.", id);
                    return Err(macc_core::MaccError::Catalog {
                        operation: "install".to_string(),
                        message: format!("Skill '{}' not found", id),
                    });
                }
                let entry = entry.unwrap();

                // Check tool is supported.
                if !entry.tools.is_empty() && !entry.tools.iter().any(|t| t == tool.as_str()) {
                    eprintln!("Skill '{}' does not support tool '{}'. Supported: {}",
                        id, tool, entry.tools.join(", "));
                    return Err(macc_core::MaccError::Catalog {
                        operation: "install".to_string(),
                        message: format!("{}: Unsupported tool '{}'", macc_core::skills_catalog::MACC_SKILL_1003, tool),
                    });
                }

                let requested_ref = reference.clone().or_else(|| entry.recommended_ref.clone())
                    .unwrap_or_else(|| entry.source.reference.clone());

                // For mutable branch refs without --pin, warn.
                let is_mutable_ref = !matches!(requested_ref.as_str(), r if r.len() == 40 && r.chars().all(|c| c.is_ascii_hexdigit()));
                if is_mutable_ref && !pin {
                    eprintln!("Warning: Ref '{}' is mutable. Use --pin to resolve to an immutable SHA.", requested_ref);
                }

                // Build planned targets from catalog entry.
                let planned_targets: Vec<(String, String)> = entry.targets
                    .get(tool.as_str())
                    .map(|dests| dests.iter().map(|d| (
                        entry.selector.subpath.clone() + "/" + d,
                        d.clone(),
                    )).collect())
                    .unwrap_or_default();

                // Load lockfile and check conflicts.
                let lockfile = self.app.engine.skills_lockfile(&paths)?;
                let conflicts = detect_conflicts(&planned_targets, &lockfile, &paths.root);

                if !conflicts.is_empty() {
                    eprintln!("Conflict detected:\n");
                    for c in &conflicts {
                        eprintln!("  Destination: {}", c.dest);
                        eprintln!("  Issue: {}", c.message);
                    }
                    if *dry_run {
                        return Ok(());
                    }
                    return Err(macc_core::MaccError::Catalog {
                        operation: "install".to_string(),
                        message: format!("{}: Install plan has {} conflict(s)", macc_core::skills_catalog::MACC_SKILL_3001, conflicts.len()),
                    });
                }

                if *dry_run {
                    println!("Install plan for '{}' ({}): {} file(s)", id, tool, planned_targets.len());
                    for (_, dest) in &planned_targets {
                        println!("  → {}", dest);
                    }
                    println!("\nNo files written (--dry-run).");
                    return Ok(());
                }

                // Build the lock entry (fetch is out of scope for MVP — record intent).
                let resolved_ref = if *pin { None } else { None }; // SHA resolution requires network
                let cache_key = git_cache_key(
                    &entry.source.url,
                    resolved_ref.as_deref().unwrap_or(&requested_ref),
                );

                let now = chrono::Utc::now().to_rfc3339();
                let mut updated_lockfile = lockfile;
                updated_lockfile.upsert(SkillLockEntry {
                    id: id.clone(),
                    tool: tool.clone(),
                    source: LockedSource {
                        kind: format!("{:?}", entry.source.kind).to_lowercase(),
                        url: Some(entry.source.url.clone()),
                        requested_ref: Some(requested_ref.clone()),
                        resolved_ref: resolved_ref.clone(),
                        checksum: entry.source.checksum.clone(),
                        subpath: entry.selector.subpath.clone(),
                        pinned: *pin && resolved_ref.is_some(),
                    },
                    package: LockedPackage {
                        manifest_path: None,
                        manifest_digest: None,
                        id: id.clone(),
                        version: None,
                    },
                    cache: CacheRef { cache_key },
                    installed: InstalledTargets {
                        at: now,
                        targets: planned_targets.iter().map(|(src, dest)| {
                            macc_core::skills_catalog::InstalledTarget {
                                src: src.clone(),
                                dest: dest.clone(),
                                digest: None,
                                owner: "macc".to_string(),
                            }
                        }).collect(),
                    },
                });
                updated_lockfile.save(&paths.skills_lock_path())?;

                println!("Recorded install intent for '{}' ({}) in skills.lock.json.", id, tool);
                if is_mutable_ref && !pin {
                    println!("Note: skill is tracked from mutable ref '{}' (not pinned).", requested_ref);
                }
                println!("Run 'macc apply' to materialize the skill files.");
            }

            SkillsSubcommand::Update { id, tool, dry_run } => {

                let statuses = self.app.engine.skills_status(&paths, tool.as_deref())?;
                let to_check: Vec<_> = statuses
                    .iter()
                    .filter(|s| id.as_deref().map_or(true, |fid| s.id == fid))
                    .collect();

                if to_check.is_empty() {
                    println!("No installed skills match the filter.");
                    return Ok(());
                }

                println!("Update plan:\n");
                for s in &to_check {
                    let action = if s.pinned {
                        "verify (pinned — no automatic move)"
                    } else {
                        "check for newer commit on mutable ref"
                    };
                    println!("  {} ({}) — {}", s.id, s.tool, action);
                }

                if *dry_run {
                    println!("\nNo files written (--dry-run).");
                } else {
                    println!("\nNote: actual ref resolution and fetch require network access.");
                    println!("Run 'macc skills verify' to check installed digest drift.");
                }
            }

            SkillsSubcommand::Verify { tool, json } => {

                let findings = self.app.engine.skills_verify(&paths)?;
                let filtered: Vec<_> = findings
                    .iter()
                    .filter(|f| tool.as_deref().map_or(true, |t| f.tool == t))
                    .collect();

                if *json {
                    println!("{}", serde_json::to_string_pretty(&filtered).unwrap_or_default());
                    return Ok(());
                }

                if filtered.is_empty() {
                    println!("Skills verify: OK — no issues found.");
                    return Ok(());
                }

                println!("Skills verify: {} finding(s)\n", filtered.len());
                for f in &filtered {
                    println!("  {} ({}) [{}]: {}", f.skill_id, f.tool, f.kind, f.message);
                }
                return Err(macc_core::MaccError::Validation(
                    format!("Skills verification found {} issue(s)", filtered.len()),
                ));
            }

            SkillsSubcommand::Prune { tool, dry_run } => {

                let lockfile = self.app.engine.skills_lockfile(&paths)?;

                // Find lockfile entries whose installed files exist.
                let orphaned: Vec<_> = lockfile.skills.iter()
                    .filter(|e| tool.as_deref().map_or(true, |t| e.tool == t))
                    .filter(|e| {
                        e.installed.targets.iter().all(|t| paths.root.join(&t.dest).exists())
                    })
                    .collect();

                if orphaned.is_empty() {
                    println!("Nothing to prune — no orphaned skills found.");
                    return Ok(());
                }

                println!("Prune plan ({} skill(s) to remove):\n", orphaned.len());
                for entry in &orphaned {
                    println!("  {} ({})", entry.id, entry.tool);
                    for t in &entry.installed.targets {
                        println!("    {}", t.dest);
                    }
                }

                if *dry_run {
                    println!("\nNo files deleted (--dry-run).");
                    return Ok(());
                }

                println!("\nNote: file removal requires 'macc apply --prune-orphaned-skills'.");
                println!("Lockfile cleanup will complete after confirmation.");
            }

            SkillsSubcommand::Diff { id, tool } => {

                let lockfile = self.app.engine.skills_lockfile(&paths)?;
                let entries: Vec<_> = lockfile.skills.iter()
                    .filter(|e| id.as_deref().map_or(true, |fid| e.id == fid))
                    .filter(|e| tool.as_deref().map_or(true, |t| e.tool == t))
                    .collect();

                if entries.is_empty() {
                    println!("No matching installed skills found.");
                    return Ok(());
                }

                let cache_dir = paths.skills_cache_dir();
                for entry in entries {
                    let diffs = macc_core::skills_catalog::diff_skill(entry, &paths.root, &cache_dir);
                    if diffs.is_empty() {
                        println!("{} ({}): no local modifications", entry.id, entry.tool);
                    } else {
                        for d in &diffs {
                            println!("--- {} ({}): {}", d.skill_id, d.tool, d.path);
                            for line in &d.diff_lines {
                                println!("{}", line);
                            }
                        }
                    }
                }
            }

            SkillsSubcommand::Uninstall { id, tool, all_tools } => {

                let mut lockfile = self.app.engine.skills_lockfile(&paths)?;

                let tools_to_remove: Vec<String> = if *all_tools {
                    lockfile.skills.iter()
                        .filter(|e| e.id == id.as_str())
                        .map(|e| e.tool.clone())
                        .collect()
                } else if let Some(t) = tool {
                    vec![t.clone()]
                } else {
                    eprintln!("Specify --tool <tool> or --all-tools.");
                    return Err(macc_core::MaccError::Validation(
                        "Uninstall requires --tool or --all-tools".to_string(),
                    ));
                };

                if tools_to_remove.is_empty() {
                    println!("Skill '{}' not found in lockfile.", id);
                    return Ok(());
                }

                for t in &tools_to_remove {
                    let removed = lockfile.remove(id.as_str(), t.as_str());
                    if removed {
                        println!("Removed '{}' ({}) from skills.lock.json.", id, t);
                    }
                }
                lockfile.save(&paths.skills_lock_path())?;
                println!("Run 'macc apply' to remove the installed files.");
            }
        }
        Ok(())
    }
}
