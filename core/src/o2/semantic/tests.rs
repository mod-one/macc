use super::*;
#[path = "audit_tests.rs"]
mod audit_tests;
fn root() -> (tempfile::TempDir, Vec<Memory>) {
    let t = crate::o2::tests::root();
    let r = t.path();
    let view = policy::inspect(r).unwrap();
    let mut p = view.policy;
    p.memory.semantic_enabled = true;
    p.memory.semantic_dimensions = 3;
    policy::update(r, &view.revision, Some(p)).unwrap();
    let docs = [
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
    ];
    let records = docs
        .into_iter()
        .map(|(title, body)| {
            let mut p = crate::o2::tests::proposal(r, None);
            p.title = title.into();
            p.body = body.into();
            let m = memory::propose(r, &p).unwrap();
            crate::o2::tests::activate(r, &m, None);
            memory::get(r, &m.id, &Access::default()).unwrap().memory
        })
        .collect();
    (t, records)
}
fn suite(records: &[Memory]) -> Suite {
    Suite {
        access: Default::default(),
        top_k: 3,
        cases: [
            ("password", 0),
            ("signon", 0),
            ("retrieval", 3),
            ("execution", 6),
        ]
        .into_iter()
        .map(|(q, start)| Case {
            query: q.into(),
            relevant_ids: records[start..start + 3]
                .iter()
                .map(|m| m.id.clone())
                .collect(),
        })
        .collect(),
    }
}
fn request(query: &str) -> Search {
    Search {
        query: query.into(),
        access: Default::default(),
        include_inactive: false,
    }
}
fn approve(root: &Path, docs: &[Memory]) -> serde_json::Value {
    let result = benchmark(root, suite(docs)).unwrap();
    assert_eq!(result["report"]["passed"], true, "{result}");
    apply_gate(
        root,
        result["report"]["id"].as_str().unwrap(),
        result["checksum"].as_str().unwrap(),
        true,
        true,
    )
    .unwrap();
    result
}
#[test]
fn corpus_local_embeddings_are_deterministic_finite_and_do_not_guess_unknown_words() {
    let (t, docs) = root();
    let p = policy::load(t.path()).unwrap().memory;
    let (_, a) = backend::fit(&docs, &p).unwrap();
    let (_, b) = backend::fit(&docs, &p).unwrap();
    assert_eq!(
        a.similarities("password").unwrap(),
        b.similarities("password").unwrap()
    );
    assert!(a
        .similarities("neverencounteredvocabulary")
        .unwrap()
        .iter()
        .all(|v| *v == 0.));
    assert!(a.bytes() <= p.semantic_max_bytes as usize);
}
#[test]
fn measured_gain_requires_separate_review_and_retrieves_paraphrases_after_approval() {
    let (t, docs) = root();
    let r = t.path();
    assert_eq!(memory::search(r, &request("password")).unwrap().len(), 1);
    let result = benchmark(r, suite(&docs)).unwrap();
    assert_eq!(result["report"]["passed"], true, "{result}");
    assert_eq!(
        memory::search(r, &request("password")).unwrap().len(),
        1,
        "benchmark must not approve itself"
    );
    let id = result["report"]["id"].as_str().unwrap();
    let sha = result["checksum"].as_str().unwrap();
    assert!(apply_gate(r, id, "wrong", true, true).is_err());
    assert!(apply_gate(r, id, sha, true, false).is_err());
    apply_gate(r, id, sha, true, true).unwrap();
    let hits = memory::search(r, &request("password")).unwrap();
    assert!(
        hits.iter().any(|h| h.memory.id == docs[2].id),
        "{:?}",
        hits.iter()
            .map(|h| &h.memory.proposal.title)
            .collect::<Vec<_>>()
    );
    assert!(hits.iter().any(|h| h.rank_reason.contains("cosine=")));
    apply_gate(r, id, sha, false, true).unwrap();
    assert_eq!(memory::search(r, &request("password")).unwrap().len(), 1);
}
#[test]
fn rejected_benchmark_cannot_be_approved() {
    let (t, docs) = root();
    let mut cases = suite(&docs);
    cases.top_k = 1;
    for (i, c) in cases.cases.iter_mut().enumerate() {
        c.relevant_ids = vec![docs[[0, 1, 4, 7][i]].id.clone()];
    }
    let report = benchmark(t.path(), cases).unwrap();
    assert_eq!(report["report"]["fts_recall"], 1.);
    assert_eq!(report["report"]["passed"], false);
    assert!(apply_gate(
        t.path(),
        report["report"]["id"].as_str().unwrap(),
        report["checksum"].as_str().unwrap(),
        true,
        true
    )
    .is_err());
}
#[test]
fn gate_revocation_between_preflight_and_transaction_drops_semantic_only_candidates() {
    let (t, docs) = root();
    let r = t.path();
    let result = approve(r, &docs);
    let q = request("password");
    let prepared = memory::prepare_search(r, &q).unwrap();
    assert!(!prepared.semantic.scores.is_empty());
    apply_gate(
        r,
        result["report"]["id"].as_str().unwrap(),
        result["checksum"].as_str().unwrap(),
        false,
        true,
    )
    .unwrap();
    let (db, a) = database(r).unwrap();
    let result = memory::search::search_in(&db, &a.project_id, r, r, &q, false, &prepared).unwrap();
    assert_eq!(result.hits.len(), 1);
    assert!(result.exclusions.iter().any(|s| s.contains("FTS retained")));
}
#[test]
fn changed_policy_or_corpus_requires_a_new_benchmark() {
    let (t, docs) = root();
    let r = t.path();
    approve(r, &docs);
    let view = policy::inspect(r).unwrap();
    let mut p = view.policy;
    p.memory.semantic_weight += 1;
    policy::update(r, &view.revision, Some(p)).unwrap();
    assert_eq!(memory::search(r, &request("password")).unwrap().len(), 1);
    approve(r, &docs);
    let mut proposal = crate::o2::tests::proposal(r, None);
    proposal.body = "A new independently reviewed constraint".into();
    let m = memory::propose(r, &proposal).unwrap();
    crate::o2::tests::activate(r, &m, None);
    assert_eq!(memory::search(r, &request("password")).unwrap().len(), 1);
}
#[test]
fn scope_staleness_conflicts_and_proposals_cannot_enter_embedding_inputs() {
    let (t, docs) = root();
    let r = t.path();
    approve(r, &docs);
    let mut p = crate::o2::tests::proposal(r, Some("other-task"));
    p.body = "Hidden password authentication claim".into();
    let private = memory::propose(r, &p).unwrap();
    crate::o2::tests::activate(r, &private, Some("other-task"));
    let proposed = memory::propose(r, &crate::o2::tests::proposal(r, None)).unwrap();
    let pool = prepare_pool(r, &Access::default()).unwrap();
    assert_eq!(pool.records.len(), 9);
    assert!(!pool
        .records
        .iter()
        .any(|m| m.id == private.id || m.id == proposed.id));
    let hits = memory::search(r, &request("password")).unwrap();
    assert!(!hits.iter().any(|h| h.memory.id == private.id));
    assert!(hits.iter().any(|h| h.memory.id == docs[2].id));
    memory::conflict(r, &docs[0].id, &docs[1].id, "Contradictory source claims").unwrap();
    assert!(memory::search(r, &request("password")).unwrap().is_empty());
    std::fs::write(r.join("contract.rs"), "changed source\n").unwrap();
    assert!(prepare_pool(r, &Access::default())
        .unwrap()
        .records
        .is_empty());
}
#[test]
fn invalid_labels_duplicates_and_resource_limits_fail_closed() {
    let (t, docs) = root();
    let r = t.path();
    let mut s = suite(&docs);
    s.cases[0].relevant_ids = vec!["foreign-id".into()];
    assert!(benchmark(r, s).is_err());
    let mut unsafe_scope = suite(&docs);
    unsafe_scope.access.tool_id = Some("credential=ghp_".to_owned() + &"x".repeat(40));
    assert!(benchmark(r, unsafe_scope).is_err());
    let mut s = suite(&docs);
    s.cases[1].query = s.cases[0].query.clone();
    assert!(benchmark(r, s).is_err());
    let mut p = policy::load(r).unwrap().memory;
    p.semantic_max_bytes = 1;
    assert!(backend::fit(&docs, &p).is_err());
    p.semantic_max_bytes = 8388608;
    p.semantic_max_ms = 0;
    assert!(backend::fit(&docs, &p).is_err());
    let view = policy::inspect(r).unwrap();
    let mut p = view.policy;
    p.memory.semantic_candidates = 3;
    policy::update(r, &view.revision, Some(p)).unwrap();
    assert_eq!(memory::search(r, &request("password")).unwrap().len(), 1);
}
#[test]
fn approved_read_only_search_never_changes_memory_states_and_preserves_fts_when_disabled() {
    let (t, docs) = root();
    let r = t.path();
    approve(r, &docs);
    let before = docs
        .iter()
        .map(|m| memory::get(r, &m.id, &Access::default()).unwrap().checksum)
        .collect::<Vec<_>>();
    memory::search(r, &request("password")).unwrap();
    let after = docs
        .iter()
        .map(|m| memory::get(r, &m.id, &Access::default()).unwrap().checksum)
        .collect::<Vec<_>>();
    assert_eq!(before, after);
    let view = policy::inspect(r).unwrap();
    let mut p = view.policy;
    p.memory.semantic_enabled = false;
    policy::update(r, &view.revision, Some(p)).unwrap();
    assert_eq!(memory::search(r, &request("password")).unwrap().len(), 1);
    assert!(!MemoryPolicy::default().semantic_enabled);
}

