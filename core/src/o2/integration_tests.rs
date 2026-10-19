use super::*;
use serde_json::json;
#[test]
fn checkpoint_context_cache_and_usage_share_the_exact_invocation() {
    let temp = tests::root();
    let root = temp.path();
    std::fs::write(root.join("worktree.prd.json"),json!({"tasks":[{"id":"A","acceptance_criteria":["preserve contract"],"explicit_sources":["contract.rs"]}]}).to_string()).unwrap();
    let memory = memory::propose(root, &tests::proposal(root, Some("A"))).unwrap();
    tests::activate(root, &memory, Some("A"));
    let spec = json!({"id":"future","performer":{"command":"fake","session":{"enabled":true,"scope":"task_role_tool","create":{"command":"fake","args":["new","{session_id}"]}}}});
    let identity = crate::coordinator::session_identity::identity(
        root,
        root,
        "A",
        "performer",
        "future",
        &spec,
        &crate::coordinator::session_identity::SessionRouting {
            tier: "".into(),
            mode: "auto".into(),
        },
    )
    .unwrap();
    let request = store::checkpoints::ContextRequest {
        identity: identity.clone(),
        phase: "dev".into(),
    };
    let prepared = store::checkpoints::prepare(root, &request).unwrap();
    assert!(prepared.prompt.contains("preserve contract"));
    assert!(prepared.prompt.contains("public contract must"));
    let first = context::list(root).unwrap().remove(0);
    assert!(!first.cache_hit);
    assert_eq!(context::get(root, &first.id).unwrap().id, first.id);
    let grant = store::acquire(
        root,
        &store::AcquireRequest {
            identity,
            request_id: uuid::Uuid::new_v4().to_string(),
            owner_pid: std::process::id() as i64,
            preferred_provider_id: None,
            generate_provider_id: true,
            checkpoint_id: prepared.checkpoint_id,
        },
    )
    .unwrap();
    store::usage::begin(
        root,
        &store::usage::BeginRequest {
            invocation: None,
            key: grant.key.clone(),
            phase: "dev".into(),
        },
    )
    .unwrap();
    let usage = store::usage::end(
        root,
        &store::usage::EndRequest {
            key: grant.key.clone(),
            outcome: "success".into(),
            raw: String::new(),
            omitted: false,
        },
    )
    .unwrap();
    assert_eq!(usage.context_bundle_id.as_deref(), Some(first.id.as_str()));
    assert!(usage.incremental.input_uncached.is_none());
    store::release(root, &grant.key, true).unwrap();
    store::checkpoints::prepare(root, &request).unwrap();
    let second = context::list(root).unwrap().remove(0);
    assert!(second.cache_hit);
    assert_eq!(first.content_fingerprint, second.content_fingerprint);
    let (db, _) = database(root).unwrap();
    let count: i64 = db
        .query_row(
            "SELECT count(*) FROM context_usage_items WHERE invocation_id=?1",
            [&usage.invocation_id],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(count, 2);
    db.execute("UPDATE context_bundles SET manifest_json=replace(manifest_json,'preserve contract','altered requirement') WHERE id=?1",[&first.id]).unwrap();
    drop(db);
    assert!(context::get(root, &first.id).is_err());
}
#[test]
fn provider_removal_preserves_unowned_files_and_replays_original_receipts() {
    let temp = tests::root();
    let root = temp.path();
    let apply = |operation: &str| {
        let providers::Capability::Supported(plan) =
            providers::plan(root, "native", operation).unwrap()
        else {
            panic!()
        };
        let sha = digest(serde_json::to_string(&plan).unwrap().as_bytes());
        let receipt = providers::apply(root, &plan.id, &sha).unwrap();
        (plan, sha, receipt)
    };
    let (deploy, sha, receipt) = apply("deploy");
    let (_, _, indexed) = apply("index");
    let (update, _, updated) = apply("update");
    assert_ne!(indexed.index, updated.index);
    let (_, _, rolled) = apply("rollback");
    assert_eq!(rolled.index, indexed.index);
    assert_eq!(
        providers::apply(root, &deploy.id, &sha).unwrap().state,
        receipt.state
    );
    let user_file = root.join(".macc/code-intelligence/native/user-notes.txt");
    std::fs::write(&user_file, "keep").unwrap();
    apply("uninstall");
    assert!(user_file.exists());
    assert!(root.join(".macc/memory.db").exists());
    assert!(root.join("contract.rs").exists());
    assert!(!root.join(indexed.index.unwrap()).exists());
    assert!(update
        .affected_paths
        .iter()
        .all(|p| p.starts_with(".macc/code-intelligence/native/")));
}

#[test]
fn doctor_reports_dirty_provider_indexes_without_repair_side_effects() {
    let temp = tests::root();
    let root = temp.path();
    for operation in ["deploy", "index"] {
        let providers::Capability::Supported(plan) =
            providers::plan(root, "native", operation).unwrap()
        else {
            panic!()
        };
        providers::apply(
            root,
            &plan.id,
            &digest(serde_json::to_string(&plan).unwrap().as_bytes()),
        )
        .unwrap();
    }
    assert!(health::findings(root)
        .iter()
        .all(|f| f.severity == crate::doctor::DiagnosticSeverity::Ok));
    std::fs::write(root.join("contract.rs"), "changed after index").unwrap();
    let before = providers::inspect(root).unwrap().receipts[0]
        .index_sha256
        .clone();
    let findings = health::findings(root);
    assert!(findings
        .iter()
        .any(|f| f.severity == crate::doctor::DiagnosticSeverity::Warning));
    assert_eq!(
        providers::inspect(root).unwrap().receipts[0].index_sha256,
        before
    );
}
