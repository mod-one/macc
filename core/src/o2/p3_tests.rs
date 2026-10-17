//! P3-A: semantic freshness, reviewed contradictions, version TTL and explainable rank.
use super::*;
use serde_json::json;
pub(super) fn active(
    root: &Path,
    evidence: memory::Evidence,
    kind: &str,
    body: &str,
) -> memory::Memory {
    let mut p = tests::proposal(root, None);
    p.evidence = vec![evidence];
    p.kind = kind.into();
    p.body = body.into();
    let m = memory::propose(root, &p).unwrap();
    tests::activate(root, &m, None);
    m
}
fn all(root: &Path) -> Vec<memory::Hit> {
    memory::search(
        root,
        &memory::Search {
            query: String::new(),
            access: Default::default(),
            include_inactive: false,
        },
    )
    .unwrap()
}
pub(super) fn file(root: &Path) -> memory::Evidence {
    serde_json::from_value(
        actions::execute(
            root,
            actions::Action::MemoryEvidence {
                path: "contract.rs".into(),
                worktree: None,
            },
        )
        .unwrap(),
    )
    .unwrap()
}
#[test]
fn symbol_changes_are_local_semantic_and_revalidation_is_explicit() {
    let t = tests::root();
    let r = t.path();
    let e = memory::symbol_evidence(r, "contract.rs", "preserve_contract", "native", None).unwrap();
    let original = e.sha256.clone();
    let m = active(r, e, "decision", "Keep this semantic contract.");
    std::fs::write(
        r.join("contract.rs"),
        "// unrelated comment\npub fn unrelated() {}\npub fn preserve_contract( ) { }\n",
    )
    .unwrap();
    assert_eq!(
        memory::symbol_evidence(r, "contract.rs", "preserve_contract", "native", None)
            .unwrap()
            .sha256,
        original
    );
    assert_eq!(all(r).len(), 1);
    std::fs::write(
        r.join("contract.rs"),
        "pub fn preserve_contract() { let changed = 1; }\n",
    )
    .unwrap();
    assert!(all(r).is_empty());
    let old = memory::get(r, &m.id, &Default::default()).unwrap();
    assert_eq!(
        old.memory.status, "active",
        "worktree-local detection must not mutate canonical state"
    );
    let stale = memory::review(
        r,
        &memory::Review {
            id: m.id.clone(),
            status: "stale".into(),
            reason: "Changed body".into(),
            confirmed: true,
            expected_checksum: old.checksum,
            evidence: None,
        },
    )
    .unwrap();
    let hit = memory::get(r, &stale.id, &Default::default()).unwrap();
    memory::review(
        r,
        &memory::Review {
            id: m.id,
            status: "active".into(),
            reason: "Reviewed new body".into(),
            confirmed: true,
            expected_checksum: hit.checksum,
            evidence: Some(vec![memory::symbol_evidence(
                r,
                "contract.rs",
                "preserve_contract",
                "native",
                None,
            )
            .unwrap()]),
        },
    )
    .unwrap();
    assert_eq!(all(r).len(), 1);
    std::fs::write(r.join("contract.rs"), "pub fn renamed() {}\n").unwrap();
    assert!(all(r).is_empty());
}
#[test]
fn symbols_reject_unsupported_missing_ambiguous_and_unparseable_sources() {
    let t = tests::root();
    let r = t.path();
    for (source, selector) in [
        ("struct S; impl S {fn f(){}} impl S {fn f(){}}", "S/f"),
        ("not valid Rust", "preserve_contract"),
        ("fn missing() {}", "preserve_contract"),
    ] {
        std::fs::write(r.join("contract.rs"), source).unwrap();
        assert!(memory::symbol_evidence(r, "contract.rs", selector, "native", None).is_err());
    }
    std::fs::write(r.join("sample.py"), "def f(): pass").unwrap();
    assert!(memory::symbol_evidence(r, "sample.py", "f", "native", None).is_err());
    assert!(memory::symbol_evidence(r, "contract.rs", "f", "graphify", None).is_err());
    std::fs::write(
        r.join("contract.rs"),
        "mod n { struct S; impl S { fn f(){ } fn other(){} } }",
    )
    .unwrap();
    let first = memory::symbol_evidence(r, "contract.rs", "n/S/f", "native", None).unwrap();
    std::fs::write(
        r.join("contract.rs"),
        "mod n { struct S; impl S { fn f(){ } fn other(){ let x=1; } } }",
    )
    .unwrap();
    assert_eq!(
        first.sha256,
        memory::symbol_evidence(r, "contract.rs", "n/S/f", "native", None)
            .unwrap()
            .sha256
    );
}
#[test]
fn conflicts_are_deduplicated_and_conservative_resolution_does_not_restore_both_claims() {
    let t = tests::root();
    let r = t.path();
    let a = active(r, file(r), "constraint", "A claim");
    let b = active(r, file(r), "constraint", "Opposite claim");
    let id = memory::conflict(r, &a.id, &b.id, "Contradictory").unwrap();
    assert_eq!(id, memory::conflict(r, &b.id, &a.id, "Same pair").unwrap());
    assert!(all(r).is_empty());
    let row = &memory::conflicts(r).unwrap()[0];
    memory::resolve_conflict(
        r,
        &id,
        "Retain inspected left",
        true,
        memory::Resolution::KeepLeft,
        row["checksum"].as_str(),
    )
    .unwrap();
    assert_eq!(all(r).len(), 1);
    assert_eq!(all(r)[0].memory.id, a.id);
    assert_eq!(
        memory::get(r, &b.id, &Default::default())
            .unwrap()
            .memory
            .status,
        "invalidated"
    );
    let c = active(r, file(r), "constraint", "Another opposing claim");
    let id = memory::conflict(r, &a.id, &c.id, "Needs review").unwrap();
    memory::resolve_conflict(r, &id, "Review pending", true, Default::default(), None).unwrap();
    assert!(all(r).is_empty());
    assert_eq!(
        memory::get(r, &a.id, &Default::default())
            .unwrap()
            .memory
            .status,
        "stale"
    );
}
#[test]
fn changed_conflict_claims_require_new_review_and_proposals_cannot_be_promoted_by_resolution() {
    let t = tests::root();
    let r = t.path();
    let a = active(r, file(r), "constraint", "A claim");
    let b = active(r, file(r), "constraint", "B claim");
    let id = memory::conflict(r, &a.id, &b.id, "Different assertions").unwrap();
    let old = memory::conflicts(r).unwrap()[0]["checksum"]
        .as_str()
        .unwrap()
        .to_string();
    let h = memory::get(r, &b.id, &Default::default()).unwrap();
    memory::review(
        r,
        &memory::Review {
            id: b.id.clone(),
            status: "stale".into(),
            reason: "Changed assumption".into(),
            confirmed: true,
            expected_checksum: h.checksum,
            evidence: None,
        },
    )
    .unwrap();
    assert!(memory::resolve_conflict(
        r,
        &id,
        "Old review",
        true,
        memory::Resolution::KeepLeft,
        Some(&old)
    )
    .is_err());
    assert_eq!(memory::conflicts(r).unwrap()[0]["status"], "open");
    let p = memory::propose(r, &tests::proposal(r, None)).unwrap();
    let id = memory::conflict(r, &p.id, &a.id, "New unreviewed claim").unwrap();
    let row = memory::conflicts(r)
        .unwrap()
        .into_iter()
        .find(|v| v["id"] == id)
        .unwrap();
    assert!(memory::resolve_conflict(
        r,
        &id,
        "Cannot promote",
        true,
        memory::Resolution::KeepLeft,
        row["checksum"].as_str()
    )
    .is_err());
    assert_eq!(
        memory::get(r, &p.id, &Default::default())
            .unwrap()
            .memory
            .status,
        "proposed"
    );
}
#[test]
fn ranking_is_explainable_unique_and_tunable_without_changing_memory_authority() {
    let t = tests::root();
    let r = t.path();
    let a = active(r, file(r), "discovery", "Same concise claim");
    let b = active(r, file(r), "discovery", "Same concise claim");
    let (db, auth) = database(r).unwrap();
    let mut m: memory::Memory = store::json::<memory::Memory>(
        sql(db.query_row(
            "SELECT record_json FROM memories WHERE id=?1",
            [&b.id],
            |row| row.get::<_, String>(0),
        ))
        .unwrap()
        .as_bytes(),
    )
    .unwrap();
    m.proposal.source_task = Some("TASK".into());
    m.proposal.evidence.push(m.proposal.evidence[0].clone());
    // Updating a fixture record must preserve its normal checksummed persistence path.
    memory::save(&db, &m).unwrap();
    drop(db);
    let query = memory::Search {
        query: "contract.rs".into(),
        access: memory::Access {
            task_id: Some("TASK".into()),
            ..Default::default()
        },
        include_inactive: false,
    };
    let hits = memory::search(r, &query).unwrap();
    assert_eq!(hits[0].memory.id, b.id);
    assert!(hits[0].rank_reason.contains("unique_evidence=15"));
    assert!(hits[0].rank_reason.contains("exact_reference=30"));
    assert!(hits
        .iter()
        .find(|h| h.memory.id == a.id)
        .unwrap()
        .rank_reason
        .contains("duplicate=-80"));
    assert_eq!(
        hits.iter().map(|h| &h.memory.id).collect::<Vec<_>>(),
        memory::search(r, &query)
            .unwrap()
            .iter()
            .map(|h| &h.memory.id)
            .collect::<Vec<_>>()
    );
    let view = policy::inspect(r).unwrap();
    let mut p = view.policy;
    p.memory.ranking_task_weight = 0;
    p.memory.ranking_reference_weight = 0;
    policy::update(r, &view.revision, Some(p)).unwrap();
    assert!(memory::search(r, &query)
        .unwrap()
        .iter()
        .all(|h| h.rank_reason.contains("task=0; exact_reference=0")));
    assert_eq!(auth.project_id, m.project_id);
}
#[cfg(unix)]
pub(super) fn tool(root: &Path) {
    let mut p = probe_tests::spec(root, "env");
    p.compaction.as_mut().unwrap().version_pattern = "^future-cli [0-9]+\\.[0-9]+\\.[0-9]+".into();
    std::fs::write(root.join(".fake-tool"),"#!/bin/sh\ncase \"$1\" in --version) printf v >> .macc/version-count; cat .macc/version;; --help) echo 'help';; *) exit 42;; esac\n").unwrap();
    std::fs::create_dir_all(root.join(".macc/tools.d")).unwrap();
    std::fs::write(root.join(".macc/tools.d/future.tool.json"),json!({"api_version":"v1","id":"future-tool","display_name":"Future","fields":[],"performer":p}).to_string()).unwrap();
    std::fs::write(root.join(".macc/version"), "future-cli 1.0.0\n").unwrap();
}
#[test]
#[cfg(unix)]
fn provider_notes_detect_runtime_updates_behind_unchanged_wrappers_and_cache_each_live_check() {
    let t = tests::root();
    let r = t.path();
    tool(r);
    active(
        r,
        memory::tool_evidence(r, "future-tool").unwrap(),
        "provider_note",
        "Observed behavior A",
    );
    active(
        r,
        memory::tool_evidence(r, "future-tool").unwrap(),
        "provider_note",
        "Observed behavior B",
    );
    std::fs::write(r.join(".macc/version-count"), "").unwrap();
    assert_eq!(all(r).len(), 2);
    assert_eq!(
        std::fs::read_to_string(r.join(".macc/version-count")).unwrap(),
        "v"
    );
    std::fs::write(r.join(".macc/version"), "future-cli 2.0.0\n").unwrap();
    assert!(all(r).is_empty());
}
#[test]
#[cfg(unix)]
fn ttl_uses_observation_time_not_status_update_and_static_decisions_do_not_expire() {
    let t = tests::root();
    let r = t.path();
    tool(r);
    let m = active(
        r,
        memory::tool_evidence(r, "future-tool").unwrap(),
        "provider_note",
        "Time sensitive behavior",
    );
    let d = active(r, file(r), "decision", "Static reviewed decision");
    let (db, a) = database(r).unwrap();
    let mut note = memory::load(&db, &m.id, &a.project_id).unwrap();
    if let Some(memory::Binding::Tool { observed_at, .. }) = &mut note.proposal.evidence[0].binding
    {
        *observed_at = now() - 15 * 86400;
    }
    note.updated_at = now();
    memory::save(&db, &note).unwrap();
    let mut decision = memory::load(&db, &d.id, &a.project_id).unwrap();
    decision.updated_at = 1;
    memory::save(&db, &decision).unwrap();
    drop(db);
    let hits = all(r);
    assert_eq!(hits.len(), 1);
    assert_eq!(hits[0].memory.id, d.id);
}
#[test]
fn legacy_evidence_round_trips_without_checksum_changes_and_notes_require_bound_version() {
    let old = json!({"kind":"file","reference":"contract.rs","sha256":null,"revision":null,"summary":"source"});
    let e: memory::Evidence = serde_json::from_value(old.clone()).unwrap();
    assert_eq!(serde_json::to_value(e).unwrap(), old);
    let t = tests::root();
    let r = t.path();
    let mut p = tests::proposal(r, None);
    p.kind = "provider_note".into();
    let m = memory::propose(r, &p).unwrap();
    let h = memory::get(r, &m.id, &Default::default()).unwrap();
    assert!(memory::review(
        r,
        &memory::Review {
            id: m.id,
            status: "active".into(),
            reason: "Insufficient evidence".into(),
            confirmed: true,
            expected_checksum: h.checksum,
            evidence: None
        }
    )
    .is_err());
    for field in policy::fields().into_iter().filter(|f| {
        f.path.starts_with("memory.ranking_") || f.path == "memory.provider_note_probe_limit"
    }) {
        assert_eq!(field.kind, "number");
    }
}

