//! Deterministic native instruction files, built from the same plan as their assets.
use crate::{
    plan::{Action, ActionPlan, Scope},
    resolve::{PlanningContext, ResolvedConfig},
    tool::{ProjectContextSection, ToolRegistry},
    MaccError, ProjectPaths, Result,
};
use std::collections::{BTreeMap, BTreeSet};

pub fn targets(ctx: &PlanningContext<'_>, tool: &str, default: &str) -> Result<Vec<String>> {
    let config = ctx
        .resolved
        .tools
        .config
        .get(tool)
        .or_else(|| ctx.resolved.tools.specific.get(tool));
    let files = match config.and_then(|v| v.pointer("/context/fileName")) {
        None => vec![default.to_owned()],
        Some(serde_json::Value::String(s)) => vec![s.clone()],
        Some(serde_json::Value::Array(a)) => a
            .iter()
            .map(|v| {
                v.as_str().map(str::to_owned).ok_or_else(|| {
                    MaccError::Validation("Context filenames must be strings".into())
                })
            })
            .collect::<Result<Vec<_>>>()?,
        _ => {
            return Err(MaccError::Validation(
                "Context filenames must be a string or array".into(),
            ))
        }
    };
    if files.is_empty() {
        return Err(MaccError::Validation(
            "At least one context filename is required".into(),
        ));
    }
    let mut out = BTreeSet::new();
    for name in files {
        if name.chars().any(char::is_control) {
            return Err(MaccError::Validation("Invalid context filename".into()));
        }
        crate::project_ownership::validate_path(ctx.paths, &name)?;
        if !name.to_ascii_lowercase().ends_with(".md") {
            return Err(MaccError::Validation(format!(
                "Context target must be a Markdown file: {name}"
            )));
        }
        out.insert(name);
    }
    Ok(out.into_iter().collect())
}

pub fn render_standards(resolved: &ResolvedConfig) -> String {
    let mut text = String::from("# Project Instructions (MACC)\n\n<!-- MACC deterministic context v2 -->\n\n## Project standards\n");
    for (key, value) in &resolved.standards.inline {
        text.push_str(&format!("- {key}: {value}\n"));
    }
    if resolved.standards.inline.is_empty() {
        text.push_str("- Follow the existing project conventions.\n");
    }
    text.push_str("\n## Execution\n- Follow the assigned task and phase, the project requirements and the native MACC decision policy.\n- Use the project's declared validation commands and report actual results; do not invent a validation pipeline or claim unverified success.\n- Use scripts for checks and automation when available. Record material decisions through MACC; an unavailable external capability must be reported explicitly.\n");
    text
}

fn planned<'a>(plan: &'a ActionPlan, name: &str) -> Option<&'a [u8]> {
    plan.actions.iter().find_map(|a| match a {
        Action::WriteFile { path, content, .. } if path == name => Some(content.as_slice()),
        _ => None,
    })
}
fn exists(ctx: &PlanningContext<'_>, plan: &ActionPlan, name: &str) -> bool {
    crate::project_ownership::validate_path(ctx.paths, name).is_ok()
        && (planned(plan, name).is_some() || ctx.paths.root.join(name).is_file())
}
fn configured_skills(resolved: &ResolvedConfig, tool: &str) -> BTreeSet<String> {
    let mut skills: BTreeSet<_> = resolved.selections.skills.iter().cloned().collect();
    if let Some(value) = resolved
        .tools
        .config
        .get(tool)
        .or_else(|| resolved.tools.specific.get(tool))
        .and_then(|v| v.get("skills"))
    {
        match value {
            serde_json::Value::Array(a) => {
                skills.extend(a.iter().filter_map(|v| v.as_str().map(str::to_owned)))
            }
            serde_json::Value::String(s) => skills.extend(
                s.split(',')
                    .map(str::trim)
                    .filter(|s| !s.is_empty())
                    .map(str::to_owned),
            ),
            _ => {}
        }
    }
    skills
}

