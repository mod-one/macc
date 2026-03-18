//! Integration tests for the full rate-limit lifecycle.
//!
//! Covers: canonical error normalization → backoff → fallback dispatch →
//! concurrency throttle → recovery, across all three adapter normalizers.
//! All tests operate entirely in-memory — no filesystem or git required.

use macc_core::coordinator::error_normalizer::{CanonicalClass, ErrorNormalizer};
use macc_core::coordinator::normalizers::{
    ClaudeErrorNormalizer, CodexErrorNormalizer, GeminiErrorNormalizer,
};
use macc_core::coordinator::rate_limit::{
    clear_throttle_state, compute_backoff_delay, is_task_delayed, is_tool_throttled,
    next_throttle_expiry, update_throttle_state, RateLimitInfo, ToolThrottleRegistry,
    ToolThrottleState,
};
use macc_core::coordinator::task_selector::{select_next_ready_task_typed, TaskSelectorConfig};
use macc_core::coordinator::model::{Task, TaskRegistry};

// ── Helpers ──────────────────────────────────────────────────────────

fn far_future_epoch() -> u64 {
    // 2099-01-01T00:00:00Z
    4_070_908_800
}

fn make_registry_with_tools(tasks: Vec<Task>) -> TaskRegistry {
    TaskRegistry { tasks, ..Default::default() }
}

fn make_todo_task(id: &str, tool: &str) -> Task {
    Task {
        id: id.to_string(),
        title: Some(format!("Task {}", id)),
        tool: Some(tool.to_string()),
        state: "todo".to_string(),
        ..Default::default()
    }
}

fn make_throttle_registry(tool: &str, until_epoch: u64, count: u32) -> ToolThrottleRegistry {
    let mut reg = ToolThrottleRegistry::default();
    reg.insert(
        tool.to_string(),
        ToolThrottleState {
            tool_id: tool.to_string(),
            throttled_until: until_epoch,
            consecutive_429_count: count,
            backoff_seconds: 60,
            last_rate_limit_info: None,
        },
    );
    reg
}

fn base_selector_config(tools: Vec<&str>, max_parallel: usize) -> TaskSelectorConfig {
    TaskSelectorConfig {
        enabled_tools: tools.iter().map(|s| s.to_string()).collect(),
        tool_priority: tools.iter().map(|s| s.to_string()).collect(),
        default_tool: tools.first().copied().unwrap_or("").to_string(),
        default_base_branch: "main".to_string(),
        max_parallel,
        now: "2026-03-18T12:00:00Z".to_string(),
        ..TaskSelectorConfig::default()
    }
}

// ── Scenario 1: Claude 529 overloaded → E601 (Overloaded) ────────────

#[test]
fn claude_529_overloaded_produces_e601() {
    let normalizer = ClaudeErrorNormalizer;
    let stderr = r#"API Error: 529 {"type":"error","error":{"type":"overloaded_error","message":"Overloaded"}} req_01ABCxyz12345678901"#;
    let result = normalizer.normalize(1, stderr, "");

    let err = result.expect("Claude 529 should produce a ToolError");
    assert_eq!(err.error_code, "E601", "overloaded_error must map to E601");
    assert!(
        matches!(err.canonical_class, CanonicalClass::Overloaded),
        "class must be Overloaded, got {:?}",
        err.canonical_class
    );
    assert!(err.retryable, "E601 must be retryable");
    assert!(!err.user_action_required, "E601 must not require user action");
    assert!(
        err.request_id.is_some(),
        "req_xxx request_id must be extracted"
    );
    assert_eq!(err.provider, "claude");
}

// ── Scenario 2: OpenAI 429 + insufficient_quota → E602 ───────────────

