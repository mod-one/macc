//! Rate-limit error codes, types, and configuration helpers.
//!
//! # Error codes
//!
//! | Code | Meaning | Retry? |
//! |------|---------|--------|
//! | **E601** | Rate-limited (transient 429) — the tool API returned a throttle response | Yes (with backoff) |
//! | **E602** | Quota exhausted (hard limit) — the account/key has no remaining quota | No (blocks task) |

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

// ── Error code constants ────────────────────────────────────────────

/// Transient rate-limit / HTTP 429. Eligible for automatic retry with
/// exponential backoff.
pub const E601_RATE_LIMITED: &str = "E601";

/// Hard quota exhaustion. The tool's API key or account has no remaining
/// quota. Should NOT be auto-retried — requires human intervention or
/// tool fallback.
pub const E602_QUOTA_EXHAUSTED: &str = "E602";

// ── Data structs ────────────────────────────────────────────────────

/// Snapshot of a single rate-limit signal received from a tool.
#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq, Eq)]
pub struct RateLimitInfo {
    /// Which tool emitted the signal (e.g. `"claude"`, `"codex"`).
    pub tool_id: String,

    /// The MACC error code (`E601` or `E602`).
    #[serde(default)]
    pub error_code: String,

    /// Seconds the tool asked us to wait (from `Retry-After` header or
    /// equivalent). `None` when the signal carried no explicit delay.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub retry_after_seconds: Option<u64>,

    /// Unix-epoch timestamp (seconds) when the signal was recorded.
    #[serde(default)]
    pub detected_at: u64,

    /// Raw header or field that conveyed the limit (for diagnostics).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_header: Option<String>,
}

/// Accumulated throttle state for a single tool, maintained by the
/// coordinator across the lifetime of a run.
#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq, Eq)]
pub struct ToolThrottleState {
    /// Which tool this state belongs to.
    pub tool_id: String,

    /// Unix-epoch timestamp (seconds) until which new dispatches to this
    /// tool should be suppressed.
    #[serde(default)]
    pub throttled_until: u64,

    /// Rolling count of consecutive 429 (E601) signals. Reset to 0 on a
    /// successful dispatch.
    #[serde(default)]
    pub consecutive_429_count: u32,

    /// Current backoff delay in seconds (computed by the backoff engine).
    #[serde(default)]
    pub backoff_seconds: u64,

    /// The most recent rate-limit info that fed into this state.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_rate_limit_info: Option<RateLimitInfo>,
}

// ── Tool throttle registry ──────────────────────────────────────────

/// Runtime registry of per-tool throttle states, keyed by tool identifier.
/// Stored in `CoordinatorRunState` (volatile — not persisted to disk).
pub type ToolThrottleRegistry = BTreeMap<String, ToolThrottleState>;

/// Return `true` when the named tool's `throttled_until` epoch is still in the
/// future relative to `now` (an ISO 8601 / RFC 3339 string).
///
/// Returns `false` for unknown tools, an unset `throttled_until`, or an empty
/// `now` string (disables filtering).
pub fn is_tool_throttled(registry: &ToolThrottleRegistry, tool_id: &str, now: &str) -> bool {
    if now.is_empty() || tool_id.is_empty() {
        return false;
    }
    let Some(state) = registry.get(tool_id) else {
        return false;
    };
    if state.throttled_until == 0 {
        return false;
    }
    let now_epoch = chrono::DateTime::parse_from_rfc3339(now)
        .map(|dt| dt.timestamp() as u64)
        .unwrap_or(0);
    now_epoch < state.throttled_until
}

/// Return the earliest `throttled_until` across all throttled tools as an ISO
/// 8601 string, or `None` if no tools are currently throttled.
pub fn next_throttle_expiry(registry: &ToolThrottleRegistry) -> Option<String> {
    use chrono::TimeZone as _;
    registry
        .values()
        .filter(|s| s.throttled_until > 0)
        .map(|s| s.throttled_until)
        .min()
        .and_then(|epoch| {
            chrono::Utc
                .timestamp_opt(epoch as i64, 0)
                .single()
                .map(|dt| dt.to_rfc3339_opts(chrono::SecondsFormat::Secs, true))
        })
}

// ── Backoff engine ──────────────────────────────────────────────────

