use super::*;
use serde_json::{json, Value};
use std::collections::BTreeMap;

pub(crate) fn large_graph() -> (tempfile::TempDir, Store, Plan, Policy) {
    let (tmp, mut store, plan, registry, mut p) = super::tests::fixture();
    p.max_context_bytes = 32768;
    configure(tmp.path(), p.clone()).unwrap();
    let mut operations = vec![Operation::UpdateTask {
        task_id: "A".into(),
        changes: BTreeMap::from([(
            "description".into(),
            json!("EXACT prerequisite requirement éè 日本語 \"quoted\"\n".repeat(400)),
        )]),
    }];
    for id in ["D", "E", "F"] {
        operations.push(Operation::AddTask {task:json!({"id":id,"dependencies":[],"description":"EXACT prerequisite requirement éè 日本語 \"quoted\"\n".repeat(400),"acceptance_criteria":["Preserve the whole contract"]})});
    }
    operations.push(Operation::UpdateTask {
        task_id: "B".into(),
        changes: BTreeMap::from([("dependencies".into(), json!(["A", "D", "E", "F"]))]),
    });
    let patch = Patch {
        plan_id: plan.id.clone(),
        base_graph_version: plan.graph_version,
        idempotency_key: "large-graph".into(),
        reason: "Representative four-prerequisite documentation task".into(),
        decision_refs: vec![],
        operations,
        metadata_changes: BTreeMap::from([
            (
                "global_constraints".into(),
                json!(["EXACT mandatory project requirement"]),
            ),
            (
                "historical_notes".into(),
                json!("Historical plan evidence. ".repeat(600)),
            ),
        ]),
    };
    let plan = store
        .apply_patch(&plan.id, &patch, &registry, "Owner")
        .unwrap();
    let runtime_registry =
        crate::coordinator::model::TaskRegistry::from_value(&json!({"tasks":plan.graph["tasks"]}))
            .unwrap();
    crate::coordinator::state::coordinator_state_registry_save(
        tmp.path(),
        &BTreeMap::new(),
        &runtime_registry.to_value().unwrap(),
    )
    .unwrap();
    (tmp, store, plan, p)
}

#[test]
fn normal_four_prerequisite_context_fits_an_existing_32k_policy_without_losing_sources() {
    let (tmp, mut store, plan, p) = large_graph();
    let (context, _) = store
        .issue(&plan, "B", "normal", "dev", tmp.path(), &p)
        .unwrap();
    assert_eq!(context.schema_version, 4);
    assert_eq!(context.dependency_contracts.len(), 4);
    assert!(
        serde_json::to_vec(&context.dependency_contracts)
            .unwrap()
            .len()
            > 32768
    );
    assert!(context.render().len() < 32768);
    assert!(!context.render().contains("EXACT prerequisite requirement"));
    assert!(context.render().contains("context-source --dependency"));
    assert!(context.render().contains("context-source --plan"));
    assert!(context
        .render()
        .contains("EXACT mandatory project requirement"));
    assert!(context.render().contains("Preserve the whole contract"));
    let page = store
        .context_source(
            &plan.id,
            "normal",
            "dev",
            None,
            Some("/global_constraints"),
            0,
            2048,
        )
        .unwrap();
    assert_eq!(
        serde_json::from_str::<Value>(page["content"].as_str().unwrap()).unwrap(),
        json!(["EXACT mandatory project requirement"])
    );
    assert!(page["next_offset"].is_null());
    assert!(store
        .context_source(&plan.id, "normal", "dev", Some("C"), None, 0, 1024)
        .is_err());
    assert_eq!(context.budget().required_bytes, context.render().len());
    assert_eq!(
        context.budget().sections.values().sum::<usize>(),
        context.render().len()
    );
    let mut legacy = context.clone();
    legacy.schema_version = 2;
    assert!(legacy.render().contains("EXACT prerequisite requirement"));
    assert!(legacy.render().len() > 32768);
}

