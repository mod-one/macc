//! Gemini/Vertex AI-specific error normalizer.
//!
//! Pattern-matches on stderr/stdout text produced by the `gemini` CLI to
//! classify errors into the canonical [`CanonicalClass`] model.
//!
//! # Google canonical error codes
//!
//! Vertex AI layers Google canonical error codes on top of HTTP status:
//! - `RESOURCE_EXHAUSTED` (429) — quota, rate-limit, or functional limit
//! - `UNAVAILABLE` (503) — transient service outage
//! - `DEADLINE_EXCEEDED` (504) — timeout
//! - `INTERNAL` (500) — server error
//! - `PERMISSION_DENIED` (403) — auth/permission
//! - `UNAUTHENTICATED` (401) — invalid credentials
//! - `INVALID_ARGUMENT` (400) — bad request
//! - `NOT_FOUND` (404) — model/endpoint not found
//!
//! # The RESOURCE_EXHAUSTED ambiguity
//!
//! 429 + RESOURCE_EXHAUSTED can mean three different things:
//! - Quota limit (e.g. "Quota exceeded") → `QuotaExhausted` (not retryable)
//! - Shared overload / rate-limit → `RateLimit` (retryable)
//! - Functional limit (e.g. context window) → `QuotaExhausted` (not retryable)
//!
//! The message body discriminates between these cases.

use std::sync::OnceLock;

use macc_adapter_shared::error_normalizer::{
    canonical_to_error_code, is_retryable, is_user_action_required, truncate_raw_message,
    CanonicalClass, ErrorNormalizer, ToolError,
};
use regex::Regex;

/// Gemini/Vertex AI-specific implementation of [`ErrorNormalizer`].
pub struct GeminiErrorNormalizer;

/// A compiled pattern and its associated canonical class.
struct Pattern {
    regex: Regex,
    class: CanonicalClass,
}

/// Get the compiled pattern list (initialized once).
///
/// Google canonical error codes are matched first (higher specificity),
/// then HTTP status codes as fallback.
fn patterns() -> &'static Vec<Pattern> {
    static PATTERNS: OnceLock<Vec<Pattern>> = OnceLock::new();
    PATTERNS.get_or_init(|| {
        vec![
            // ── RESOURCE_EXHAUSTED: quota vs rate-limit ────────────
            // Quota / functional limit — must precede generic RESOURCE_EXHAUSTED
            Pattern {
                regex: Regex::new(
                    r"(?i)RESOURCE_EXHAUSTED.{0,100}(quota|limit\s+per|exceeded.{0,30}(limit|cap)|tokens?.per|requests?.per)",
                )
                .unwrap(),
                class: CanonicalClass::QuotaExhausted,
            },
            // Generic RESOURCE_EXHAUSTED (no quota keywords) → rate-limit
            Pattern {
                regex: Regex::new(r"(?i)RESOURCE_EXHAUSTED").unwrap(),
                class: CanonicalClass::RateLimit,
            },
            // ── Non-retryable Google codes ──────────────────────────
            // Unauthenticated (401)
            Pattern {
                regex: Regex::new(r"(?i)(UNAUTHENTICATED|401\s+Unauthorized)").unwrap(),
                class: CanonicalClass::Auth,
            },
            // Permission denied (403)
            Pattern {
                regex: Regex::new(r"(?i)(PERMISSION_DENIED|403\s+Forbidden)").unwrap(),
                class: CanonicalClass::PolicyViolation,
            },
            // Invalid argument (400)
            Pattern {
                regex: Regex::new(r"(?i)(INVALID_ARGUMENT|400\s+Bad\s+Request)").unwrap(),
                class: CanonicalClass::OutputMalformed,
            },
            // Not found (404)
            Pattern {
                regex: Regex::new(r"(?i)(NOT_FOUND|404\s+Not\s+Found)").unwrap(),
                class: CanonicalClass::ToolNotFound,
            },
            // ── Retryable Google codes ──────────────────────────────
            // Unavailable (503)
            Pattern {
                regex: Regex::new(r"(?i)(UNAVAILABLE|503\s+Service\s+Unavailable)").unwrap(),
                class: CanonicalClass::Overloaded,
            },
            // Deadline exceeded (504)
            Pattern {
                regex: Regex::new(r"(?i)(DEADLINE_EXCEEDED|504\s+Gateway\s+Timeout)").unwrap(),
                class: CanonicalClass::Timeout,
            },
            // Internal (500)
            Pattern {
                regex: Regex::new(r"(?i)(INTERNAL|500\s+Internal\s+Server\s+Error)").unwrap(),
                class: CanonicalClass::Internal,
            },
            // ── Fallback patterns ──────────────────────────────────
            // Network errors
            Pattern {
                regex: Regex::new(
                    r"(?i)(ECONNREFUSED|ECONNRESET|ETIMEDOUT|DNS|network.error|connection.refused|getaddrinfo)",
                )
                .unwrap(),
                class: CanonicalClass::Network,
            },
            // Billing
            Pattern {
                regex: Regex::new(r"(?i)(billing|payment.required|account.suspended)").unwrap(),
                class: CanonicalClass::Billing,
            },
            // Generic 429 without RESOURCE_EXHAUSTED
            Pattern {
                regex: Regex::new(r"(?i)(429|too many requests|rate.limit)").unwrap(),
                class: CanonicalClass::RateLimit,
            },
        ]
    })
}

