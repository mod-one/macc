//! Derive the lockfile from the exact source snapshots and copied file bytes in an Apply review.
use super::*;
use crate::{
    catalog::SourceKind,
    plan::{Action, ActionPlan, Scope},
    resolve::{MaterializedFetchUnit, SelectionKind},
    MaccError, ProjectPaths, Result,
};
use std::collections::BTreeMap;

/// Common file materialization also serves adapters whose plan only emits configuration.
pub fn plan_files(
    plan: &mut ActionPlan,
    units: &[MaterializedFetchUnit],
    roots: &BTreeMap<String, String>,
) -> Result<()> {
    for unit in units {
        for selection in unit
            .selections
            .iter()
            .filter(|s| s.kind == SelectionKind::Skill)
        {
            if selection.id.is_empty()
                || std::path::Path::new(&selection.id).components().count() != 1
                || !locking::relative(&selection.id)
                || !locking::relative(&selection.subpath)
            {
                return Err(MaccError::Validation(
                    "Unsafe skill identity or subpath".into(),
                ));
            }
            let source = unit.source_root_path.join(&selection.subpath);
            crate::packages::validate_skill_folder(&source, true).map_err(MaccError::Validation)?;
            for (tool, root) in roots
                .iter()
                .filter(|(tool, _)| selection.tools.is_empty() || selection.tools.contains(tool))
            {
                let _ = tool;
                crate::plan::builders::expand_directory_to_plan(
                    plan,
                    &source,
                    &std::path::Path::new(root).join(&selection.id),
                )
                .map_err(MaccError::Validation)?;
                #[cfg(unix)]
                {
                    use std::os::unix::fs::PermissionsExt;
                    let prefix = format!("{root}/{}/", selection.id);
                    let executable = plan
                        .actions
                        .iter()
                        .filter_map(|action| match action {
                            Action::WriteFile { path, .. }
                                if path.starts_with(&prefix)
                                    && source
                                        .join(path.strip_prefix(&prefix).unwrap())
                                        .metadata()
                                        .is_ok_and(|metadata| {
                                            metadata.permissions().mode() & 0o111 != 0
                                        }) =>
                            {
                                Some(path.clone())
                            }
                            _ => None,
                        })
                        .collect::<Vec<_>>();
                    for path in executable {
                        plan.add_action(Action::SetExecutable {
                            path,
                            scope: Scope::Project,
                        });
                    }
                }
            }
        }
    }
    Ok(())
}
pub fn plan_lock(
    paths: &ProjectPaths,
    plan: &mut ActionPlan,
    units: &[MaterializedFetchUnit],
    roots: &BTreeMap<String, String>,
) -> Result<()> {
    let mut lock = SkillsLockFile::load(&paths.skills_lock_path())?;
    if !paths.skills_lock_path().exists() {
        lock.generated_at.clear();
    }
    let before = lock.clone();
    if units.iter().any(|u| {
        u.provenance.is_some() && u.selections.iter().any(|s| s.kind == SelectionKind::Skill)
    }) {
        plan.skills_lock_revision = Some(locking::revision(paths)?);
    }
    for unit in units {
        let Some(provenance) = &unit.provenance else {
            continue;
        };
        for selection in unit
            .selections
            .iter()
            .filter(|s| s.kind == SelectionKind::Skill)
        {
            if !locking::relative(&selection.subpath) {
                return Err(MaccError::Validation("Unsafe skill subpath".into()));
            }
            for (tool, root) in roots
                .iter()
                .filter(|(tool, _)| selection.tools.is_empty() || selection.tools.contains(tool))
            {
                let prefix = format!("{root}/{}/", selection.id);
                let mut targets: Vec<_> = plan
                    .actions
                    .iter()
                    .filter_map(|a| match a {
                        Action::WriteFile {
                            path,
                            content,
                            scope: Scope::Project,
                        } if path.starts_with(&prefix) => Some(InstalledTarget {
                            src: format!(
                                "{}/{}",
                                selection.subpath.trim_end_matches('/'),
                                path.strip_prefix(&prefix).unwrap()
                            )
                            .trim_start_matches('/')
                            .into(),
                            dest: path.clone(),
                            digest: Some(format!("sha256:{:x}", Sha256::digest(content))),
                            owner: "macc".into(),
                        }),
                        _ => None,
                    })
                    .collect();
                targets.sort_by(|a, b| a.dest.cmp(&b.dest));
                targets.dedup_by(|a, b| a.dest == b.dest);
                if targets.is_empty() {
                    continue;
                }
                let old = lock.find(&selection.id, tool);
                for target in &targets {
                    let tracked = old
                        .and_then(|entry| {
                            entry
                                .installed
                                .targets
                                .iter()
                                .find(|existing| existing.dest == target.dest)
                        })
                        .and_then(|existing| existing.digest.as_ref());
                    if tracked.is_none() {
                        if let Ok(actual) = file_digest(&paths.root.join(&target.dest)) {
                            if target.digest.as_ref() != Some(&actual) {
                                return Err(MaccError::Catalog{operation:"install".into(),message:format!("{}: existing skill file {} has no matching ownership digest; preserve or reconcile it before installation",MACC_SKILL_3001,target.dest)});
                            }
                        }
                    }
                }
                if let Some(old) = old {
                    for target in &old.installed.targets {
                        if !locking::relative(&target.dest) {
                            return Err(MaccError::Validation(
                                "Unsafe installed skill path".into(),
                            ));
                        }
                        if let (Some(expected), Ok(actual)) =
                            (&target.digest, file_digest(&paths.root.join(&target.dest)))
                        {
                            if &actual != expected
                                && targets.iter().any(|t| {
                                    t.dest == target.dest && t.digest.as_ref() != Some(&actual)
                                })
                            {
                                return Err(MaccError::Catalog{operation:"install".into(),message:format!("{}: locally modified skill file {}; preserve or restore it before updating",MACC_SKILL_3001,target.dest)});
                            }
                        }
                    }
                }
                let source = &provenance.resolved;
                if source.kind == SourceKind::Git && !locking::is_sha(&source.reference) {
                    return Err(MaccError::Validation(
                        "Materialized Git skill has no immutable SHA".into(),
                    ));
                }
                let requested = if let Some(reference) = &provenance.tracking_ref {
                    reference.clone()
                } else if locking::is_sha(&provenance.requested.reference) {
                    old.and_then(|e| e.source.requested_ref.clone())
                        .unwrap_or_else(|| provenance.requested.reference.clone())
                } else {
                    provenance.requested.reference.clone()
                };
                let manifest = unit
                    .source_root_path
                    .join(&selection.subpath)
                    .join("macc.package.json");
                let entry = SkillLockEntry {
                    id: selection.id.clone(),
                    tool: tool.clone(),
                    source: LockedSource {
                        kind: format!("{:?}", source.kind).to_lowercase(),
                        url: Some(source.url.clone()),
                        requested_ref: Some(requested),
                        resolved_ref: if source.kind == SourceKind::Git {
                            Some(source.reference.clone())
                        } else {
                            None
                        },
                        checksum: source.checksum.clone(),
                        subpath: selection.subpath.clone(),
                        pinned: source.kind == SourceKind::Git || source.checksum.is_some(),
                    },
                    package: LockedPackage {
                        manifest_path: manifest
                            .is_file()
                            .then(|| format!("{}/macc.package.json", selection.subpath)),
                        manifest_digest: if manifest.is_file() {
                            Some(file_digest(&manifest)?)
                        } else {
                            None
                        },
                        id: selection.id.clone(),
                        version: None,
                    },
                    cache: CacheRef {
                        cache_key: provenance.cache_key.clone(),
                    },
                    installed: InstalledTargets {
                        at: old.map(|e| e.installed.at.clone()).unwrap_or_default(),
                        targets,
                    },
                };
                if let Some(old) = lock
                    .skills
                    .iter_mut()
                    .find(|e| e.id == entry.id && e.tool == entry.tool)
                {
                    *old = entry;
                } else {
                    lock.skills.push(entry);
                }
            }
        }
    }
    lock.skills
        .sort_by(|a, b| (&a.id, &a.tool).cmp(&(&b.id, &b.tool)));
    if lock != before {
        plan.skills_lock_revision = Some(locking::revision(paths)?);
        plan.add_action(Action::WriteFile {
            path: ".macc/skills.lock.json".into(),
            content: serde_json::to_vec_pretty(&lock)
                .map_err(|e| MaccError::Validation(e.to_string()))?,
            scope: Scope::Project,
        });
    }
    Ok(())
}

