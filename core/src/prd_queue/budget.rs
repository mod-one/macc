//! Planned dispatch stops retain the current PRD without inventing a failure.
use super::*;
use crate::coordinator_storage::CoordinatorRun;

pub(super) fn pause(
    root: &Path,
    progress: &mut Progress,
    run: Option<&CoordinatorRun>,
    unfinished: usize,
) -> Result<bool> {
    if unfinished == 0
        || !run.is_some_and(|run| {
            run.status == "success" && run.stop_reason.as_deref() == Some("dispatch limit reached")
        })
    {
        return Ok(false);
    }
    let entry = &progress.entries[progress.next];
    progress.status = "paused".into();
    progress.reason = Some(format!(
        "Dispatch limit reached; PRD {} retained with {unfinished} unfinished task(s). Review blocked prerequisites and tool cooldowns, then start another run to continue.", entry.path
    ));
    save(root, progress)?;
    crate::coordinator::helpers::append_coordinator_event_with_severity(
        root,
        "prd_queue_paused",
        "-",
        "run",
        "paused",
        progress.reason.as_deref().unwrap_or_default(),
        "info",
    )?;
    Ok(true)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::coordinator_storage::{CoordinatorStoragePaths, SqliteStorage};
    #[test]
    fn budget_pause_preserves_cursor_and_success_record_and_does_not_mask_failures() {
        let tmp = tempfile::tempdir().unwrap();
        let paths = crate::ProjectPaths::from_root(tmp.path());
        let db = SqliteStorage::new(CoordinatorStoragePaths::from_project_paths(&paths));
        let mut run = CoordinatorRun {
            run_id: "run-budget".into(),
            pid: 0,
            hostname: "test".into(),
            started_at: "2026-10-19T00:00:00Z".into(),
            last_tick_at: None,
            stopped_at: Some("2026-10-19T00:01:00Z".into()),
            status: "success".into(),
            epoch: 1,
            version: "test".into(),
            stop_reason: Some("dispatch limit reached".into()),
        };
        db.upsert_coordinator_run(&run).unwrap();
        let mut progress = Progress {
            entries: vec![
                Entry {
                    path: "a.json".into(),
                    fingerprint: "a".into(),
                    task_ids: vec!["A".into()],
                },
                Entry {
                    path: "b.json".into(),
                    fingerprint: "b".into(),
                    task_ids: vec!["B".into()],
                },
            ],
            ..Default::default()
        };
        assert!(pause(tmp.path(), &mut progress, Some(&run), 1).unwrap());
        let saved = load(tmp.path()).unwrap();
        assert_eq!(saved.next, 0);
        assert_eq!(saved.status, "paused");
        assert_eq!(
            db.get_coordinator_run(&run.run_id).unwrap().unwrap().status,
            "success"
        );
        assert!(db
            .load_recent_events(10)
            .unwrap()
            .iter()
            .any(|e| e.event_type == "prd_queue_paused"));
        run.status = "blocked".into();
        assert!(!pause(tmp.path(), &mut progress, Some(&run), 1).unwrap());
        run.status = "success".into();
        assert!(!pause(tmp.path(), &mut progress, Some(&run), 0).unwrap());
        run.stop_reason = Some("all tasks completed".into());
        assert!(!pause(tmp.path(), &mut progress, Some(&run), 1).unwrap());
    }
}