#[test]
fn codex_429_insufficient_quota_produces_e602() {
    let normalizer = CodexErrorNormalizer;
    let stderr = "429 You exceeded your current quota, please check your plan and billing details. insufficient_quota";
    let result = normalizer.normalize(1, stderr, "");

    let err = result.expect("Codex quota error should produce a ToolError");
    assert_eq!(err.error_code, "E602", "insufficient_quota must map to E602");
    assert!(
        matches!(err.canonical_class, CanonicalClass::QuotaExhausted),
        "class must be QuotaExhausted, got {:?}",
        err.canonical_class
    );
    assert!(!err.retryable, "E602 must NOT be retryable");
    assert!(
        err.user_action_required,
        "E602 must require user action"
    );
    assert_eq!(err.provider, "codex");
}

// ── Scenario 3: Gemini RESOURCE_EXHAUSTED → E601 (RateLimit) ─────────

#[test]
fn gemini_resource_exhausted_without_quota_produces_e601() {
    let normalizer = GeminiErrorNormalizer;
    let stderr = "429 RESOURCE_EXHAUSTED: Rate limit exceeded. Please retry after some time.";
    let result = normalizer.normalize(1, stderr, "");

    let err = result.expect("Gemini RESOURCE_EXHAUSTED should produce a ToolError");
    assert_eq!(
        err.error_code, "E601",
        "RESOURCE_EXHAUSTED without quota keyword must map to E601"
    );
    assert!(
        matches!(err.canonical_class, CanonicalClass::RateLimit),
        "class must be RateLimit, got {:?}",
        err.canonical_class
    );
    assert!(err.retryable, "E601 must be retryable");
    assert_eq!(err.provider, "gemini");
}

#[test]
fn gemini_resource_exhausted_with_quota_keyword_produces_e602() {
    let normalizer = GeminiErrorNormalizer;
    let stderr = "429 RESOURCE_EXHAUSTED: You have exceeded your quota limit. Please increase your quota or reduce your usage.";
    let result = normalizer.normalize(1, stderr, "");

    let err = result.expect("Gemini quota exhaustion should produce a ToolError");
    assert_eq!(
        err.error_code, "E602",
        "RESOURCE_EXHAUSTED with quota keyword must map to E602"
    );
    assert!(
        matches!(err.canonical_class, CanonicalClass::QuotaExhausted),
        "class must be QuotaExhausted, got {:?}",
        err.canonical_class
    );
    assert!(!err.retryable, "E602 must NOT be retryable");
    assert_eq!(err.provider, "gemini");
}

// ── Scenario 3b: Session conflict → E603 ─────────────────────────────

#[test]
fn claude_session_conflict_produces_e603() {
    let normalizer = ClaudeErrorNormalizer;
    let stderr = "Error: session already in use. Please start a new session.";
    let result = normalizer.normalize(1, stderr, "");

    let err = result.expect("Session conflict should produce a ToolError");
    assert_eq!(err.error_code, "E603", "session conflict must map to E603");
    assert!(
        matches!(err.canonical_class, CanonicalClass::SessionConflict),
        "class must be SessionConflict, got {:?}",
        err.canonical_class
    );
    assert!(err.retryable, "E603 must be retryable");
    assert_eq!(err.provider, "claude");
}

// ── Scenario 4: Backoff + throttle + recovery lifecycle ───────────────

#[test]
fn backoff_increases_exponentially_and_is_capped() {
    let base = 60u64;
    let max = 3600u64;

    let d1 = compute_backoff_delay(1, base, max, None);
    let d2 = compute_backoff_delay(2, base, max, None);
    let d3 = compute_backoff_delay(3, base, max, None);
    let d_large = compute_backoff_delay(20, base, max, None);

    // Each step must be >= the previous (monotone with jitter).
    assert!(d2 >= d1, "backoff must grow: d2={} d1={}", d2, d1);
    assert!(d3 >= d2, "backoff must grow: d3={} d2={}", d3, d2);
    // Large attempt must be capped (with 10% jitter the max is 3600 + 360 = 3960).
    assert!(
        d_large <= max + max / 10 + 1,
        "backoff must be capped, got {}",
        d_large
    );
    // First attempt must be at least base.
    assert!(d1 >= base, "first backoff must be >= base ({} < {})", d1, base);
}

