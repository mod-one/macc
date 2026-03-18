//! Per-adapter error normalizer implementations used by the coordinator engine.
//!
//! Each struct implements [`ErrorNormalizer`] and pattern-matches on the
//! stderr/stdout text emitted by the corresponding CLI tool. The coordinator
//! uses [`super::engine::get_normalizer_for_tool`] to pick the right one.
//!
//! # Dependency note
//!
//! These implementations live in `macc-core` (not in the adapter crates) so
//! that the coordinator engine can call them without creating circular crate
//! dependencies. The adapter crates (`macc-adapter-claude`, etc.) have their
//! own independent implementations that share the same logic for their own
//! unit tests.

use std::sync::OnceLock;

use regex::Regex;

use super::error_normalizer::{
    canonical_to_error_code, is_retryable, is_user_action_required, truncate_raw_message,
    CanonicalClass, ErrorNormalizer, ToolError,
};

// ── Shared helpers ───────────────────────────────────────────────────

struct Pattern {
    regex: Regex,
    class: CanonicalClass,
}

fn extract_request_id(text: &str, re: &Regex) -> Option<String> {
    re.find(text).map(|m| m.as_str().to_string())
}

fn extract_retry_after(text: &str, re: &Regex) -> Option<u64> {
    re.captures(text)
        .and_then(|c| c.get(1))
        .and_then(|m| m.as_str().parse::<u64>().ok())
}

fn build_tool_error(
    provider: &str,
    class: CanonicalClass,
    combined: &str,
    request_id: Option<String>,
    retry_after_seconds: Option<u64>,
) -> ToolError {
    let error_code = canonical_to_error_code(&class).to_string();
    let retryable = is_retryable(&class);
    let user_action_required = is_user_action_required(&class);
    let raw_message = truncate_raw_message(combined.trim());
    ToolError {
        provider: provider.to_string(),
        canonical_class: class,
        retryable,
        retry_after_seconds,
        user_action_required,
        raw_message,
        error_code,
        request_id,
        attempt: 0,
        operation: String::new(),
    }
}

fn match_patterns(patterns: &[Pattern], combined: &str) -> Option<CanonicalClass> {
    patterns
        .iter()
        .find(|p| p.regex.is_match(combined))
        .map(|p| p.class.clone())
}

// ── Claude ───────────────────────────────────────────────────────────

/// Coordinator-side normalizer for the `claude` CLI tool.
pub struct ClaudeErrorNormalizer;

fn claude_patterns() -> &'static Vec<Pattern> {
    static PATTERNS: OnceLock<Vec<Pattern>> = OnceLock::new();
    PATTERNS.get_or_init(|| {
        vec![
            // Quota exhaustion — check before generic 429
            Pattern {
                regex: Regex::new(
                    r"(?i)(hit.*limit|usage.*limit|insufficient_quota|quota.*exceeded|plan.*limit)",
                )
                .unwrap(),
                class: CanonicalClass::QuotaExhausted,
            },
            // Auth
            Pattern {
                regex: Regex::new(
                    r"(?i)(AuthenticationError|invalid.*api.*key|authentication.*failed)",
                )
                .unwrap(),
                class: CanonicalClass::Auth,
            },
            // Policy / permission
            Pattern {
                regex: Regex::new(r"(?i)(PermissionError|permission.*denied|policy.*violation)")
                    .unwrap(),
                class: CanonicalClass::PolicyViolation,
            },
            // Billing
            Pattern {
                regex: Regex::new(r"(?i)(billing|payment.*required|account.*deactivated)").unwrap(),
                class: CanonicalClass::Billing,
            },
            // Session conflict
            Pattern {
                regex: Regex::new(r"(?i)(session.*conflict|session.*already.*in.*use)").unwrap(),
                class: CanonicalClass::SessionConflict,
            },
            // 529 overloaded (Claude-specific)
            Pattern {
                regex: Regex::new(r"(?i)(529|overloaded|API.*overloaded)").unwrap(),
                class: CanonicalClass::Overloaded,
            },
            // 429 rate-limit
            Pattern {
                regex: Regex::new(r"(?i)(429|rate.*limit|too.*many.*requests|RateLimitError)")
                    .unwrap(),
                class: CanonicalClass::RateLimit,
            },
            // Network
            Pattern {
                regex: Regex::new(
                    r"(?i)(APIConnectionError|ECONNREFUSED|ECONNRESET|connection.*refused|DNS|getaddrinfo)",
                )
                .unwrap(),
                class: CanonicalClass::Network,
            },
            // Timeout
            Pattern {
                regex: Regex::new(r"(?i)(APITimeoutError|timed?\s*out|DEADLINE_EXCEEDED)")
                    .unwrap(),
                class: CanonicalClass::Timeout,
            },
            // Internal
            Pattern {
                regex: Regex::new(r"(?i)(500\s+internal|InternalServerError|server_error)")
                    .unwrap(),
                class: CanonicalClass::Internal,
            },
        ]
    })
}

