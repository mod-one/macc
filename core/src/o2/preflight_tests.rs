//! No external process/AST work under authority locks; proofs are request-local.
use super::*;
fn query() -> memory::Search {
    memory::Search {
        query: String::new(),
        access: Default::default(),
        include_inactive: false,
    }
}
fn verify(root: &Path, prepared: &memory::PreparedFreshness) -> memory::search::Retrieval {
    let _guard = store::lock(root).unwrap();
    let (db, a) = database(root).unwrap();
    memory::search::search_in(&db, &a.project_id, root, root, &query(), false, prepared).unwrap()
}
#[test]
fn source_policy_and_record_changes_after_preflight_are_rejected() {
    let t = tests::root();
    let r = t.path();
    let m = p3_tests::active(r, p3_tests::file(r), "decision", "Current proof");
    let prepared = memory::prepare_search(r, &query()).unwrap();
    assert_eq!(verify(r, &prepared).hits.len(), 1);
    std::fs::write(
        r.join("contract.rs"),
        "pub fn preserve_contract(){let changed=1;}\n",
    )
    .unwrap();
    assert!(verify(r, &prepared).hits.is_empty());
    std::fs::write(r.join("contract.rs"), "pub fn preserve_contract() {}\n").unwrap();
    let prepared = memory::prepare_search(r, &query()).unwrap();
    let view = policy::inspect(r).unwrap();
    let mut p = view.policy;
    p.memory.ranking_task_weight += 1;
    policy::update(r, &view.revision, Some(p)).unwrap();
    assert!(verify(r, &prepared).hits.is_empty());
    let prepared = memory::prepare_search(r, &query()).unwrap();
    let (db, a) = database(r).unwrap();
    let mut record = memory::load(&db, &m.id, &a.project_id).unwrap();
    record.proposal.confidence = 0.95;
    memory::save(&db, &record).unwrap();
    drop(db);
    assert!(verify(r, &prepared).hits.is_empty());
}
#[test]
fn semantic_proofs_use_project_policy_and_the_requested_worktree() {
    let t = tests::root();
    let r = t.path();
    let other = tempfile::tempdir().unwrap();
    let wt = other.path().join("worker");
    crate::git::run_git_output_mapped(
        r,
        &["worktree", "add", "--detach", wt.to_str().unwrap(), "HEAD"],
        "Create scoped proof fixture",
    )
    .unwrap();
    assert!(!policy::load(&wt).unwrap().code_intelligence.enabled);
    let evidence =
        memory::symbol_evidence(r, "contract.rs", "preserve_contract", "native", wt.to_str())
            .unwrap();
    let mut p = tests::proposal(r, None);
    p.scope = "worktree".into();
    p.scope_ref = Some(wt.display().to_string());
    p.evidence = vec![evidence];
    let m = memory::propose(r, &p).unwrap();
    let access = memory::Access {
        worktree: Some(wt.display().to_string()),
        ..Default::default()
    };
    let h = memory::get(r, &m.id, &access).unwrap();
    memory::review(
        r,
        &memory::Review {
            id: m.id.clone(),
            status: "active".into(),
            reason: "Reviewed worktree symbol".into(),
            confirmed: true,
            expected_checksum: h.checksum,
            evidence: None,
        },
    )
    .unwrap();
    std::fs::write(
        wt.join("contract.rs"),
        "pub fn unrelated(){} pub fn preserve_contract(){}\n",
    )
    .unwrap();
    assert_eq!(
        memory::search(
            r,
            &memory::Search {
                access: access.clone(),
                ..query()
            }
        )
        .unwrap()
        .len(),
        1
    );
    std::fs::write(
        wt.join("contract.rs"),
        "pub fn preserve_contract(){let changed=1;}\n",
    )
    .unwrap();
    assert!(memory::search(
        r,
        &memory::Search {
            access: access.clone(),
            ..query()
        }
    )
    .unwrap()
    .is_empty());
    assert_eq!(
        memory::get(r, &m.id, &access).unwrap().memory.status,
        "active"
    );
}
#[test]
#[cfg(unix)]
fn live_versions_do_not_hold_the_session_authority_lock() {
    let t = tests::root();
    let r = t.path();
    p3_tests::tool(r);
    let script=std::fs::read_to_string(r.join(".fake-tool")).unwrap().replace("--version) ","--version) if test -f .macc/hold; then touch .macc/probe-started; while test -f .macc/hold; do sleep 0.01; done; fi; ");
    std::fs::write(r.join(".fake-tool"), script).unwrap();
    p3_tests::active(
        r,
        memory::tool_evidence(r, "future-tool").unwrap(),
        "provider_note",
        "Bound observed behavior",
    );
    std::fs::write(r.join(".macc/hold"), "").unwrap();
    let root = r.to_path_buf();
    let worker = std::thread::spawn(move || memory::search(&root, &query()));
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
    while !r.join(".macc/probe-started").exists() && std::time::Instant::now() < deadline {
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
    let still_running = !worker.is_finished();
    let guard = crate::fs_lock::AdvisoryLock::try_acquire(
        &store::legacy_path(r).with_extension("json.lock"),
        "Concurrent heartbeat during freshness",
    )
    .unwrap();
    let available = guard.is_some();
    drop(guard);
    std::fs::remove_file(r.join(".macc/hold")).unwrap();
    let result = worker.join().unwrap().unwrap();
    assert!(
        still_running && available,
        "A slow registered tool blocked the authority/heartbeat lock"
    );
    assert_eq!(result.len(), 1);
}
