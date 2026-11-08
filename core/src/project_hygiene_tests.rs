#[cfg(test)]
mod tests {
    use super::super::*;
    #[test]
    fn typed_checks_redact_values_without_rejecting_application_uuids_or_placeholders() {
        let mut out = vec![];
        scan(
            &serde_json::json!({"app_id":"83f3b11c-cf94-4753-a378-c9740615f68e", "api_key":"${API_KEY}","session_id":"private-session", "password":"private-secret", "source":{"url":"/home/private/skill"}}),
            "config",
            &mut out,
        );
        assert_eq!(out.len(), 3);
        let rendered = serde_json::to_string(&out).unwrap();
        assert!(!rendered.contains("private-session"));
        assert!(!rendered.contains("private-secret"));
        assert!(!rendered.contains("/home/private"));
    }
}

#[cfg(test)]
mod workflow_tests {
    use super::super::*;
    #[test]
    fn apply_reviews_catalog_portability_without_changing_previewed_file() {
        let tmp = tempfile::tempdir().unwrap();
        let paths = ProjectPaths::from_root(tmp.path());
        std::fs::create_dir_all(paths.project_catalog_dir()).unwrap();
        let file = paths.project_skills_catalog_path();
        let source = paths.root.join("docs/skill").to_string_lossy().into_owned();
        let value = serde_json::json!({"entries":[{"id":"test","extra":"preserved","source":{"kind":"local","url":source}}],"custom":"preserved"});
        let before = serde_json::to_vec(&value).unwrap();
        std::fs::write(&file, &before).unwrap();
        let mut plan = crate::plan::ActionPlan::new();
        plan_portable_catalogs(&paths, &mut plan).unwrap();
        assert_eq!(std::fs::read(&file).unwrap(), before);
        let ops = crate::plan::collect_plan_operations(&paths, &plan);
        assert_eq!(ops.len(), 1);
        crate::apply_operations(&paths, &ops, false, |_, _, _| {}).unwrap();
        let result: Value = serde_json::from_slice(&std::fs::read(file).unwrap()).unwrap();
        assert_eq!(result["entries"][0]["source"]["url"], "docs/skill");
        assert_eq!(result["custom"], "preserved");
        assert_eq!(result["entries"][0]["extra"], "preserved");
    }
    #[test]
    fn tracked_runtime_is_reported_without_changing_git_index() {
        let tmp = tempfile::tempdir().unwrap();
        let paths = ProjectPaths::from_root(tmp.path());
        let git = |args: &[&str]| {
            let out =
                crate::git::run_git_output_mapped(tmp.path(), args, "hygiene fixture").unwrap();
            assert!(out.status.success());
            out.stdout
        };
        git(&["init", "-q"]);
        std::fs::create_dir_all(paths.macc_dir.join("state")).unwrap();
        std::fs::write(paths.macc_dir.join("state/tool-sessions.json"), "{}").unwrap();
        git(&["add", ".macc/state/tool-sessions.json"]);
        let before = git(&["ls-files", "--stage"]);
        assert!(findings(&paths)
            .iter()
            .any(|f| f.id == "MACC-GIT-TRACKED-RUNTIME"));
        assert_eq!(before, git(&["ls-files", "--stage"]));
    }
}

#[test]
fn local_machine_catalog_is_available_but_remains_outside_portable_files() {
    use super::*;
    let tmp = tempfile::tempdir().unwrap();
    let paths = ProjectPaths::from_root(tmp.path());
    let path = paths
        .macc_dir
        .join("persistent/catalog/skills.catalog.json");
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(&path,serde_json::json!({"schema_version":"1.0","type":"skills","updated_at":"","entries":[{"id":"local-private","name":"Local","description":"Local override","tags":[],"selector":{"subpath":"."},"source":{"kind":"local","url":"/private/machine/skill","ref":""}}]}).to_string()).unwrap();
    assert!(crate::catalog::load_effective_skills_catalog(&paths)
        .unwrap()
        .entries
        .iter()
        .any(|e| e.id == "local-private"));
    assert!(findings(&paths).is_empty());
    assert_eq!(
        portable_local_url(tmp.path(), "/private/machine/skill"),
        None
    );
    assert_eq!(
        portable_local_url(tmp.path(), tmp.path().join("docs/skill").to_str().unwrap()),
        Some("docs/skill".into())
    );
}