#[test]
fn a_changed_non_selected_embedding_source_invalidates_the_whole_semantic_proof() {
    let (t, docs) = root();
    let r = t.path();
    std::fs::write(r.join("extra.rs"), "pub fn extra() {}\n").unwrap();
    crate::git::run_git_output_mapped(
        r,
        &["add", "extra.rs"],
        "Track independent semantic evidence",
    )
    .unwrap();
    let extra: memory::Evidence = serde_json::from_value(
        actions::execute(
            r,
            actions::Action::MemoryEvidence {
                path: "extra.rs".into(),
                worktree: None,
            },
        )
        .unwrap(),
    )
    .unwrap();
    let first = memory::get(r, &docs[8].id, &Access::default()).unwrap();
    memory::review(
        r,
        &memory::Review {
            id: docs[8].id.clone(),
            status: "stale".into(),
            reason: "Review replacement source".into(),
            confirmed: true,
            expected_checksum: first.checksum,
            evidence: None,
        },
    )
    .unwrap();
    let next = memory::get(r, &docs[8].id, &Access::default()).unwrap();
    memory::review(
        r,
        &memory::Review {
            id: docs[8].id.clone(),
            status: "active".into(),
            reason: "Reviewed independent source".into(),
            confirmed: true,
            expected_checksum: next.checksum,
            evidence: Some(vec![extra]),
        },
    )
    .unwrap();
    approve(r, &docs);
    let q = request("password");
    let prepared = memory::prepare_search(r, &q).unwrap();
    std::fs::write(r.join("extra.rs"), "pub fn extra(){let changed=1;}\n").unwrap();
    let (db, a) = database(r).unwrap();
    let response =
        memory::search::search_in(&db, &a.project_id, r, r, &q, false, &prepared).unwrap();
    assert_eq!(response.hits.len(), 1);
    assert!(!response.hits[0].rank_reason.contains("cosine="));
}
struct BadSpace(Vec<f64>);
impl EmbeddingSpace for BadSpace {
    fn bytes(&self) -> usize {
        0
    }
    fn similarities(&self, _: &str) -> Result<Vec<f64>> {
        Ok(self.0.clone())
    }
}
struct RemoteFixture;
impl EmbeddingBackend for RemoteFixture {
    fn descriptor(&self) -> Descriptor {
        Descriptor {
            id: "remote_fixture".into(),
            recipe: "remote-v1".into(),
            local_only: false,
            description: "Forbidden remote fixture".into(),
        }
    }
    fn fit(&self, _: &[String], _: &EmbeddingLimits) -> Result<Box<dyn EmbeddingSpace>> {
        panic!("Remote backend must not execute")
    }
}
inventory::submit! {Registration{create:||Box::new(RemoteFixture)}}
#[test]
fn remote_backends_and_malformed_embeddings_are_rejected_before_use() {
    assert!(backend::backend("remote_fixture").is_err());
    let (t, docs) = root();
    let p = policy::load(t.path()).unwrap().memory;
    assert!(backend::scores(&BadSpace(vec![f64::NAN; 9]), "password", &docs, &p).is_err());
    assert!(backend::scores(&BadSpace(vec![0.; 8]), "password", &docs, &p).is_err());
    assert!(backend::scores(&BadSpace(vec![2.; 9]), "password", &docs, &p).is_err());
}
#[test]
fn catalog_and_policy_bounds_preserve_opt_in_and_reject_remote_configuration() {
    let mut p = policy::Policy::default();
    assert!(!p.memory.semantic_enabled);
    for field in [
        "semantic_enabled",
        "semantic_backend",
        "semantic_dimensions",
        "semantic_candidates",
        "semantic_vocabulary",
        "semantic_weight",
        "semantic_min_similarity",
        "semantic_max_ms",
        "semantic_max_bytes",
        "semantic_min_recall_gain",
    ] {
        assert!(policy::fields()
            .iter()
            .any(|f| f.path == format!("memory.{field}")));
    }
    p.memory.semantic_backend = "remote_fixture".into();
    assert!(p.validate().is_err());
    p.memory.semantic_backend = "local_lsa".into();
    p.memory.semantic_min_recall_gain = 0.;
    assert!(p.validate().is_err());
}

