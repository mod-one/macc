use super::*;
use serde_json::json;

#[test]
fn review_postcheck_preserves_unexpected_files_instead_of_cleaning_them() {
    let tmp = tempfile::tempdir().unwrap();
    let p = tmp.path();
    let git = |args: &[&str]| {
        let out = crate::git::run_git_output_mapped(p, args, "review safety fixture").unwrap();
        assert!(
            out.status.success(),
            "{}",
            String::from_utf8_lossy(&out.stderr)
        );
        String::from_utf8_lossy(&out.stdout).trim().to_owned()
    };
    git(&["init", "-q", "-b", "main"]);
    git(&["config", "user.name", "Test"]);
    git(&["config", "user.email", "test@example.invalid"]);
    std::fs::write(p.join("source.txt"), "baseline").unwrap();
    git(&["add", "."]);
    git(&["commit", "-qm", "baseline"]);
    let base = git(&["rev-parse", "HEAD"]);
    std::fs::write(p.join("source.txt"), "implementation").unwrap();
    git(&["add", "."]);
    git(&["commit", "-qm", "implementation"]);
    struct Review;
    impl PhaseExecutor for Review {
        fn run_phase(
            &self,
            task: &Task,
            _: &str,
            _: Option<&str>,
            _: usize,
        ) -> Result<std::result::Result<String, String>> {
            std::fs::write(
                std::path::Path::new(task.worktree_path().unwrap()).join("retained-proof.txt"),
                "do not delete this evidence",
            )
            .unwrap();
            Ok(Ok("REVIEW_VERDICT: CHANGES_REQUESTED".into()))
        }
    }
    let mut task=serde_json::from_value(json!({"id":"A","state":"reviewing","worktree":{"worktree_path":p,"branch":"main","base_branch":base}})).unwrap();
    crate::coordinator::review_budget::reserve(&mut task, Some(1), 1).unwrap();
    assert!(run_review_phase(&Review, &task, None)
        .unwrap()
        .unwrap_err()
        .contains("postcheck"));
    assert_eq!(
        std::fs::read_to_string(p.join("retained-proof.txt")).unwrap(),
        "do not delete this evidence"
    );
}

#[cfg(unix)]
#[test]
fn review_terminal_event_and_evidence_agree_after_postchecks() {
    use std::{fs, os::unix::fs::PermissionsExt};
    for (script, expected, severity) in [
        ("echo 'No verdict'", "validation_failed", "blocking"),
        (
            "touch unexpected; echo 'REVIEW_VERDICT: OK'",
            "validation_failed",
            "blocking",
        ),
        (
            "echo 'REVIEW_VERDICT: CHANGES_REQUESTED'",
            "changes_requested",
            "info",
        ),
        ("echo 'REVIEW_VERDICT: OK'", "validated", "info"),
    ] {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        let git = |args: &[&str]| {
            let out = crate::git::run_git_output_mapped(root, args, "review fixture").unwrap();
            assert!(
                out.status.success(),
                "{}",
                String::from_utf8_lossy(&out.stderr)
            );
            String::from_utf8(out.stdout).unwrap().trim().to_owned()
        };
        git(&["init", "-q", "-b", "main"]);
        git(&["config", "user.name", "Test"]);
        git(&["config", "user.email", "test@example.invalid"]);
        fs::write(root.join(".gitignore"), ".macc/\n").unwrap();
        git(&["add", ".gitignore"]);
        git(&["commit", "-qm", "base"]);
        let base = git(&["rev-parse", "HEAD"]);
        git(&["commit", "--allow-empty", "-qm", "implementation"]);
        let ipc = crate::coordinator::ipc::performer_ipc_addr_path_pub(root);
        fs::create_dir_all(ipc.parent().unwrap()).unwrap();
        fs::write(ipc, "unused-fixture-address").unwrap();
        let runner = root.join(".macc/reviewer.sh");
        fs::write(&runner, format!("#!/bin/sh\n{script}\n")).unwrap();
        fs::set_permissions(&runner, fs::Permissions::from_mode(0o700)).unwrap();
        fs::write(
            root.join(".macc/tool.json"),
            json!({"id":"fixture","performer":{"runner":runner}}).to_string(),
        )
        .unwrap();

        let mut task = serde_json::from_value(json!({"id":"A","state":"reviewing","worktree":{"worktree_path":root,"branch":"main","base_branch":base}})).unwrap();
        crate::coordinator::review_budget::reserve(&mut task, Some(1), 1).unwrap();
        let review = crate::coordinator::control_plane::run_review_phase_for_task_native(
            root,
            &task,
            Some("fixture"),
            None,
        )
        .unwrap();
        assert_eq!(
            review.outcome.is_err(),
            expected == "validation_failed",
            "{:?}",
            review.outcome
        );
        let path = fs::read_dir(root.join(".macc/log/performer/A/phases"))
            .unwrap()
            .next()
            .unwrap()
            .unwrap()
            .path();
        let record: serde_json::Value =
            serde_json::from_slice(&fs::read(path.join("outcome.json")).unwrap()).unwrap();
        assert_eq!(record["status"], expected);
        let events: Vec<serde_json::Value> =
            fs::read_to_string(root.join(".macc/log/events.jsonl"))
                .unwrap()
                .lines()
                .map(|line| serde_json::from_str::<serde_json::Value>(line).unwrap())
                .filter(|e| e["event_type"] == "phase_completed")
                .collect();
        assert_eq!(events.len(), 1);
        assert_eq!(events[0]["severity"], severity);
        let message = events[0]["message"].as_str().unwrap();
        assert!(message.contains(expected));
        if let Err(reason) = review.outcome {
            assert_eq!(record["validation_error"], reason);
            assert!(message.contains(&reason));
        }
    }
}

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

#[test]
fn retained_phase_worker_gets_current_bundled_output_transport() {
    let dir = tempfile::tempdir().unwrap();
    let paths = crate::ProjectPaths::from_root(dir.path());
    crate::ensure_embedded_automation_scripts(&paths).unwrap();
    let adapters = paths.automation_dir().join("embedded/adapters");
    let runner = adapters.join("codex/codex.performer.sh");
    let transport = adapters.join("shared/provider_process.sh");
    let expected_runner = std::fs::read(&runner).unwrap();
    let expected_transport = std::fs::read(&transport).unwrap();
    std::fs::write(&runner, "# old runner").unwrap();
    std::fs::write(&transport, "# old transport").unwrap();
    assert_eq!(
        resolve_phase_runner(dir.path(), dir.path(), "codex")
            .unwrap()
            .as_ref(),
        Some(&runner)
    );
    assert_eq!(std::fs::read(&runner).unwrap(), expected_runner);
    assert_eq!(std::fs::read(&transport).unwrap(), expected_transport);
}