/// Compute exponential backoff delay in seconds.
///
/// Returns `min(base * 2^(attempt-1), max) + 10% deterministic jitter`.
/// If `retry_after` is `Some` and ≥ the computed value it takes precedence.
///
/// # Examples
/// ```
/// use macc_core::coordinator::rate_limit::compute_backoff_delay;
/// assert!(compute_backoff_delay(1, 30, 300, None) >= 30);
/// assert!(compute_backoff_delay(5, 30, 300, None) >= 300);
/// ```
pub fn compute_backoff_delay(
    attempt: usize,
    base_seconds: u64,
    max_seconds: u64,
    retry_after: Option<u64>,
) -> u64 {
    let shift = attempt.saturating_sub(1).min(62) as u32;
    let raw = base_seconds.saturating_mul(1u64 << shift);
    let capped = raw.min(max_seconds);
    // Deterministic 10% jitter — avoids thundering-herd without randomness.
    let delay = capped.saturating_add(capped / 10);
    // Honour the server's Retry-After header when it calls for a longer wait.
    retry_after.filter(|&ra| ra >= delay).unwrap_or(delay)
}

/// Apply a new rate-limit signal to the accumulated throttle state for a tool.
///
/// * `backoff`  — delay (seconds) from [`compute_backoff_delay`]
/// * `now_ts`   — current Unix-epoch timestamp (seconds)
pub fn update_throttle_state(
    state: &mut ToolThrottleState,
    info: &RateLimitInfo,
    backoff: u64,
    now_ts: u64,
) {
    state.consecutive_429_count = state.consecutive_429_count.saturating_add(1);
    state.backoff_seconds = backoff;
    state.throttled_until = now_ts.saturating_add(backoff);
    state.last_rate_limit_info = Some(info.clone());
}

/// Return `true` when a task's `delayed_until` timestamp is still in the future.
///
/// Uses lexicographic ISO 8601 comparison — valid as long as both `now` and
/// `delayed_until` use the same UTC suffix (`Z` or `+00:00`).  Returns `false`
/// when either timestamp is absent or empty (task is immediately eligible).
pub fn is_task_delayed(task: &crate::coordinator::model::Task, now: &str) -> bool {
    if now.is_empty() {
        return false;
    }
    match task.task_runtime.delayed_until.as_deref() {
        Some(delayed_until) if !delayed_until.is_empty() => delayed_until > now,
        _ => false,
    }
}

/// Reset throttle state after a successful dispatch to the tool.
pub fn clear_throttle_state(state: &mut ToolThrottleState) {
    state.consecutive_429_count = 0;
    state.backoff_seconds = 0;
    state.throttled_until = 0;
    state.last_rate_limit_info = None;
}

