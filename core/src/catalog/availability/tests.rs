use super::*;
use std::sync::atomic::{AtomicUsize, Ordering};
struct Fake {
    state: State,
    calls: AtomicUsize,
}
impl Checker for Fake {
    fn check(&self, _: &std::path::Path, _: &SkillEntry, now: i64) -> Availability {
        self.calls.fetch_add(1, Ordering::SeqCst);
        Availability::new(self.state.clone(), "fixture evidence", now)
    }
}
fn entry() -> SkillEntry {
    serde_json::from_value(serde_json::json!({"id":"test","name":"Test","description":"fixture","tags":[],"selector":{"subpath":"skills/test"},"source":{"kind":"git","url":"https://github.com/example/test.git","ref":"main","checksum":null}})).unwrap()
}
#[test]
fn confirmed_absence_disables_but_transient_failure_does_not_erase_evidence() {
    let dir = tempfile::tempdir().unwrap();
    let paths = ProjectPaths::from_root(dir.path());
    let mut entries = vec![entry()];
    let missing = Fake {
        state: State::Unavailable,
        calls: AtomicUsize::new(0),
    };
    let now = chrono::Utc::now().timestamp();
    refresh_with(&paths, &entries, &missing, false, now).unwrap();
    annotate(&paths, &mut entries);
    assert!(ensure_selectable(&entries[0]).is_err());
    refresh_with(&paths, &entries, &missing, false, now + 1).unwrap();
    assert_eq!(missing.calls.load(Ordering::SeqCst), 1);
    let network = Fake {
        state: State::Unknown,
        calls: AtomicUsize::new(0),
    };
    refresh_with(&paths, &entries, &network, true, now + 2).unwrap();
    annotate(&paths, &mut entries);
    assert!(ensure_selectable(&entries[0]).is_err());
    assert!(entries[0]
        .availability
        .as_ref()
        .unwrap()
        .last_error
        .is_some());
    let online = Fake {
        state: State::Available,
        calls: AtomicUsize::new(0),
    };
    refresh_with(&paths, &entries, &online, true, now + 3).unwrap();
    annotate(&paths, &mut entries);
    assert!(ensure_selectable(&entries[0]).is_ok());
    entries[0].selector.subpath = "skills/moved".into();
    annotate(&paths, &mut entries);
    assert!(entries[0].availability.is_none());
}
#[test]
fn unknown_source_remains_selectable_and_metadata_does_not_modify_catalog_or_selections() {
    let dir = tempfile::tempdir().unwrap();
    let paths = ProjectPaths::from_root(dir.path());
    let mut entries = vec![entry()];
    let checker = Fake {
        state: State::Unknown,
        calls: AtomicUsize::new(0),
    };
    refresh_with(&paths, &entries, &checker, true, 100).unwrap();
    annotate(&paths, &mut entries);
    assert!(ensure_selectable(&entries[0]).is_ok());
    assert!(!paths.config_path.exists());
    assert!(!paths.project_skills_catalog_path().exists());
}

#[test]
fn selected_unavailable_skill_blocks_apply_without_changing_selection() {
    let dir = tempfile::tempdir().unwrap();
    let paths = ProjectPaths::from_root(dir.path());
    let mut catalog = super::super::SkillsCatalog::default();
    catalog.entries.push(entry());
    catalog
        .save_atomically(&paths, &paths.project_skills_catalog_path())
        .unwrap();
    let mut config = crate::config::CanonicalConfig::default();
    config.selections = Some(crate::config::SelectionsConfig {
        skills: vec!["test".into()],
        ..Default::default()
    });
    let resolved = crate::resolve::resolve(&config, &crate::resolve::CliOverrides::default());
    let checker = Fake {
        state: State::Unavailable,
        calls: AtomicUsize::new(0),
    };
    refresh_with(
        &paths,
        &catalog.entries,
        &checker,
        true,
        chrono::Utc::now().timestamp(),
    )
    .unwrap();
    let error = crate::resolve::resolve_fetch_units(&paths, &resolved)
        .unwrap_err()
        .to_string();
    assert!(error.contains("Skill 'test' is unavailable"), "{error}");
    assert_eq!(config.selections.unwrap().skills, vec!["test"]);
}

