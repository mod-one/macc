//! Rate-limit error codes, types, and configuration helpers.
//!
//! # Error codes
//!
//! | Code | Meaning | Retry? |
//! |------|---------|--------|
//! | **E601** | Rate-limited (transient 429) — the tool API returned a throttle response | Yes (with backoff) |
//! | **E602** | Quota exhausted (hard limit) — the account/key has no remaining quota | No (blocks task) |

use serde::{Deserialize, Serialize};

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
#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq)]
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
#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq)]
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
