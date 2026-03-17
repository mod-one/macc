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
}