#[test]
fn paged_sources_are_exact_utf8_and_bound_the_encoded_output() {
    let (tmp, mut store, plan, p) = large_graph();
    let (context, _) = store
        .issue(&plan, "B", "paged", "dev", tmp.path(), &p)
        .unwrap();
    let mut offset = 0;
    let mut content = String::new();
    let mut count = 0;
    loop {
        let page = store
            .context_source(&plan.id, "paged", "dev", Some("A"), None, offset, 2048)
            .unwrap();
        assert!(serde_json::to_vec_pretty(&page).unwrap().len() <= 2048);
        assert_eq!(
            page["source_digest"],
            digest(&context.dependency_contracts["A"])
        );
        content.push_str(page["content"].as_str().unwrap());
        count += 1;
        match page["next_offset"].as_u64() {
            Some(next) => {
                assert!(next as usize > offset);
                offset = next as usize;
            }
            None => break,
        }
    }
    assert!(count > 1);
    assert_eq!(
        serde_json::from_str::<Value>(&content).unwrap(),
        context.dependency_contracts["A"]
    );
    assert!(store
        .context_source(&plan.id, "paged", "dev", Some("A"), None, usize::MAX, 2048)
        .is_err());
    assert!(store
        .context_source(&plan.id, "paged", "dev", None, Some("/unknown"), 0, 2048)
        .is_err());
}

#[test]
fn a_hidden_prerequisite_change_invalidates_context_receipts_and_paged_reads() {
    let (tmp, mut store, plan, p) = large_graph();
    let (context, _) = store
        .issue(&plan, "B", "old", "dev", tmp.path(), &p)
        .unwrap();
    store
        .issued(&plan.id, "old", "dev", &context.render(), &p)
        .unwrap();
    let registry = registry(tmp.path()).unwrap();
    let patch = Patch {
        plan_id: plan.id.clone(),
        base_graph_version: plan.graph_version,
        idempotency_key: "changed-source".into(),
        reason: "Updated prerequisite obligation".into(),
        decision_refs: vec![],
        operations: vec![Operation::UpdateTask {
            task_id: "A".into(),
            changes: BTreeMap::from([("description".into(), json!("Changed exact requirement"))]),
        }],
        metadata_changes: BTreeMap::new(),
    };
    let next = store
        .apply_patch(&plan.id, &patch, &registry, "Owner")
        .unwrap();
    let current = store.context(&next, "B", "next", "dev", &p).unwrap();
    assert_ne!(context.context_digest, current.context_digest);
    assert_ne!(context.contract_digest, current.contract_digest);
    assert!(store
        .context_source(&plan.id, "old", "dev", Some("A"), None, 0, 2048)
        .unwrap_err()
        .to_string()
        .contains("DM_STALE"));
    assert!(store
        .issued(&plan.id, "old", "dev", &context.render(), &p)
        .is_err());
}

#[test]
fn budget_preview_does_not_claim_or_issue_and_reports_oversize_before_dispatch() {
    let (tmp, store, _plan, mut p) = large_graph();
    p.max_context_bytes = 2048;
    configure(tmp.path(), p.clone()).unwrap();
    let before = registry(tmp.path()).unwrap();
    let report = context_budget(tmp.path(), "B", "dev").unwrap().unwrap();
    assert!(!report.fits);
    assert_eq!(report.budget_bytes, 2048);
    assert_eq!(report.prerequisite_count, 4);
    assert!(report
        .check()
        .unwrap_err()
        .to_string()
        .contains("required_bytes="));
    let after = registry(tmp.path()).unwrap();
    assert!(
        after.tasks == before.tasks,
        "Budget preview changed task state"
    );
    assert!(after.resource_locks == before.resource_locks);
    assert!(after
        .tasks
        .iter()
        .all(|t| t.task_runtime.claim_id.is_none()));
    let count: i64 = store
        .db
        .query_row("SELECT count(*) FROM contexts", [], |r| r.get(0))
        .unwrap();
    assert_eq!(count, 0);
}