#[test]
fn retry_after_header_takes_precedence_when_larger() {
    let delay_no_header = compute_backoff_delay(1, 60, 3600, None);
    let delay_large_header = compute_backoff_delay(1, 60, 3600, Some(999));
    assert_eq!(
        delay_large_header, 999,
        "Retry-After header must override computed delay when larger"
    );
    let delay_small_header = compute_backoff_delay(1, 60, 3600, Some(1));
    assert_eq!(
        delay_small_header, delay_no_header,
        "Retry-After smaller than computed must be ignored"
    );
}

#[test]
fn update_throttle_state_increments_count_and_sets_delay() {
    let mut state = ToolThrottleState {
        tool_id: "claude".into(),
        ..Default::default()
    };
    let info = RateLimitInfo {
        tool_id: "claude".into(),
        ..Default::default()
    };
    let now_ts = 1_000_000u64;
    let backoff = 120u64;

    update_throttle_state(&mut state, &info, backoff, now_ts);

    assert_eq!(state.consecutive_429_count, 1);
    assert_eq!(state.backoff_seconds, backoff);
    assert_eq!(state.throttled_until, now_ts + backoff);

    // Second E601.
    update_throttle_state(&mut state, &info, 240, now_ts + 1);
    assert_eq!(state.consecutive_429_count, 2);
    assert_eq!(state.throttled_until, now_ts + 1 + 240);
}

#[test]
fn is_tool_throttled_uses_epoch_comparison() {
    let epoch_future = far_future_epoch();
    let epoch_past = 1u64;

    let reg_future = make_throttle_registry("claude", epoch_future, 1);
    let reg_past = make_throttle_registry("claude", epoch_past, 1);
    let now = "2026-03-18T12:00:00Z";

    assert!(
        is_tool_throttled(&reg_future, "claude", now),
        "future throttled_until must be throttled"
    );
    assert!(
        !is_tool_throttled(&reg_past, "claude", now),
        "past throttled_until must not be throttled"
    );
    assert!(
        !is_tool_throttled(&reg_future, "codex", now),
        "unknown tool must not be throttled"
    );
}

#[test]
fn clear_throttle_state_resets_all_fields() {
    let mut state = ToolThrottleState {
        tool_id: "claude".into(),
        throttled_until: far_future_epoch(),
        consecutive_429_count: 3,
        backoff_seconds: 360,
        last_rate_limit_info: Some(RateLimitInfo {
            tool_id: "claude".into(),
            ..Default::default()
        }),
    };
    clear_throttle_state(&mut state);

    assert_eq!(state.consecutive_429_count, 0);
    assert_eq!(state.backoff_seconds, 0);
    assert_eq!(state.throttled_until, 0);
    assert!(state.last_rate_limit_info.is_none());
}

// ── Scenario 5: Fallback dispatch when primary is throttled ───────────

#[test]
fn fallback_dispatch_selects_next_tool_when_primary_throttled() {
    let registry = make_registry_with_tools(vec![
        make_todo_task("T1", "claude"),
        make_todo_task("T2", "claude"),
        make_todo_task("T3", "codex"),
    ]);

    let mut cfg = base_selector_config(vec!["claude", "codex"], 4);
    cfg.throttle_registry = make_throttle_registry("claude", far_future_epoch(), 2);
    cfg.rate_limit_fallback_enabled = true;

    let selected = select_next_ready_task_typed(&registry, &cfg)
        .expect("fallback dispatch must select a task");

    // When claude is throttled, the selector must fall back to codex.
    assert_eq!(
        selected.tool, "codex",
        "fallback tool must be codex when claude is throttled"
    );
    assert!(selected.is_fallback, "is_fallback must be true");
}

