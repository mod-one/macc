//! Canonical, provider-agnostic error model for tool runtime errors.
//!
//! MACC shells out to CLI tools (claude, codex, gemini) and receives errors
//! as stderr/stdout text and exit codes — not HTTP response objects. Each
//! provider uses different semantics for the same HTTP status codes:
//!
//! - Claude: 429 = rate-limit, 529 = overload
//! - OpenAI/Codex: 429 = rate-limit OR quota (message-dependent)
//! - Gemini/Vertex: 429 + RESOURCE_EXHAUSTED = quota, overload, or functional limit
//!
//! This module defines the **canonical contract** that all per-adapter
//! normalizers implement and all coordinator consumers use. The MACC core
//! never reasons on raw provider errors — only on [`CanonicalClass`] values.
//!
//! # Error code mapping
//!
//! | CanonicalClass   | E-series | Retryable? | User action? |
//! |------------------|----------|------------|--------------|
//! | Auth             | E201     | No         | Yes          |
//! | Billing          | E201     | No         | Yes          |
//! | RateLimit        | E601     | Yes        | No           |
//! | QuotaExhausted   | E602     | No         | Yes          |
//! | Overloaded       | E601     | Yes        | No           |
//! | Timeout          | E101     | Yes        | No           |
//! | SessionConflict  | E603     | Yes        | No           |
//! | ToolNotFound     | E102     | No         | Yes          |
//! | OutputMalformed  | E103     | No         | No           |
//! | Network          | E101     | Yes        | No           |
//! | PolicyViolation  | E201     | No         | Yes          |
//! | Internal         | E901     | Yes        | No           |
//! | Unknown          | E901     | No         | No           |

use serde::{Deserialize, Serialize};

// ── Error code constants ────────────────────────────────────────────

/// Session conflict — the tool reported a session ID collision or reuse
/// error. Retryable with a fresh session.
pub const E603_SESSION_CONFLICT: &str = "E603";

// ── CanonicalClass ──────────────────────────────────────────────────

/// Provider-agnostic error classification.
///
/// Each adapter normalizer maps its native error patterns to one of these
/// classes. The coordinator runtime only ever switches on `CanonicalClass`,
/// never on raw provider strings or HTTP status codes.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Hash, Default)]
pub enum CanonicalClass {
    /// Invalid or expired API key / token.
    Auth,
    /// Account billing or payment issue.
    Billing,
    /// Transient rate-limit (HTTP 429 or equivalent). Retryable with backoff.
    RateLimit,
    /// Hard quota exhaustion (monthly cap, budget limit). NOT retryable.
    QuotaExhausted,
    /// Provider is overloaded (HTTP 529, 503). Retryable with backoff.
    Overloaded,
    /// Request or connection timeout.
    Timeout,
    /// Session ID collision or reuse error. Retryable with a new session.
    SessionConflict,
    /// Tool CLI binary not found in PATH.
    ToolNotFound,
    /// Tool output could not be parsed / is malformed.
    OutputMalformed,
    /// DNS, TLS, or connection-level failure.
    Network,
    /// Content or safety policy violation.
    PolicyViolation,
    /// Provider internal error (HTTP 500).
    Internal,
    /// Cannot classify the error.
    #[default]
    Unknown,
}

impl std::fmt::Display for CanonicalClass {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let label = match self {
            Self::Auth => "auth",
            Self::Billing => "billing",
            Self::RateLimit => "rate_limit",
            Self::QuotaExhausted => "quota_exhausted",
            Self::Overloaded => "overloaded",
            Self::Timeout => "timeout",
            Self::SessionConflict => "session_conflict",
            Self::ToolNotFound => "tool_not_found",
            Self::OutputMalformed => "output_malformed",
            Self::Network => "network",
            Self::PolicyViolation => "policy_violation",
            Self::Internal => "internal",
            Self::Unknown => "unknown",
        };
        f.write_str(label)
    }
}

// ── ToolError ───────────────────────────────────────────────────────

