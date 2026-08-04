//! Integration tests for normalizer routing through the coordinator engine.
//!
//! These tests verify that per-adapter error normalizers are correctly
//! dispatched via [`NormalizerRegistry`] and that the job-completion path
//! produces the right [`CanonicalClass`], error codes, and task state
//! transitions.
//!
//! They live in `macc-registry` (not `macc-core`) because:
//! - `macc-core` is tool-agnostic and cannot import adapter crates.
//! - `macc-registry` links all three adapter crates, so
//!   `NormalizerRegistry::from_inventory()` is fully populated.

use macc_core::coordinator::engine::{apply_job_completion, JobCompletionInput, NormalizerInput};
use macc_core::coordinator::error_normalizer::{CanonicalClass, NormalizerRegistry};

// ── Helpers ──────────────────────────────────────────────────────────

fn registry() -> NormalizerRegistry {
    // Force the linker to include all adapter crates so their
    // `inventory::submit!` calls run and populate the registry.
    let _ = (
        macc_adapter_claude::ClaudeAdapter,
        macc_adapter_codex::CodexAdapter,
        macc_adapter_gemini::GeminiAdapter,
    );
    NormalizerRegistry::from_inventory()
}

fn make_failure_task(tool_id: &str) -> serde_json::Value {
    serde_json::json!({
        "id": "TN1",
        "state": "claimed",
        "tool": tool_id,
        "task_runtime": { "status": "running", "pid": 1 }
    })
}

fn make_failure_input(stderr: &str, stdout: &str) -> JobCompletionInput {
    JobCompletionInput {
        success: false,
        attempt: 1,
        max_attempts: 1,
        timed_out: false,
        phase_timeout_seconds: 300,
        elapsed_seconds: 10,
        status_text: "performer exited with error".to_string(),
        completion_kind: None,
        error_code: None,
        error_origin: None,
        error_message: None,
        result_explanation: None,
        auto_retry_error_codes: Vec::new(),
        auto_retry_max: 0,
        backoff_base_seconds: 30,
        backoff_max_seconds: 300,
        normalizer_input: Some(NormalizerInput {
            exit_code: 1,
            stderr: stderr.to_string(),
            stdout: stdout.to_string(),
        }),
    }
}

// ── Normalizer routing ────────────────────────────────────────────────

#[test]
fn normalizer_routes_claude_529_to_overloaded() {
    let mut task_val = make_failure_task("claude");
    let input = make_failure_input("Error: 529 API overloaded", "");
    let out = apply_job_completion(&mut task_val, &input, &registry(), "2026-02-21T00:00:00Z");
    assert_eq!(out.status_label, "rate_limit_backoff");
    assert_eq!(task_val["task_runtime"]["last_error_code"], "E601");
    assert_eq!(task_val["state"], "todo");
    assert!(
        !task_val["task_runtime"]["delayed_until"].is_null(),
        "delayed_until should be set"
    );
    let te = out.tool_error.unwrap();
    assert_eq!(te.canonical_class, CanonicalClass::Overloaded);
    assert_eq!(te.error_code, "E601");
    assert_eq!(te.provider, "claude");
    assert!(te.retryable);
}

#[test]
fn normalizer_routes_codex_insufficient_quota_to_e602() {
    let mut task_val = make_failure_task("codex");
    let input = make_failure_input(
        "429 insufficient_quota: You exceeded your current quota",
        "",
    );
    let out = apply_job_completion(&mut task_val, &input, &registry(), "2026-02-21T00:00:00Z");
    assert_eq!(task_val["task_runtime"]["last_error_code"], "E602");
    let te = out.tool_error.unwrap();
    assert_eq!(te.canonical_class, CanonicalClass::QuotaExhausted);
    assert_eq!(te.error_code, "E602");
    assert_eq!(te.provider, "codex");
    assert!(!te.retryable);
}

