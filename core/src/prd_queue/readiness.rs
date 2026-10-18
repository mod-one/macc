//! Read-only task-source resolution for diagnostics and client launch reviews.
use super::*;
use crate::{config::CoordinatorConfig, ProjectPaths};

#[derive(Debug, Clone)]
pub struct Readiness {
    pub current: CurrentPrd,
    pub path: PathBuf,
    pub ready_tasks: usize,
}

pub fn inspect(paths: &ProjectPaths, config: Option<&CoordinatorConfig>) -> Result<Readiness> {
    let current = current(&paths.root, config);
    if config.is_some_and(|c| !c.prd_files.is_empty()) {
        let entries = validate(&paths.root, &config.unwrap().prd_files)?;
        let progress = load(&paths.root)?;
        if !progress.entries.is_empty() && progress.entries != entries {
            return Err(invalid(
                "PRD queue changed; review and reset progress before continuing.",
            ));
        }
    }
    let mut path = paths.root.join(&current.path);
    // Historical single-file projects keep their PRD under .macc/.
    if config.is_none_or(|c| c.prd_files.is_empty() && c.prd_file.is_none()) && !path.exists() {
        path = paths.macc_dir.join("prd.json");
    }
    if current.completed {
        return Ok(Readiness {
            current,
            path,
            ready_tasks: 0,
        });
    }
    let value: Value = serde_json::from_slice(&read(&path)?)
        .map_err(|e| invalid(format!("{}: {e}", path.display())))?;
    let tasks = value
        .get("tasks")
        .and_then(Value::as_array)
        .ok_or_else(|| invalid(format!("{}: tasks array required", path.display())))?;
    let ready_tasks = tasks
        .iter()
        .filter(|t| {
            t.get("passes").and_then(Value::as_bool) != Some(true)
                && matches!(
                    t.get("state").and_then(Value::as_str),
                    None | Some("todo" | "ready")
                )
        })
        .count();
    Ok(Readiness {
        current,
        path,
        ready_tasks,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    fn fixture() -> (tempfile::TempDir, ProjectPaths, CoordinatorConfig) {
        let root = tempfile::tempdir().unwrap();
        let paths = ProjectPaths::from_root(root.path());
        std::fs::create_dir_all(&paths.macc_dir).unwrap();
        std::fs::write(
            root.path().join("first.json"),
            r#"{"tasks":[{"id":"A","passes":true}]}"#,
        )
        .unwrap();
        std::fs::write(
            root.path().join("second.json"),
            r#"{"tasks":[{"id":"B","state":"todo"}]}"#,
        )
        .unwrap();
        let cfg = CoordinatorConfig {
            prd_files: vec!["first.json".into(), "second.json".into()],
            ..Default::default()
        };
        (root, paths, cfg)
    }
    #[test]
    fn diagnostics_and_onboarding_follow_the_saved_queue_cursor() {
        let (_root, paths, cfg) = fixture();
        let mut canonical = crate::config::CanonicalConfig::default();
        canonical.automation.coordinator = Some(cfg.clone());
        std::fs::write(
            &paths.config_path,
            serde_yaml::to_string(&canonical).unwrap(),
        )
        .unwrap();
        let entries = validate(&paths.root, &cfg.prd_files).unwrap();
        save(
            &paths.root,
            &Progress {
                entries,
                next: 1,
                status: "ready".into(),
                reason: None,
            },
        )
        .unwrap();
        let source = inspect(&paths, Some(&cfg)).unwrap();
        assert_eq!(source.current.position, 2);
        assert_eq!(source.ready_tasks, 1);
        let findings = crate::doctor::check_task_readiness(&paths.macc_dir);
        assert_eq!(findings[0].id, "MACC-TASK-READY");
        assert!(findings[0].title.contains("second.json"));
        let ladder =
            crate::onboarding::compute_readiness_from_state(&paths, Some(&canonical), false);
        assert!(matches!(
            ladder.steps[3].status,
            crate::onboarding::ReadinessStatus::Done
        ));
    }
    #[test]
    fn later_ready_tasks_do_not_bypass_the_current_prd() {
        let (_root, paths, cfg) = fixture();
        assert_eq!(inspect(&paths, Some(&cfg)).unwrap().ready_tasks, 0);
    }
    #[test]
    fn changed_and_completed_queues_are_reported_distinctly() {
        let (_root, paths, cfg) = fixture();
        let entries = validate(&paths.root, &cfg.prd_files).unwrap();
        save(
            &paths.root,
            &Progress {
                entries,
                next: 2,
                status: "completed".into(),
                reason: None,
            },
        )
        .unwrap();
        assert!(inspect(&paths, Some(&cfg)).unwrap().current.completed);
        std::fs::write(paths.root.join("second.json"), r#"{"tasks":[{"id":"C"}]}"#).unwrap();
        assert!(inspect(&paths, Some(&cfg))
            .unwrap_err()
            .to_string()
            .contains("queue changed"));
    }
    #[test]
    fn legacy_prd_and_new_tasks_without_runtime_state_remain_supported() {
        let root = tempfile::tempdir().unwrap();
        let paths = ProjectPaths::from_root(root.path());
        std::fs::create_dir_all(&paths.macc_dir).unwrap();
        std::fs::write(
            paths.macc_dir.join("prd.json"),
            r#"{"tasks":[{"id":"A"},{"id":"B","passes":true}]}"#,
        )
        .unwrap();
        assert_eq!(inspect(&paths, None).unwrap().ready_tasks, 1);
    }
}
