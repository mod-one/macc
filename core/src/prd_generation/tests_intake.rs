use super::validation::validate_prd_file;
use serde_json::{json, Value};
fn proposal() -> Value {
    json!({"idempotency_key":"choice-v1","task_id":"A","kind":"decision","question":"Which contract?","investigated":["Read consumers"],"recommendation":"Preserve current contract","options":[{"id":"keep","label":"Preserve contract"}],"recommended_option_id":"keep"})
}
fn validate(requests: Value, enabled: bool) -> super::ValidationResult {
    let tmp = tempfile::tempdir().unwrap();
    std::fs::create_dir(tmp.path().join(".macc")).unwrap();
    std::fs::write(tmp.path().join(".macc/macc.yaml"),json!({"tools":{"enabled":[]},"automation":{"coordinator":{"decisions":{"enabled":enabled}}}}).to_string()).unwrap();
    let file = tmp.path().join("prd.json");
    std::fs::write(
        &file,
        json!({"lot":"test","tasks":[{"id":"A"}],"decision_requests":requests}).to_string(),
    )
    .unwrap();
    validate_prd_file(&file, None)
}
#[test]
fn initial_decisions_use_native_shape_references_and_effect_boundaries() {
    assert!(validate(json!([proposal()]), true).ok);
    let mut bad = Vec::new();
    bad.push(json!({"not":"array"}));
    bad.push(Value::Null);
    for (key, value) in [
        ("task_id", json!("UNKNOWN")),
        ("affected_tasks", json!(["OTHER"])),
        ("recommended_option_id", json!("other")),
        ("resolution", json!({"actor":"Human"})),
        (
            "options",
            json!([{"id":"keep","label":"Keep","requires_replan":true}]),
        ),
        (
            "proposed_patch",
            json!({"plan_id":"fake","base_graph_version":1,"idempotency_key":"x","reason":"x","operations":[]}),
        ),
        ("response_fields", json!([{"id":"__private","label":"No"}])),
        ("expires_at", json!("yesterday")),
    ] {
        let mut r = proposal();
        r[key] = value;
        bad.push(json!([r]));
    }
    for requests in bad {
        let result = validate(requests.clone(), true);
        assert!(!result.ok, "Accepted invalid intake {requests}");
        assert!(result
            .errors
            .iter()
            .any(|e| e.contains("PRD-GEN-DECISION-REQUESTS")));
    }
    assert!(!validate(json!([proposal(), proposal()]), true).ok);
    assert!(!validate(json!(vec![proposal(); 257]), true).ok);
}
#[test]
fn intervention_requires_real_action_details_and_never_fabricates_a_result() {
    let mut request = proposal();
    request["kind"] = json!("intervention");
    assert!(!validate(json!([request.clone()]), true).ok);
    request["intervention"] = json!({"action":"Connect the phone","inaccessible_reason":"No physical access","expected_result":"Actual device output"});
    assert!(validate(json!([request.clone()]), true).ok);
    request["performed"] = json!("yes");
    assert!(!validate(json!([request]), true).ok);
}
#[test]
fn disabled_decisions_warn_without_creating_authority_or_rejecting_valid_drafts() {
    let result = validate(json!([proposal()]), false);
    assert!(result.ok, "{:?}", result.errors);
    assert!(result
        .warnings
        .iter()
        .any(|w| w.contains("PRD-GEN-DECISIONS-DISABLED")));
    assert!(!validate(json!([proposal()]), true)
        .warnings
        .iter()
        .any(|w| w.contains("DECISIONS-DISABLED")));
    assert!(!validate(json!([]), false)
        .warnings
        .iter()
        .any(|w| w.contains("DECISIONS-DISABLED")));
}
#[test]
fn request_budget_uses_destination_policy_and_preserves_input() {
    let tmp = tempfile::tempdir().unwrap();
    std::fs::create_dir(tmp.path().join(".macc")).unwrap();
    std::fs::write(tmp.path().join(".macc/macc.yaml"),json!({"tools":{"enabled":[]},"automation":{"coordinator":{"decisions":{"enabled":true,"max_request_bytes":2048}}}}).to_string()).unwrap();
    let mut r = proposal();
    r["context"] = json!("é".repeat(1500));
    let bytes = json!({"tasks":[{"id":"A"}],"decision_requests":[r]}).to_string();
    let file = tmp.path().join("prd.json");
    std::fs::write(&file, &bytes).unwrap();
    assert!(!validate_prd_file(&file, None).ok);
    assert_eq!(std::fs::read_to_string(file).unwrap(), bytes);
    assert!(!tmp.path().join(".git").exists());
}
