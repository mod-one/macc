//! Claude-specific error normalizer.
//!
//! Pattern-matches on stderr/stdout text produced by the `claude` CLI to
//! classify errors into the canonical [`CanonicalClass`] model.
//!
//! # Pattern priority
//!
//! Patterns are checked in a specific order because some errors overlap:
//! 1. Quota exhaustion ("hit your limit") — non-retryable, before generic 429
//! 2. Authentication/permission errors — non-retryable
//! 3. Billing/payment errors — non-retryable
//! 4. Session conflict — retryable with new session
//! 5. Overloaded (529 / overloaded_error) — retryable
//! 6. Rate limit (429 / rate_limit_error) — retryable
//! 7. Network errors — retryable
//! 8. Internal errors (500) — retryable

use std::sync::OnceLock;

use macc_adapter_shared::error_normalizer::{
    canonical_to_error_code, is_retryable, is_user_action_required, truncate_raw_message,
    CanonicalClass, ErrorNormalizer, ToolError,
};
use regex::Regex;

/// Claude-specific implementation of [`ErrorNormalizer`].
pub struct ClaudeErrorNormalizer;

/// A compiled pattern and its associated canonical class.
struct Pattern {
    regex: Regex,
    class: CanonicalClass,
}

/// Get the compiled pattern list (initialized once).
fn patterns() -> &'static Vec<Pattern> {
    static PATTERNS: OnceLock<Vec<Pattern>> = OnceLock::new();
    PATTERNS.get_or_init(|| {
        vec![
            // ── Non-retryable (check first) ────────────────────────
            // Quota / usage cap — must precede generic 429
            Pattern {
                regex: Regex::new(r"(?i)(hit your limit|usage limit|usage cap|resets \d+[ap]m)")
                    .unwrap(),
                class: CanonicalClass::QuotaExhausted,
            },
            // Authentication
            Pattern {
                regex: Regex::new(r"(?i)(authentication_error|invalid.api.key|invalid_api_key)")
                    .unwrap(),
                class: CanonicalClass::Auth,
            },
            // Permission / policy
            Pattern {
                regex: Regex::new(r"(?i)permission_error").unwrap(),
                class: CanonicalClass::PolicyViolation,
            },
            // Billing
            Pattern {
                regex: Regex::new(
                    r"(?i)(billing|payment.required|insufficient.credit|account.suspended)",
                )
                .unwrap(),
                class: CanonicalClass::Billing,
            },
            // ── Retryable ──────────────────────────────────────────
            // Session conflict
            Pattern {
                regex: Regex::new(r"(?i)session\s+id\s+\S+\s+is\s+already\s+in\s+use").unwrap(),
                class: CanonicalClass::SessionConflict,
            },
            // Overloaded (529 or overloaded_error without specific status)
            Pattern {
                regex: Regex::new(r"(?i)(529|overloaded_error|overloaded)").unwrap(),
                class: CanonicalClass::Overloaded,
            },
            // Rate limit (429 / rate_limit_error)
            Pattern {
                regex: Regex::new(r"(?i)(429|rate_limit_error|rate.limit|too many requests)")
                    .unwrap(),
                class: CanonicalClass::RateLimit,
            },
            // Network
            Pattern {
                regex: Regex::new(
                    r"(?i)(ECONNREFUSED|ECONNRESET|ETIMEDOUT|DNS|network.error|connection.refused|connection.reset|getaddrinfo)",
                )
                .unwrap(),
                class: CanonicalClass::Network,
            },
            // Timeout
            Pattern {
                regex: Regex::new(r"(?i)(timeout|timed?\s*out|DEADLINE_EXCEEDED)").unwrap(),
                class: CanonicalClass::Timeout,
            },
            // Internal server error
            Pattern {
                regex: Regex::new(r"(?i)(500\s+internal|internal_server_error|server_error)")
                    .unwrap(),
                class: CanonicalClass::Internal,
            },
        ]
    })
}

/// Regex for extracting Claude request IDs (e.g. `req_011CZ9GZzDNEuAo5ALcxo3UK`).
fn request_id_regex() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"req_[a-zA-Z0-9]{10,}").unwrap())
}

/// Regex for extracting `retry-after` or reset-time hints from text.
fn retry_after_regex() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"(?i)(?:retry.after|retry_after)\s*[:=]\s*(\d+)").unwrap())
}

/// Regex for extracting "resets Xam/pm (UTC)" from quota messages.
fn resets_time_regex() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"(?i)resets\s+(\d{1,2})(am|pm)\s*\(UTC\)").unwrap())
}

