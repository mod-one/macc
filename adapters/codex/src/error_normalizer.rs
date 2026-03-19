//! Codex/OpenAI-specific error normalizer.
//!
//! Pattern-matches on stderr/stdout text produced by the `codex` CLI to
//! classify errors into the canonical [`CanonicalClass`] model.
//!
//! # The 429 ambiguity
//!
//! OpenAI uses HTTP 429 for **both** transient rate-limits and hard quota
//! exhaustion. The message body is the discriminant:
//! - "Rate limit" / `RateLimitError` → transient, retryable with backoff
//! - "insufficient_quota" / "budget exceeded" / "plan" → hard cap, not retryable
//!
//! This is the core reason MACC cannot use a single `if status == 429` check.
//!
//! # Pattern priority
//!
//! 1. Quota exhaustion (429 + quota/budget keywords) — non-retryable, before generic 429
//! 2. Authentication / invalid key — non-retryable
//! 3. Permission errors — non-retryable
//! 4. Billing errors — non-retryable
//! 5. Websocket session errors — retryable with new session
//! 6. 503 "Slow Down" / overloaded — retryable
//! 7. Rate limit (429 / RateLimitError) — retryable
//! 8. Network errors (APIConnectionError) — retryable
//! 9. Timeout errors (APITimeoutError) — retryable
//! 10. Internal server errors (500) — retryable

use std::sync::OnceLock;

use macc_adapter_shared::error_normalizer::{
    canonical_to_error_code, is_retryable, is_user_action_required, truncate_raw_message,
    CanonicalClass, ErrorNormalizer, ToolError,
};
use regex::Regex;

/// Codex/OpenAI-specific implementation of [`ErrorNormalizer`].
pub struct CodexErrorNormalizer;

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
            // Quota exhaustion — must precede generic 429/RateLimitError
            Pattern {
                regex: Regex::new(
                    r"(?i)(insufficient_quota|budget.exceeded|exceeded.your.current.quota|plan.limit|usage.limit)",
                )
                .unwrap(),
                class: CanonicalClass::QuotaExhausted,
            },
            // Authentication
            Pattern {
                regex: Regex::new(
                    r"(?i)(AuthenticationError|invalid_api_key|invalid.api.key|Incorrect API key)",
                )
                .unwrap(),
                class: CanonicalClass::Auth,
            },
            // Permission
            Pattern {
                regex: Regex::new(r"(?i)(PermissionError|permission_denied|not.permitted)")
                    .unwrap(),
                class: CanonicalClass::PolicyViolation,
            },
            // Billing
            Pattern {
                regex: Regex::new(
                    r"(?i)(billing|payment.required|account.deactivated|account.suspended)",
                )
                .unwrap(),
                class: CanonicalClass::Billing,
            },
            // ── Retryable ──────────────────────────────────────────
            // Websocket session errors (OpenAI Realtime / Codex specific)
            Pattern {
                regex: Regex::new(
                    r"(?i)(previous_response_not_found|websocket_connection_limit_reached|session.already.in.use)",
                )
                .unwrap(),
                class: CanonicalClass::SessionConflict,
            },
            // 503 "Slow Down" — surge protection / overloaded
            Pattern {
                regex: Regex::new(r"(?i)(503.*slow\s*down|slow\s*down.*503|overloaded)").unwrap(),
                class: CanonicalClass::Overloaded,
            },
            // Rate limit (429 / RateLimitError / "rate limit")
            Pattern {
                regex: Regex::new(
                    r"(?i)(429|RateLimitError|rate.limit|too many requests)",
                )
                .unwrap(),
                class: CanonicalClass::RateLimit,
            },
            // Network (APIConnectionError, connection errors)
            Pattern {
                regex: Regex::new(
                    r"(?i)(APIConnectionError|ECONNREFUSED|ECONNRESET|connection.refused|connection.reset|DNS|getaddrinfo|network.error)",
                )
                .unwrap(),
                class: CanonicalClass::Network,
            },
            // Timeout (APITimeoutError)
            Pattern {
                regex: Regex::new(
                    r"(?i)(APITimeoutError|timeout|timed?\s*out|DEADLINE_EXCEEDED)",
                )
                .unwrap(),
                class: CanonicalClass::Timeout,
            },
            // Internal server error (500 / InternalServerError / server_error)
            Pattern {
                regex: Regex::new(
                    r"(?i)(500\s+internal|InternalServerError|server_error)",
                )
                .unwrap(),
                class: CanonicalClass::Internal,
            },
            // Generic 503 (without "Slow Down" — still overloaded)
            Pattern {
                regex: Regex::new(r"(?i)503\s+(service\s+unavailable)?").unwrap(),
                class: CanonicalClass::Overloaded,
            },
        ]
    })
}

