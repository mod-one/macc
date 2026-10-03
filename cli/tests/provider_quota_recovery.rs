//! Provider quota messages must reach requeue and the existing countdown path.
use macc_core::coordinator::engine::{
    apply_job_completion_in_registry, JobCompletionInput, NormalizerInput,
};
use macc_core::coordinator::error_normalizer::NormalizerRegistry;
use macc_core::coordinator::rate_limit::{
    is_tool_throttled, next_throttle_expiry, ToolThrottleRegistry, ToolThrottleState,
};
use serde_json::json;

fn quota_failure(tool: &str, message: &str) -> ToolThrottleState {
    // Link the real inventory registrations; do not stub provider normalization.
    let _ = macc_adapter_claude::error_normalizer::ClaudeErrorNormalizer;
    let _ = macc_adapter_codex::error_normalizer::CodexErrorNormalizer;
    let normalizers = NormalizerRegistry::from_inventory();
    assert!(normalizers.get(tool).is_some());
    let now = chrono::Utc::now().to_rfc3339();
    let mut registry = json!({"tasks":[{
        "id":"QUOTA-TEST", "title":"Quota regression", "state":"claimed",
        "tool":tool, "task_runtime":{"status":"running","current_phase":"dev"}
    }]});
    let completion = apply_job_completion_in_registry(
        &mut registry,
        "QUOTA-TEST",
        &JobCompletionInput {
            success: false,
            attempt: 1,
            max_attempts: 1,
            timed_out: false,
            phase_timeout_seconds: 0,
            elapsed_seconds: 1,
            status_text: "quota exhausted; ".into(),
            completion_kind: None,
            error_code: Some("E602".into()),
            error_origin: Some("runner".into()),
            error_message: Some("quota exhausted; ".into()),
            result_explanation: None,
            unmet_preconditions: vec![],
            auto_retry_error_codes: vec![],
            auto_retry_max: 0,
            backoff_base_seconds: 30,
            backoff_max_seconds: 300,
            normalizer_input: Some(NormalizerInput {
                exit_code: 1,
                stderr: String::new(),
                stdout: message.into(),
            }),
        },
        &normalizers,
        &now,
    )
    .unwrap();
    assert_eq!(completion.status_label, "quota_exhausted_requeue");
    assert_eq!(registry["tasks"][0]["state"], "todo");
    let error = completion.tool_error.unwrap();
    assert_eq!(error.error_code, "E602");
    let cooldown = error.retry_after_seconds.expect("provider reset hint");
    let throttle: ToolThrottleState =
        serde_json::from_value(registry["tasks"][0]["task_runtime"]["throttle_state"].clone())
            .unwrap();
    assert_eq!(throttle.tool_id, tool);
    assert_eq!(throttle.backoff_seconds, cooldown);
    assert!(cooldown > 0);
    throttle
}

#[test]
fn claude_session_limit_requeues_and_exposes_utc_countdown_without_throttling_codex() {
    let throttle = quota_failure(
        "claude",
        "You've hit your session limit · resets 12:30am (UTC)",
    );
    let mut tools = ToolThrottleRegistry::new();
    tools.insert("claude".into(), throttle);
    let now = chrono::Utc::now().to_rfc3339();
    assert!(is_tool_throttled(&tools, "claude", &now));
    assert!(!is_tool_throttled(&tools, "codex", &now));
    let expiry = next_throttle_expiry(&tools).unwrap();
    // The same expiry consumed by clients for the countdown has minute precision.
    let expiry = chrono::DateTime::parse_from_rfc3339(&expiry).unwrap();
    let expected = expiry.date_naive().and_hms_opt(0, 30, 0).unwrap().and_utc();
    // Normalization and state application read the clock independently.
    assert!(
        (expiry.timestamp() - expected.timestamp()).abs() <= 1,
        "{expiry}"
    );
}

#[test]
fn codex_usage_limit_preserves_its_absolute_retry_hint_and_countdown() {
    let reset = chrono::Utc::now() + chrono::Duration::hours(3);
    let message = format!(
        "ERROR: You’ve hit your usage limit. Try again at {}.",
        reset.format("%b %-d, %Y %-I:%M %p")
    );
    let throttle = quota_failure("codex", &message);
    let mut tools = ToolThrottleRegistry::new();
    tools.insert("codex".into(), throttle);
    let now = chrono::Utc::now().to_rfc3339();
    assert!(is_tool_throttled(&tools, "codex", &now));
    assert!(!is_tool_throttled(&tools, "claude", &now));
    let expiry = next_throttle_expiry(&tools).unwrap();
    let expiry = chrono::DateTime::parse_from_rfc3339(&expiry).unwrap();
    let expected =
        chrono::DateTime::parse_from_rfc3339(&reset.format("%Y-%m-%dT%H:%M:00Z").to_string())
            .unwrap();
    assert!((expiry.timestamp() - expected.timestamp()).abs() <= 1);
}
