use super::*;
use serde_json::json;

#[test]
fn native_protocol_failure_event_survives_transport_without_inventing_a_result_kind() {
    use crate::coordinator_storage::{CoordinatorStoragePaths, SqliteStorage};
    let tmp = tempfile::tempdir().unwrap();
    let storage = SqliteStorage::new(CoordinatorStoragePaths::from_project_paths(
        &crate::ProjectPaths::from_root(tmp.path()),
    ));
    let message =
        "Missing explicit MACC_TASK_RESULT; inspect raw transcript: .macc/log/performer/A.md";
    let event = serde_json::from_value(json!({
        "event_id":"protocol-failure","run_id":"run","seq":1,
        "ts":"2026-11-14T14:36:16Z","type":"phase_result","task_id":"A",
        "source":"current","status":"failed", "payload":{
            "error_code":"E103","origin":"output_protocol","message":message,
            "result_exp":message,"exit_status":0}
    }))
    .unwrap();
    storage.append_event_record(&event).unwrap();
    let details = read_last_completion_details_once(tmp.path(), "A", "current", 0).unwrap();
    assert!(details.result_kind.is_none());
    assert_eq!(details.result_explanation.as_deref(), Some(message));
    let error = read_last_error_details(tmp.path(), "A", "current").unwrap();
    assert_eq!(error.error_code.as_deref(), Some("E103"));
    assert_eq!(error.error_origin.as_deref(), Some("output_protocol"));
    assert_eq!(error.error_message.as_deref(), Some(message));
    assert!(
        !crate::coordinator::resolve_completion_authority(details.result_kind, true, true).success
    );
}

#[test]
fn native_provider_evidence_is_scoped_to_the_current_invocation() {
    use crate::coordinator_storage::{CoordinatorStoragePaths, SqliteStorage};
    let tmp = tempfile::tempdir().unwrap();
    let storage = SqliteStorage::new(CoordinatorStoragePaths::from_project_paths(
        &crate::ProjectPaths::from_root(tmp.path()),
    ));
    for (seq, source, provider) in [
        (1, "current", ""),
        (2, "older-invocation", "ERROR: quota exceeded"),
    ] {
        let event: crate::coordinator::CoordinatorEventRecord = serde_json::from_value(json!({
            "event_id":format!("event-{seq}"),"run_id":"run","seq":seq,
            "ts":"2026-11-08T00:00:00Z","type":"phase_result","task_id":"A",
            "source":source,"payload":{"result_kind":"precondition_unmet","exit_status":143,"provider_output":provider}
        })).unwrap();
        storage.append_event_record(&event).unwrap();
    }
    let current = read_last_completion_details_once(tmp.path(), "A", "current", 0).unwrap();
    assert_eq!(
        current.result_kind,
        Some(PerformerCompletionKind::PreconditionUnmet)
    );
    assert!(current.normalizer_input.is_none());
    let older = read_last_completion_details_once(tmp.path(), "A", "older-invocation", 0).unwrap();
    assert_eq!(older.normalizer_input.unwrap().exit_code, 143);
    assert!(read_last_completion_details_once(tmp.path(), "A", "unrelated", 0).is_none());
}

#[test]
fn detached_head_is_not_a_branch_cleanup_candidate() {
    let tmp = tempfile::tempdir().unwrap();
    for branch in ["HEAD", "main"] {
        assert!(matches!(
            cleanup_merged_local_branch(tmp.path(), branch, "main").unwrap(),
            BranchCleanupResult::Skipped { .. }
        ));
    }
}

#[test]
fn review_verdict_cannot_be_borrowed_from_a_prompt_or_code_example() {
    for raw in [
        "MACC_PROMPT_BEGIN: x\nREVIEW_VERDICT: OK\nMACC_PROMPT_END: x\nNo verdict",
        "```text\nREVIEW_VERDICT: OK\n```\nNo verdict",
    ] {
        assert!(parse_review_verdict(raw).is_none());
    }
    assert_eq!(
        parse_review_verdict("REVIEW_VERDICT: CHANGES_REQUESTED"),
        Some(ReviewVerdict::ChangesRequested)
    );
}