/// Regex for extracting request IDs from Vertex AI responses.
fn request_id_regex() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        // Vertex AI uses various request ID formats
        Regex::new(r#"(?:request_id|requestId|x-request-id)[":\s]+([a-zA-Z0-9_-]{10,})"#).unwrap()
    })
}

/// Regex for extracting `retry-after` hints.
fn retry_after_regex() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r#"(?i)(?:retry.after|retry_after)\s*[:=]\s*(\d+)"#).unwrap())
}

impl ErrorNormalizer for GeminiErrorNormalizer {
    fn normalize(&self, exit_code: i32, stderr: &str, stdout: &str) -> Option<ToolError> {
        let combined = format!("{}\n{}", stderr, stdout);

        if combined.trim().is_empty() {
            return None;
        }

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
            .captures(&combined)
            .and_then(|caps| caps.get(1))
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
            provider: "gemini".into(),
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
        GeminiErrorNormalizer.normalize(exit_code, stderr, stdout)
    }

    // ── RESOURCE_EXHAUSTED: quota vs rate-limit ─────────────────────

    #[test]
    fn resource_exhausted_quota() {
        let stderr = "429 RESOURCE_EXHAUSTED: Quota exceeded for aiplatform.googleapis.com/generate_content_requests_per_minute";
        let err = norm(1, stderr, "").unwrap();
        assert_eq!(err.canonical_class, CanonicalClass::QuotaExhausted);
        assert!(!err.retryable);
        assert!(err.user_action_required);
        assert_eq!(err.error_code, "E602");
        assert_eq!(err.provider, "gemini");
    }

    #[test]
    fn resource_exhausted_limit_per() {
        let stderr = "RESOURCE_EXHAUSTED: limit per minute exceeded";
        let err = norm(1, stderr, "").unwrap();
        assert_eq!(err.canonical_class, CanonicalClass::QuotaExhausted);
        assert!(!err.retryable);
    }

    #[test]
    fn resource_exhausted_tokens_per() {
        let stderr = "RESOURCE_EXHAUSTED: tokens per minute limit reached";
        let err = norm(1, stderr, "").unwrap();
        assert_eq!(err.canonical_class, CanonicalClass::QuotaExhausted);
        assert!(!err.retryable);
    }

