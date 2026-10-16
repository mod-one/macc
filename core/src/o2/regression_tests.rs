use super::*;
use serde_json::json;
use std::process::Command;
fn request(root: &Path, wt: &Path, task: &str) -> store::checkpoints::ContextRequest {
    let identity=crate::coordinator::session_identity::identity(root,wt,task,"performer","future-tool",&json!({"id":"future-tool","performer":{"runner":"fake.sh","command":"fake","session":{"enabled":true,"scope":"task_role_tool","create":{"command":"fake","args":["new","{session_id}"]}}}}),&crate::coordinator::session_identity::SessionRouting{tier:"".into(),mode:"auto".into()}).unwrap();
    store::checkpoints::ContextRequest {
        identity,
        phase: "dev".into(),
    }
}
#[test]
fn divergent_worker_excludes_memory_without_invalidating_other_branches() {
    let temp = tests::root();
    let root = temp.path();
    let m = memory::propose(root, &tests::proposal(root, None)).unwrap();
    tests::activate(root, &m, None);
    let wt = root.join(".macc/worktree/worker-01");
    let out = Command::new("git")
        .arg("-C")
        .arg(root)
        .args(["worktree", "add", "-b", "worker-a"])
        .arg(&wt)
        .output()
        .unwrap();
    assert!(out.status.success(), "{:?}", out);
    std::fs::write(wt.join("contract.rs"), "worker-only edit").unwrap();
    std::fs::write(
        wt.join("worktree.prd.json"),
        json!({"tasks":[{"id":"A","acceptance_criteria":["preserve contract"]}]}).to_string(),
    )
    .unwrap();
    let prepared = store::checkpoints::prepare(root, &request(root, &wt, "A")).unwrap();
    assert!(!prepared.prompt.contains("public contract must"));
    assert!(context::list(root).unwrap()[0]
        .exclusions
        .iter()
        .any(|s| s.contains("snapshot_mismatch")));
    let query = memory::Search {
        query: "contract".into(),
        access: Default::default(),
        include_inactive: false,
    };
    assert_eq!(memory::search(root, &query).unwrap().len(), 1);
    assert_eq!(
        memory::get(root, &m.id, &Default::default())
            .unwrap()
            .memory
            .status,
        "active"
    );
    std::fs::write(wt.join("contract.rs"), "pub fn preserve_contract() {}\n").unwrap();
    std::fs::write(root.join("contract.rs"), "root-only edit").unwrap();
    std::fs::write(
        root.join("worktree.prd.json"),
        json!({"tasks":[{"id":"B","acceptance_criteria":["preserve contract"]}]}).to_string(),
    )
    .unwrap();
    store::checkpoints::prepare(root, &request(root, root, "B")).unwrap();
    let on_worker = memory::Search {
        access: memory::Access {
            worktree: Some(wt.to_string_lossy().into_owned()),
            ..Default::default()
        },
        ..query
    };
    assert_eq!(memory::search(root, &on_worker).unwrap().len(), 1);
    assert_eq!(
        memory::get(root, &m.id, &Default::default())
            .unwrap()
            .memory
            .status,
        "active"
    );
}
#[test]
fn automatic_feature_lookup_is_scoped_and_included_in_cache_identity() {
    let temp = tests::root();
    let root = temp.path();
    let mut p = tests::proposal(root, None);
    p.scope = "feature".into();
    p.scope_ref = Some("F".into());
    let m = memory::propose(root, &p).unwrap();
    let hit = memory::get(
        root,
        &m.id,
        &memory::Access {
            feature: Some("F".into()),
            ..Default::default()
        },
    )
    .unwrap();
    memory::review(
        root,
        &memory::Review {
            id: m.id,
            status: "active".into(),
            reason: "Verified feature source".into(),
            confirmed: true,
            expected_checksum: hit.checksum,
            evidence: None,
        },
    )
    .unwrap();
    // Feature-scoped review is independent of the explicit read access helper.
    for (task, feature, key) in [("A", "F", "feature_id"), ("B", "G", "feature")] {
        let mut prd = json!({"tasks":[{"id":task,"acceptance_criteria":["preserve contract"]}]});
        prd["tasks"][0][key] = json!(feature);
        std::fs::write(root.join("worktree.prd.json"), prd.to_string()).unwrap();
        let c = store::checkpoints::prepare(root, &request(root, root, task)).unwrap();
        assert_eq!(c.prompt.contains("public contract must"), feature == "F");
    }
}
#[test]
fn bounded_agent_pages_preserve_evidence_without_exceeding_complete_payload_limit() {
    let temp = tests::root();
    let root = temp.path();
    let view = policy::inspect(root).unwrap();
    let mut p = view.policy;
    p.context_engine.pull_limit_tokens = 100;
    policy::update(root, &view.revision, Some(p)).unwrap();
    let mut proposal = tests::proposal(root, None);
    proposal.body = "contract € proof ".repeat(60);
    let m = memory::propose(root, &proposal).unwrap();
    tests::activate(root, &m, None);
    let page = memory::pull(
        root,
        &memory::Search {
            query: "contract".into(),
            access: Default::default(),
            include_inactive: false,
        },
        0,
    )
    .unwrap();
    assert!(crate::context::estimate_tokens(&serde_json::to_string(&page).unwrap()) <= 100);
    assert_eq!(page.references, vec![m.id.clone()]);
    let mut offset = 0;
    let mut full = String::new();
    loop {
        let part = memory::read(root, &m.id, &Default::default(), offset).unwrap();
        assert!(part.estimated_tokens <= 100);
        assert!(crate::context::estimate_tokens(&serde_json::to_string(&part).unwrap()) <= 100);
        full.push_str(&part.fragment);
        if let Some(next) = part.next_offset {
            assert!(next > offset);
            offset = next;
        } else {
            break;
        }
    }
    let decoded: serde_json::Value = serde_json::from_str(&full).unwrap();
    assert_eq!(decoded["memory"]["proposal"]["body"], proposal.body);
    let reduced = context::reduce(root, "log", &format!("ERROR {}", "€".repeat(3000))).unwrap();
    assert!(reduced.estimated_tokens <= 100);
    assert!(reduced.summary.contains(&reduced.raw_ref));
}
#[test]
fn version_one_upgrade_preserves_ledger_and_backs_up_before_adding_ownership() {
    let temp = tests::root();
    let root = temp.path();
    let m = memory::propose(root, &tests::proposal(root, None)).unwrap();
    let (db, _) = database(root).unwrap();
    db.execute("DROP TABLE native_setting_ownership", [])
        .unwrap();
    db.execute(
        "UPDATE session_meta SET value='1' WHERE key='o2_schema_version'",
        [],
    )
    .unwrap();
    drop(db);
    assert!(!policy::inspect(root).unwrap().ready);
    let preview = upgrade(root, false, None).unwrap();
    let applied = upgrade(root, true, Some(&preview.source_sha256)).unwrap();
    assert!(Path::new(applied.backup.as_ref().unwrap()).is_file());
    assert!(policy::inspect(root).unwrap().ready);
    assert_eq!(
        memory::get(root, &m.id, &Default::default())
            .unwrap()
            .memory
            .status,
        "proposed"
    );
}