// ── Unit tests ──────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rate_limit_info_roundtrip() {
        let info = RateLimitInfo {
            tool_id: "claude".into(),
            error_code: E601_RATE_LIMITED.into(),
            retry_after_seconds: Some(60),
            detected_at: 1_700_000_000,
            source_header: Some("Retry-After".into()),
        };

        let json = serde_json::to_string(&info).expect("serialize");
        let back: RateLimitInfo = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(info, back);
    }

    #[test]
    fn rate_limit_info_default() {
        let info = RateLimitInfo::default();
        assert_eq!(info.tool_id, "");
        assert_eq!(info.error_code, "");
        assert_eq!(info.retry_after_seconds, None);
        assert_eq!(info.detected_at, 0);
        assert_eq!(info.source_header, None);
    }

    #[test]
    fn tool_throttle_state_roundtrip() {
        let state = ToolThrottleState {
            tool_id: "codex".into(),
            throttled_until: 1_700_001_000,
            consecutive_429_count: 3,
            backoff_seconds: 120,
            last_rate_limit_info: Some(RateLimitInfo {
                tool_id: "codex".into(),
                error_code: E601_RATE_LIMITED.into(),
                retry_after_seconds: Some(30),
                detected_at: 1_700_000_900,
                source_header: None,
            }),
        };

        let json = serde_json::to_string(&state).expect("serialize");
        let back: ToolThrottleState = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(state, back);
    }

    #[test]
    fn tool_throttle_state_default() {
        let state = ToolThrottleState::default();
        assert_eq!(state.tool_id, "");
        assert_eq!(state.throttled_until, 0);
        assert_eq!(state.consecutive_429_count, 0);
        assert_eq!(state.backoff_seconds, 0);
        assert_eq!(state.last_rate_limit_info, None);
    }

    #[test]
    fn tool_throttle_state_without_last_info_roundtrip() {
        let state = ToolThrottleState {
            tool_id: "gemini".into(),
            throttled_until: 0,
            consecutive_429_count: 0,
            backoff_seconds: 0,
            last_rate_limit_info: None,
        };

        let json = serde_json::to_string(&state).expect("serialize");
        // None fields should be absent
        assert!(!json.contains("last_rate_limit_info"));
        let back: ToolThrottleState = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(state, back);
    }

    #[test]
    fn error_code_constants() {
        assert_eq!(E601_RATE_LIMITED, "E601");
        assert_eq!(E602_QUOTA_EXHAUSTED, "E602");
    }

    // ── is_tool_throttled / next_throttle_expiry ────────────────────

    fn registry_with(tool_id: &str, throttled_until: u64) -> ToolThrottleRegistry {
        let mut reg = ToolThrottleRegistry::new();
        reg.insert(
            tool_id.to_string(),
            ToolThrottleState {
                tool_id: tool_id.to_string(),
                throttled_until,
                ..Default::default()
            },
        );
        reg
    }

    #[test]
    fn is_tool_throttled_returns_true_when_future() {
        // 2_000_000_000 epoch ≈ 2033-05-18, clearly after 2027-01-16
        let reg = registry_with("claude", 2_000_000_000);
        assert!(is_tool_throttled(&reg, "claude", "2027-01-16T00:00:00Z"));
    }

    #[test]
    fn is_tool_throttled_returns_false_when_past() {
        let reg = registry_with("claude", 1_000_000_000);
        assert!(!is_tool_throttled(&reg, "claude", "2027-01-16T00:00:00Z"));
    }

    #[test]
    fn is_tool_throttled_returns_false_for_unknown_tool() {
        let reg = registry_with("claude", 9_999_999_999);
        assert!(!is_tool_throttled(&reg, "codex", "2020-01-01T00:00:00Z"));
    }

    #[test]
    fn is_tool_throttled_returns_false_for_empty_now() {
        let reg = registry_with("claude", 9_999_999_999);
        assert!(!is_tool_throttled(&reg, "claude", ""));
    }

    #[test]
    fn next_throttle_expiry_returns_earliest() {
        let mut reg = ToolThrottleRegistry::new();
        reg.insert(
            "a".into(),
            ToolThrottleState {
                tool_id: "a".into(),
                throttled_until: 2_000_000_000,
                ..Default::default()
            },
        );
        reg.insert(
            "b".into(),
            ToolThrottleState {
                tool_id: "b".into(),
                throttled_until: 1_500_000_000,
                ..Default::default()
            },
        );
        let expiry = next_throttle_expiry(&reg).unwrap();
        // 1_500_000_000 seconds epoch = 2017-07-14T02:40:00Z
        assert!(
            expiry.starts_with("2017-"),
            "expected 2017 epoch, got {}",
            expiry
        );
    }

    #[test]
    fn next_throttle_expiry_empty_registry() {
        assert!(next_throttle_expiry(&ToolThrottleRegistry::new()).is_none());
    }

    // ── compute_backoff_delay ────────────────────────────────────────

    #[test]
    fn backoff_attempt_1_base_30() {
        // 30 * 2^0 = 30, + 10% = 33
        assert_eq!(compute_backoff_delay(1, 30, 300, None), 33);
    }

    #[test]
    fn backoff_attempt_2_base_30() {
        // 30 * 2^1 = 60, + 10% = 66
        assert_eq!(compute_backoff_delay(2, 30, 300, None), 66);
    }

    #[test]
    fn backoff_attempt_3_base_30() {
        // 30 * 2^2 = 120, + 10% = 132
        assert_eq!(compute_backoff_delay(3, 30, 300, None), 132);
    }

    #[test]
    fn backoff_attempt_4_base_30() {
        // 30 * 2^3 = 240, + 10% = 264
        assert_eq!(compute_backoff_delay(4, 30, 300, None), 264);
    }

    #[test]
    fn backoff_attempt_5_capped_at_max() {
        // 30 * 2^4 = 480 → capped at 300, + 10% = 330
        assert_eq!(compute_backoff_delay(5, 30, 300, None), 330);
    }

    #[test]
    fn backoff_attempt_6_still_capped() {
        // 30 * 2^5 = 960 → capped at 300, + 10% = 330
        assert_eq!(compute_backoff_delay(6, 30, 300, None), 330);
    }

    #[test]
    fn backoff_retry_after_overrides_when_larger() {
        // Computed = 66s, Retry-After = 120s → use 120s
        assert_eq!(compute_backoff_delay(2, 30, 300, Some(120)), 120);
    }

    #[test]
    fn backoff_retry_after_ignored_when_smaller() {
        // Computed = 66s, Retry-After = 30s → use computed 66s
        assert_eq!(compute_backoff_delay(2, 30, 300, Some(30)), 66);
    }

    // ── update_throttle_state / clear_throttle_state ─────────────────

    #[test]
    fn update_throttle_state_increments_count_and_sets_fields() {
        let mut state = ToolThrottleState::default();
        let info = RateLimitInfo {
            tool_id: "claude".into(),
            error_code: E601_RATE_LIMITED.into(),
            retry_after_seconds: None,
            detected_at: 1_000_000,
            source_header: None,
        };
        update_throttle_state(&mut state, &info, 60, 1_000_000);
        assert_eq!(state.consecutive_429_count, 1);
        assert_eq!(state.backoff_seconds, 60);
        assert_eq!(state.throttled_until, 1_000_060);
        assert!(state.last_rate_limit_info.is_some());
        assert_eq!(
            state.last_rate_limit_info.unwrap().error_code,
            E601_RATE_LIMITED
        );
    }

    #[test]
    fn update_throttle_state_accumulates_consecutive_count() {
        let mut state = ToolThrottleState::default();
        let info = RateLimitInfo {
            tool_id: "codex".into(),
            error_code: E601_RATE_LIMITED.into(),
            retry_after_seconds: None,
            detected_at: 0,
            source_header: None,
        };
        update_throttle_state(&mut state, &info, 33, 0);
        update_throttle_state(&mut state, &info, 66, 33);
        assert_eq!(state.consecutive_429_count, 2);
        assert_eq!(state.backoff_seconds, 66);
        assert_eq!(state.throttled_until, 99);
    }

    // ── is_task_delayed ──────────────────────────────────────────────

    fn task_with_delayed_until(delayed_until: Option<&str>) -> crate::coordinator::model::Task {
        use crate::coordinator::model::TaskRuntime;
        let mut task = crate::coordinator::model::Task::default();
        task.task_runtime = TaskRuntime {
            delayed_until: delayed_until.map(|s| s.to_string()),
            ..Default::default()
        };
        task
    }

    #[test]
    fn is_task_delayed_future_timestamp_returns_true() {
        let task = task_with_delayed_until(Some("2026-03-18T12:05:00Z"));
        assert!(is_task_delayed(&task, "2026-03-18T12:00:00Z"));
    }

    #[test]
    fn is_task_delayed_past_timestamp_returns_false() {
        let task = task_with_delayed_until(Some("2026-03-18T11:55:00Z"));
        assert!(!is_task_delayed(&task, "2026-03-18T12:00:00Z"));
    }

    #[test]
    fn is_task_delayed_no_delayed_until_returns_false() {
        let task = task_with_delayed_until(None);
        assert!(!is_task_delayed(&task, "2026-03-18T12:00:00Z"));
    }

    #[test]
    fn is_task_delayed_empty_now_returns_false() {
        let task = task_with_delayed_until(Some("9999-12-31T23:59:59Z"));
        assert!(!is_task_delayed(&task, ""));
    }

    #[test]
    fn is_task_delayed_exact_match_not_delayed() {
        // When delayed_until == now, the window has expired → eligible.
        let task = task_with_delayed_until(Some("2026-03-18T12:00:00Z"));
        assert!(!is_task_delayed(&task, "2026-03-18T12:00:00Z"));
    }

    #[test]
    fn clear_throttle_state_resets_without_touching_tool_id() {
        let mut state = ToolThrottleState {
            tool_id: "claude".into(),
            throttled_until: 9_999,
            consecutive_429_count: 5,
            backoff_seconds: 120,
            last_rate_limit_info: Some(RateLimitInfo::default()),
        };
        clear_throttle_state(&mut state);
        assert_eq!(state.consecutive_429_count, 0);
        assert_eq!(state.backoff_seconds, 0);
        assert_eq!(state.throttled_until, 0);
        assert!(state.last_rate_limit_info.is_none());
        assert_eq!(state.tool_id, "claude");
    }
}
