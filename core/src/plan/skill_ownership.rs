//! Preserve whole user skill installations before planning receipts or writing assets.
use super::{ActionPlan, Scope};
use crate::{project_ownership, MaccError, ProjectPaths, Result};
use std::collections::BTreeSet;

fn protected(paths: &ProjectPaths, plan: &ActionPlan) -> Result<BTreeSet<String>> {
    let inventory = project_ownership::read(paths)?;
    let mut result = BTreeSet::new();
    for root in &plan.skill_roots {
        // A user may implement a skill as a symlink. Never follow or replace it.
        if crate::normalize_relative_path(root).as_deref() != Some(root) {
            return Err(MaccError::Validation(
                "Unsafe skill installation root".into(),
            ));
        }
        if paths
            .root
            .join(root)
            .symlink_metadata()
            .is_ok_and(|m| m.file_type().is_symlink())
        {
            result.insert(root.clone());
            continue;
        }
        project_ownership::validate_path(paths, root)?;
        if paths.root.join(root).exists() && !inventory.directories.contains(root) {
            result.insert(root.clone());
        }
    }
    Ok(result)
}

pub(crate) fn preserve_existing(paths: &ProjectPaths, plan: &mut ActionPlan) -> Result<()> {
    let roots = protected(paths, plan)?;
    if plan.preserved_skill_roots != roots {
        plan.preserved_skill_roots = roots.clone();
        plan.warnings
            .retain(|warning| !warning.starts_with("MACC-SKILL-USER-OWNED:"));
    }
    plan.actions.retain(|action| {
        action.scope() != Scope::Project
            || !roots
                .iter()
                .any(|root| action.path() == root || action.path().starts_with(&format!("{root}/")))
    });
    plan.skill_file_paths
        .retain(|path| !roots.iter().any(|r| path.starts_with(&format!("{r}/"))));
    for root in roots {
        let warning = format!("MACC-SKILL-USER-OWNED: Existing user skill {root} was preserved unchanged; MACC will not install or update files inside it");
        if !plan.warnings.contains(&warning) {
            plan.warnings.push(warning);
        }
    }
    Ok(())
}

/// Reject a changed ownership boundary between review and execution, before any write.
pub(crate) fn validate_review(paths: &ProjectPaths, plan: &ActionPlan) -> Result<()> {
    let mut fresh = plan.clone();
    preserve_existing(paths, &mut fresh)?;
    if fresh != *plan {
        return Err(MaccError::Validation("Skill ownership changed after preview; review Apply again to preserve the existing user skill".into()));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::plan::{Action, PlannedOpKind};
    fn plan() -> ActionPlan {
        let mut plan = ActionPlan::new();
        plan.skill_roots.insert(".tool/skills/custom".into());
        for path in [
            ".tool/skills/custom/SKILL.md",
            ".tool/skills/custom/extra.txt",
        ] {
            plan.add_action(Action::WriteFile {
                path: path.into(),
                content: b"generated".to_vec(),
                scope: Scope::Project,
            });
            plan.skill_file_paths.insert(path.into());
        }
        plan.add_action(Action::SetExecutable {
            path: ".tool/skills/custom/SKILL.md".into(),
            scope: Scope::Project,
        });
        plan
    }

    #[test]
    fn entire_preexisting_skill_is_preserved_and_warned_in_shared_preview_and_result() {
        let tmp = tempfile::tempdir().unwrap();
        let paths = ProjectPaths::from_root(tmp.path());
        let root = paths.root.join(".tool/skills/custom");
        std::fs::create_dir_all(&root).unwrap();
        std::fs::write(root.join("SKILL.md"), "my skill").unwrap();
        let mut plan = plan();
        preserve_existing(&paths, &mut plan).unwrap();
        assert!(plan.actions.is_empty());
        assert!(plan.skill_file_paths.is_empty());
        assert_eq!(plan.warnings.len(), 1);
        preserve_existing(&paths, &mut plan).unwrap();
        assert_eq!(plan.warnings.len(), 1);
        let review = crate::service::apply_presentation::review(&paths, &plan, &[]);
        assert!(review.contains("MACC-SKILL-USER-OWNED"));
        let report = crate::apply_plan(&paths, &mut plan, false).unwrap();
        assert_eq!(report.warnings, plan.warnings);
        assert_eq!(
            std::fs::read_to_string(root.join("SKILL.md")).unwrap(),
            "my skill"
        );
        assert!(!root.join("extra.txt").exists());
        assert!(!project_ownership::read(&paths)
            .unwrap()
            .directories
            .contains(".tool/skills/custom"));
        // A disappeared user skill must not silently leave an enabled installation absent.
        std::fs::remove_dir_all(&root).unwrap();
        assert!(validate_review(&paths, &plan).is_err());
    }

    #[test]
    fn macc_created_skill_can_be_updated_but_new_user_directory_invalidates_review() {
        let tmp = tempfile::tempdir().unwrap();
        let paths = ProjectPaths::from_root(tmp.path());
        let mut initial = plan();
        preserve_existing(&paths, &mut initial).unwrap();
        let ops = crate::plan::collect_plan_operations(&paths, &initial);
        assert!(ops.iter().any(|op| op.kind == PlannedOpKind::Write));
        crate::apply_plan(&paths, &mut initial, false).unwrap();
        let mut upgrade = plan();
        preserve_existing(&paths, &mut upgrade).unwrap();
        assert!(upgrade.warnings.is_empty());
        assert!(!upgrade.actions.is_empty());
        let other = tempfile::tempdir().unwrap();
        let other = ProjectPaths::from_root(other.path());
        let mut reviewed = plan();
        preserve_existing(&other, &mut reviewed).unwrap();
        std::fs::create_dir_all(other.root.join(".tool/skills/custom")).unwrap();
        let error = crate::apply_plan(&other, &mut reviewed, false).unwrap_err();
        assert!(error.to_string().contains("ownership changed"));
        assert!(!other.root.join(".tool/skills/custom/SKILL.md").exists());
    }

    #[cfg(unix)]
    #[test]
    fn user_skill_symlink_is_preserved_without_following_or_replacing_it() {
        let tmp = tempfile::tempdir().unwrap();
        let external = tempfile::tempdir().unwrap();
        let paths = ProjectPaths::from_root(tmp.path());
        std::fs::create_dir_all(paths.root.join(".tool/skills")).unwrap();
        std::fs::write(external.path().join("SKILL.md"), "user skill").unwrap();
        std::os::unix::fs::symlink(external.path(), paths.root.join(".tool/skills/custom"))
            .unwrap();
        let mut plan = plan();
        preserve_existing(&paths, &mut plan).unwrap();
        crate::apply_plan(&paths, &mut plan, false).unwrap();
        assert!(paths
            .root
            .join(".tool/skills/custom")
            .symlink_metadata()
            .unwrap()
            .file_type()
            .is_symlink());
        assert_eq!(
            std::fs::read_to_string(external.path().join("SKILL.md")).unwrap(),
            "user skill"
        );
        assert!(!external.path().join("extra.txt").exists());
    }
}