#[test]
fn fallback_disabled_does_not_filter_throttled_tools() {
    // When rate_limit_fallback_enabled=false, the throttle registry is NOT
    // used as a dispatch filter — the throttled tool is still selected.
    // Protection against re-dispatch happens via task.task_runtime.delayed_until
    // (is_task_delayed), not via the tool throttle registry alone.
    let registry = make_registry_with_tools(vec![make_todo_task("T1", "claude")]);

    let mut cfg = base_selector_config(vec!["claude"], 4);
    cfg.throttle_registry = make_throttle_registry("claude", far_future_epoch(), 1);
    cfg.rate_limit_fallback_enabled = false;

    let selected = select_next_ready_task_typed(&registry, &cfg);
    // The task IS dispatched (throttle filter bypassed); delayed_until on the
    // task itself is the actual guard used by the coordinator control-plane.
    assert!(
        selected.is_some(),
        "throttle filter is bypassed when rate_limit_fallback_enabled=false"
    );
    let s = selected.unwrap();
    assert_eq!(s.tool, "claude");
    assert!(!s.is_fallback, "must not be marked fallback when filter is disabled");
}

#[test]
fn recovery_clears_throttle_and_tool_becomes_eligible_again() {
    // Start with claude throttled.
    let mut throttle_reg = make_throttle_registry("claude", far_future_epoch(), 3);

    // Simulate successful completion: clear throttle.
    let state = throttle_reg.get_mut("claude").unwrap();
    clear_throttle_state(state);

    // Now claude should no longer be throttled.
    let now = "2026-03-18T12:00:00Z";
    assert!(
        !is_tool_throttled(&throttle_reg, "claude", now),
        "cleared tool must not be throttled"
    );

    let registry = make_registry_with_tools(vec![make_todo_task("T1", "claude")]);
    let mut cfg = base_selector_config(vec!["claude", "codex"], 4);
    cfg.throttle_registry = throttle_reg;
    cfg.rate_limit_fallback_enabled = true;

    let selected = select_next_ready_task_typed(&registry, &cfg)
        .expect("claude must be dispatchable after throttle cleared");

    assert_eq!(selected.tool, "claude");
    assert!(!selected.is_fallback, "primary tool must not be marked as fallback after recovery");
}

// ── Scenario 6: is_task_delayed ──────────────────────────────────────

#[test]
fn task_with_future_delayed_until_is_skipped() {
    let mut task = make_todo_task("T1", "claude");
    // ISO 8601 timestamp in the far future.
    task.task_runtime.delayed_until = Some("2099-01-01T00:00:00Z".to_string());

    let now = "2026-03-18T12:00:00Z";
    assert!(
        is_task_delayed(&task, now),
        "task with future delayed_until must be considered delayed"
    );
}

#[test]
fn task_with_past_delayed_until_is_eligible() {
    let mut task = make_todo_task("T1", "claude");
    task.task_runtime.delayed_until = Some("2000-01-01T00:00:00Z".to_string());

    let now = "2026-03-18T12:00:00Z";
    assert!(
        !is_task_delayed(&task, now),
        "task with past delayed_until must be eligible"
    );
}

#[test]
fn task_without_delayed_until_is_always_eligible() {
    let task = make_todo_task("T1", "claude");
    assert!(!is_task_delayed(&task, "2026-03-18T12:00:00Z"));
    assert!(!is_task_delayed(&task, ""));
}

// ── Scenario 7: next_throttle_expiry ─────────────────────────────────

#[test]
fn next_throttle_expiry_returns_earliest_non_zero_expiry() {
    let mut reg = ToolThrottleRegistry::default();
    // epoch 2000-01-01T00:00:00Z = 946_684_800
    reg.insert(
        "claude".into(),
        ToolThrottleState {
            tool_id: "claude".into(),
            throttled_until: 946_684_800,
            ..Default::default()
        },
    );
    // epoch 2099-01-01T00:00:00Z = 4_070_908_800
    reg.insert(
        "codex".into(),
        ToolThrottleState {
            tool_id: "codex".into(),
            throttled_until: far_future_epoch(),
            ..Default::default()
        },
    );

    let expiry = next_throttle_expiry(&reg).expect("must return earliest expiry");
    // Earliest is claude's epoch
    assert!(
        expiry.contains("2000"),
        "earliest expiry must be 2000-01-01, got {}",
        expiry
    );
}

#[test]
fn next_throttle_expiry_returns_none_when_registry_empty() {
    assert!(next_throttle_expiry(&ToolThrottleRegistry::default()).is_none());
}