fn claude_request_id_regex() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"req_[a-zA-Z0-9]{10,}").unwrap())
}

fn retry_after_regex() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"(?i)(?:retry[-_]after)\s*[:=]\s*(\d+)").unwrap())
}

impl ErrorNormalizer for ClaudeErrorNormalizer {
    fn normalize(&self, exit_code: i32, stderr: &str, stdout: &str) -> Option<ToolError> {
        let combined = format!("{}\n{}", stderr, stdout);
        if combined.trim().is_empty() {
            return None;
        }
        let class = match match_patterns(claude_patterns(), &combined) {
            Some(c) => c,
            None if exit_code != 0 => CanonicalClass::Unknown,
            None => return None,
        };
        let request_id = extract_request_id(&combined, claude_request_id_regex());
        let retry_after_seconds = extract_retry_after(&combined, retry_after_regex());
        Some(build_tool_error(
            "claude",
            class,
            &combined,
            request_id,
            retry_after_seconds,
        ))
    }
}

// ── Codex ────────────────────────────────────────────────────────────

/// Coordinator-side normalizer for the `codex` CLI tool.
pub struct CodexErrorNormalizer;

fn codex_patterns() -> &'static Vec<Pattern> {
    static PATTERNS: OnceLock<Vec<Pattern>> = OnceLock::new();
    PATTERNS.get_or_init(|| {
        vec![
            // Quota exhaustion — must precede generic 429
            Pattern {
                regex: Regex::new(
                    r"(?i)(insufficient_quota|budget.*exceeded|exceeded.*current.*quota|plan.*limit|usage.*limit)",
                )
                .unwrap(),
                class: CanonicalClass::QuotaExhausted,
            },
            // Auth
            Pattern {
                regex: Regex::new(
                    r"(?i)(AuthenticationError|invalid_api_key|invalid.*api.*key|Incorrect API key)",
                )
                .unwrap(),
                class: CanonicalClass::Auth,
            },
            // Permission
            Pattern {
                regex: Regex::new(r"(?i)(PermissionError|permission_denied|not.*permitted)").unwrap(),
                class: CanonicalClass::PolicyViolation,
            },
            // Billing
            Pattern {
                regex: Regex::new(
                    r"(?i)(billing|payment.*required|account.*deactivated|account.*suspended)",
                )
                .unwrap(),
                class: CanonicalClass::Billing,
            },
            // Websocket session errors
            Pattern {
                regex: Regex::new(
                    r"(?i)(previous_response_not_found|websocket_connection_limit_reached|session.*already.*in.*use)",
                )
                .unwrap(),
                class: CanonicalClass::SessionConflict,
            },
            // 503 Slow Down — surge protection
            Pattern {
                regex: Regex::new(r"(?i)(503.*slow\s*down|slow\s*down.*503|overloaded)").unwrap(),
                class: CanonicalClass::Overloaded,
            },
            // 429 / rate-limit
            Pattern {
                regex: Regex::new(
                    r"(?i)(429|RateLimitError|rate.*limit|too.*many.*requests)",
                )
                .unwrap(),
                class: CanonicalClass::RateLimit,
            },
            // Network
            Pattern {
                regex: Regex::new(
                    r"(?i)(APIConnectionError|ECONNREFUSED|ECONNRESET|connection.*refused|DNS|getaddrinfo|network.*error)",
                )
                .unwrap(),
                class: CanonicalClass::Network,
            },
            // Timeout
            Pattern {
                regex: Regex::new(r"(?i)(APITimeoutError|timed?\s*out|DEADLINE_EXCEEDED)").unwrap(),
                class: CanonicalClass::Timeout,
            },
            // Internal
            Pattern {
                regex: Regex::new(r"(?i)(500\s+internal|InternalServerError|server_error)").unwrap(),
                class: CanonicalClass::Internal,
            },
            // Generic 503
            Pattern {
                regex: Regex::new(r"(?i)503\s+service").unwrap(),
                class: CanonicalClass::Overloaded,
            },
        ]
    })
}

