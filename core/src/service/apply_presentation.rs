//! Client-independent review text; significant warnings precede consent.
use crate::{
    plan::{ActionPlan, ActionStatus, PlannedOp},
    ProjectPaths,
};
pub fn review(paths: &ProjectPaths, plan: &ActionPlan, ops: &[PlannedOp]) -> String {
    let mut changed = 0;
    let mut unchanged = 0;
    let mut user = 0;
    for op in ops {
        if op.scope == crate::plan::Scope::User {
            user += 1;
        }
        if let Some(after) = &op.after {
            if crate::plan::compute_write_status(
                &op.path,
                after,
                &crate::plan::ExistingFile {
                    exists: op.before.is_some(),
                    bytes: op.before.clone(),
                    is_text_guess: true,
                },
            ) == ActionStatus::Unchanged
            {
                unchanged += 1;
            } else {
                changed += 1;
            }
        }
    }
    let mut text=format!("MACC Apply · {}\n\nConfiguration {changed} file changes · {unchanged} already current\nScope         Project{}\nBackups       {}\n",paths.root.file_name().unwrap_or_default().to_string_lossy(),if user>0{" and user files (explicit consent required)"}else{" only"},paths.backups_dir.display());
    if let Some(activation) = &plan.o2_activation {
        text.push_str(&format!(
            "O2 storage    {} · backup included\n",
            activation.operation
        ));
    }
    if let Some(preparation) = &plan.preparation {
        text.push_str(&format!(
            "Tools         {}\n",
            preparation
                .tools
                .iter()
                .map(|t| t.spec.display_name.as_str())
                .collect::<Vec<_>>()
                .join(", ")
        ));
        for provider in &preparation.providers {
            text.push_str(&format!(
                "Provider      {} · {} · {}\n",
                provider.id, provider.version, provider.origin
            ));
        }
        if preparation.network {
            text.push_str("Network       Declared installers/login may access the network\n");
        }
    }
    let native: Vec<_> = crate::hooks::native::specs()
        .into_iter()
        .filter(|s| {
            plan.actions.iter().any(
                |a| matches!(a, crate::plan::Action::WriteFile{path,..} if path == &s.launcher),
            )
        })
        .map(|s| s.tool)
        .collect();
    if !native.is_empty() {
        text.push_str(&format!("Native hooks  {} · local verification; provider trust/runtime still require confirmation\n",native.join(", ")));
    }
    if let Ok(config) = crate::load_canonical_config(&paths.config_path) {
        match crate::resolve::resolve_fetch_units(
            paths,
            &crate::resolve::resolve(&config, &Default::default()),
        ) {
            Ok(units) => {
                let remote: Vec<_> = units
                    .iter()
                    .filter(|u| u.source.kind != crate::catalog::SourceKind::Local)
                    .collect();
                let proposed_lock = plan.actions.iter().find_map(|action| match action {
                    crate::plan::Action::WriteFile { path, content, .. }
                        if path == ".macc/skills.lock.json" =>
                    {
                        serde_json::from_slice::<crate::skills_catalog::SkillsLockFile>(content)
                            .ok()
                    }
                    _ => None,
                });
                let unpinned = remote
                    .iter()
                    .filter(|u| {
                        let will_pin = proposed_lock.as_ref().is_some_and(|lock| {
                            !u.selections.is_empty()
                                && u.selections.iter().all(|selection| {
                                    selection.kind == crate::resolve::SelectionKind::Skill
                                        && lock.skills.iter().any(|entry| {
                                            entry.id == selection.id
                                                && (selection.tools.is_empty()
                                                    || selection.tools.contains(&entry.tool))
                                                && (entry
                                                    .source
                                                    .resolved_ref
                                                    .as_deref()
                                                    .is_some_and(
                                                        crate::skills_catalog::locking::is_sha,
                                                    )
                                                    || entry.source.checksum.is_some())
                                        })
                                })
                        });
                        !will_pin
                            && u.source.checksum.as_ref().is_none_or(|s| s.is_empty())
                            && !(u.source.reference.len() == 40
                                && u.source.reference.bytes().all(|b| b.is_ascii_hexdigit()))
                    })
                    .count();
                if !remote.is_empty() {
                    text.push_str(&format!(
                        "Sources       {} remote declarations · {} without an immutable pin\n",
                        remote.len(),
                        unpinned
                    ));
                }
                if unpinned > 0 {
                    text.push_str("WARN  Unpinned source declarations can change between runs. Review or lock them for reproducibility.\n");
                }
            }
            Err(error) => text.push_str(&format!(
                "WARN  Source declarations could not be inspected: {error}\n"
            )),
        }
    }
    for warning in warnings(ops) {
        text.push_str(&format!("WARN  {warning}\n"));
    }
    text
}
pub fn warnings(ops: &[PlannedOp]) -> Vec<String> {
    let mut warnings = vec![];
    for op in ops {
        if let Some(after) = &op.after {
            for finding in crate::security::scan_bytes(&op.path, after) {
                warnings.push(format!(
                    "{}: {} ({})",
                    op.path, finding.pattern_name, finding.redacted_match
                ));
            }
        }
    }
    warnings.sort();
    warnings.dedup();
    warnings
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn design_tokens_and_normal_settings_are_not_security_findings() {
        let tmp = tempfile::tempdir().unwrap();
        let paths = ProjectPaths::from_root(tmp.path());
        let mut plan = ActionPlan::new();
        for path in [
            ".agents/skills/design/tokens/colors.css",
            ".claude/settings.json",
            "FamilyTree Settings.dc.html",
        ] {
            plan.add_action(crate::plan::Action::WriteFile {
                path: path.into(),
                content: b"{}".to_vec(),
                scope: crate::plan::Scope::Project,
            });
        }
        let ops = crate::plan::collect_plan_operations(&paths, &plan);
        assert!(warnings(&ops).is_empty());
        let result = crate::apply_operations(&paths, &ops, false, |_, _, _| {}).unwrap();
        assert!(result.warnings.is_empty());
        plan.add_action(crate::plan::Action::WriteFile {
            path: "config.json".into(),
            content: b"{\"key\":\"AKIA1234567890ABCDEF\"}".to_vec(),
            scope: crate::plan::Scope::Project,
        });
        assert!(!warnings(&crate::plan::collect_plan_operations(&paths, &plan)).is_empty());
        assert!(
            crate::validate_plan(&plan, false).is_err(),
            "actual credentials must remain blocked"
        );
    }
}