/// Build adapter assets and all shared context files once. No tool invocation or fetch.
pub fn plan_tools(ctx: &PlanningContext<'_>, registry: &ToolRegistry) -> Result<ActionPlan> {
    let mut plan = ActionPlan::new();
    let mut groups: BTreeMap<String, Vec<String>> = BTreeMap::new();
    let mut extra = BTreeMap::<String, Vec<ProjectContextSection>>::new();
    let mut protected = BTreeSet::new();
    for tool in &ctx.resolved.tools.enabled {
        let Some(adapter) = registry.get(tool) else {
            continue;
        };
        let output = adapter.plan(ctx)?;
        plan.skill_roots.extend(output.skill_roots);
        plan.skill_file_paths.extend(output.skill_file_paths);
        plan.warnings.extend(output.warnings);
        plan.actions.extend(output.actions);
        for target in adapter.context_file_targets(ctx)? {
            if ctx
                .resolved
                .tools
                .config
                .get(tool)
                .or_else(|| ctx.resolved.tools.specific.get(tool))
                .and_then(|v| v.pointer("/context/protect"))
                .and_then(|v| v.as_bool())
                == Some(true)
            {
                protected.insert(target.clone());
            }
            groups.entry(target).or_default().push(tool.clone());
        }
        extra.insert(tool.clone(), adapter.context_file_sections(ctx)?);
    }
    for (target, mut tools) in groups {
        tools.sort();
        tools.dedup();
        // A single renderer owns shared targets such as Gemini + Antigravity's GEMINI.md.
        if plan
            .actions
            .iter()
            .any(|a| matches!(a,Action::WriteFile{path,..} if path==&target))
        {
            return Err(MaccError::Validation(format!(
                "Adapter writes the centrally managed context target {target}"
            )));
        }
        let mut effective = ctx.resolved.clone();
        let languages: BTreeSet<_> = tools
            .iter()
            .filter_map(|tool| {
                ctx.resolved
                    .tools
                    .config
                    .get(tool)
                    .or_else(|| ctx.resolved.tools.specific.get(tool))
                    .and_then(|v| v.get("language"))
                    .and_then(|v| v.as_str())
                    .or_else(|| {
                        ctx.resolved
                            .standards
                            .inline
                            .get("language")
                            .map(String::as_str)
                    })
            })
            .collect();
        if languages.len() > 1 {
            return Err(MaccError::Validation(format!(
                "Shared context {target} has incompatible tool language settings"
            )));
        }
        if let Some(language) = languages.iter().next() {
            effective
                .standards
                .inline
                .insert("language".into(), (*language).into());
        }
        let mut text = render_standards(&effective);
        text.push_str(&format!("\nContext consumers: {}.\n", tools.join(", ")));

        if let Some(path) = &ctx.resolved.standards.path {
            if exists(ctx, &plan, path) {
                text.push_str(&format!("\nAdditional project standards: `{path}`.\n"));
            } else {
                plan.warnings.push(format!(
                    "Context {target}: standards file {path} is unavailable; no import generated"
                ));
            }
        }
        for tool in &tools {
            let adapter = registry.get(tool).expect("registered adapter");
            let root = adapter.skill_install_directory();
            let mut available = vec![];
            for skill in configured_skills(ctx.resolved, tool) {
                let path = format!("{root}/{skill}/SKILL.md");
                if exists(ctx, &plan, &path) {
                    available.push((skill, path));
                } else {
                    plan.warnings.push(format!("Context {target} ({tool}): selected skill {skill} is not available for this tool; no workflow requirement generated"));
                }
            }
            if !available.is_empty() {
                text.push_str(&format!("\n## Available skills ({tool})\n"));
                text.push_str("Choose the skill matching the assigned purpose and read its SKILL.md. The current MACC attempt restrictions and decision policy remain authoritative.\n");
                for (skill, path) in available {
                    let purpose = match skill.as_str() {
                        "macc-performer" => "Use for the assigned task implementation, or its native planning-only/investigation-only attempt within that attempt's restrictions. ",
                        "macc-prd-planner" => "Use to create or revise a MACC PRD; changes to an active plan require native versioned proposals. ",
                        "macc-reviewer" => "Use for an independent review of the assigned task's changes; do not implement code during review. ",
                        "macc-auditor" => "Use after implementation/review for an independent final compliance audit of the integrated result against requirements and PRD, before acceptance, merge or release; report findings without implementing fixes. ",
                        _ => "",
                    };
                    text.push_str(&format!("- `{skill}`: {purpose}`{path}`\n"));
                }
            }
            let refs = adapter.context_reference_files();
            let refs: Vec<_> = refs.into_iter().filter(|p| exists(ctx, &plan, p)).collect();
            if !refs.is_empty() {
                text.push_str(&format!("\n## Supporting files ({tool})\n"));
                for path in refs {
                    text.push_str(&format!("- `{path}`\n"));
                }
            }
            for section in extra.get(tool).into_iter().flatten() {
                text.push_str(&format!(
                    "\n## {}\n{}\n",
                    section.heading,
                    section.content.trim()
                ));
            }
        }
        plan.actions.push(Action::WriteFile {
            path: target,
            content: text.into_bytes(),
            scope: Scope::Project,
        });
    }
    preserve_custom(ctx.paths, &protected, &mut plan)?;
    validate_writes(&plan)?;
    plan.normalize();
    Ok(plan)
}

pub fn validate_writes(plan: &ActionPlan) -> Result<()> {
    let mut files = BTreeMap::new();
    for action in &plan.actions {
        if let Action::WriteFile { path, content, .. } = action {
            if files
                .insert(path, content)
                .is_some_and(|previous| previous != content)
            {
                return Err(MaccError::Validation(format!(
                    "Conflicting generated writes for {path}; use one shared renderer"
                )));
            }
        }
    }
    Ok(())
}

pub(crate) fn owned_unchanged(paths: &ProjectPaths, path: &str) -> Result<bool> {
    let inventory = crate::project_ownership::read(paths)?;
    Ok(!inventory.protected.contains(path)
        && inventory.files.get(path).is_some_and(|expected| {
            crate::project_ownership::digest(&paths.root.join(path))
                .is_ok_and(|actual| &actual == expected)
        }))
}

pub(crate) fn preserve_custom(
    paths: &ProjectPaths,
    protected: &BTreeSet<String>,
    plan: &mut ActionPlan,
) -> Result<()> {
    let mut retained = vec![];
    for action in plan.actions.drain(..) {
        if let Action::WriteFile { path, content, .. } = &action {
            if protected.contains(path)
                && paths.root.join(path).is_file()
                && !owned_unchanged(paths, path)?
            {
                if std::fs::read(paths.root.join(path)).map_err(crate::persistent::error)?
                    != *content
                {
                    plan.warnings.push(format!("Preserved customized context {path}; review the generated context migration before replacing it"));
                }
                retained.push(Action::Noop {
                    description: format!("Preserved existing context {path}"),
                    scope: Scope::Project,
                });
                continue;
            }
        }
        retained.push(action);
    }
    plan.actions = retained;
    Ok(())
}