#[test]
fn runtime_status_is_not_persisted_into_source_catalog_and_offline_never_refreshes() {
    let dir = tempfile::tempdir().unwrap();
    let paths = ProjectPaths::from_root(dir.path());
    let mut skill = entry();
    skill.availability = Some(Availability::new(
        State::Unavailable,
        "untrusted annotation",
        1,
    ));
    let mut catalog = super::super::SkillsCatalog::default();
    catalog.entries.push(skill);
    catalog
        .save_atomically(&paths, &paths.project_skills_catalog_path())
        .unwrap();
    let raw = std::fs::read_to_string(paths.project_skills_catalog_path()).unwrap();
    assert!(!raw.contains("availability"));
    let loaded = super::super::load_skills_catalog_with_local(&paths).unwrap();
    assert!(loaded
        .entries
        .iter()
        .find(|s| s.id == "test")
        .unwrap()
        .availability
        .is_none());
    // Missing configuration is offline by default for source checks.
    refresh(&paths, true).unwrap();
    schedule_refresh(&paths, true);
    assert!(!checking(&paths));
    assert!(!path(&paths).exists());
}

#[test]
fn local_skill_lifecycle_is_checked_without_network() {
    let dir = tempfile::tempdir().unwrap();
    let paths = ProjectPaths::from_root(dir.path());
    let mut skill = entry();
    skill.source.kind = SourceKind::Local;
    skill.source.url = ".".into();
    skill.selector.subpath = "skill".into();
    let mut entries = vec![skill];
    annotate(&paths, &mut entries);
    assert!(ensure_selectable(&entries[0]).is_err());
    std::fs::create_dir_all(dir.path().join("skill")).unwrap();
    std::fs::write(dir.path().join("skill/SKILL.md"), "fixture").unwrap();
    annotate(&paths, &mut entries);
    assert!(ensure_selectable(&entries[0]).is_ok());
}

#[test]
fn install_checks_the_requested_reference_instead_of_reusing_another_refs_failure() {
    let dir = tempfile::tempdir().unwrap();
    let paths = ProjectPaths::from_root(dir.path());
    let mut skill = entry();
    refresh_with(
        &paths,
        std::slice::from_ref(&skill),
        &Fake {
            state: State::Unavailable,
            calls: AtomicUsize::new(0),
        },
        true,
        chrono::Utc::now().timestamp(),
    )
    .unwrap();
    assert!(check_install(&paths, &mut skill).is_err());
    skill.source.reference = "fixed-release".into();
    // Offline: the other ref is unknown and follows ordinary cached installer checks.
    assert!(check_install(&paths, &mut skill).is_ok());
    assert!(skill.availability.is_none());
}

#[test]
fn legacy_install_cannot_bypass_availability_gate_or_invoke_the_backend() {
    struct NoBackend;
    impl crate::catalog::service::CatalogInstallBackend for NoBackend {
        fn list_tools(
            &self,
            _: &ProjectPaths,
        ) -> (
            Vec<crate::tool::ToolDescriptor>,
            Vec<crate::tool::ToolDiagnostic>,
        ) {
            panic!("blocked before backend");
        }
        fn materialize_fetch_unit(
            &self,
            _: &ProjectPaths,
            _: crate::resolve::FetchUnit,
            _: bool,
            _: bool,
        ) -> Result<crate::resolve::MaterializedFetchUnit> {
            panic!("blocked before fetch");
        }
        fn apply(
            &self,
            _: &ProjectPaths,
            _: &mut crate::plan::ActionPlan,
            _: bool,
        ) -> Result<crate::ApplyReport> {
            panic!("blocked before apply");
        }
    }
    let dir = tempfile::tempdir().unwrap();
    let paths = ProjectPaths::from_root(dir.path());
    let mut catalog = crate::catalog::SkillsCatalog::default();
    catalog.entries.push(entry());
    catalog
        .save_atomically(&paths, &paths.project_skills_catalog_path())
        .unwrap();
    refresh_with(
        &paths,
        &catalog.entries,
        &Fake {
            state: State::Unavailable,
            calls: AtomicUsize::new(0),
        },
        true,
        chrono::Utc::now().timestamp(),
    )
    .unwrap();
    let error = crate::catalog::service::install_skill(
        &paths,
        "fixture",
        "test",
        &crate::config::CanonicalConfig::default(),
        &NoBackend,
    )
    .unwrap_err();
    assert!(error.to_string().contains("Skill 'test' is unavailable"));
}