#[test]
fn prepared_scores_cannot_be_reused_for_another_query() {
    let (t, docs) = root();
    let r = t.path();
    approve(r, &docs);
    let prepared = memory::prepare_search(r, &request("password")).unwrap();
    let q = request("retrieval");
    let (db, a) = database(r).unwrap();
    let response =
        memory::search::search_in(&db, &a.project_id, r, r, &q, false, &prepared).unwrap();
    assert_eq!(response.hits.len(), 1);
    assert_eq!(response.hits[0].memory.id, docs[4].id);
    assert!(!response.hits[0].rank_reason.contains("cosine="));
}
#[test]
fn changed_task_affinity_requires_a_matching_quality_gate_even_for_identical_records() {
    let (t, docs) = root();
    let r = t.path();
    let (db, a) = database(r).unwrap();
    let mut m = memory::load(&db, &docs[0].id, &a.project_id).unwrap();
    m.proposal.source_task = Some("rank-task".into());
    memory::save(&db, &m).unwrap();
    drop(db);
    approve(r, &docs);
    let mut q = request("password");
    q.access.task_id = Some("rank-task".into());
    assert_eq!(memory::search(r, &q).unwrap().len(), 1);
    q.access.task_id = Some("another-task".into());
    assert!(memory::search(r, &q)
        .unwrap()
        .iter()
        .any(|h| h.memory.id == docs[2].id));
}