    #[test]
    fn resource_exhausted_generic_rate_limit() {
        // No quota keywords → rate-limit (retryable)
        let stderr = "429 RESOURCE_EXHAUSTED: Rate limit for model";
        let err = norm(1, stderr, "").unwrap();
        assert_eq!(err.canonical_class, CanonicalClass::RateLimit);
        assert!(err.retryable);
        assert!(!err.user_action_required);
        assert_eq!(err.error_code, "E601");
    }

    #[test]
    fn resource_exhausted_bare() {
        let stderr = "RESOURCE_EXHAUSTED";
        let err = norm(1, stderr, "").unwrap();
        assert_eq!(err.canonical_class, CanonicalClass::RateLimit);
        assert!(err.retryable);
    }

    // ── UNAVAILABLE (503) ───────────────────────────────────────────

    #[test]
    fn unavailable() {
        let stderr = "503 UNAVAILABLE: The service is temporarily unavailable";
        let err = norm(1, stderr, "").unwrap();
        assert_eq!(err.canonical_class, CanonicalClass::Overloaded);
        assert!(err.retryable);
        assert!(!err.user_action_required);
        assert_eq!(err.error_code, "E601");
    }

    #[test]
    fn service_unavailable_http() {
        let stderr = "Error: 503 Service Unavailable";
        let err = norm(1, stderr, "").unwrap();
        assert_eq!(err.canonical_class, CanonicalClass::Overloaded);
        assert!(err.retryable);
    }

    // ── DEADLINE_EXCEEDED (504) ─────────────────────────────────────

    #[test]
    fn deadline_exceeded() {
        let stderr = "504 DEADLINE_EXCEEDED: Request timed out";
        let err = norm(1, stderr, "").unwrap();
        assert_eq!(err.canonical_class, CanonicalClass::Timeout);
        assert!(err.retryable);
        assert!(!err.user_action_required);
    }

    #[test]
    fn gateway_timeout_http() {
        let stderr = "Error: 504 Gateway Timeout";
        let err = norm(1, stderr, "").unwrap();
        assert_eq!(err.canonical_class, CanonicalClass::Timeout);
        assert!(err.retryable);
    }

    // ── INTERNAL (500) ──────────────────────────────────────────────

    #[test]
    fn internal_error() {
        let stderr = "500 INTERNAL: An internal error occurred";
        let err = norm(1, stderr, "").unwrap();
        assert_eq!(err.canonical_class, CanonicalClass::Internal);
        assert!(err.retryable);
    }

    #[test]
    fn internal_server_error_http() {
        let stderr = "Error: 500 Internal Server Error";
        let err = norm(1, stderr, "").unwrap();
        assert_eq!(err.canonical_class, CanonicalClass::Internal);
        assert!(err.retryable);
    }

    // ── PERMISSION_DENIED (403) ─────────────────────────────────────

    #[test]
    fn permission_denied() {
        let stderr = "403 PERMISSION_DENIED: The caller does not have permission";
        let err = norm(1, stderr, "").unwrap();
        assert_eq!(err.canonical_class, CanonicalClass::PolicyViolation);
        assert!(!err.retryable);
        assert!(err.user_action_required);
        assert_eq!(err.error_code, "E201");
    }

    #[test]
    fn forbidden_http() {
        let stderr = "Error: 403 Forbidden";
        let err = norm(1, stderr, "").unwrap();
        assert_eq!(err.canonical_class, CanonicalClass::PolicyViolation);
        assert!(!err.retryable);
    }

    // ── UNAUTHENTICATED (401) ───────────────────────────────────────

    #[test]
    fn unauthenticated() {
        let stderr = "401 UNAUTHENTICATED: Request had invalid authentication credentials";
        let err = norm(1, stderr, "").unwrap();
        assert_eq!(err.canonical_class, CanonicalClass::Auth);
        assert!(!err.retryable);
        assert!(err.user_action_required);
        assert_eq!(err.error_code, "E201");
    }

    #[test]
    fn unauthorized_http() {
        let stderr = "Error: 401 Unauthorized";
        let err = norm(1, stderr, "").unwrap();
        assert_eq!(err.canonical_class, CanonicalClass::Auth);
        assert!(!err.retryable);
    }

