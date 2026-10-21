use super::*;
use crate::{
    catalog::{Source, SourceKind},
    plan::{builders, ActionPlan},
    resolve::{MaterializedFetchUnit, Selection, SelectionKind, SourceProvenance},
    ProjectPaths,
};
use std::collections::BTreeMap;
fn fixture(
    root: &std::path::Path,
    tool: &str,
    sha: &str,
    bytes: &str,
) -> (MaterializedFetchUnit, ActionPlan) {
    let snapshot = root.join(format!("snapshot-{tool}"));
    std::fs::create_dir_all(&snapshot).unwrap();
    std::fs::write(snapshot.join("SKILL.md"), bytes).unwrap();
    let requested = Source {
        kind: SourceKind::Git,
        url: "https://example.invalid/skills.git".into(),
        reference: "main".into(),
        checksum: None,
        subpaths: vec!["".into()],
    };
    let mut resolved = requested.clone();
    resolved.reference = sha.into();
    let unit = MaterializedFetchUnit {
        source_root_path: snapshot.clone(),
        selections: vec![Selection {
            id: "fixture".into(),
            subpath: "".into(),
            kind: SelectionKind::Skill,
            tools: vec![tool.into()],
        }],
        provenance: Some(SourceProvenance {
            requested,
            resolved,
            cache_key: format!("sources-v2/{sha}"),
            tracking_ref: None,
        }),
    };
    let mut plan = ActionPlan::new();
    builders::plan_skill_install(&mut plan, tool, "fixture", &snapshot, "").unwrap();
    (unit, plan)
}
#[test]
fn reviewed_lock_records_actual_sha_and_bytes_and_is_stable_on_reapply() {
    let dir = tempfile::tempdir().unwrap();
    let paths = ProjectPaths::from_root(dir.path());
    let (unit, mut plan) = fixture(dir.path(), "tool-a", &"a".repeat(40), "one");
    let roots = BTreeMap::from([("tool-a".into(), ".tool-a/skills".into())]);
    lifecycle::plan_lock(&paths, &mut plan, std::slice::from_ref(&unit), &roots).unwrap();
    crate::apply_plan(&paths, &mut plan, false).unwrap();
    let lock = SkillsLockFile::load(&paths.skills_lock_path()).unwrap();
    let entry = lock.find("fixture", "tool-a").unwrap();
    assert_eq!(
        entry.source.resolved_ref.as_deref(),
        Some("aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa")
    );
    assert!(entry.source.pinned);
    assert_eq!(
        entry.installed.targets[0].digest,
        Some(sha256_digest(b"one"))
    );
    let (_, mut again) = fixture(dir.path(), "tool-a", &"a".repeat(40), "one");
    lifecycle::plan_lock(&paths, &mut again, &[unit], &roots).unwrap();
    assert!(again.skills_lock_revision.is_some());
    assert!(!again.actions.iter().any(|action|matches!(action,crate::plan::Action::WriteFile{path,..} if path==".macc/skills.lock.json")));
    let mut changed = lock;
    changed.skills[0].source.resolved_ref = Some("b".repeat(40));
    changed.save(&paths.skills_lock_path()).unwrap();
    assert!(locking::guard(&paths, &again).is_err());
}
#[test]
fn local_edits_are_preserved_and_workers_keep_their_existing_pins() {
    let dir = tempfile::tempdir().unwrap();
    let paths = ProjectPaths::from_root(dir.path());
    let (unit, mut plan) = fixture(dir.path(), "tool-a", &"a".repeat(40), "one");
    let roots = BTreeMap::from([("tool-a".into(), ".tool-a/skills".into())]);
    lifecycle::plan_lock(&paths, &mut plan, std::slice::from_ref(&unit), &roots).unwrap();
    crate::apply_plan(&paths, &mut plan, false).unwrap();
    std::fs::write(
        dir.path().join(".tool-a/skills/fixture/SKILL.md"),
        "local work",
    )
    .unwrap();
    let (_, mut again) = fixture(dir.path(), "tool-a", &"b".repeat(40), "two");
    assert!(lifecycle::plan_lock(&paths, &mut again, &[unit], &roots).is_err());
    assert_eq!(
        std::fs::read_to_string(dir.path().join(".tool-a/skills/fixture/SKILL.md")).unwrap(),
        "local work"
    );
    let worker = ProjectPaths::from_root(dir.path().join("worker"));
    locking::inherit(&paths, &worker).unwrap();
    let first = std::fs::read(worker.skills_lock_path()).unwrap();
    let mut lock = SkillsLockFile::load(&paths.skills_lock_path()).unwrap();
    lock.skills[0].source.resolved_ref = Some("c".repeat(40));
    lock.save(&paths.skills_lock_path()).unwrap();
    locking::inherit(&paths, &worker).unwrap();
    assert_eq!(std::fs::read(worker.skills_lock_path()).unwrap(), first);
}
#[test]
fn different_tool_versions_resolve_independently_from_the_lock() {
    let dir = tempfile::tempdir().unwrap();
    let paths = ProjectPaths::from_root(dir.path());
    let (a, mut plan) = fixture(dir.path(), "tool-a", &"a".repeat(40), "one");
    let (b, other) = fixture(dir.path(), "tool-b", &"b".repeat(40), "two");
    plan.actions.extend(other.actions);
    let roots = BTreeMap::from([
        ("tool-a".into(), ".tool-a/skills".into()),
        ("tool-b".into(), ".tool-b/skills".into()),
    ]);
    lifecycle::plan_lock(&paths, &mut plan, &[a, b], &roots).unwrap();
    crate::apply_plan(&paths, &mut plan, false).unwrap();
    let entry:crate::catalog::SkillEntry=serde_json::from_value(serde_json::json!({"id":"fixture","name":"Fixture","description":"","tags":[],"selector":{"subpath":""},"source":{"kind":"git","url":"https://example.invalid/skills.git","ref":"main"}})).unwrap();
    let mut catalog = crate::catalog::SkillsCatalog::default();
    catalog.entries.push(entry);
    catalog
        .save_atomically(&paths, &paths.project_skills_catalog_path())
        .unwrap();
    let mut canonical = crate::config::CanonicalConfig::default();
    canonical.tools.enabled = vec!["tool-a".into(), "tool-b".into()];
    canonical.selections = Some(crate::config::SelectionsConfig {
        skills: vec!["fixture".into()],
        ..Default::default()
    });
    let resolved = crate::resolve::resolve(&canonical, &Default::default());
    let units = crate::resolve::resolve_fetch_units(&paths, &resolved).unwrap();
    let versions = units
        .iter()
        .filter(|u| u.selections.iter().any(|s| s.id == "fixture"))
        .map(|u| u.source.reference.clone())
        .collect::<std::collections::BTreeSet<_>>();
    assert_eq!(
        versions,
        std::collections::BTreeSet::from(["a".repeat(40), "b".repeat(40)])
    );
    canonical.selections = None;
    canonical
        .tools
        .config
        .insert("tool-a".into(), serde_json::json!({"skills": ["fixture"]}));
    let resolved = crate::resolve::resolve(&canonical, &Default::default());
    let scoped = crate::resolve::resolve_fetch_units(&paths, &resolved).unwrap();
    let selections = scoped
        .iter()
        .flat_map(|unit| &unit.selections)
        .filter(|selection| selection.id == "fixture")
        .collect::<Vec<_>>();
    assert_eq!(selections.len(), 1);
    assert_eq!(selections[0].tools, vec!["tool-a".to_string()]);
}