#[test]
fn new_defaults_increase_headroom_but_explicit_project_caps_remain_authoritative() {
    let p: Policy = serde_json::from_value(json!({"enabled":true})).unwrap();
    assert_eq!(p.max_context_bytes, 131072);
    let p: Policy =
        serde_json::from_value(json!({"enabled":true,"max_context_bytes":32768})).unwrap();
    assert_eq!(p.max_context_bytes, 32768);
    let (_tmp, store, plan, _registry, mut p) = super::tests::fixture();
    p.max_context_bytes = 2048;
    let error = store
        .context(&plan, "A", "budget", "dev", &p)
        .unwrap_err()
        .to_string();
    assert!(error.contains(CONTEXT_BUDGET_CODE));
    assert!(error.contains("E903"));
    assert!(error.contains("no binding was truncated"));
}

#[test]
fn exact_decision_wording_is_still_inline_and_never_truncated() {
    let (_tmp, mut store, plan, _registry, mut p) = super::tests::fixture();
    p.human_question_min_risk = Consultation::NeverAsk;
    let exact = "EXACT-MANDATORY-WORDING ".repeat(700);
    let request:Request=serde_json::from_value(json!({"idempotency_key":"exact-binding","topic":"stable-public-contract","task_id":"A","question":"Which contract?","context":"Two contracts differ","investigated":["Read all consumers"],"recommendation":exact,"risk":"high","impact":{"shared_contract":true,"evidence":["Shared consumers"]}})).unwrap();
    store
        .submit(&plan, request, "agent:A", "agent", &p)
        .unwrap();
    let context = store.context(&plan, "A", "exact", "dev", &p).unwrap();
    assert!(context.render().contains(&exact));
    assert!(!context.bindings.is_empty());
    assert!(context.budget().sections["bindings"] > 16000);
    p.max_context_bytes = 8192;
    let error = store
        .context(&plan, "A", "bounded", "dev", &p)
        .unwrap_err()
        .to_string();
    assert!(error.contains(CONTEXT_BUDGET_CODE));
    assert!(error.contains("bindings=1"));
}

#[test]
fn heavily_escaped_sources_still_make_paging_progress() {
    let (tmp, mut store, plan, _registry, p) = super::tests::fixture();
    let patch = Patch {
        plan_id: plan.id.clone(),
        base_graph_version: plan.graph_version,
        idempotency_key: "escaped-source".into(),
        reason: "Source contains escaped data".into(),
        decision_refs: vec![],
        operations: vec![Operation::UpdateTask {
            task_id: "A".into(),
            changes: BTreeMap::from([("description".into(), json!("\\".repeat(3000)))]),
        }],
        metadata_changes: BTreeMap::new(),
    };
    let registry = registry(tmp.path()).unwrap();
    let plan = store
        .apply_patch(&plan.id, &patch, &registry, "Owner")
        .unwrap();
    let (context, _) = store
        .issue(&plan, "B", "escaped", "dev", tmp.path(), &p)
        .unwrap();
    let mut text = String::new();
    let mut offset = 0;
    loop {
        let page = store
            .context_source(
                &plan.id,
                "escaped",
                "dev",
                Some("A"),
                Some("/description"),
                offset,
                2048,
            )
            .unwrap();
        assert!(serde_json::to_vec_pretty(&page).unwrap().len() + 1 <= 2048);
        text.push_str(page["content"].as_str().unwrap());
        if let Some(next) = page["next_offset"].as_u64() {
            assert!(next as usize > offset);
            offset = next as usize;
        } else {
            break;
        }
    }
    assert_eq!(
        serde_json::from_str::<Value>(&text).unwrap(),
        context.dependency_contracts["A"]["description"]
    );
}
