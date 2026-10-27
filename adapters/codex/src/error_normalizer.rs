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
                    r"(?i)(MACC_TOOL_LIMIT:\s*quota_exhausted|insufficient_quota|budget.exceeded|exceeded.your.current.quota|plan.limit|usage.limit)",
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

/// Regex for extracting `retry-after` hints from text (seconds form).
fn retry_after_regex() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"(?i)(?:retry.after|retry_after)\s*[:=]\s*(\d+)").unwrap())
}

/// Regex for the absolute-timestamp "try again at <Month> <Day>, <Year> <HH>:<MM> [AP]M"
/// or short "try again at <HH>:<MM> [AP]M" form that codex emits on quota exhaustion.
fn try_again_at_regex() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        Regex::new(
            r"(?i)try again at (?:(?P<month>[A-Za-z]+) (?P<day>\d+)[a-z]*, (?P<year>\d{4}) )?(?P<hour>\d{1,2}):(?P<min>\d{2})\s*(?P<ampm>[AaPp][Mm])",
        )
        .unwrap()
    })
}

/// Tolerance in seconds for considering a past reset timestamp as near-current
/// (to account for clock skew between remote server and local host).
const STALE_TIMESTAMP_TOLERANCE_SECONDS: i64 = 300; // 5 minutes

/// Parse an absolute "try again at …" timestamp from the codex output into
/// seconds from now, or return `None` when parsing fails or when the timestamp
/// is significantly in the past (stale historical message).
fn parse_try_again_at_seconds(text: &str) -> Option<u64> {
    parse_try_again_at_seconds_at(text, chrono::Local::now())
}

fn parse_try_again_at_seconds_at(
    text: &str,
    now_local: chrono::DateTime<chrono::Local>,
) -> Option<u64> {
    let caps = try_again_at_regex().captures(text)?;
    let hour_raw: u32 = caps.name("hour")?.as_str().parse().ok()?;
    let min: u32 = caps.name("min")?.as_str().parse().ok()?;
    let ampm = caps.name("ampm")?.as_str().to_ascii_uppercase();

    use chrono::{Datelike, TimeZone as _};

    let (year, month, day) = if let (Some(m_str), Some(d_str), Some(y_str)) = (
        caps.name("month"),
        caps.name("day"),
        caps.name("year"),
    ) {
        let month = match m_str.as_str().to_ascii_lowercase().as_str() {
            "january" | "jan" => 1,
            "february" | "feb" => 2,
            "march" | "mar" => 3,
            "april" | "apr" => 4,
            "may" => 5,
            "june" | "jun" => 6,
            "july" | "jul" => 7,
            "august" | "aug" => 8,
            "september" | "sep" | "sept" => 9,
            "october" | "oct" => 10,
            "november" | "nov" => 11,
            "december" | "dec" => 12,
            _ => return None,
        };
        let day: u32 = d_str.as_str().parse().ok()?;
        let year: i32 = y_str.as_str().parse().ok()?;
        (year, month, day)
    } else {
        (now_local.year(), now_local.month(), now_local.day())
    };

    let hour = match ampm.as_str() {
        "AM" if hour_raw == 12 => 0,
        "AM" => hour_raw,
        "PM" if hour_raw == 12 => 12,
        "PM" => hour_raw + 12,
        _ => return None,
    };

    // Codex's human-readable deadline has no offset; interpret it in the runner's timezone.
    let mut retry_dt = chrono::Local
        .with_ymd_and_hms(year, month, day, hour, min, 0)
        .latest()?;

    let now_ts = now_local.timestamp();
    let mut retry_ts = retry_dt.timestamp();

    // If short format was used (no month/year specified) and time is more than 12 hours
    // in the past, it may be shortly after midnight on the following day.
    if caps.name("month").is_none() && retry_ts < now_ts - 3600 * 12 {
        if let Some(next_day_dt) = retry_dt.checked_add_signed(chrono::Duration::days(1)) {
            if next_day_dt.timestamp() > now_ts {
                retry_dt = next_day_dt;
                retry_ts = retry_dt.timestamp();
            }
        }
    }

    if retry_ts <= now_ts {
        let past_seconds = now_ts - retry_ts;
        if past_seconds > STALE_TIMESTAMP_TOLERANCE_SECONDS {
            // Point 4: Significantly in the past — stale quota message, return None.
            return None;
        }
        // Near-current past timestamp within clock skew tolerance floor.
        return Some(60);
    }
    Some((retry_ts - now_ts) as u64)
}

/// Returns true if the text contains a "try again at …" timestamp that is
/// significantly in the past (older than STALE_TIMESTAMP_TOLERANCE_SECONDS).
fn is_try_again_at_significantly_in_past(text: &str) -> bool {
    is_try_again_at_significantly_in_past_at(text, chrono::Local::now())
}