#[test]
fn settings_or_disable_race_cannot_authorize_old_scores_with_new_freshness_proofs() {
    let (t, docs) = root();
    let r = t.path();
    approve(r, &docs);
    let q = request("password");
    for disable in [false, true] {
        let old = memory::prepare_search(r, &q).unwrap();
        let view = policy::inspect(r).unwrap();
        let mut p = view.policy;
        if disable {
            p.memory.semantic_enabled = false;
        } else {
            p.memory.semantic_weight += 1;
        }
        policy::update(r, &view.revision, Some(p)).unwrap();
        let mut new = prepare_pool(r, &q.access).unwrap().prepared;
        new.semantic = old.semantic;
        let (db, a) = database(r).unwrap();
        let result = memory::search::search_in(&db, &a.project_id, r, r, &q, false, &new).unwrap();
        assert_eq!(result.hits.len(), 1);
        assert!(!result.hits[0].rank_reason.contains("cosine="));
        drop(db);
        if !disable {
            approve(r, &docs);
        }
    }
}

#[test]
fn bounded_source_enumeration_cannot_silently_approve_a_partial_corpus() {
    let (t, docs) = root();
    let r = t.path();
    approve(r, &docs);
    let (db, _) = database(r).unwrap();
    for i in 0..492 {
        let mut m = docs[0].clone();
        m.id = uuid::Uuid::new_v4().to_string();
        m.proposal.title = "Archive fact".into();
        m.proposal.body = "Unrelated low confidence archive fact".into();
        m.proposal.confidence = 0.1;
        m.updated_at = now() + i;
        memory::save(&db, &m).unwrap();
    }
    drop(db);
    assert!(prepare_pool(r, &Access::default()).is_err());
    assert_eq!(memory::search(r, &request("password")).unwrap().len(), 1);
}