#[test]
fn normalizer_routes_gemini_resource_exhausted_quota_to_e602() {
    let mut task_val = make_failure_task("gemini");
    let input = make_failure_input(
        "429 RESOURCE_EXHAUSTED: Quota exceeded for requests per minute",
        "",
    );
    let out = apply_job_completion(&mut task_val, &input, &registry(), "2026-02-21T00:00:00Z");
    assert_eq!(task_val["task_runtime"]["last_error_code"], "E602");
    let te = out.tool_error.unwrap();
    assert_eq!(te.canonical_class, CanonicalClass::QuotaExhausted);
    assert_eq!(te.provider, "gemini");
}

#[test]
fn normalizer_routes_gemini_resource_exhausted_rate_limit_to_e601() {
    let mut task_val = make_failure_task("gemini");
    let input = make_failure_input("429 RESOURCE_EXHAUSTED: Rate limit for model", "");
    let out = apply_job_completion(&mut task_val, &input, &registry(), "2026-02-21T00:00:00Z");
    assert_eq!(task_val["task_runtime"]["last_error_code"], "E601");
    let te = out.tool_error.unwrap();
    assert_eq!(te.canonical_class, CanonicalClass::RateLimit);
}

// ── Extra fields stored in task_runtime ──────────────────────────────

#[test]
fn tool_error_stored_in_extra() {
    let mut task_val = make_failure_task("claude");
    let input = make_failure_input("Error: 529 API overloaded", "");
    apply_job_completion(&mut task_val, &input, &registry(), "2026-02-21T00:00:00Z");
    let stored = &task_val["task_runtime"]["tool_error"];
    assert!(!stored.is_null(), "tool_error should be stored in extra");
    assert_eq!(stored["canonical_class"], "Overloaded");
    assert_eq!(stored["error_code"], "E601");
    assert_eq!(stored["provider"], "claude");
}

#[test]
fn rate_limit_info_stored_in_extra_for_e601() {
    let mut task_val = make_failure_task("claude");
    let input = make_failure_input("Error: 429 Rate limit exceeded", "");
    apply_job_completion(&mut task_val, &input, &registry(), "2026-02-21T00:00:00Z");
    let rli = &task_val["task_runtime"]["rate_limit_info"];
    assert!(!rli.is_null(), "rate_limit_info should be stored for E601");
    assert_eq!(rli["tool_id"], "claude");
    assert_eq!(rli["error_code"], "E601");
}

#[test]
fn rate_limit_info_stored_in_extra_for_e602() {
    let mut task_val = make_failure_task("codex");
    let input = make_failure_input("429 insufficient_quota: quota exceeded", "");
    apply_job_completion(&mut task_val, &input, &registry(), "2026-02-21T00:00:00Z");
    let rli = &task_val["task_runtime"]["rate_limit_info"];
    assert!(!rli.is_null(), "rate_limit_info should be stored for E602");
    assert_eq!(rli["tool_id"], "codex");
    assert_eq!(rli["error_code"], "E602");
}

// ── Exit-code override (stdout signals success despite non-zero exit) ─

#[test]
fn exit_code_override_already_satisfied_with_transient_error() {
    // Performer signals already_satisfied in stdout but exits non-zero
    // due to a 529 overload on teardown. Should be treated as success.
    let mut task_val = make_failure_task("claude");
    let input = make_failure_input("Error: 529 API overloaded", "already_satisfied");
    let out = apply_job_completion(&mut task_val, &input, &registry(), "2026-02-21T00:00:00Z");
    assert_eq!(out.status_label, "already_satisfied");
    assert_eq!(task_val["state"], "merged");
    assert_eq!(task_val["task_runtime"]["status"], "idle");
    assert_eq!(
        task_val["task_runtime"]["completion_kind"],
        "already_satisfied"
    );
}

#[test]
fn exit_code_override_macc_task_result_success_marker() {
    let mut task_val = make_failure_task("claude");
    let input = make_failure_input("Error: 529 overloaded", "MACC_TASK_RESULT: success");
    let out = apply_job_completion(&mut task_val, &input, &registry(), "2026-02-21T00:00:00Z");
    assert_eq!(out.status_label, "already_satisfied");
    assert_eq!(task_val["state"], "merged");
}

