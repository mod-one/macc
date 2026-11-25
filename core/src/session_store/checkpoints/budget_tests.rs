use super::tests::{claim, context, fixture};
use super::*;

#[test]
fn quota_resume_uses_finish_headroom_without_truncating_retained_evidence() {
    let temp = fixture();
    let root = temp.path();
    let view = inspect(root).unwrap();
    let mut policy = view.policy;
    policy.checkpoints.max_content_bytes = 16384;
    update_policy(root, &view.revision, &policy).unwrap();
    let request = context(root, "A", "performer", "future-tool");
    let prepared = prepare(root, &request).unwrap();
    let lease = claim(root, &request, &prepared);
    for i in 0..180 {
        std::fs::write(
            root.join(format!("proof-{i}-{}.txt", "x".repeat(100))),
            "retained",
        )
        .unwrap();
    }
    let finished = finish(
        root,
        &FinishRequest {
            key: lease.key.clone(),
            phase: "dev".into(),
            result: "quota".into(),
            acknowledged: true,
        },
    )
    .unwrap();
    assert!(serde_json::to_vec(&finished).unwrap().len() > 16384);
    release(root, &lease.key, true).unwrap();
    for _ in 0..3 {
        let resumed = prepare(root, &request).unwrap();
        let saved = get(root, resumed.checkpoint_id.as_ref().unwrap()).unwrap();
        assert_eq!(saved.mandatory_task, finished.mandatory_task);
        assert_eq!(
            saved.acceptance_criteria_ids,
            finished.acceptance_criteria_ids
        );
        assert_eq!(saved.changed_files, finished.changed_files);
        assert!(saved
            .blocking_issues
            .iter()
            .any(|issue| issue.contains("quota")));
        assert_eq!(saved.validation["max_content_bytes"], 65536);
        assert!(saved.completed_steps.is_empty());
        assert!(serde_json::to_vec(&saved).unwrap().len() <= 65536);
    }
    let resumed = prepare(root, &request).unwrap();
    let lease = claim(root, &request, &resumed);
    release(root, &lease.key, true).unwrap();
}

#[test]
fn large_inventory_is_retrievable_across_finish_preview_and_resume() {
    let temp = fixture();
    let root = temp.path();
    let request = context(root, "A", "performer", "future-tool");
    let prepared = prepare(root, &request).unwrap();
    let lease = claim(root, &request, &prepared);
    let mut names = Vec::new();
    for i in 0..512 {
        let path = format!("generated-{i:04}-{}", "x".repeat(140));
        std::fs::write(root.join(&path), "content").unwrap();
        names.push(path);
    }
    let saved = finish(
        root,
        &FinishRequest {
            key: lease.key.clone(),
            phase: "dev".into(),
            result: "failure".into(),
            acknowledged: true,
        },
    )
    .unwrap();
    release(root, &lease.key, true).unwrap();
    let inventory = &saved.validation["changed_file_inventory"];
    assert_eq!(inventory["count"], names.len());
    assert_eq!(changed_files(root, &saved.id).unwrap(), names);
    let (db, _) = db::active(root).unwrap();
    let raw: Vec<u8> = db
        .query_row(
            "SELECT paths_json FROM checkpoint_file_inventories WHERE sha256=?1",
            [inventory["sha256"].as_str().unwrap()],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(digest(&raw), inventory["sha256"]);
    assert_eq!(serde_json::from_slice::<Vec<String>>(&raw).unwrap(), names);
    assert!(serde_json::to_vec(&saved).unwrap().len() <= 65536);
    let preview = reconcile(root, &saved.id, None).unwrap();
    assert_eq!(
        preview["current"]["validation"]["changed_file_inventory"],
        *inventory
    );
    let resumed = prepare(root, &request).unwrap();
    let next = get(root, resumed.checkpoint_id.as_ref().unwrap()).unwrap();
    assert_eq!(next.mandatory_task, saved.mandatory_task);
    assert_eq!(next.validation["changed_file_inventory"], *inventory);
    assert!(next.blocking_issues.iter().any(|s| s.contains("failure")));

    // Even after the listing has been externalized, mandatory evidence must fit.
    let mut oversized = next;
    oversized.review_objections = json!("review objection ".repeat(5000));
    oversized.changed_files = names;
    assert!(
        inventory::Inventory::project(&mut oversized, 65536, "max_finish_content_bytes")
            .err()
            .unwrap()
            .to_string()
            .contains("limit=65536 bytes")
    );
}

#[test]
fn finish_retains_large_mandatory_task_and_changed_files_with_pinned_headroom() {
    let temp = fixture();
    let root = temp.path();
    let prd = root.join("worktree.prd.json");
    let mut task: Value = serde_json::from_slice(&std::fs::read(&prd).unwrap()).unwrap();
    task["tasks"][0]["description"] = json!("criterion ".repeat(1000));
    std::fs::write(&prd, task.to_string()).unwrap();
    let request = context(root, "A", "performer", "future-tool");
    let prepared = prepare(root, &request).unwrap();
    let original = get(root, prepared.checkpoint_id.as_ref().unwrap()).unwrap();
    let lease = claim(root, &request, &prepared);
    // Optional execution evidence expands after the bounded starting checkpoint.
    for i in 0..100 {
        std::fs::write(
            root.join(format!("evidence-{i:03}-{}.txt", "x".repeat(80))),
            "proof",
        )
        .unwrap();
    }
    // A policy edit during execution must not shrink its reserved finish budget.
    let mut policy = policy::load(root).unwrap();
    policy.checkpoints.max_content_bytes = 2048;
    policy.checkpoints.max_finish_content_bytes = 2048;
    std::fs::write(
        root.join(".macc/macc.yaml"),
        serde_yaml::to_string(&json!({"tools":{"enabled":[]},"session_management":policy}))
            .unwrap(),
    )
    .unwrap();
    let finished = finish(
        root,
        &FinishRequest {
            key: lease.key.clone(),
            phase: "dev".into(),
            result: "precondition_unmet".into(),
            acknowledged: true,
        },
    )
    .unwrap();
    assert_eq!(finished.mandatory_task, original.mandatory_task);
    assert_eq!(finished.changed_files.len(), 100);
    let bytes = serde_json::to_vec(&finished).unwrap().len();
    assert!(bytes > 16384 && bytes <= 65536, "{bytes}");
    assert!(finished.completed_steps.is_empty());
    release(root, &lease.key, true).unwrap();
}

#[test]
fn oversized_start_and_invalid_finish_policy_fail_without_truncating_criteria() {
    let temp = fixture();
    let root = temp.path();
    let mut policy = SessionPolicy::default();
    policy.checkpoints.max_finish_content_bytes = 2048;
    assert!(policy.validate().is_err());
    policy.checkpoints.max_finish_content_bytes = 262145;
    assert!(policy.validate().is_err());
    let max_bytes =
        usize::try_from(policy::load(root).unwrap().checkpoints.max_content_bytes).unwrap();
    let task = json!({"tasks":[{"id":"A","acceptance_criteria":["required ".repeat(max_bytes / "required ".len() + 1)]}]});
    std::fs::write(root.join("worktree.prd.json"), task.to_string()).unwrap();
    assert!(
        prepare(root, &context(root, "A", "performer", "future-tool"))
            .unwrap_err()
            .to_string()
            .contains("mandatory criteria were not truncated")
    );
}