/// Parse "resets Xam/pm (UTC)" into seconds from now until that reset time.
/// Returns None if the pattern is not found or the time is invalid.
fn parse_reset_time_as_seconds(text: &str) -> Option<u64> {
    let caps = resets_time_regex().captures(text)?;
    let hour_12: u32 = caps.get(1)?.as_str().parse().ok()?;
    let ampm = caps.get(2)?.as_str().to_ascii_lowercase();

    let hour_24 = match (hour_12, ampm.as_str()) {
        (12, "am") => 0,
        (12, "pm") => 12,
        (h, "am") if h <= 11 => h,
        (h, "pm") if h <= 11 => h + 12,
        _ => return None,
    };

    let now = chrono::Utc::now();
    let today_reset = now
        .date_naive()
        .and_hms_opt(hour_24, 0, 0)?;
    let today_reset_utc = today_reset.and_utc();

    // If reset time is in the past, it means tomorrow.
    let reset = if today_reset_utc <= now {
        today_reset_utc + chrono::Duration::days(1)
    } else {
        today_reset_utc
    };

    let diff = (reset - now).num_seconds();
    if diff > 0 { Some(diff as u64) } else { None }
}

impl ErrorNormalizer for ClaudeErrorNormalizer {
    fn normalize(&self, exit_code: i32, stderr: &str, stdout: &str) -> Option<ToolError> {
        // Combine both streams — Claude can emit errors to either, and SSE
        // stream failures appear in stdout after a successful HTTP 200.
        let combined = format!("{}\n{}", stderr, stdout);

        // Fast path: if both streams are empty or trivially short, nothing to classify.
        if combined.trim().is_empty() {
            return None;
        }

        // Run patterns in priority order, return on first match.
        let mut matched_class: Option<CanonicalClass> = None;
        for pat in patterns() {
            if pat.regex.is_match(&combined) {
                matched_class = Some(pat.class.clone());
                break;
            }
        }

        // If no pattern matched but exit code is non-zero, return Unknown.
        // If exit code is 0 and no pattern matched, there's no error.
        let class = match matched_class {
            Some(c) => c,
            None if exit_code != 0 => CanonicalClass::Unknown,
            None => return None,
        };

        // Extract request_id if present.
        let request_id = request_id_regex()
            .find(&combined)
            .map(|m| m.as_str().to_string());

        // Extract retry-after hint if present.  First try the standard
        // "retry-after: N" header, then fall back to parsing the human-readable
        // "resets Xam/pm (UTC)" reset time from quota exhaustion messages.
        let retry_after_seconds = retry_after_regex()
            .captures(&combined)
            .and_then(|caps| caps.get(1))
            .and_then(|m| m.as_str().parse::<u64>().ok())
            .or_else(|| parse_reset_time_as_seconds(&combined));

        let error_code = canonical_to_error_code(&class).to_string();
        let retryable = is_retryable(&class);
        let user_action_required = is_user_action_required(&class);
        let raw_message = truncate_raw_message(combined.trim());

        Some(ToolError {
            provider: "claude".into(),
            canonical_class: class,
            retryable,
            retry_after_seconds,
            user_action_required,
            raw_message,
            error_code,
            request_id,
            attempt: 0,
            operation: String::new(),
        })
    }
}