fn is_try_again_at_significantly_in_past_at(
    text: &str,
    now_local: chrono::DateTime<chrono::Local>,
) -> bool {
    let Some(caps) = try_again_at_regex().captures(text) else {
        return false;
    };
    let Some(hour_raw) = caps.name("hour").and_then(|m| m.as_str().parse::<u32>().ok()) else {
        return false;
    };
    let Some(min) = caps.name("min").and_then(|m| m.as_str().parse::<u32>().ok()) else {
        return false;
    };
    let ampm = match caps.name("ampm") {
        Some(m) => m.as_str().to_ascii_uppercase(),
        None => return false,
    };

    use chrono::{Datelike, TimeZone as _};

    let (year, month, day) = if let (Some(m_str), Some(d_str), Some(y_str)) = (
        caps.name("month"),
        caps.name("day"),
        caps.name("year"),
    ) {
        let month = match m_str.as_str().to_ascii_lowercase().as_str() {
            "january" | "jan" => 1,
            "february" | "feb" => 2,
            "march" | "mar" => 3,
            "april" | "apr" => 4,
            "may" => 5,
            "june" | "jun" => 6,
            "july" | "jul" => 7,
            "august" | "aug" => 8,
            "september" | "sep" | "sept" => 9,
            "october" | "oct" => 10,
            "november" | "nov" => 11,
            "december" | "dec" => 12,
            _ => return false,
        };
        let Some(day) = d_str.as_str().parse::<u32>().ok() else { return false; };
        let Some(year) = y_str.as_str().parse::<i32>().ok() else { return false; };
        (year, month, day)
    } else {
        (now_local.year(), now_local.month(), now_local.day())
    };

    let hour = match ampm.as_str() {
        "AM" if hour_raw == 12 => 0,
        "AM" => hour_raw,
        "PM" if hour_raw == 12 => 12,
        "PM" => hour_raw + 12,
        _ => return false,
    };

    let Some(mut retry_dt) = chrono::Local
        .with_ymd_and_hms(year, month, day, hour, min, 0)
        .latest() else {
        return false;
    };

    let now_ts = now_local.timestamp();
    let mut retry_ts = retry_dt.timestamp();

    if caps.name("month").is_none() && retry_ts < now_ts - 3600 * 12 {
        if let Some(next_day_dt) = retry_dt.checked_add_signed(chrono::Duration::days(1)) {
            if next_day_dt.timestamp() > now_ts {
                retry_dt = next_day_dt;
                retry_ts = retry_dt.timestamp();
            }
        }
    }

    retry_ts < now_ts - STALE_TIMESTAMP_TOLERANCE_SECONDS
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
                // Point 4: Reject quota exhaustion if the extracted "try again at" timestamp
                // is significantly in the past (historical error from prompt/log).
                if pat.class == CanonicalClass::QuotaExhausted
                    && is_try_again_at_significantly_in_past(&combined)
                {
                    continue;
                }
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

        // Try the seconds-form header first (e.g. `retry-after: 60`), then
        // fall back to the absolute-timestamp form that codex emits on quota
        // exhaustion (e.g. "try again at May 25th, 2026 3:20 AM").
        let retry_after_seconds = retry_after_regex()
            .captures(&combined)
            .and_then(|caps| caps.get(1))
            .and_then(|m| m.as_str().parse::<u64>().ok())
            .or_else(|| parse_try_again_at_seconds(&combined));

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
    fn extracts_retry_after_seconds_form() {
        let stderr = "429 Rate limit reached. retry-after: 60";
        let err = norm(1, stderr, "").unwrap();
        assert_eq!(err.retry_after_seconds, Some(60));
    }

    // ── Absolute-timestamp "try again at …" parsing ─────────────────

    #[test]
    fn parse_try_again_at_future_timestamp() {
        // Build a timestamp that is always in the future (year 9999).
        let text = "try again at December 31st, 9999 11:59 PM";
        let secs = parse_try_again_at_seconds(text);
        // Should parse successfully and return a large positive number.
        assert!(
            secs.is_some(),
            "expected a parsed value for a far-future timestamp"
        );
        assert!(
            secs.unwrap() > 3600,
            "expected many seconds for a far-future timestamp"
        );
    }

    #[test]
    fn parse_try_again_at_far_past_timestamp_returns_none() {
        // Point 4: A timestamp significantly in the past should be rejected as stale.
        let text = "try again at January 1st, 2000 12:00 AM";
        let secs = parse_try_again_at_seconds(text);
        assert_eq!(secs, None);
        assert!(is_try_again_at_significantly_in_past(text));
    }

    #[test]
    fn parse_try_again_at_near_past_timestamp_returns_cooldown() {
        use chrono::Local;
        // A timestamp just 10 seconds in the past (within tolerance) returns 60s cooldown floor.
        let now = Local::now();
        let near_past = now - chrono::Duration::seconds(10);
        let month_name = match near_past.format("%B").to_string().as_str() {
            m => m.to_string(),
        };
        let text = format!(
            "try again at {} {}th, {} {}",
            month_name,
            near_past.format("%d"),
            near_past.format("%Y"),
            near_past.format("%I:%M %p")
        );
        let secs = parse_try_again_at_seconds_at(&text, now);
        assert_eq!(secs, Some(60));
        assert!(!is_try_again_at_significantly_in_past_at(&text, now));
    }

    #[test]
    fn parse_try_again_at_short_format_future_and_past() {
        use chrono::Local;
        let now = Local::now();
        // Short format in future (+1 hour)
        let future = now + chrono::Duration::hours(1);
        let future_text = format!("try again at {}", future.format("%I:%M %p"));
        let secs = parse_try_again_at_seconds_at(&future_text, now);
        assert!(secs.is_some());
        assert!(secs.unwrap() >= 3500 && secs.unwrap() <= 3660);
        assert!(!is_try_again_at_significantly_in_past_at(&future_text, now));

        // Short format significantly in past (-2 hours, like in the bug report)
        let past = now - chrono::Duration::hours(2);
        let past_text = format!("try again at {}", past.format("%I:%M %p"));
        let secs_past = parse_try_again_at_seconds_at(&past_text, now);
        assert_eq!(secs_past, None);
        assert!(is_try_again_at_significantly_in_past_at(&past_text, now));
    }

    #[test]
    fn quota_exhausted_rejected_when_timestamp_significantly_in_past() {
        // Point 4: If exit code was 0, but output contains an old historical quota message
        // from a past run or prompt (e.g. 2 hours ago), normalize() must REJECT the quota detection!
        let stderr = "ERROR: You've hit your usage limit. Upgrade to Pro \
            or try again at January 1st, 2020 5:14 PM.";
        let err = norm(0, stderr, "MACC_TASK_RESULT: success_with_changes");
        assert!(err.is_none(), "expected quota to be rejected for past timestamp at exit 0");

        // With exit code 1, it should not be QuotaExhausted, but Unknown
        let err_nonzero = norm(1, stderr, "");
        assert!(err_nonzero.is_some());
        assert_ne!(err_nonzero.unwrap().canonical_class, CanonicalClass::QuotaExhausted);
    }

    #[test]
    fn parse_try_again_at_midday() {
        // 12:00 PM = noon = 12:00 UTC — test the 12-PM edge case in AM/PM logic.
        let text = "try again at January 1st, 9999 12:00 PM";
        let secs = parse_try_again_at_seconds(text);
        assert!(secs.is_some());
    }

    #[test]
    fn parse_try_again_at_midnight() {
        // 12:00 AM = midnight = 00:00 UTC — test the 12-AM edge case.
        let text = "try again at January 1st, 9999 12:00 AM";
        let secs = parse_try_again_at_seconds(text);
        assert!(secs.is_some());
    }

    #[test]
    fn parse_try_again_at_bad_month_returns_none() {
        let text = "try again at Octember 5th, 2026 3:00 AM";
        assert_eq!(parse_try_again_at_seconds(text), None);
    }

    /// The exact format codex emits when a quota limit is hit at exit-0.
    /// After `detect_tool_limit_exit()` forces exit 1, the normalizer receives
    /// this text and must classify it as QuotaExhausted with a retry_after hint.
    #[test]
    fn quota_exhausted_codex_cli_format_with_retry_hint() {
        // Simulates the combined stderr that arrives when performer_lib forces
        // exit 1 after detecting a zero-exit quota error from codex.
        let stderr = "ERROR: You've hit your usage limit. Upgrade to Pro \
            (https://chatgpt.com/explore/pro), visit \
            https://chatgpt.com/codex/settings/usage to purchase more credits \
            or try again at May 25th, 9999 3:20 AM.\n\
            \nMACC_TOOL_LIMIT: quota_exhausted tool=codex \
            retry_hint=\"try again at May 25th, 9999 3:20 AM\"\n\
            The tool 'codex' has exhausted its usage quota and exited 0 \
            without doing any work.";

        let err = norm(1, stderr, "").unwrap();
        assert_eq!(err.canonical_class, CanonicalClass::QuotaExhausted);
        assert!(!err.retryable);
        assert!(err.user_action_required);
        assert_eq!(err.error_code, "E602");
        // retry_after_seconds must be populated from the absolute timestamp.
        assert!(
            err.retry_after_seconds.is_some(),
            "retry_after_seconds should be extracted from the 'try again at' hint"
        );
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
