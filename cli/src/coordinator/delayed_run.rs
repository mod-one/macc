use chrono::{DateTime, Local, LocalResult, NaiveDateTime, TimeZone};
use std::time::Duration;

#[derive(Debug, Clone)]
pub struct ResolvedDelayedStart {
    pub scheduled_at: DateTime<Local>,
    pub delay: Duration,
    pub source: DelayedStartSource,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DelayedStartSource {
    Relative,
    Absolute,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DelayedRunOutcome {
    Ready,
    Cancelled,
}

#[derive(Debug, thiserror::Error)]
pub enum DelayedRunError {
    #[error("error: invalid value '{input}' for '--in <DURATION>': expected a duration such as '30m', '2h', or '1h 30m'")]
    InvalidDuration { input: String, reason: String },

    #[error("error: '--in' duration must be greater than zero; omit '--in' to run immediately")]
    ZeroDuration,

    #[error("error: invalid value '{input}' for '--at <DATETIME>': expected RFC 3339 or local format 'YYYY-MM-DDTHH:MM[:SS]'")]
    InvalidDateTime { input: String },

    #[error("error: scheduled start time is in the past: {scheduled_at}")]
    PastDateTime { scheduled_at: String },

    #[error("error: local date-time '{input}' is ambiguous in the current timezone; provide an explicit UTC offset")]
    AmbiguousLocalDateTime { input: String },

    #[error("error: local date-time '{input}' does not exist in the current timezone; provide a valid time or explicit UTC offset")]
    NonexistentLocalDateTime { input: String },

    #[error("error: scheduled start time exceeds the supported date-time range")]
    DateTimeOverflow,

    #[error("failed to listen for cancellation signal: {0}")]
    Signal(#[source] std::io::Error),
}

pub fn parse_relative_delay(input: &str) -> Result<Duration, DelayedRunError> {
    let input_trimmed = input.trim();
    if input_trimmed.is_empty() {
        return Err(DelayedRunError::InvalidDuration {
            input: input.to_string(),
            reason: "empty input".to_string(),
        });
    }

    let parsed = humantime::parse_duration(input_trimmed).map_err(|e| {
        DelayedRunError::InvalidDuration {
            input: input.to_string(),
            reason: e.to_string(),
        }
    })?;

    if parsed == Duration::ZERO {
        return Err(DelayedRunError::ZeroDuration);
    }

    Ok(parsed)
}

pub fn parse_absolute_time(
    input: &str,
    now: DateTime<Local>,
) -> Result<DateTime<Local>, DelayedRunError> {
    let input_trimmed = input.trim();
    if let Ok(value) = DateTime::parse_from_rfc3339(input_trimmed) {
        let local = value.with_timezone(&Local);
        return validate_future_time(local, now);
    }

    let naive = ["%Y-%m-%dT%H:%M:%S", "%Y-%m-%dT%H:%M"]
        .iter()
        .find_map(|format| NaiveDateTime::parse_from_str(input_trimmed, format).ok())
        .ok_or_else(|| DelayedRunError::InvalidDateTime {
            input: input.to_string(),
        })?;

    let local = match Local.from_local_datetime(&naive) {
        LocalResult::Single(value) => value,
        LocalResult::Ambiguous(_, _) => {
            return Err(DelayedRunError::AmbiguousLocalDateTime {
                input: input.to_string(),
            });
        }
        LocalResult::None => {
            return Err(DelayedRunError::NonexistentLocalDateTime {
                input: input.to_string(),
            });
        }
    };

    validate_future_time(local, now)
}

fn validate_future_time(
    target: DateTime<Local>,
    now: DateTime<Local>,
) -> Result<DateTime<Local>, DelayedRunError> {
    if target <= now {
        return Err(DelayedRunError::PastDateTime {
            scheduled_at: target.to_rfc3339(),
        });
    }
    Ok(target)
}

fn resolve_relative_delay(
    duration: Duration,
    now: DateTime<Local>,
) -> Result<DateTime<Local>, DelayedRunError> {
    let chrono_dur = chrono::Duration::from_std(duration).map_err(|_| DelayedRunError::DateTimeOverflow)?;
    let target = now.checked_add_signed(chrono_dur).ok_or(DelayedRunError::DateTimeOverflow)?;
    Ok(target)
}

pub fn resolve_delayed_start(
    run_in: Option<&str>,
    at: Option<&str>,
    now: DateTime<Local>,
) -> Result<Option<ResolvedDelayedStart>, DelayedRunError> {
    if let Some(in_str) = run_in {
        let duration = parse_relative_delay(in_str)?;
        let scheduled_at = resolve_relative_delay(duration, now)?;
        Ok(Some(ResolvedDelayedStart {
            scheduled_at,
            delay: duration,
            source: DelayedStartSource::Relative,
        }))
    } else if let Some(at_str) = at {
        let scheduled_at = parse_absolute_time(at_str, now)?;
        let duration_to_target = scheduled_at.signed_duration_since(now);
        let delay = duration_to_target.to_std().map_err(|_| DelayedRunError::DateTimeOverflow)?;
        Ok(Some(ResolvedDelayedStart {
            scheduled_at,
            delay,
            source: DelayedStartSource::Absolute,
        }))
    } else {
        Ok(None)
    }
}

pub async fn wait_until_start(
    schedule: &ResolvedDelayedStart,
) -> Result<DelayedRunOutcome, DelayedRunError> {
    wait_with_cancellation(schedule.delay, tokio::signal::ctrl_c()).await
}

pub fn block_on_wait(
    schedule: &ResolvedDelayedStart,
) -> Result<DelayedRunOutcome, DelayedRunError> {
    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(DelayedRunError::Signal)?;
    rt.block_on(wait_until_start(schedule))
}

pub async fn wait_with_cancellation<C>(
    delay: Duration,
    cancellation: C,
) -> Result<DelayedRunOutcome, DelayedRunError>
where
    C: std::future::Future<Output = Result<(), std::io::Error>>,
{
    tokio::select! {
        _ = tokio::time::sleep(delay) => Ok(DelayedRunOutcome::Ready),
        signal_result = cancellation => {
            signal_result.map_err(DelayedRunError::Signal)?;
            Ok(DelayedRunOutcome::Cancelled)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    #[test]
    fn test_parse_relative_delay() {
        assert_eq!(parse_relative_delay("30s").unwrap(), Duration::from_secs(30));
        assert_eq!(parse_relative_delay("30m").unwrap(), Duration::from_secs(1800));
        assert_eq!(parse_relative_delay("2h").unwrap(), Duration::from_secs(7200));
        // Compound duration (spec §16.1)
        assert_eq!(parse_relative_delay("1h 30m").unwrap(), Duration::from_secs(5400));
        assert_eq!(parse_relative_delay("1d").unwrap(), Duration::from_secs(86400));

        assert!(matches!(parse_relative_delay("0s"), Err(DelayedRunError::ZeroDuration)));
        assert!(matches!(parse_relative_delay("abc"), Err(DelayedRunError::InvalidDuration { .. })));
        assert!(matches!(parse_relative_delay(""), Err(DelayedRunError::InvalidDuration { .. })));
        // Negative input (spec §16.1)
        assert!(matches!(parse_relative_delay("-5m"), Err(DelayedRunError::InvalidDuration { .. })));
    }

    #[test]
    fn test_parse_absolute_time() {
        let now = Local.with_ymd_and_hms(2026, 6, 26, 12, 0, 0).unwrap();

        // Future absolute time
        let future_rfc = "2026-06-26T13:00:00Z";
        let parsed = parse_absolute_time(future_rfc, now).unwrap();
        assert_eq!(parsed.with_timezone(&chrono::Utc).to_rfc3339(), "2026-06-26T13:00:00+00:00");

        let future_local_sec = "2026-06-26T14:30:15";
        let parsed_sec = parse_absolute_time(future_local_sec, now).unwrap();
        assert_eq!(parsed_sec.format("%Y-%m-%dT%H:%M:%S").to_string(), "2026-06-26T14:30:15");

        let future_local = "2026-06-26T14:30";
        let parsed_local = parse_absolute_time(future_local, now).unwrap();
        assert_eq!(parsed_local.format("%Y-%m-%dT%H:%M").to_string(), "2026-06-26T14:30");

        // Past time
        let past = "2026-06-26T11:00:00Z";
        assert!(matches!(parse_absolute_time(past, now), Err(DelayedRunError::PastDateTime { .. })));

        // Invalid format
        let invalid = "2026/06/26 12:00";
        assert!(matches!(parse_absolute_time(invalid, now), Err(DelayedRunError::InvalidDateTime { .. })));
    }

    #[test]
    fn test_resolve_delayed_start() {
        let now = Local.with_ymd_and_hms(2026, 6, 26, 12, 0, 0).unwrap();

        // Neither
        let resolved = resolve_delayed_start(None, None, now).unwrap();
        assert!(resolved.is_none());

        // Relative
        let resolved = resolve_delayed_start(Some("30m"), None, now).unwrap().unwrap();
        assert_eq!(resolved.delay, Duration::from_secs(1800));
        assert_eq!(resolved.source, DelayedStartSource::Relative);
        assert_eq!(resolved.scheduled_at, now + chrono::Duration::seconds(1800));

        // Absolute
        let resolved = resolve_delayed_start(None, Some("2026-06-26T14:00:00"), now).unwrap().unwrap();
        assert_eq!(resolved.delay, Duration::from_secs(7200));
        assert_eq!(resolved.source, DelayedStartSource::Absolute);

        // Relative overflow (spec §16.3)
        let overflow = resolve_delayed_start(Some("999999999999999999d"), None, now);
        assert!(overflow.is_err());
    }

    #[tokio::test(start_paused = true)]
    async fn test_waiting_success() {
        let now = Local::now();
        let schedule = ResolvedDelayedStart {
            scheduled_at: now + chrono::Duration::seconds(2),
            delay: Duration::from_secs(2),
            source: DelayedStartSource::Relative,
        };

        // We use a future that doesn't resolve for the cancellation to simulate no Ctrl+C
        let cancellation = futures_util::future::pending();
        let outcome = wait_with_cancellation(schedule.delay, cancellation).await.unwrap();
        assert_eq!(outcome, DelayedRunOutcome::Ready);
    }

    #[tokio::test(start_paused = true)]
    async fn test_waiting_cancelled() {
        let schedule = ResolvedDelayedStart {
            scheduled_at: Local::now() + chrono::Duration::seconds(10),
            delay: Duration::from_secs(10),
            source: DelayedStartSource::Relative,
        };

        // Instantly cancelled
        let cancellation = futures_util::future::ready(Ok(()));
        let outcome = wait_with_cancellation(schedule.delay, cancellation).await.unwrap();
        assert_eq!(outcome, DelayedRunOutcome::Cancelled);
    }

    #[tokio::test(start_paused = true)]
    async fn test_waiting_signal_error() {
        let delay = Duration::from_secs(10);
        // Simulate a signal listener failure (spec §16.4)
        let cancellation = futures_util::future::ready(
            Err(std::io::Error::new(std::io::ErrorKind::Other, "signal setup failed")),
        );
        let result = wait_with_cancellation(delay, cancellation).await;
        assert!(matches!(result, Err(DelayedRunError::Signal(_))));
    }
}