// ── Unit tests ──────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Timelike;

    fn norm(exit_code: i32, stderr: &str, stdout: &str) -> Option<ToolError> {
        ClaudeErrorNormalizer.normalize(exit_code, stderr, stdout)
    }

    // ── 529 / Overloaded ────────────────────────────────────────────

    #[test]
    fn overloaded_529_stderr() {
        let stderr = r#"API Error: 529 {"type":"error","error":{"type":"overloaded_error","message":"Overloaded"}}"#;
        let err = norm(1, stderr, "").unwrap();
        assert_eq!(err.canonical_class, CanonicalClass::Overloaded);
        assert!(err.retryable);
        assert!(!err.user_action_required);
        assert_eq!(err.error_code, "E601");
        assert_eq!(err.provider, "claude");
    }

    #[test]
    fn overloaded_error_without_529() {
        let stderr =
            r#"{"type":"error","error":{"type":"overloaded_error","message":"Overloaded"}}"#;
        let err = norm(1, stderr, "").unwrap();
        assert_eq!(err.canonical_class, CanonicalClass::Overloaded);
        assert!(err.retryable);
    }

    // ── 429 / Rate limit ────────────────────────────────────────────

    #[test]
    fn rate_limit_429() {
        let stderr = "Error: 429 Too Many Requests";
        let err = norm(1, stderr, "").unwrap();
        assert_eq!(err.canonical_class, CanonicalClass::RateLimit);
        assert!(err.retryable);
        assert!(!err.user_action_required);
        assert_eq!(err.error_code, "E601");
    }

    #[test]
    fn rate_limit_error_type() {
        let stderr =
            r#"{"type":"error","error":{"type":"rate_limit_error","message":"Rate limited"}}"#;
        let err = norm(1, stderr, "").unwrap();
        assert_eq!(err.canonical_class, CanonicalClass::RateLimit);
        assert!(err.retryable);
    }

    #[test]
    fn rate_limit_with_retry_after() {
        let stderr = "429 Too Many Requests\nretry-after: 30";
        let err = norm(1, stderr, "").unwrap();
        assert_eq!(err.canonical_class, CanonicalClass::RateLimit);
        assert_eq!(err.retry_after_seconds, Some(30));
    }

    // ── Quota exhausted ─────────────────────────────────────────────

    #[test]
    fn quota_exhausted_hit_limit() {
        let stderr = "You've hit your limit · resets 8pm (UTC)";
        let err = norm(1, stderr, "").unwrap();
        assert_eq!(err.canonical_class, CanonicalClass::QuotaExhausted);
        assert!(!err.retryable);
        assert!(err.user_action_required);
        assert_eq!(err.error_code, "E602");
        // retry_after_seconds should be computed from "resets 8pm (UTC)"
        assert!(
            err.retry_after_seconds.is_some(),
            "should extract reset time from 'resets 8pm (UTC)'"
        );
    }

    #[test]
    fn quota_exhausted_usage_limit() {
        let stderr = "Error: usage limit exceeded for your plan";
        let err = norm(1, stderr, "").unwrap();
        assert_eq!(err.canonical_class, CanonicalClass::QuotaExhausted);
        assert!(!err.retryable);
        assert!(err.user_action_required);
    }

    #[test]
    fn quota_exhausted_out_of_extra_usage() {
        let stderr = "Error: Session ID 9778ec72-16da-483e-9dc7-a09d593cc566 is already in use. You're out of extra usage · resets 10pm (UTC)";
        let err = norm(1, stderr, "").unwrap();
        // "resets 10pm" should match the quota pattern before session conflict
        assert_eq!(err.canonical_class, CanonicalClass::QuotaExhausted);
        assert!(
            err.retry_after_seconds.is_some(),
            "should extract reset time from 'resets 10pm (UTC)'"
        );
    }

    #[test]
    fn parse_reset_time_produces_positive_seconds() {
        // Use a future time to guarantee a positive result
        let now_utc = chrono::Utc::now();
        // Pick an hour that is 2 hours from now (wraps around midnight)
        let future_hour = ((now_utc.hour() + 2) % 24) as u32;
        let (h12, ampm) = if future_hour == 0 {
            (12, "am")
        } else if future_hour < 12 {
            (future_hour, "am")
        } else if future_hour == 12 {
            (12, "pm")
        } else {
            (future_hour - 12, "pm")
        };
        let text = format!("resets {}{}  (UTC)", h12, ampm);
        let secs = parse_reset_time_as_seconds(&text);
        assert!(secs.is_some(), "should parse '{}'", text);
        let secs = secs.unwrap();
        // Should be roughly 2 hours (7200s) give or take minutes
        assert!(secs > 3600, "expected >3600s, got {}", secs);
        assert!(secs < 10800, "expected <10800s, got {}", secs);
    }

    #[test]
    fn parse_reset_time_no_match() {
        assert!(parse_reset_time_as_seconds("no reset info here").is_none());
    }

    #[test]
    fn quota_takes_priority_over_429() {
        // If both "hit your limit" and "429" appear, quota wins (checked first)
        let stderr = "429 You've hit your limit · resets 8pm (UTC)";
        let err = norm(1, stderr, "").unwrap();
        assert_eq!(err.canonical_class, CanonicalClass::QuotaExhausted);
        assert!(!err.retryable);
    }

    // ── Session conflict ────────────────────────────────────────────

    #[test]
    fn session_conflict() {
        let stderr =
            "Error: Session ID b2784509-a8e7-4a3a-b5f5-7e4d7c8e9f12 is already in use by another client.";
        let err = norm(1, stderr, "").unwrap();
        assert_eq!(err.canonical_class, CanonicalClass::SessionConflict);
        assert!(err.retryable);
        assert!(!err.user_action_required);
        assert_eq!(err.error_code, "E603");
    }

    // ── Authentication ──────────────────────────────────────────────

    #[test]
    fn authentication_error() {
        let stderr = r#"{"type":"error","error":{"type":"authentication_error","message":"Invalid API key"}}"#;
        let err = norm(1, stderr, "").unwrap();
        assert_eq!(err.canonical_class, CanonicalClass::Auth);
        assert!(!err.retryable);
        assert!(err.user_action_required);
        assert_eq!(err.error_code, "E201");
    }

    #[test]
    fn invalid_api_key() {
        let stderr = "Error: invalid_api_key - Your API key is not valid.";
        let err = norm(1, stderr, "").unwrap();
        assert_eq!(err.canonical_class, CanonicalClass::Auth);
        assert!(!err.retryable);
        assert!(err.user_action_required);
    }

    // ── Permission / policy ─────────────────────────────────────────

    #[test]
    fn permission_error() {
        let stderr =
            r#"{"type":"error","error":{"type":"permission_error","message":"Not allowed"}}"#;
        let err = norm(1, stderr, "").unwrap();
        assert_eq!(err.canonical_class, CanonicalClass::PolicyViolation);
        assert!(!err.retryable);
        assert!(err.user_action_required);
        assert_eq!(err.error_code, "E201");
    }

    // ── SSE stream failure (exit 0, error in stdout) ────────────────

    #[test]
    fn sse_stream_error_in_stdout() {
        // Claude can return HTTP 200 then fail mid-stream. Exit code is 0
        // but error text appears in stdout.
        let stdout = r#"Error during stream: {"type":"error","error":{"type":"overloaded_error","message":"Overloaded"}}"#;
        let err = norm(0, "", stdout).unwrap();
        assert_eq!(err.canonical_class, CanonicalClass::Overloaded);
        assert!(err.retryable);
    }

    // ── Request ID extraction ───────────────────────────────────────

    #[test]
    fn extracts_request_id() {
        let stderr = r#"API Error: 529 {"type":"error","error":{"type":"overloaded_error"},"request_id":"req_011CZ9GZzDNEuAo5ALcxo3UK"}"#;
        let err = norm(1, stderr, "").unwrap();
        assert_eq!(
            err.request_id,
            Some("req_011CZ9GZzDNEuAo5ALcxo3UK".to_string())
        );
    }

    #[test]
    fn no_request_id_when_absent() {
        let stderr = "Error: 429 Too Many Requests";
        let err = norm(1, stderr, "").unwrap();
        assert_eq!(err.request_id, None);
    }

    // ── Network errors ──────────────────────────────────────────────

    #[test]
    fn network_connection_refused() {
        let stderr = "Error: ECONNREFUSED 127.0.0.1:443";
        let err = norm(1, stderr, "").unwrap();
        assert_eq!(err.canonical_class, CanonicalClass::Network);
        assert!(err.retryable);
    }

    // ── Internal server error ───────────────────────────────────────

    #[test]
    fn internal_server_error() {
        let stderr = "API Error: 500 Internal Server Error";
        let err = norm(1, stderr, "").unwrap();
        assert_eq!(err.canonical_class, CanonicalClass::Internal);
        assert!(err.retryable);
    }

    // ── Unknown non-zero exit ───────────────────────────────────────

    #[test]
    fn unknown_error_nonzero_exit() {
        let stderr = "Something completely unexpected happened";
        let err = norm(1, stderr, "").unwrap();
        assert_eq!(err.canonical_class, CanonicalClass::Unknown);
        assert!(!err.retryable);
        assert!(!err.user_action_required);
        assert_eq!(err.error_code, "E901");
    }

    // ── No error (exit 0, clean output) ─────────────────────────────

    #[test]
    fn no_error_exit_zero_clean() {
        assert!(norm(0, "", "All tasks completed successfully").is_none());
    }

    #[test]
    fn no_error_empty_output() {
        assert!(norm(0, "", "").is_none());
    }

    // ── Raw message truncation ──────────────────────────────────────

    #[test]
    fn raw_message_is_truncated() {
        let long_stderr = format!("Error: 529 overloaded {}", "x".repeat(600));
        let err = norm(1, &long_stderr, "").unwrap();
        assert!(err.raw_message.len() <= 503); // 500 + "…" (3 bytes)
    }

    // ── Billing ─────────────────────────────────────────────────────

    #[test]
    fn billing_error() {
        let stderr = "Error: payment required - please update your billing information";
        let err = norm(1, stderr, "").unwrap();
        assert_eq!(err.canonical_class, CanonicalClass::Billing);
        assert!(!err.retryable);
        assert!(err.user_action_required);
    }

    // ── Timeout ─────────────────────────────────────────────────────

    #[test]
    fn timeout_error() {
        let stderr = "Error: Request timed out after 120 seconds";
        let err = norm(1, stderr, "").unwrap();
        assert_eq!(err.canonical_class, CanonicalClass::Timeout);
        assert!(err.retryable);
    }
}
