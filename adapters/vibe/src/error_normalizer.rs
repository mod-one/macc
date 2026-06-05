use std::sync::OnceLock;

use macc_adapter_shared::error_normalizer::{
    canonical_to_error_code, is_retryable, is_user_action_required, truncate_raw_message,
    CanonicalClass, ErrorNormalizer, ToolError,
};
use regex::Regex;

pub struct VibeErrorNormalizer;

struct Pattern {
    regex: Regex,
    class: CanonicalClass,
}

fn patterns() -> &'static Vec<Pattern> {
    static PATTERNS: OnceLock<Vec<Pattern>> = OnceLock::new();
    PATTERNS.get_or_init(|| {
        vec![
            // Quota
            Pattern {
                regex: Regex::new(r"(?i)(hit your limit|usage limit|quota exceeded|resets \d+[ap]m)").unwrap(),
                class: CanonicalClass::QuotaExhausted,
            },
            // Auth
            Pattern {
                regex: Regex::new(r"(?i)(unauthorized|invalid api key|authentication failed|mistral_api_key)").unwrap(),
                class: CanonicalClass::Auth,
            },
            // Policy
            Pattern {
                regex: Regex::new(r"(?i)(policy violation|content policy|safety block)").unwrap(),
                class: CanonicalClass::PolicyViolation,
            },
            // Billing
            Pattern {
                regex: Regex::new(r"(?i)(payment required|billing error)").unwrap(),
                class: CanonicalClass::Billing,
            },
            // Overloaded
            Pattern {
                regex: Regex::new(r"(?i)(529|server overloaded|too busy)").unwrap(),
                class: CanonicalClass::Overloaded,
            },
            // Rate Limit
            Pattern {
                regex: Regex::new(r"(?i)(429|too many requests|rate limit)").unwrap(),
                class: CanonicalClass::RateLimit,
            },
            // Network
            Pattern {
                regex: Regex::new(r"(?i)(ECONNREFUSED|ECONNRESET|ETIMEDOUT|DNS|network.error|connection.refused|connection.reset|getaddrinfo)").unwrap(),
                class: CanonicalClass::Network,
            },
            // Timeout
            Pattern {
                regex: Regex::new(r"(?i)(timeout|timed?\s*out|DEADLINE_EXCEEDED)").unwrap(),
                class: CanonicalClass::Timeout,
            },
            // Internal
            Pattern {
                regex: Regex::new(r"(?i)(500\s+internal|internal_server_error|server_error)").unwrap(),
                class: CanonicalClass::Internal,
            },
        ]
    })
}

fn request_id_regex() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"req_[a-zA-Z0-9]{10,}").unwrap())
}

fn retry_after_regex() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"(?i)(?:retry.after|retry_after)\s*[:=]\s*(\d+)").unwrap())
}

impl ErrorNormalizer for VibeErrorNormalizer {
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
            provider: "vibe".into(),
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
        VibeErrorNormalizer.normalize(exit_code, stderr, stdout)
    }

    #[test]
    fn test_quota_exhausted() {
        let err = norm(1, "Mistral API Error: hit your limit", "").unwrap();
        assert_eq!(err.canonical_class, CanonicalClass::QuotaExhausted);
        assert!(!err.retryable);
        assert_eq!(err.provider, "vibe");
    }

    #[test]
    fn test_rate_limited_with_retry_after() {
        let err = norm(1, "Error 429: rate limit exceeded. retry-after: 45", "").unwrap();
        assert_eq!(err.canonical_class, CanonicalClass::RateLimit);
        assert!(err.retryable);
        assert_eq!(err.retry_after_seconds, Some(45));
    }
}
