//! Real P3 workflows survive the same bundle path used by macc save/restore.
use super::*;
use crate::o2::{memory, policy, providers, semantic, tuning};

#[test]
fn p3_facts_conflicts_benchmarks_provider_receipts_and_trials_round_trip() {
    let (_tmp, paths, saves) = setup();
    let root = &paths.root;
    fs::write(root.join(".gitignore"), ".macc/\n").unwrap();
    fs::write(root.join("contract.rs"), "pub fn contract() {}\n").unwrap();
    for args in [
        vec!["init", "-b", "main"],
        vec!["config", "user.name", "Test"],
        vec!["config", "user.email", "test@example.invalid"],
        vec!["add", "."],
        vec!["commit", "-m", "fixture"],
    ] {
        crate::git::run_git_output_mapped(root, &args, "Initialize snapshot fixture").unwrap();
    }
    let view = policy::inspect(root).unwrap();
    let mut p = view.policy;
    p.memory.enabled = true;
    p.memory.semantic_enabled = true;
    p.memory.semantic_dimensions = 3;
    p.code_intelligence.enabled = true;
    p.code_intelligence.native_map_enabled = true;
    p.context_engine.tuning_enabled = true;
    policy::update(root, &view.revision, Some(p.clone())).unwrap();
    let evidence =
        memory::symbol_evidence(root, "contract.rs", "contract", "native", None).unwrap();
    let proposal = |title: &str, body: &str| memory::Proposal {
        kind: "constraint".into(),
        title: title.into(),
        body: body.into(),
        scope: "project".into(),
        scope_ref: None,
        confidence: 0.9,
        evidence: vec![evidence.clone()],
        tags: vec![],
        source_task: None,
        source_revision: None,
    };
    let mut records = vec![];
    for (title, body) in [
        (
            "Credential verification",
            "authentication login password credentials session identity",
        ),
        (
            "Signon identity",
            "authentication access signon identity token session",
        ),
        (
            "Access session",
            "login access identity session credential verification",
        ),
        (
            "Database write",
            "database storage transaction persistence query index",
        ),
        (
            "Search storage",
            "database retrieval index query storage records",
        ),
        (
            "Storage reliability",
            "transaction records persistence database index storage",
        ),
        (
            "Processing workers",
            "queue worker job throughput scheduler processing",
        ),
        (
            "Job execution",
            "queue task execution worker scheduler dispatch",
        ),
        (
            "Dispatch capacity",
            "processing job worker dispatch queue throughput",
        ),
    ] {
        let m = memory::propose(root, &proposal(title, body)).unwrap();
        let hit = memory::get(root, &m.id, &Default::default()).unwrap();
        memory::review(
            root,
            &memory::Review {
                id: m.id.clone(),
                status: "active".into(),
                reason: "Reviewed fixture".into(),
                confirmed: true,
                expected_checksum: hit.checksum,
                evidence: None,
            },
        )
        .unwrap();
        records.push(m.id);
    }
    let left = memory::propose(root, &proposal("Unreviewed claim", "Pending claim one")).unwrap();
    let right = memory::propose(
        root,
        &proposal("Unreviewed alternative", "Pending claim two"),
    )
    .unwrap();
    memory::conflict(root, &left.id, &right.id, "Requires reviewed evidence").unwrap();
    for action in ["deploy", "index"] {
        let providers::Capability::Supported(plan) =
            providers::plan(root, "native", action).unwrap()
        else {
            panic!()
        };
        providers::apply(
            root,
            &plan.id,
            &store::digest(&serde_json::to_vec(&plan).unwrap()),
        )
        .unwrap();
    }
    let suite = semantic::Suite {
        access: Default::default(),
        top_k: 3,
        cases: [
            ("password", 0),
            ("signon", 0),
            ("retrieval", 3),
            ("execution", 6),
        ]
        .into_iter()
        .map(|(q, i)| semantic::Case {
            query: q.into(),
            relevant_ids: records[i..i + 3].to_vec(),
        })
        .collect(),
    };
    let benchmark = semantic::benchmark(root, suite).unwrap();
    assert_eq!(benchmark["report"]["passed"], true);
    semantic::apply_gate(
        root,
        benchmark["report"]["id"].as_str().unwrap(),
        benchmark["checksum"].as_str().unwrap(),
        true,
        true,
    )
    .unwrap();
    let trial = tuning::create(
        root,
        tuning::Plan {
            field: "context_engine.push_memory_limit_tokens".into(),
            candidate: 800.,
            cases: (0..20)
                .map(|i| tuning::Case {
                    task_id: format!("TASK-{i}"),
                    revision: crate::git::head_commit(root).unwrap(),
                })
                .collect(),
            protocol_path: "contract.rs".into(),
            protocol_sha256: store::digest(&fs::read(root.join("contract.rs")).unwrap()),
        },
        true,
    )
    .unwrap();
    let trial_id = trial["report"]["id"].as_str().unwrap();
    let cancelled = tuning::close(root, trial_id, true, true).unwrap();
    let before = semantic::status(root, &Default::default()).unwrap();
    assert!(before["approved_event"].is_string());
    let audit = |root: &Path| {
        let db = rusqlite::Connection::open(root.join(".macc/memory.db")).unwrap();
        let mut q = db
            .prepare("SELECT kind,record_id,payload_json FROM o2_audit ORDER BY rowid")
            .unwrap();
        q.query_map([], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, String>(2)?,
            ))
        })
        .unwrap()
        .collect::<rusqlite::Result<Vec<_>>>()
        .unwrap()
    };
    let original_audit = audit(root);
    save(&paths, &saves);
    {
        let db = rusqlite::Connection::open(root.join(".macc/memory.db")).unwrap();
        db.execute_batch("DELETE FROM memory_conflicts; DELETE FROM memory_evidence; DELETE FROM memory_search_projection; DELETE FROM memories; DELETE FROM provider_receipts; DELETE FROM o2_audit;").unwrap();
    }
    fs::write(&paths.config_path, "tools:\n  enabled: []\n").unwrap();
    restore::restore_from(&paths, "full", &restore_options(), &saves).unwrap();
    assert_eq!(policy::load(root).unwrap(), p);
    assert_eq!(audit(root), original_audit);
    assert_eq!(memory::conflicts(root).unwrap().len(), 1);
    let restored = semantic::status(root, &Default::default()).unwrap();
    assert_eq!(restored["approved_event"], before["approved_event"]);
    assert_eq!(restored["reports"][0]["checksum"], benchmark["checksum"]);
    assert_eq!(
        tuning::inspect(root, Some(trial_id)).unwrap()["result"],
        cancelled
    );
    assert!(
        memory::search(
            root,
            &memory::Search {
                query: "password".into(),
                access: Default::default(),
                include_inactive: false
            }
        )
        .unwrap()
        .len()
            > 1
    );
    let providers::Capability::Supported(answer) = providers::query(
        root,
        &providers::Query {
            text: "contract".into(),
            kind: "symbol_definition".into(),
            path: Some("contract.rs".into()),
            worktree: None,
        },
    )
    .unwrap() else {
        panic!()
    };
    assert_eq!(answer.hits.len(), 1);
}