/// A normalized error produced by a per-adapter [`ErrorNormalizer`].
///
/// This is the single error type that flows from adapter normalizers to
/// the coordinator engine and all downstream consumers (TUI, Web, logs).
#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq)]
pub struct ToolError {
    /// Which provider produced the error (e.g. `"claude"`, `"codex"`, `"gemini"`).
    pub provider: String,

    /// The canonical classification of this error.
    #[serde(default)]
    pub canonical_class: CanonicalClass,

    /// Whether this error is eligible for automatic retry.
    #[serde(default)]
    pub retryable: bool,

    /// Suggested wait time in seconds before retrying (from `Retry-After`
    /// header or equivalent). `None` when no explicit delay was provided.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub retry_after_seconds: Option<u64>,

    /// Whether resolving this error requires human intervention.
    #[serde(default)]
    pub user_action_required: bool,

    /// First 500 characters of the raw error output (for diagnostics).
    #[serde(default)]
    pub raw_message: String,

    /// MACC E-series error code derived from [`canonical_class`].
    #[serde(default)]
    pub error_code: String,

    /// Provider-assigned request identifier (e.g. Claude `req_xxx`), if
    /// extractable from the output.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub request_id: Option<String>,

    /// Which attempt number produced this error.
    #[serde(default)]
    pub attempt: u32,

    /// The operation that was being performed (e.g. `"performer_run"`,
    /// `"context_gen"`).
    #[serde(default)]
    pub operation: String,
}

// ── ErrorNormalizer trait ────────────────────────────────────────────

/// Trait implemented by each per-adapter error normalizer.
///
/// Each adapter (claude, codex, gemini) provides its own implementation
/// that pattern-matches on stderr/stdout text to produce a canonical
/// [`ToolError`]. The coordinator calls the appropriate normalizer based
/// on the tool ID.
pub trait ErrorNormalizer: Send + Sync {
    /// Attempt to classify the tool's output into a canonical [`ToolError`].
    ///
    /// Returns `None` if the output does not match any known error pattern,
    /// in which case the caller falls through to generic E101 classification.
    fn normalize(&self, exit_code: i32, stderr: &str, stdout: &str) -> Option<ToolError>;
}

// ── Mapping helpers ─────────────────────────────────────────────────

/// Map a [`CanonicalClass`] to its MACC E-series error code.
pub fn canonical_to_error_code(class: &CanonicalClass) -> &'static str {
    match class {
        CanonicalClass::Auth => "E201",
        CanonicalClass::Billing => "E201",
        CanonicalClass::RateLimit => "E601",
        CanonicalClass::QuotaExhausted => "E602",
        CanonicalClass::Overloaded => "E601",
        CanonicalClass::Timeout => "E101",
        CanonicalClass::SessionConflict => E603_SESSION_CONFLICT,
        CanonicalClass::ToolNotFound => "E102",
        CanonicalClass::OutputMalformed => "E103",
        CanonicalClass::Network => "E101",
        CanonicalClass::PolicyViolation => "E201",
        CanonicalClass::Internal => "E901",
        CanonicalClass::Unknown => "E901",
    }
}

/// Returns `true` if the given [`CanonicalClass`] is eligible for automatic
/// retry by default.
pub fn is_retryable(class: &CanonicalClass) -> bool {
    matches!(
        class,
        CanonicalClass::RateLimit
            | CanonicalClass::Overloaded
            | CanonicalClass::Timeout
            | CanonicalClass::SessionConflict
            | CanonicalClass::Network
            | CanonicalClass::Internal
    )
}

/// Returns `true` if the given [`CanonicalClass`] requires human
/// intervention to resolve (e.g. fixing an API key, topping up billing).
pub fn is_user_action_required(class: &CanonicalClass) -> bool {
    matches!(
        class,
        CanonicalClass::Auth
            | CanonicalClass::Billing
            | CanonicalClass::QuotaExhausted
            | CanonicalClass::PolicyViolation
            | CanonicalClass::ToolNotFound
    )
}

