//! Publish a queue result only after delivery validation and cursor persistence.
use super::*;
use crate::coordinator::control_plane::CoordinatorLog;

pub(super) fn finish(root: &Path, logger: Option<&dyn CoordinatorLog>, result: &Result<()>) {
    let Some(log) = logger else { return };
    let progress = load(root).ok();
    let (status, reason) = match result {
        Err(error) => {
            let recorded = progress
                .as_ref()
                .filter(|p| p.reason.as_deref() == Some(error.to_string().as_str()));
            let status = match recorded.map(|p| p.status.as_str()) {
                Some("blocked") => "blocked",
                Some("failed") => "failed",
                Some("paused") => "paused",
                Some("stopped_by_user") => "stopped_by_user",
                _ if matches!(error, MaccError::Validation(_)) => "blocked",
                _ => "failed",
            };
            (status, error.to_string())
        }
        Ok(()) => match progress.as_ref().map(|p| p.status.as_str()) {
            Some("paused") => (
                "paused",
                progress
                    .as_ref()
                    .and_then(|p| p.reason.clone())
                    .unwrap_or_default(),
            ),
            _ => ("success", "all PRDs delivered".into()),
        },
    };
    let _ = log.note(format!(
        "\n# Run footer\n- ended_at: {}\n- result: {status}\n- exit_reason: {reason}\n",
        crate::coordinator::helpers::now_iso_coordinator(),
    ));
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    #[derive(Default)]
    struct Capture(Mutex<Vec<String>>);
    impl CoordinatorLog for Capture {
        fn note(&self, line: String) -> Result<()> {
            self.0.lock().unwrap().push(line);
            Ok(())
        }
    }

    #[test]
    fn final_footer_reports_delivery_failure_instead_of_execution_success() {
        let root = tempfile::tempdir().unwrap();
        let log = Capture::default();
        let error = invalid("PRD is not fully delivered; queue stopped");
        save(
            root.path(),
            &Progress {
                status: "blocked".into(),
                reason: Some(error.to_string()),
                ..Default::default()
            },
        )
        .unwrap();
        finish(root.path(), Some(&log), &Err(error));
        let lines = log.0.lock().unwrap();
        assert_eq!(lines.len(), 1);
        assert!(lines[0].contains("- result: blocked\n"));
        assert!(lines[0].contains("exit_reason: Validation error: PRD is not fully delivered"));
        assert!(!lines[0].contains("result: success"));
    }

    #[test]
    fn final_footer_distinguishes_completed_paused_and_early_failure() {
        let root = tempfile::tempdir().unwrap();
        for (status, reason, expected) in [
            ("completed", None, "success"),
            ("paused", Some("Dispatch limit reached".into()), "paused"),
        ] {
            let log = Capture::default();
            save(
                root.path(),
                &Progress {
                    status: status.into(),
                    reason,
                    ..Default::default()
                },
            )
            .unwrap();
            finish(root.path(), Some(&log), &Ok(()));
            assert!(log.0.lock().unwrap()[0].contains(&format!("- result: {expected}\n")));
        }
        let log = Capture::default();
        finish(root.path(), Some(&log), &Err(invalid("PRD changed")));
        let lines = log.0.lock().unwrap();
        assert!(lines[0].contains("- result: blocked\n"));
        assert!(lines[0].contains("exit_reason: Validation error: PRD changed"));
        assert!(!lines[0].contains("Dispatch limit reached"));
    }

    #[tokio::test]
    async fn queue_entry_point_logs_early_errors_even_after_a_previous_success() {
        let root = tempfile::tempdir().unwrap();
        save(
            root.path(),
            &Progress {
                status: "completed".into(),
                ..Default::default()
            },
        )
        .unwrap();
        std::fs::write(root.path().join("invalid.json"), "{}").unwrap();
        let config = crate::config::CoordinatorConfig {
            prd_files: vec!["invalid.json".into()],
            ..Default::default()
        };
        let log = Capture::default();
        let result = run(
            root.path(),
            &Default::default(),
            &config,
            &Default::default(),
            Some(&log),
        )
        .await;
        assert!(result.is_err());
        let text = log.0.lock().unwrap().join("\n");
        assert_eq!(text.matches("# Run footer").count(), 1);
        assert!(text.contains("- result: blocked\n"));
        assert!(text.contains("non-empty tasks array required"));
        assert!(!text.contains("result: success"));
    }
}