#[test]
fn freshness_flags_fail_closed_without_invalidating_other_worktrees() {
    let t = tests::root();
    let r = t.path();
    let e = memory::symbol_evidence(r, "contract.rs", "preserve_contract", "native", None).unwrap();
    let m = active(r, e, "decision", "Reviewed symbol");
    let view = policy::inspect(r).unwrap();
    let mut p = view.policy;
    p.code_intelligence.enabled = false;
    policy::update(r, &view.revision, Some(p)).unwrap();
    assert!(all(r).is_empty());
    assert!(
        memory::symbol_evidence(r, "contract.rs", "preserve_contract", "native", None).is_err()
    );
    assert_eq!(
        memory::get(r, &m.id, &Default::default())
            .unwrap()
            .memory
            .status,
        "active"
    );
    let view = policy::inspect(r).unwrap();
    let mut p = view.policy;
    p.memory.enabled = false;
    p.context_engine.enabled = false;
    policy::update(r, &view.revision, Some(p)).unwrap();
    assert!(all(r).is_empty());
}

#[test]
fn ranking_details_stay_in_manifest_and_not_in_the_pushed_model_payload() {
    let selected = context::Selection {
        id: "memory".into(),
        checksum: "hash".into(),
        source_refs: vec!["contract.rs".into()],
        reason: "internal scoring details".into(),
        estimated_tokens: 100,
        title: "Contract".into(),
        body: "Keep stable".into(),
    };
    let payload = context_prompt::render(std::slice::from_ref(&selected), "manifest").unwrap();
    assert!(payload.contains("Keep stable") && payload.contains("contract.rs"));
    assert!(!payload.contains("internal scoring details"));
    assert!(serde_json::to_string(&selected)
        .unwrap()
        .contains("internal scoring details"));
}

