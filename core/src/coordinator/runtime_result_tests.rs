use super::*;
#[test]
fn successful_runner_summary_cannot_inherit_an_echoed_placeholder() {
    let tmp = tempfile::tempdir().unwrap();
    let log = performer_task_log_path(tmp.path(), "A");
    std::fs::create_dir_all(log.parent().unwrap()).unwrap();
    std::fs::write(log,"MACC_PROMPT_BEGIN: instructions-only\nMACC_TASK_RESULT_EXP: prior work unsalvageable: <reason>\nMACC_TASK_RESULT: error_without_changes\nMACC_PROMPT_END: instructions-only\ncodex\nDone\nMACC_TASK_RESULT: success_with_changes\n- Result kind: success_with_changes\n- Explanation: prior work unsalvageable: <reason>\n").unwrap();
    let details = read_completion_details_from_worktree_log(tmp.path(), "A").unwrap();
    assert_eq!(
        details.result_kind,
        Some(PerformerCompletionKind::SuccessWithChanges)
    );
    assert!(details.result_explanation.is_none());
}
#[test]
fn detached_head_is_not_a_branch_cleanup_candidate() {
    let tmp = tempfile::tempdir().unwrap();
    assert!(matches!(
        cleanup_merged_local_branch(tmp.path(), "HEAD", "main").unwrap(),
        BranchCleanupResult::Skipped { .. }
    ));
    assert!(matches!(
        cleanup_merged_local_branch(tmp.path(), "main", "main").unwrap(),
        BranchCleanupResult::Skipped { .. }
    ));
}

#[test]
fn review_verdict_cannot_be_borrowed_from_an_echoed_prompt_or_code_example() {
    assert!(super::parse_review_verdict(
        "MACC_PROMPT_BEGIN: x\nREVIEW_VERDICT: OK\nMACC_PROMPT_END: x\nNo verdict"
    )
    .is_none());
    assert!(super::parse_review_verdict("```text\nREVIEW_VERDICT: OK\n```\nNo verdict").is_none());
    assert_eq!(
        super::parse_review_verdict("REVIEW_VERDICT: CHANGES_REQUESTED"),
        Some(super::ReviewVerdict::ChangesRequested)
    );
}
