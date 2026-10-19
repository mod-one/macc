use super::*;
use crate::o2::tests::{activate, proposal, root};
fn request(task: &str) -> Search {
    Search {
        query: "contract".into(),
        access: Access {
            task_id: Some(task.into()),
            ..Default::default()
        },
        include_inactive: false,
    }
}
#[test]
fn source_drift_is_explained_without_mutating_memory_or_leaking_scope() {
    let root = root();
    let m = propose(root.path(), &proposal(root.path(), Some("TASK-A"))).unwrap();
    activate(root.path(), &m, Some("TASK-A"));
    let request = request("TASK-A");
    let before = get(root.path(), &m.id, &request.access).unwrap();
    let source = root.path().join("contract.rs");
    let bytes = std::fs::read(&source).unwrap();
    std::fs::write(&source, b"pub fn changed_contract() {}\n").unwrap();
    let result = explain(root.path(), &request).unwrap();
    assert!(result.records.is_empty());
    assert!(result
        .exclusions
        .iter()
        .any(|e| e.contains(&m.id) && e.contains("snapshot_mismatch")));
    assert_eq!(
        get(root.path(), &m.id, &request.access).unwrap().checksum,
        before.checksum
    );
    let other = explain(root.path(), &self::request("TASK-B")).unwrap();
    assert!(other.records.is_empty());
    assert!(other.exclusions.is_empty());
    std::fs::write(&source, &bytes).unwrap();
    let result = explain(root.path(), &request).unwrap();
    assert_eq!(result.records[0].memory.id, m.id);
    assert!(result.exclusions.is_empty());
    // Default retrieval retains its existing bounded transport.
    let normal = serde_json::to_value(pull(root.path(), &request, 0).unwrap()).unwrap();
    assert_eq!(
        normal
            .as_object()
            .unwrap()
            .keys()
            .map(String::as_str)
            .collect::<Vec<_>>(),
        vec![
            "estimated_tokens",
            "limit_tokens",
            "next_cursor",
            "records",
            "references"
        ]
    );
    assert!(normal.get("exclusions").is_none());
}
#[test]
fn proposed_and_invalidated_facts_have_scoped_reasons() {
    let root = root();
    let m = propose(root.path(), &proposal(root.path(), Some("TASK-A"))).unwrap();
    let request = request("TASK-A");
    let result = explain(root.path(), &request).unwrap();
    assert!(result.records.is_empty());
    assert!(result.exclusions[0].contains("status=proposed"));
    activate(root.path(), &m, Some("TASK-A"));
    let hit = get(root.path(), &m.id, &request.access).unwrap();
    review(
        root.path(),
        &Review {
            id: m.id.clone(),
            status: "invalidated".into(),
            reason: "Retired requirement".into(),
            confirmed: true,
            expected_checksum: hit.checksum,
            evidence: None,
        },
    )
    .unwrap();
    let result = explain(root.path(), &request).unwrap();
    assert!(result.records.is_empty());
    assert!(result.exclusions[0].contains("status=invalidated"));
    assert!(explain(root.path(), &self::request("TASK-B"))
        .unwrap()
        .exclusions
        .is_empty());
}
#[test]
fn disabled_memory_has_an_explanation() {
    let root = root();
    let current = policy::inspect(root.path()).unwrap();
    let mut p = current.policy;
    p.memory.enabled = false;
    p.context_engine.enabled = false;
    policy::update(root.path(), &current.revision, Some(p)).unwrap();
    let result = explain(root.path(), &request("TASK-A")).unwrap();
    assert!(result.records.is_empty());
    assert!(result.exclusions[0].contains("disabled"));
}
#[test]
fn inactive_scan_never_displaces_active_retrieval_candidates() {
    let root = root();
    let p = policy::inspect(root.path()).unwrap();
    let mut policy = p.policy;
    policy.memory.search_limit = 1;
    policy::update(root.path(), &p.revision, Some(policy)).unwrap();
    let active = propose(root.path(), &proposal(root.path(), Some("TASK-A"))).unwrap();
    activate(root.path(), &active, Some("TASK-A"));
    for _ in 0..3 {
        propose(root.path(), &proposal(root.path(), Some("TASK-A"))).unwrap();
    }
    let request = request("TASK-A");
    let normal = search(root.path(), &request).unwrap();
    let diagnostics = explain(root.path(), &request).unwrap();
    assert_eq!(normal[0].memory.id, diagnostics.records[0].memory.id);
    assert_eq!(diagnostics.records.len(), 1);
    assert_eq!(diagnostics.exclusions.len(), 3);
}
#[test]
fn confidence_and_conflict_have_distinct_reasons() {
    let root = root();
    let mut low = proposal(root.path(), Some("TASK-A"));
    low.confidence = 0.2;
    let low = propose(root.path(), &low).unwrap();
    activate(root.path(), &low, Some("TASK-A"));
    let left = propose(root.path(), &proposal(root.path(), Some("TASK-A"))).unwrap();
    let right = propose(root.path(), &proposal(root.path(), Some("TASK-A"))).unwrap();
    activate(root.path(), &left, Some("TASK-A"));
    activate(root.path(), &right, Some("TASK-A"));
    conflict(
        root.path(),
        &left.id,
        &right.id,
        "Conflicting contract assertions",
    )
    .unwrap();
    let result = explain(root.path(), &request("TASK-A")).unwrap();
    assert!(result.records.is_empty());
    assert!(result
        .exclusions
        .iter()
        .any(|e| e.contains(&low.id) && e.contains("confidence 0.2 below policy minimum")));
    assert!(result
        .exclusions
        .iter()
        .any(|e| e.contains(&left.id) && e.contains("unresolved memory conflict")));
}