fn codex_request_id_regex() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"req_[a-zA-Z0-9]{10,}").unwrap())
}

impl ErrorNormalizer for CodexErrorNormalizer {
    fn normalize(&self, exit_code: i32, stderr: &str, stdout: &str) -> Option<ToolError> {
        let combined = format!("{}\n{}", stderr, stdout);
        if combined.trim().is_empty() {
            return None;
        }
        let class = match match_patterns(codex_patterns(), &combined) {
            Some(c) => c,
            None if exit_code != 0 => CanonicalClass::Unknown,
            None => return None,
        };
        let request_id = extract_request_id(&combined, codex_request_id_regex());
        let retry_after_seconds = extract_retry_after(&combined, retry_after_regex());
        Some(build_tool_error(
            "codex",
            class,
            &combined,
            request_id,
            retry_after_seconds,
        ))
    }
}

// ── Gemini ───────────────────────────────────────────────────────────

/// Coordinator-side normalizer for the `gemini` CLI tool.
pub struct GeminiErrorNormalizer;

fn gemini_patterns() -> &'static Vec<Pattern> {
    static PATTERNS: OnceLock<Vec<Pattern>> = OnceLock::new();
    PATTERNS.get_or_init(|| {
        vec![
            // RESOURCE_EXHAUSTED + quota keywords — must precede bare RESOURCE_EXHAUSTED
            Pattern {
                regex: Regex::new(
                    r"(?i)RESOURCE_EXHAUSTED.{0,100}(quota|limit\s*per|exceeded.{0,30}(limit|cap)|tokens?\s*per|requests?\s*per)",
                )
                .unwrap(),
                class: CanonicalClass::QuotaExhausted,
            },
            // Generic RESOURCE_EXHAUSTED without quota keywords → rate-limit
            Pattern {
                regex: Regex::new(r"(?i)RESOURCE_EXHAUSTED").unwrap(),
                class: CanonicalClass::RateLimit,
            },
            // UNAUTHENTICATED (401)
            Pattern {
                regex: Regex::new(r"(?i)(UNAUTHENTICATED|401\s+Unauthorized)").unwrap(),
                class: CanonicalClass::Auth,
            },
            // PERMISSION_DENIED (403)
            Pattern {
                regex: Regex::new(r"(?i)(PERMISSION_DENIED|403\s+Forbidden)").unwrap(),
                class: CanonicalClass::PolicyViolation,
            },
            // INVALID_ARGUMENT (400)
            Pattern {
                regex: Regex::new(r"(?i)(INVALID_ARGUMENT|400\s+Bad\s+Request)").unwrap(),
                class: CanonicalClass::OutputMalformed,
            },
            // NOT_FOUND (404)
            Pattern {
                regex: Regex::new(r"(?i)(NOT_FOUND|404\s+Not\s+Found)").unwrap(),
                class: CanonicalClass::ToolNotFound,
            },
            // UNAVAILABLE (503)
            Pattern {
                regex: Regex::new(r"(?i)(UNAVAILABLE|503\s+Service\s+Unavailable)").unwrap(),
                class: CanonicalClass::Overloaded,
            },
            // DEADLINE_EXCEEDED (504)
            Pattern {
                regex: Regex::new(r"(?i)(DEADLINE_EXCEEDED|504\s+Gateway\s+Timeout)").unwrap(),
                class: CanonicalClass::Timeout,
            },
            // INTERNAL (500)
            Pattern {
                regex: Regex::new(r"(?i)(INTERNAL|500\s+Internal\s+Server\s+Error)").unwrap(),
                class: CanonicalClass::Internal,
            },
            // Network
            Pattern {
                regex: Regex::new(
                    r"(?i)(ECONNREFUSED|ECONNRESET|ETIMEDOUT|DNS|connection.*refused|getaddrinfo|network.*error)",
                )
                .unwrap(),
                class: CanonicalClass::Network,
            },
            // Billing
            Pattern {
                regex: Regex::new(r"(?i)(billing|payment.*required|account.*suspended)").unwrap(),
                class: CanonicalClass::Billing,
            },
            // Generic 429 without RESOURCE_EXHAUSTED
            Pattern {
                regex: Regex::new(r"(?i)(429|too.*many.*requests|rate.*limit)").unwrap(),
                class: CanonicalClass::RateLimit,
            },
        ]
    })
}