#[test]
fn exit_code_override_does_not_fire_for_hard_quota_error() {
    // QuotaExhausted is not transient, so override must NOT fire even if
    // stdout says "already_satisfied".
    let mut task_val = make_failure_task("codex");
    let input = make_failure_input("429 insufficient_quota", "already_satisfied");
    let out = apply_job_completion(&mut task_val, &input, &registry(), "2026-02-21T00:00:00Z");
    assert_eq!(out.status_label, "quota_exhausted_requeue");
    assert_eq!(task_val["state"], "todo");
}

// ── RL-BACKOFF-003: backoff engine integration ────────────────────────

#[test]
fn e601_requeues_todo_with_delayed_until() {
    let mut task_val = make_failure_task("claude");
    let input = make_failure_input("Error: 429 Rate limit exceeded", "");
    let out = apply_job_completion(&mut task_val, &input, &registry(), "2026-02-21T00:00:00Z");
    assert_eq!(out.status_label, "rate_limit_backoff");
    assert_eq!(task_val["state"], "todo");
    assert_eq!(task_val["task_runtime"]["status"], "idle");
    assert_eq!(task_val["task_runtime"]["last_error_code"], "E601");
    let delayed = task_val["task_runtime"]["delayed_until"].as_str().unwrap();
    assert!(!delayed.is_empty(), "delayed_until must be set for E601");
    assert!(
        chrono::DateTime::parse_from_rfc3339(delayed).is_ok(),
        "delayed_until must be valid RFC 3339"
    );
}

#[test]
fn e601_throttle_state_stored_in_extra() {
    let mut task_val = make_failure_task("claude");
    let input = make_failure_input("Error: 429 Rate limit exceeded", "");
    apply_job_completion(&mut task_val, &input, &registry(), "2026-02-21T00:00:00Z");
    let ts = &task_val["task_runtime"]["throttle_state"];
    assert!(!ts.is_null(), "throttle_state should be stored for E601");
    assert_eq!(ts["consecutive_429_count"], 1);
    assert!(ts["backoff_seconds"].as_u64().unwrap() > 0);
    assert!(ts["throttled_until"].as_u64().unwrap() > 0);
}

#[test]
fn e602_requeues_task_with_cooldown() {
    let mut task_val = make_failure_task("codex");
    let input = make_failure_input(
        "429 insufficient_quota: You exceeded your current quota",
        "",
    );
    let out = apply_job_completion(&mut task_val, &input, &registry(), "2026-02-21T00:00:00Z");
    assert_eq!(out.status_label, "quota_exhausted_requeue");
    assert_eq!(task_val["state"], "todo");
    assert_eq!(task_val["task_runtime"]["status"], "idle");
    assert_eq!(task_val["task_runtime"]["last_error_code"], "E602");
    assert!(
        !task_val["task_runtime"]["delayed_until"].is_null(),
        "delayed_until must be set for E602 re-queue"
    );
}

#[test]
fn e601_delayed_until_is_in_the_future() {
    let now = "2026-02-21T00:00:00Z";
    let mut task_val = make_failure_task("gemini");
    let input = make_failure_input("429 RESOURCE_EXHAUSTED: Rate limit for model", "");
    apply_job_completion(&mut task_val, &input, &registry(), now);
    let delayed = task_val["task_runtime"]["delayed_until"]
        .as_str()
        .expect("delayed_until must be a string");
    let delayed_dt = chrono::DateTime::parse_from_rfc3339(delayed).unwrap();
    let now_dt = chrono::DateTime::parse_from_rfc3339(now).unwrap();
    assert!(
        delayed_dt > now_dt,
        "delayed_until ({}) must be in the future relative to now ({})",
        delayed,
        now
    );
}

// ── NormalizerRegistry routing ────────────────────────────────────────

#[test]
fn registry_resolves_all_three_adapters() {
    let reg = registry();
    assert!(reg.get("claude").is_some(), "claude must be registered");
    assert!(reg.get("codex").is_some(), "codex must be registered");
    assert!(reg.get("gemini").is_some(), "gemini must be registered");
    assert!(reg.get("unknown-tool").is_none());
    assert!(reg.get("").is_none());
}