#[test]
fn symbol_check_budgets_cache_repeated_symbols_and_exclude_unchecked_candidates() {
    let t = tests::root();
    let r = t.path();
    std::fs::write(
        r.join("contract.rs"),
        "pub fn preserve_contract(){} pub fn other(){}
",
    )
    .unwrap();
    let first =
        memory::symbol_evidence(r, "contract.rs", "preserve_contract", "native", None).unwrap();
    let a = active(r, first.clone(), "decision", "First claim");
    let b = active(r, first, "decision", "Second claim on same symbol");
    let c = active(
        r,
        memory::symbol_evidence(r, "contract.rs", "other", "native", None).unwrap(),
        "decision",
        "Other symbol",
    );
    let (db, auth) = database(r).unwrap();
    for (id, stamp) in [(&a.id, now() + 100), (&b.id, now() + 50), (&c.id, now())] {
        let mut m = memory::load(&db, id, &auth.project_id).unwrap();
        m.updated_at = stamp;
        memory::save(&db, &m).unwrap();
    }
    drop(db);
    let view = policy::inspect(r).unwrap();
    let mut p = view.policy;
    p.memory.symbol_check_limit = 1;
    policy::update(r, &view.revision, Some(p)).unwrap();
    let hits = all(r);
    assert_eq!(hits.len(), 2);
    assert!(hits.iter().all(|h| h.memory.id != c.id));
    let rows = memory::search(
        r,
        &memory::Search {
            query: String::new(),
            access: Default::default(),
            include_inactive: true,
        },
    )
    .unwrap();
    assert!(rows
        .iter()
        .find(|h| h.memory.id == c.id)
        .unwrap()
        .memory
        .staleness_reason
        .as_ref()
        .unwrap()
        .contains("budget exhausted"));
    assert_eq!(
        memory::get(r, &c.id, &Default::default())
            .unwrap()
            .memory
            .status,
        "active"
    );
}
