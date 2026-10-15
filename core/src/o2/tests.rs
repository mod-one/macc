use super::*;
use serde_json::json;
use std::process::Command;
pub(super) fn root() -> tempfile::TempDir {
    let t = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(t.path().join(".macc/state")).unwrap();
    for args in [
        vec!["init", "-b", "main"],
        vec!["config", "user.name", "Test"],
        vec!["config", "user.email", "test@example.invalid"],
    ] {
        assert!(Command::new("git")
            .arg("-C")
            .arg(t.path())
            .args(args)
            .output()
            .unwrap()
            .status
            .success());
    }
    std::fs::write(
        t.path().join(".gitignore"),
        ".macc/\n.fake-tool\n.native/\n",
    )
    .unwrap();
    std::fs::write(
        t.path().join("contract.rs"),
        "pub fn preserve_contract() {}\n",
    )
    .unwrap();
    for args in [vec!["add", "."], vec!["commit", "-m", "initial"]] {
        assert!(Command::new("git")
            .arg("-C")
            .arg(t.path())
            .args(args)
            .output()
            .unwrap()
            .status
            .success());
    }
    let plan = store::migrate(t.path(), false, None).unwrap();
    store::migrate(t.path(), true, Some(&plan.source_sha256)).unwrap();
    let p = policy::inspect(t.path()).unwrap();
    let mut v = p.policy;
    v.memory.enabled = true;
    v.context_engine.enabled = true;
    v.code_intelligence.enabled = true;
    policy::update(t.path(), &p.revision, Some(v)).unwrap();
    t
}
pub(super) fn proposal(root: &Path, task: Option<&str>) -> memory::Proposal {
    let bytes = std::fs::read(root.join("contract.rs")).unwrap();
    memory::Proposal {
        kind: "constraint".into(),
        title: "Preserve contract".into(),
        body: "The public contract must remain compatible with source evidence.".into(),
        scope: if task.is_some() { "task" } else { "project" }.into(),
        scope_ref: task.map(str::to_string),
        confidence: 0.9,
        evidence: vec![memory::Evidence {
            kind: "file".into(),
            reference: "contract.rs".into(),
            sha256: Some(digest(&bytes)),
            revision: Some(crate::git::head_commit(root).unwrap()),
            summary: "Exact source".into(),
        }],
        tags: vec!["api".into()],
        source_task: task.map(str::to_string),
        source_revision: None,
    }
}
pub(super) fn activate(root: &Path, m: &memory::Memory, task: Option<&str>) {
    let hit = memory::get(
        root,
        &m.id,
        &memory::Access {
            task_id: task.map(str::to_string),
            ..Default::default()
        },
    )
    .unwrap();
    memory::review(
        root,
        &memory::Review {
            id: m.id.clone(),
            status: "active".into(),
            reason: "Verified source and requirement".into(),
            confirmed: true,
            expected_checksum: hit.checksum,
            evidence: None,
        },
    )
    .unwrap();
}
fn query(text: &str, task: Option<&str>, all: bool) -> memory::Search {
    memory::Search {
        query: text.into(),
        access: memory::Access {
            task_id: task.map(str::to_string),
            ..Default::default()
        },
        include_inactive: all,
    }
}
#[test]
fn memory_requires_review_and_enforces_scope_freshness_and_fts() {
    let t = root();
    let r = t.path();
    let m = memory::propose(r, &proposal(r, Some("A"))).unwrap();
    assert!(memory::search(r, &query("contract", Some("A"), false))
        .unwrap()
        .is_empty());
    activate(r, &m, Some("A"));
    assert_eq!(
        memory::search(r, &query("contract", Some("A"), false))
            .unwrap()
            .len(),
        1
    );
    assert!(memory::search(r, &query("contract", Some("B"), true))
        .unwrap()
        .is_empty());
    memory::fts_rebuild(r).unwrap();
    std::fs::write(r.join("contract.rs"), "changed").unwrap();
    assert!(memory::search(r, &query("contract", Some("A"), false))
        .unwrap()
        .is_empty());
    assert_eq!(
        memory::search(r, &query("contract", Some("A"), true)).unwrap()[0]
            .memory
            .status,
        "stale"
    );
}
#[test]
fn secrets_commit_only_and_unconfirmed_reviews_are_rejected() {
    let t = root();
    let r = t.path();
    let mut p = proposal(r, None);
    p.body = "sk-abcdefghijklmnopqrstuvwxyz1234567890".into();
    assert!(memory::propose(r, &p).is_err());
    p = proposal(r, None);
    p.evidence = vec![memory::Evidence {
        kind: "commit".into(),
        reference: crate::git::head_commit(r).unwrap(),
        sha256: None,
        revision: None,
        summary: "commit exists".into(),
    }];
    let m = memory::propose(r, &p).unwrap();
    let hit = memory::get(r, &m.id, &Default::default()).unwrap();
    for confirmed in [false, true] {
        assert!(memory::review(
            r,
            &memory::Review {
                id: m.id.clone(),
                status: "active".into(),
                reason: "test".into(),
                confirmed,
                expected_checksum: hit.checksum.clone(),
                evidence: None,
            }
        )
        .is_err());
    }
}
#[test]
fn open_conflicts_exclude_automatic_facts() {
    let t = root();
    let r = t.path();
    let left = memory::propose(r, &proposal(r, None)).unwrap();
    let right = memory::propose(r, &proposal(r, None)).unwrap();
    activate(r, &left, None);
    activate(r, &right, None);
    memory::conflict(
        r,
        &left.id,
        &right.id,
        "Contradictory claims require review",
    )
    .unwrap();
    assert!(memory::search(r, &query("contract", None, false))
        .unwrap()
        .is_empty());
    assert_eq!(memory::search(r, &query("", None, true)).unwrap().len(), 2);
}
#[test]
fn schema_upgrade_requires_review_and_preserves_backups() {
    let t = root();
    let r = t.path();
    let (db, _) = database(r).unwrap();
    db.execute("DELETE FROM session_meta WHERE key='o2_schema_version'", [])
        .unwrap();
    drop(db);
    let plan = upgrade(r, false, None).unwrap();
    assert!(upgrade(r, true, Some("stale")).is_err());
    let report = upgrade(r, true, Some(&plan.source_sha256)).unwrap();
    assert!(Path::new(report.backup.as_ref().unwrap()).exists());
    assert!(policy::inspect(r).unwrap().ready);
}
#[test]
fn context_keeps_mandatory_prd_when_optional_budget_is_zero() {
    let t = root();
    let r = t.path();
    let m = memory::propose(r, &proposal(r, None)).unwrap();
    activate(r, &m, None);
    let view = policy::inspect(r).unwrap();
    let mut p = view.policy;
    p.context_engine.push_memory_limit_tokens = 0;
    policy::update(r, &view.revision, Some(p)).unwrap();
    let c:store::checkpoints::Checkpoint=serde_json::from_value(json!({"schema_version":1,"id":"cp-a","project_id":store::authority(r).unwrap().unwrap().project_id,"task_id":"A","role":"performer","tool_id":"future-tool","phase":"dev","worktree":r,"branch":"main","source_revision":crate::git::head_commit(r).unwrap(),"worktree_dirty_digest":"dirty","worktree_content_digest":"content","source_hashes":{"design.md":"pinned"},"acceptance_criteria_ids":["AC-1"],"mandatory_task":{"tasks":[{"id":"A","acceptance_criteria":["Preserve every mandatory constraint"],"design_ref":"design.md"}]},"completed_steps":[],"remaining_steps":["verify"],"changed_files":[],"decisions":[],"test_results":[],"blocking_issues":[],"review_objections":null,"memory_refs":[],"source_refs":["design.md"],"next_action":"verify","reason":"test","validation":{}})).unwrap();
    let _guard = store::lock(r).unwrap();
    let (mut db, _) = database(r).unwrap();
    let tx = db.transaction().unwrap();
    let prompt = context::for_checkpoint(&tx, r, &c).unwrap();
    tx.commit().unwrap();
    drop(db);
    drop(_guard);
    assert!(!prompt.contains("public contract must"));
    let bundle = context::list(r).unwrap().remove(0);
    assert_eq!(bundle.mandatory_task, c.mandatory_task);
    assert_eq!(bundle.source_hashes, c.source_hashes);
    assert_eq!(bundle.optional_estimated_tokens, 0);
}
#[test]
fn native_lifecycle_is_reviewed_local_and_stale_plans_fail() {
    let t = root();
    let r = t.path();
    let get_plan = |action: &str| match providers::plan(r, "native", action).unwrap() {
        providers::Capability::Supported(p) => p,
        _ => panic!("native plan unavailable"),
    };
    let p = get_plan("deploy");
    let raw = serde_json::to_string(&p).unwrap();
    assert!(providers::apply(r, &p.id, "wrong").is_err());
    let receipt = providers::apply(r, &p.id, &digest(raw.as_bytes())).unwrap();
    assert_eq!(receipt.state, "deployed");
    let p = get_plan("index");
    let checksum = digest(serde_json::to_string(&p).unwrap().as_bytes());
    std::fs::write(r.join("contract.rs"), "changed").unwrap();
    assert!(providers::apply(r, &p.id, &checksum).is_err());
    let p = get_plan("index");
    let checksum = digest(serde_json::to_string(&p).unwrap().as_bytes());
    let result = providers::apply(r, &p.id, &checksum).unwrap();
    assert_eq!(result.state, "healthy");
    assert!(r.join(result.index.unwrap()).exists());
}
#[test]
fn native_queries_are_literal_bounded_and_report_unsupported_semantics() {
    let t = root();
    let r = t.path();
    let q = providers::Query {
        text: "preserve_contract".into(),
        worktree: None,
        kind: "file_search".into(),
    };
    match providers::query(r, &q).unwrap() {
        providers::Capability::Supported(result) => {
            assert_eq!(result.hits.len(), 1);
            assert_eq!(result.hits[0].path, "contract.rs");
        }
        _ => panic!("native query unavailable"),
    };
    let q = providers::Query {
        kind: "dependency_path".into(),
        ..q
    };
    assert!(matches!(
        providers::query(r, &q).unwrap(),
        providers::Capability::Unsupported { .. }
    ));
}
#[test]
fn reduction_is_unicode_safe_and_keeps_private_raw_artifact() {
    let t = root();
    let text = format!("error: {}", "é漢🙂".repeat(10000));
    let reduced = context::reduce(t.path(), "log", &text).unwrap();
    assert!(reduced.summary.len() < 9000);
    assert_eq!(
        std::fs::read_to_string(t.path().join(&reduced.raw_ref)).unwrap(),
        text
    );
}
#[test]
fn unknown_pressure_and_in_flight_calls_never_guess_compaction() {
    let p = policy::CompactionPolicy {
        enabled: true,
        ..Default::default()
    };
    assert_eq!(
        compaction::decision(&p, 3, 20, None, None, false).pressure,
        None
    );
    assert_eq!(
        compaction::decision(&p, 20, 20, None, None, false).action,
        "checkpoint_rotate"
    );
    assert_eq!(
        compaction::decision(&p, 20, 20, Some(90), Some(100), true).action,
        "defer"
    );
    assert_eq!(
        compaction::decision(&p, 4, 20, Some(90), Some(100), false).action,
        "checkpoint_rotate"
    );
}