    // ── INVALID_ARGUMENT (400) ──────────────────────────────────────

    #[test]
    fn invalid_argument() {
        let stderr = "400 INVALID_ARGUMENT: Request contains an invalid argument";
        let err = norm(1, stderr, "").unwrap();
        assert_eq!(err.canonical_class, CanonicalClass::OutputMalformed);
        assert!(!err.retryable);
        assert!(!err.user_action_required);
    }

    #[test]
    fn bad_request_http() {
        let stderr = "Error: 400 Bad Request";
        let err = norm(1, stderr, "").unwrap();
        assert_eq!(err.canonical_class, CanonicalClass::OutputMalformed);
        assert!(!err.retryable);
    }

    // ── NOT_FOUND (404) ─────────────────────────────────────────────

    #[test]
    fn not_found() {
        let stderr = "404 NOT_FOUND: Model gemini-pro-xyz not found";
        let err = norm(1, stderr, "").unwrap();
        assert_eq!(err.canonical_class, CanonicalClass::ToolNotFound);
        assert!(!err.retryable);
        assert!(err.user_action_required);
        assert_eq!(err.error_code, "E102");
    }

    #[test]
    fn not_found_http() {
        let stderr = "Error: 404 Not Found";
        let err = norm(1, stderr, "").unwrap();
        assert_eq!(err.canonical_class, CanonicalClass::ToolNotFound);
        assert!(!err.retryable);
    }

    // ── Network errors ──────────────────────────────────────────────

    #[test]
    fn network_connection_refused() {
        let stderr = "Error: ECONNREFUSED 127.0.0.1:443";
        let err = norm(1, stderr, "").unwrap();
        assert_eq!(err.canonical_class, CanonicalClass::Network);
        assert!(err.retryable);
    }

    // ── Billing ─────────────────────────────────────────────────────

    #[test]
    fn billing_error() {
        let stderr = "Error: billing account suspended";
        let err = norm(1, stderr, "").unwrap();
        assert_eq!(err.canonical_class, CanonicalClass::Billing);
        assert!(!err.retryable);
        assert!(err.user_action_required);
    }

    // ── Request ID extraction ───────────────────────────────────────

    #[test]
    fn extracts_request_id() {
        let stderr = r#"429 RESOURCE_EXHAUSTED {"requestId":"vertex-ai-abc123def456"}"#;
        let err = norm(1, stderr, "").unwrap();
        assert_eq!(
            err.request_id,
            Some("vertex-ai-abc123def456".to_string())
        );
    }

    #[test]
    fn no_request_id_when_absent() {
        let stderr = "503 UNAVAILABLE: Service down";
        let err = norm(1, stderr, "").unwrap();
        assert_eq!(err.request_id, None);
    }

    // ── Retry-after extraction ──────────────────────────────────────

    #[test]
    fn extracts_retry_after() {
        let stderr = "429 RESOURCE_EXHAUSTED retry-after: 45";
        let err = norm(1, stderr, "").unwrap();
        assert_eq!(err.retry_after_seconds, Some(45));
    }

    // ── Generic 429 without RESOURCE_EXHAUSTED ──────────────────────

    #[test]
    fn generic_429() {
        let stderr = "Error: 429 Too Many Requests";
        let err = norm(1, stderr, "").unwrap();
        assert_eq!(err.canonical_class, CanonicalClass::RateLimit);
        assert!(err.retryable);
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
        assert!(norm(0, "", "Generation complete").is_none());
    }

    #[test]
    fn no_error_empty_output() {
        assert!(norm(0, "", "").is_none());
    }

    // ── Raw message truncation ──────────────────────────────────────

    #[test]
    fn raw_message_is_truncated() {
        let long_stderr = format!("429 RESOURCE_EXHAUSTED {}", "x".repeat(600));
        let err = norm(1, &long_stderr, "").unwrap();
        assert!(err.raw_message.len() <= 503);
    }
}