#[test]
fn explicit_sha_reinstallation_replaces_the_previous_tracking_branch() {
    let dir = tempfile::tempdir().unwrap();
    let paths = ProjectPaths::from_root(dir.path());
    let sha = "a".repeat(40);
    let (mut unit, _) = fixture(dir.path(), "tool-a", &sha, "one");
    let roots = BTreeMap::from([("tool-a".into(), ".tool-a/skills".into())]);
    let plan = lifecycle::installation_plan(&paths, std::slice::from_ref(&unit), &roots).unwrap();
    lifecycle::apply_installation(&paths, &plan, std::slice::from_ref(&unit), &roots).unwrap();
    unit.provenance.as_mut().unwrap().requested.reference = sha.clone();
    unit.provenance.as_mut().unwrap().tracking_ref = Some(sha.clone());
    let plan = lifecycle::installation_plan(&paths, std::slice::from_ref(&unit), &roots).unwrap();
    lifecycle::apply_installation(&paths, &plan, &[unit], &roots).unwrap();
    let lock = SkillsLockFile::load(&paths.skills_lock_path()).unwrap();
    assert_eq!(
        lock.find("fixture", "tool-a")
            .unwrap()
            .source
            .requested_ref
            .as_deref(),
        Some(sha.as_str())
    );
}

#[test]
fn targeted_installation_rejects_unrelated_actions_and_clear_keeps_pins() {
    let dir = tempfile::tempdir().unwrap();
    let paths = ProjectPaths::from_root(dir.path());
    let (unit, _) = fixture(dir.path(), "tool-a", &"a".repeat(40), "one");
    let roots = BTreeMap::from([("tool-a".into(), ".tool-a/skills".into())]);
    let units = vec![unit];
    let mut plan = lifecycle::installation_plan(&paths, &units, &roots).unwrap();
    plan.add_action(crate::plan::Action::WriteFile {
        path: "outside-skill.txt".into(),
        content: b"unrelated".to_vec(),
        scope: crate::plan::Scope::Project,
    });
    assert!(lifecycle::apply_installation(&paths, &plan, &units, &roots).is_err());
    assert!(!dir.path().join("outside-skill.txt").exists());
    let plan = lifecycle::installation_plan(&paths, &units, &roots).unwrap();
    lifecycle::apply_installation(&paths, &plan, &units, &roots).unwrap();
    let lock = std::fs::read(paths.skills_lock_path()).unwrap();
    crate::clear(&paths).unwrap();
    assert_eq!(std::fs::read(paths.skills_lock_path()).unwrap(), lock);
}