/// A targeted package installation reviews only package files and their lock receipt.
/// Full project/tool/O2 preparation remains the responsibility of the project Apply use case.
pub fn installation_plan(
    paths: &ProjectPaths,
    units: &[MaterializedFetchUnit],
    roots: &BTreeMap<String, String>,
) -> Result<ActionPlan> {
    let mut plan = ActionPlan::new();
    plan_files(&mut plan, units, roots)?;
    plan_lock(paths, &mut plan, units, roots)?;
    plan.normalize();
    Ok(plan)
}
pub fn apply_installation(
    paths: &ProjectPaths,
    plan: &ActionPlan,
    units: &[MaterializedFetchUnit],
    roots: &BTreeMap<String, String>,
) -> Result<crate::ApplyReport> {
    let _guard = locking::guard(paths, plan)?;
    let fresh = installation_plan(paths, units, roots)?;
    if fresh != *plan {
        return Err(MaccError::Validation(
            "Skill installation changed after preview; review again".into(),
        ));
    }
    crate::validate_plan(plan, false)?;
    for action in &plan.actions {
        match action {
            Action::WriteFile {
                path,
                scope: Scope::Project,
                ..
            }
            | Action::SetExecutable {
                path,
                scope: Scope::Project,
            } if path == ".macc/skills.lock.json"
                || units
                    .iter()
                    .flat_map(|unit| &unit.selections)
                    .filter(|selection| selection.kind == SelectionKind::Skill)
                    .any(|selection| {
                        roots.iter().any(|(tool, root)| {
                            (selection.tools.is_empty() || selection.tools.contains(tool))
                                && path.starts_with(&format!("{root}/{}/", selection.id))
                        })
                    }) => {}
            _ => {
                return Err(MaccError::Validation(
                    "Skill installation plan contains an unrelated operation".into(),
                ))
            }
        }
    }
    let operations = crate::plan::collect_plan_operations(paths, plan);
    crate::apply_operations(paths, &operations, false, |_, _, _| {})
}
