use std::sync::OnceLock;

use macc_adapter_shared::error_normalizer::{
    canonical_to_error_code, is_retryable, is_user_action_required, truncate_raw_message,
    CanonicalClass, ErrorNormalizer, ToolError,
};
use regex::Regex;

/// Antigravity-specific implementation of [`ErrorNormalizer`].
pub struct AgyErrorNormalizer;

/// A compiled pattern and its associated canonical class.
struct Pattern {
    regex: Regex,
    class: CanonicalClass,
}

fn patterns() -> &'static Vec<Pattern> {
    static PATTERNS: OnceLock<Vec<Pattern>> = OnceLock::new();
    PATTERNS.get_or_init(|| {
        vec![
            Pattern {
                regex: Regex::new(
                    r"(?i)(MODEL_CAPACITY_EXHAUSTED|[Nn]o\s+capacity\s+available\s+for\s+model)",
                )
                .unwrap(),
                class: CanonicalClass::Overloaded,
            },
            Pattern {
                regex: Regex::new(
                    r"(?i)RESOURCE_EXHAUSTED.{0,100}(quota|limit\s+per|exceeded.{0,30}(limit|cap)|tokens?.per|requests?.per)",
                )
                .unwrap(),
                class: CanonicalClass::QuotaExhausted,
            },
            Pattern {
                regex: Regex::new(r"(?i)RESOURCE_EXHAUSTED").unwrap(),
                class: CanonicalClass::RateLimit,
            },
            Pattern {
                regex: Regex::new(r"(?i)(UNAUTHENTICATED|401\s+Unauthorized)").unwrap(),
                class: CanonicalClass::Auth,
            },
            Pattern {
                regex: Regex::new(r"(?i)(PERMISSION_DENIED|403\s+Forbidden)").unwrap(),
                class: CanonicalClass::PolicyViolation,
            },
            Pattern {
                regex: Regex::new(r"(?i)(INVALID_ARGUMENT|400\s+Bad\s+Request)").unwrap(),
                class: CanonicalClass::OutputMalformed,
            },
            Pattern {
                regex: Regex::new(r"(?i)(NOT_FOUND|404\s+Not\s+Found)").unwrap(),
                class: CanonicalClass::ToolNotFound,
            },
            Pattern {
                regex: Regex::new(r"(?i)(UNAVAILABLE|503\s+Service\s+Unavailable)").unwrap(),
                class: CanonicalClass::Overloaded,
            },
            Pattern {
                regex: Regex::new(r"(?i)(DEADLINE_EXCEEDED|504\s+Gateway\s+Timeout)").unwrap(),
                class: CanonicalClass::Timeout,
            },
            Pattern {
                regex: Regex::new(r"(?i)(INTERNAL|500\s+Internal\s+Server\s+Error)").unwrap(),
                class: CanonicalClass::Internal,
            },
            Pattern {
                regex: Regex::new(
                    r"(?i)(ECONNREFUSED|ECONNRESET|ETIMEDOUT|DNS|network.error|connection.refused|getaddrinfo)",
                )
                .unwrap(),
                class: CanonicalClass::Network,
            },
            Pattern {
                regex: Regex::new(r"(?i)(billing|payment.required|account.suspended)").unwrap(),
                class: CanonicalClass::Billing,
            },
            Pattern {
                regex: Regex::new(r"(?i)(429|too many requests|rate.limit)").unwrap(),
                class: CanonicalClass::RateLimit,
            },
        ]
    })
}

fn request_id_regex() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        Regex::new(r#"(?:request_id|requestId|x-request-id)[":\s]+([a-zA-Z0-9_-]{10,})"#).unwrap()
    })
}

fn retry_after_regex() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r#"(?i)(?:retry.after|retry_after)\s*[:=]\s*(\d+)"#).unwrap())
}

impl ErrorNormalizer for AgyErrorNormalizer {
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
            provider: "agy".into(),
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

#[cfg(test)]
mod tests {
    use super::*;

    fn norm(exit_code: i32, stderr: &str, stdout: &str) -> Option<ToolError> {
        AgyErrorNormalizer.normalize(exit_code, stderr, stdout)
    }

    #[test]
    fn test_agy_overloaded() {
        let stderr = "No capacity available for model gemini-3.5-pro on the server.";
        let err = norm(1, stderr, "").unwrap();
        assert_eq!(err.canonical_class, CanonicalClass::Overloaded);
        assert!(err.retryable);
        assert_eq!(err.provider, "agy");
    }

    #[test]
    fn test_agy_auth() {
        let stderr = "401 UNAUTHENTICATED: Request had invalid credentials";
        let err = norm(1, stderr, "").unwrap();
        assert_eq!(err.canonical_class, CanonicalClass::Auth);
        assert!(!err.retryable);
    }
}