/// Regex for extracting OpenAI request IDs (e.g. `req_abc123...`).
fn request_id_regex() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"req_[a-zA-Z0-9]{10,}").unwrap())
}

/// Regex for extracting `retry-after` hints from text.
fn retry_after_regex() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"(?i)(?:retry.after|retry_after)\s*[:=]\s*(\d+)").unwrap())
}

impl ErrorNormalizer for CodexErrorNormalizer {
    fn normalize(&self, exit_code: i32, stderr: &str, stdout: &str) -> Option<ToolError> {
        let combined = format!("{}\n{}", stderr, stdout);

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

        let class = match matched_class {
            Some(c) => c,
            None if exit_code != 0 => CanonicalClass::Unknown,
            None => return None,
        };

        let request_id = request_id_regex()
            .find(&combined)
            .map(|m| m.as_str().to_string());

        let retry_after_seconds = retry_after_regex()
            .captures(&combined)
            .and_then(|caps| caps.get(1))
            .and_then(|m| m.as_str().parse::<u64>().ok());

        let error_code = canonical_to_error_code(&class).to_string();
        let retryable = is_retryable(&class);
        let user_action_required = is_user_action_required(&class);
        let raw_message = truncate_raw_message(combined.trim());

        Some(ToolError {
            provider: "codex".into(),
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

    fn norm(exit_code: i32, stderr: &str, stdout: &str) -> Option<ToolError> {
        CodexErrorNormalizer.normalize(exit_code, stderr, stdout)
    }

    // ── 429 rate-limit vs quota ─────────────────────────────────────

    #[test]
    fn rate_limit_429() {
        let stderr = "Error: 429 Rate limit reached for default-model";
        let err = norm(1, stderr, "").unwrap();
        assert_eq!(err.canonical_class, CanonicalClass::RateLimit);
        assert!(err.retryable);
        assert!(!err.user_action_required);
        assert_eq!(err.error_code, "E601");
        assert_eq!(err.provider, "codex");
    }

    #[test]
    fn rate_limit_error_sdk_type() {
        let stderr = "openai.RateLimitError: Rate limit exceeded";
        let err = norm(1, stderr, "").unwrap();
        assert_eq!(err.canonical_class, CanonicalClass::RateLimit);
        assert!(err.retryable);
    }

    #[test]
    fn quota_exhausted_insufficient_quota() {
        let stderr = r#"Error: 429 {"error":{"message":"You exceeded your current quota, please check your plan and billing details.","type":"insufficient_quota","code":"insufficient_quota"}}"#;
        let err = norm(1, stderr, "").unwrap();
        assert_eq!(err.canonical_class, CanonicalClass::QuotaExhausted);
        assert!(!err.retryable);
        assert!(err.user_action_required);
        assert_eq!(err.error_code, "E602");
    }

    #[test]
    fn quota_exhausted_budget_exceeded() {
        let stderr = "Error: budget exceeded for this project";
        let err = norm(1, stderr, "").unwrap();
        assert_eq!(err.canonical_class, CanonicalClass::QuotaExhausted);
        assert!(!err.retryable);
        assert!(err.user_action_required);
    }

    #[test]
    fn quota_takes_priority_over_429() {
        // Both "429" and "insufficient_quota" present — quota wins
        let stderr = "429 You exceeded your current quota: insufficient_quota";
        let err = norm(1, stderr, "").unwrap();
        assert_eq!(err.canonical_class, CanonicalClass::QuotaExhausted);
        assert!(!err.retryable);
    }

    // ── 503 Slow Down ───────────────────────────────────────────────

    #[test]
    fn overloaded_503_slow_down() {
        let stderr = "Error: 503 Service Unavailable: Slow Down";
        let err = norm(1, stderr, "").unwrap();
        assert_eq!(err.canonical_class, CanonicalClass::Overloaded);
        assert!(err.retryable);
        assert!(!err.user_action_required);
        assert_eq!(err.error_code, "E601");
    }

    #[test]
    fn overloaded_generic_503() {
        let stderr = "Error: 503 Service Unavailable";
        let err = norm(1, stderr, "").unwrap();
        assert_eq!(err.canonical_class, CanonicalClass::Overloaded);
        assert!(err.retryable);
    }

    // ── Authentication ──────────────────────────────────────────────

    #[test]
    fn authentication_error_sdk() {
        let stderr = "openai.AuthenticationError: Incorrect API key provided: sk-...xxxx";
        let err = norm(1, stderr, "").unwrap();
        assert_eq!(err.canonical_class, CanonicalClass::Auth);
        assert!(!err.retryable);
        assert!(err.user_action_required);
        assert_eq!(err.error_code, "E201");
    }

    #[test]
    fn invalid_api_key() {
        let stderr = r#"{"error":{"message":"Invalid API key","type":"invalid_api_key"}}"#;
        let err = norm(1, stderr, "").unwrap();
        assert_eq!(err.canonical_class, CanonicalClass::Auth);
        assert!(!err.retryable);
    }

    // ── Network / Connection ────────────────────────────────────────

    #[test]
    fn api_connection_error() {
        let stderr = "openai.APIConnectionError: Connection error.";
        let err = norm(1, stderr, "").unwrap();
        assert_eq!(err.canonical_class, CanonicalClass::Network);
        assert!(err.retryable);
        assert!(!err.user_action_required);
    }

    #[test]
    fn connection_refused() {
        let stderr = "Error: ECONNREFUSED 127.0.0.1:443";
        let err = norm(1, stderr, "").unwrap();
        assert_eq!(err.canonical_class, CanonicalClass::Network);
        assert!(err.retryable);
    }

    // ── Timeout ─────────────────────────────────────────────────────

    #[test]
    fn api_timeout_error() {
        let stderr = "openai.APITimeoutError: Request timed out.";
        let err = norm(1, stderr, "").unwrap();
        assert_eq!(err.canonical_class, CanonicalClass::Timeout);
        assert!(err.retryable);
    }

    // ── Internal server error ───────────────────────────────────────

    #[test]
    fn internal_server_error_500() {
        let stderr = "openai.InternalServerError: Internal server error (500)";
        let err = norm(1, stderr, "").unwrap();
        assert_eq!(err.canonical_class, CanonicalClass::Internal);
        assert!(err.retryable);
    }

    #[test]
    fn server_error_type() {
        let stderr = r#"{"error":{"type":"server_error","message":"Internal error"}}"#;
        let err = norm(1, stderr, "").unwrap();
        assert_eq!(err.canonical_class, CanonicalClass::Internal);
        assert!(err.retryable);
    }

    // ── Websocket / session errors ──────────────────────────────────

    #[test]
    fn websocket_previous_response_not_found() {
        let stderr = "Error: previous_response_not_found - The response was not found";
        let err = norm(1, stderr, "").unwrap();
        assert_eq!(err.canonical_class, CanonicalClass::SessionConflict);
        assert!(err.retryable);
        assert!(!err.user_action_required);
        assert_eq!(err.error_code, "E603");
    }

    #[test]
    fn websocket_connection_limit() {
        let stderr = "Error: websocket_connection_limit_reached";
        let err = norm(1, stderr, "").unwrap();
        assert_eq!(err.canonical_class, CanonicalClass::SessionConflict);
        assert!(err.retryable);
    }

    // ── Permission ──────────────────────────────────────────────────

    #[test]
    fn permission_error() {
        let stderr = "openai.PermissionError: You are not permitted to access this resource";
        let err = norm(1, stderr, "").unwrap();
        assert_eq!(err.canonical_class, CanonicalClass::PolicyViolation);
        assert!(!err.retryable);
        assert!(err.user_action_required);
    }

    // ── Billing ─────────────────────────────────────────────────────

    #[test]
    fn billing_error() {
        let stderr = "Error: account deactivated - please update billing";
        let err = norm(1, stderr, "").unwrap();
        assert_eq!(err.canonical_class, CanonicalClass::Billing);
        assert!(!err.retryable);
        assert!(err.user_action_required);
    }

    // ── Request ID extraction ───────────────────────────────────────

    #[test]
    fn extracts_request_id() {
        let stderr = r#"Error: 429 Rate limit {"request_id":"req_01abcdef1234567890"}"#;
        let err = norm(1, stderr, "").unwrap();
        assert_eq!(err.request_id, Some("req_01abcdef1234567890".to_string()));
    }

    #[test]
    fn no_request_id_when_absent() {
        let stderr = "Error: 429 Too Many Requests";
        let err = norm(1, stderr, "").unwrap();
        assert_eq!(err.request_id, None);
    }

    // ── Retry-after extraction ──────────────────────────────────────

    #[test]
    fn extracts_retry_after() {
        let stderr = "429 Rate limit reached. retry-after: 60";
        let err = norm(1, stderr, "").unwrap();
        assert_eq!(err.retry_after_seconds, Some(60));
    }

    // ── Unknown / no error ──────────────────────────────────────────

    #[test]
    fn unknown_error_nonzero_exit() {
        let stderr = "Something completely unexpected happened";
        let err = norm(1, stderr, "").unwrap();
        assert_eq!(err.canonical_class, CanonicalClass::Unknown);
        assert!(!err.retryable);
        assert_eq!(err.error_code, "E901");
    }

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
        let long_stderr = format!("Error: 429 rate limit {}", "x".repeat(600));
        let err = norm(1, &long_stderr, "").unwrap();
        assert!(err.raw_message.len() <= 503);
    }
}
