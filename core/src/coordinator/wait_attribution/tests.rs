use super::*;
use crate::coordinator::task_selector::{diagnose_unschedulable_tasks, TaskSelectorConfig};
use crate::coordinator_storage::{
    CoordinatorRun, CoordinatorSnapshot, CoordinatorStorage, CoordinatorStoragePaths, SqliteStorage,
};
use crate::service::run_summary::load_latest_run_summary;
use crate::{supervisor::incident, ProjectPaths};
use serde_json::json;

#[test]
fn mixed_origin_wait_survives_completion_and_requeue_in_all_diagnostics() {
    for state in ["blocked", "todo"] {
        let temp = tempfile::tempdir().unwrap();
        let paths = ProjectPaths::from_root(temp.path());
        let db = SqliteStorage::new(CoordinatorStoragePaths::from_project_paths(&paths));
        let wait = json!({"decision_wait":"runner-1","decision_wait_owner":"WINDOWS","decision_wait_status":"blocked_external","last_error_code":"E903","last_error_origin":"decisions","last_error_message":"DM_PENDING: provision an interactive Windows runner"});
        let mut snapshot = CoordinatorSnapshot::empty();
        snapshot.registry = TaskRegistry::from_value(&json!({"tasks":[
            {"id":"DOC","state":"blocked","dependencies":["VERIFY"],"task_runtime":wait},
            {"id":"VERIFY","state":"blocked","dependencies":["MOBILE"],"task_runtime":wait},
            {"id":"MOBILE","state":"blocked","dependencies":["WINDOWS"],"task_runtime":{"last_error_code":"E905"}},
            {"id":"WINDOWS","state":state,"task_runtime":{"last_error_code":"E903","run_id":"run","last_error_origin":"precondition","result_explanation":"(no explanation provided: the tool reported precondition_unmet without the required MACC_TASK_RESULT_EXP line)"}},
            {"id":"OTHER","state":"blocked","dependencies":["WINDOWS"],"task_runtime":{"last_error_code":"E103","last_error_origin":"output_protocol"}}
        ]})).unwrap();
        db.save_snapshot(&snapshot).unwrap();
        db.upsert_coordinator_run(&CoordinatorRun {
            run_id: "run".into(),
            pid: 99999999,
            hostname: "test".into(),
            started_at: "2026-11-19T03:00:00Z".into(),
            last_tick_at: None,
            stopped_at: Some("2026-11-19T04:00:00Z".into()),
            status: "blocked".into(),
            epoch: 1,
            version: "test".into(),
            stop_reason: Some("blocked".into()),
        })
        .unwrap();
        let summary = load_latest_run_summary(&db, &snapshot.registry)
            .unwrap()
            .unwrap();
        assert_eq!(summary.task_id.as_deref(), Some("WINDOWS"));
        assert!(summary.cause.contains("interactive Windows"));
        assert!(
            summary.next_action.contains("runner-1")
                && summary.next_action.contains("interactive Windows")
        );
        assert!(summary.dependent_task_ids.contains(&"DOC".to_string()));

        let decisions =
            diagnose_unschedulable_tasks(&snapshot.registry, &TaskSelectorConfig::default());
        for id in ["WINDOWS", "OTHER"] {
            assert!(
                decisions.iter().any(|d| d.id == id && d.blocked_root),
                "{id}: {decisions:?}"
            );
        }
        for id in ["DOC", "MOBILE", "VERIFY"] {
            assert!(decisions
                .iter()
                .any(|d| d.id == id && d.depends_on_blocked && !d.blocked_root));
        }
        assert_eq!(decisions.iter().filter(|d| d.id == "WINDOWS").count(), 1);
        let incident = incident::inspect(&paths).unwrap().unwrap();
        assert_eq!(incident.tasks.len(), 2);
        let owner = incident.tasks.iter().find(|t| t.id == "WINDOWS").unwrap();
        assert_eq!(owner.state, state);
        assert_eq!(owner.task_runtime.unmet_preconditions.len(), 1);
        assert!(incident.tasks.iter().any(|t| t.id == "OTHER"));
        let saved = db.load_registry().unwrap();
        assert!(!saved
            .find_task("WINDOWS")
            .unwrap()
            .task_runtime
            .extra
            .contains_key("decision_wait"));
    }
}