/// Truncate a raw error message to at most 500 characters for storage.
pub fn truncate_raw_message(msg: &str) -> String {
    if msg.len() <= 500 {
        msg.to_string()
    } else {
        let mut s = msg[..500].to_string();
        s.push('…');
        s
    }
}

// ── Unit tests ──────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn canonical_to_error_code_mapping() {
        assert_eq!(canonical_to_error_code(&CanonicalClass::Auth), "E201");
        assert_eq!(canonical_to_error_code(&CanonicalClass::Billing), "E201");
        assert_eq!(canonical_to_error_code(&CanonicalClass::RateLimit), "E601");
        assert_eq!(
            canonical_to_error_code(&CanonicalClass::QuotaExhausted),
            "E602"
        );
        assert_eq!(canonical_to_error_code(&CanonicalClass::Overloaded), "E601");
        assert_eq!(canonical_to_error_code(&CanonicalClass::Timeout), "E101");
        assert_eq!(
            canonical_to_error_code(&CanonicalClass::SessionConflict),
            "E603"
        );
        assert_eq!(
            canonical_to_error_code(&CanonicalClass::ToolNotFound),
            "E102"
        );
        assert_eq!(
            canonical_to_error_code(&CanonicalClass::OutputMalformed),
            "E103"
        );
        assert_eq!(canonical_to_error_code(&CanonicalClass::Network), "E101");
        assert_eq!(
            canonical_to_error_code(&CanonicalClass::PolicyViolation),
            "E201"
        );
        assert_eq!(canonical_to_error_code(&CanonicalClass::Internal), "E901");
        assert_eq!(canonical_to_error_code(&CanonicalClass::Unknown), "E901");
    }

    #[test]
    fn retryable_classification() {
        // Retryable
        assert!(is_retryable(&CanonicalClass::RateLimit));
        assert!(is_retryable(&CanonicalClass::Overloaded));
        assert!(is_retryable(&CanonicalClass::Timeout));
        assert!(is_retryable(&CanonicalClass::SessionConflict));
        assert!(is_retryable(&CanonicalClass::Network));
        assert!(is_retryable(&CanonicalClass::Internal));

        // Not retryable
        assert!(!is_retryable(&CanonicalClass::Auth));
        assert!(!is_retryable(&CanonicalClass::Billing));
        assert!(!is_retryable(&CanonicalClass::QuotaExhausted));
        assert!(!is_retryable(&CanonicalClass::ToolNotFound));
        assert!(!is_retryable(&CanonicalClass::OutputMalformed));
        assert!(!is_retryable(&CanonicalClass::PolicyViolation));
        assert!(!is_retryable(&CanonicalClass::Unknown));
    }

    #[test]
    fn user_action_required_classification() {
        // Requires user action
        assert!(is_user_action_required(&CanonicalClass::Auth));
        assert!(is_user_action_required(&CanonicalClass::Billing));
        assert!(is_user_action_required(&CanonicalClass::QuotaExhausted));
        assert!(is_user_action_required(&CanonicalClass::PolicyViolation));
        assert!(is_user_action_required(&CanonicalClass::ToolNotFound));

        // Does not require user action
        assert!(!is_user_action_required(&CanonicalClass::RateLimit));
        assert!(!is_user_action_required(&CanonicalClass::Overloaded));
        assert!(!is_user_action_required(&CanonicalClass::Timeout));
        assert!(!is_user_action_required(&CanonicalClass::SessionConflict));
        assert!(!is_user_action_required(&CanonicalClass::Network));
        assert!(!is_user_action_required(&CanonicalClass::Internal));
        assert!(!is_user_action_required(&CanonicalClass::Unknown));
        assert!(!is_user_action_required(&CanonicalClass::OutputMalformed));
    }

    #[test]
    fn tool_error_serialization_roundtrip() {
        let err = ToolError {
            provider: "claude".into(),
            canonical_class: CanonicalClass::Overloaded,
            retryable: true,
            retry_after_seconds: Some(30),
            user_action_required: false,
            raw_message: "API Error: 529 overloaded".into(),
            error_code: "E601".into(),
            request_id: Some("req_011CZ9GZzDNEuAo5ALcxo3UK".into()),
            attempt: 1,
            operation: "performer_run".into(),
        };

        let json = serde_json::to_string(&err).expect("serialize");
        let back: ToolError = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(err, back);
    }

    #[test]
    fn tool_error_default() {
        let err = ToolError::default();
        assert_eq!(err.provider, "");
        assert_eq!(err.canonical_class, CanonicalClass::Unknown);
        assert!(!err.retryable);
        assert_eq!(err.retry_after_seconds, None);
        assert!(!err.user_action_required);
        assert_eq!(err.raw_message, "");
        assert_eq!(err.error_code, "");
        assert_eq!(err.request_id, None);
        assert_eq!(err.attempt, 0);
        assert_eq!(err.operation, "");
    }

    #[test]
    fn tool_error_skip_serializing_none_fields() {
        let err = ToolError {
            provider: "codex".into(),
            canonical_class: CanonicalClass::RateLimit,
            retryable: true,
            retry_after_seconds: None,
            user_action_required: false,
            raw_message: "429 Too Many Requests".into(),
            error_code: "E601".into(),
            request_id: None,
            attempt: 2,
            operation: "performer_run".into(),
        };

        let json = serde_json::to_string(&err).expect("serialize");
        assert!(!json.contains("retry_after_seconds"));
        assert!(!json.contains("request_id"));

        let back: ToolError = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(err, back);
    }

    #[test]
    fn canonical_class_default_is_unknown() {
        assert_eq!(CanonicalClass::default(), CanonicalClass::Unknown);
    }

    #[test]
    fn canonical_class_display() {
        assert_eq!(CanonicalClass::Auth.to_string(), "auth");
        assert_eq!(CanonicalClass::RateLimit.to_string(), "rate_limit");
        assert_eq!(
            CanonicalClass::QuotaExhausted.to_string(),
            "quota_exhausted"
        );
        assert_eq!(CanonicalClass::Overloaded.to_string(), "overloaded");
        assert_eq!(
            CanonicalClass::SessionConflict.to_string(),
            "session_conflict"
        );
        assert_eq!(CanonicalClass::Unknown.to_string(), "unknown");
    }

    #[test]
    fn truncate_raw_message_short() {
        let msg = "short error";
        assert_eq!(truncate_raw_message(msg), "short error");
    }

    #[test]
    fn truncate_raw_message_long() {
        let msg = "x".repeat(600);
        let truncated = truncate_raw_message(&msg);
        assert_eq!(truncated.len(), 503); // 500 + "…" (3 bytes UTF-8)
        assert!(truncated.ends_with('…'));
    }

    #[test]
    fn e603_constant() {
        assert_eq!(E603_SESSION_CONFLICT, "E603");
    }

    #[test]
    fn retryable_and_error_code_consistency() {
        // Verify that retryable classes map to error codes that should be
        // in the default retry list.
        let retryable_classes = [
            CanonicalClass::RateLimit,
            CanonicalClass::Overloaded,
            CanonicalClass::Timeout,
            CanonicalClass::SessionConflict,
            CanonicalClass::Network,
            CanonicalClass::Internal,
        ];
        for class in &retryable_classes {
            assert!(is_retryable(class), "{class} should be retryable");
        }

        // Non-retryable classes that require user action should not be retried
        let non_retryable_user = [
            CanonicalClass::Auth,
            CanonicalClass::Billing,
            CanonicalClass::QuotaExhausted,
            CanonicalClass::PolicyViolation,
        ];
        for class in &non_retryable_user {
            assert!(!is_retryable(class), "{class} should NOT be retryable");
            assert!(
                is_user_action_required(class),
                "{class} should require user action"
            );
        }
    }
}