fn gemini_request_id_regex() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        Regex::new(r#"(?:request_id|requestId|x-request-id)[":\s]+([a-zA-Z0-9_-]{10,})"#).unwrap()
    })
}

impl ErrorNormalizer for GeminiErrorNormalizer {
    fn normalize(&self, exit_code: i32, stderr: &str, stdout: &str) -> Option<ToolError> {
        let combined = format!("{}\n{}", stderr, stdout);
        if combined.trim().is_empty() {
            return None;
        }
        let class = match match_patterns(gemini_patterns(), &combined) {
            Some(c) => c,
            None if exit_code != 0 => CanonicalClass::Unknown,
            None => return None,
        };
        let request_id = gemini_request_id_regex()
            .captures(&combined)
            .and_then(|c| c.get(1))
            .map(|m| m.as_str().to_string());
        let retry_after_seconds = extract_retry_after(&combined, retry_after_regex());
        Some(build_tool_error(
            "gemini",
            class,
            &combined,
            request_id,
            retry_after_seconds,
        ))
    }
}

// ── Unit tests ───────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    // ── Claude ─────────────────────────────────────────────────────

    #[test]
    fn claude_529_overloaded() {
        let e = ClaudeErrorNormalizer
            .normalize(1, "Error: 529 API overloaded", "")
            .unwrap();
        assert_eq!(e.canonical_class, CanonicalClass::Overloaded);
        assert!(e.retryable);
        assert_eq!(e.error_code, "E601");
        assert_eq!(e.provider, "claude");
    }

    #[test]
    fn claude_429_rate_limit() {
        let e = ClaudeErrorNormalizer
            .normalize(1, "429 Too many requests", "")
            .unwrap();
        assert_eq!(e.canonical_class, CanonicalClass::RateLimit);
        assert!(e.retryable);
        assert_eq!(e.error_code, "E601");
    }

    #[test]
    fn claude_quota_takes_priority() {
        let e = ClaudeErrorNormalizer
            .normalize(1, "429 usage limit exceeded for this key", "")
            .unwrap();
        assert_eq!(e.canonical_class, CanonicalClass::QuotaExhausted);
        assert!(!e.retryable);
        assert_eq!(e.error_code, "E602");
    }

    #[test]
    fn claude_auth_error() {
        let e = ClaudeErrorNormalizer
            .normalize(1, "AuthenticationError: invalid api key", "")
            .unwrap();
        assert_eq!(e.canonical_class, CanonicalClass::Auth);
        assert!(!e.retryable);
    }

    #[test]
    fn claude_no_error_clean_exit() {
        assert!(ClaudeErrorNormalizer.normalize(0, "", "done").is_none());
    }

    // ── Codex ──────────────────────────────────────────────────────

    #[test]
    fn codex_429_rate_limit() {
        let e = CodexErrorNormalizer
            .normalize(1, "Error: 429 Rate limit reached", "")
            .unwrap();
        assert_eq!(e.canonical_class, CanonicalClass::RateLimit);
        assert!(e.retryable);
        assert_eq!(e.error_code, "E601");
        assert_eq!(e.provider, "codex");
    }

    #[test]
    fn codex_quota_exhausted() {
        let e = CodexErrorNormalizer
            .normalize(1, "429 insufficient_quota: exceeded your current quota", "")
            .unwrap();
        assert_eq!(e.canonical_class, CanonicalClass::QuotaExhausted);
        assert!(!e.retryable);
        assert_eq!(e.error_code, "E602");
    }

    #[test]
    fn codex_quota_takes_priority_over_429() {
        let e = CodexErrorNormalizer
            .normalize(
                1,
                "429 You exceeded your current quota: insufficient_quota",
                "",
            )
            .unwrap();
        assert_eq!(e.canonical_class, CanonicalClass::QuotaExhausted);
        assert!(!e.retryable);
    }

    #[test]
    fn codex_503_slow_down() {
        let e = CodexErrorNormalizer
            .normalize(1, "503 Slow Down: service overloaded", "")
            .unwrap();
        assert_eq!(e.canonical_class, CanonicalClass::Overloaded);
        assert!(e.retryable);
    }

    #[test]
    fn codex_auth_error() {
        let e = CodexErrorNormalizer
            .normalize(1, "openai.AuthenticationError: invalid_api_key", "")
            .unwrap();
        assert_eq!(e.canonical_class, CanonicalClass::Auth);
        assert!(!e.retryable);
    }

    #[test]
    fn codex_timeout() {
        let e = CodexErrorNormalizer
            .normalize(1, "openai.APITimeoutError: timed out", "")
            .unwrap();
        assert_eq!(e.canonical_class, CanonicalClass::Timeout);
        assert!(e.retryable);
    }

    #[test]
    fn codex_connection_refused() {
        let e = CodexErrorNormalizer
            .normalize(1, "APIConnectionError: ECONNREFUSED 127.0.0.1:443", "")
            .unwrap();
        assert_eq!(e.canonical_class, CanonicalClass::Network);
        assert!(e.retryable);
    }

    #[test]
    fn codex_no_error_clean_exit() {
        assert!(CodexErrorNormalizer.normalize(0, "", "done").is_none());
    }

    // ── Gemini ─────────────────────────────────────────────────────

    #[test]
    fn gemini_resource_exhausted_quota() {
        let e = GeminiErrorNormalizer
            .normalize(
                1,
                "429 RESOURCE_EXHAUSTED: Quota exceeded for requests per minute",
                "",
            )
            .unwrap();
        assert_eq!(e.canonical_class, CanonicalClass::QuotaExhausted);
        assert!(!e.retryable);
        assert_eq!(e.error_code, "E602");
        assert_eq!(e.provider, "gemini");
    }

    #[test]
    fn gemini_resource_exhausted_rate_limit() {
        let e = GeminiErrorNormalizer
            .normalize(1, "429 RESOURCE_EXHAUSTED: Rate limit for model", "")
            .unwrap();
        assert_eq!(e.canonical_class, CanonicalClass::RateLimit);
        assert!(e.retryable);
        assert_eq!(e.error_code, "E601");
    }

    #[test]
    fn gemini_unavailable() {
        let e = GeminiErrorNormalizer
            .normalize(
                1,
                "503 UNAVAILABLE: The service is temporarily unavailable",
                "",
            )
            .unwrap();
        assert_eq!(e.canonical_class, CanonicalClass::Overloaded);
        assert!(e.retryable);
    }

    #[test]
    fn gemini_deadline_exceeded() {
        let e = GeminiErrorNormalizer
            .normalize(1, "504 DEADLINE_EXCEEDED: Request timed out", "")
            .unwrap();
        assert_eq!(e.canonical_class, CanonicalClass::Timeout);
        assert!(e.retryable);
    }

    #[test]
    fn gemini_permission_denied() {
        let e = GeminiErrorNormalizer
            .normalize(
                1,
                "403 PERMISSION_DENIED: The caller does not have permission",
                "",
            )
            .unwrap();
        assert_eq!(e.canonical_class, CanonicalClass::PolicyViolation);
        assert!(!e.retryable);
    }

    #[test]
    fn gemini_invalid_argument() {
        let e = GeminiErrorNormalizer
            .normalize(
                1,
                "400 INVALID_ARGUMENT: Request contains invalid field",
                "",
            )
            .unwrap();
        assert_eq!(e.canonical_class, CanonicalClass::OutputMalformed);
        assert!(!e.retryable);
    }

    #[test]
    fn gemini_no_error_clean_exit() {
        assert!(GeminiErrorNormalizer.normalize(0, "", "done").is_none());
    }
}
