//! Timestamp-scoped fallback for launcher failures absent from the SQLite event mirror.
use super::diagnostic::{build_failure_report, FailureReport};
use crate::{MaccError, ProjectPaths, Result};
use chrono::{DateTime, Utc};
use std::io::{Read, Seek, SeekFrom};

pub(super) fn failure(
    paths: &ProjectPaths,
    since: Option<&DateTime<Utc>>,
) -> Result<Option<FailureReport>> {
    let path = paths.root.join(".macc/log/events.jsonl");
    let mut file = match std::fs::File::open(&path) {
        Ok(f) => f,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(source) => {
            return Err(MaccError::Io {
                path: path.display().to_string(),
                action: "read scoped coordinator errors".into(),
                source,
            })
        }
    };
    let read = |source| MaccError::Io {
        path: path.display().to_string(),
        action: "read scoped coordinator errors".into(),
        source,
    };
    let len = file.metadata().map_err(read)?.len();
    file.seek(SeekFrom::Start(len.saturating_sub(256 * 1024)))
        .map_err(read)?;
    let mut bytes = Vec::new();
    file.read_to_end(&mut bytes).map_err(read)?;
    let tail = String::from_utf8_lossy(&bytes);
    for line in tail.lines().rev() {
        let Ok(event) = serde_json::from_str::<serde_json::Value>(line) else {
            continue;
        };
        let ts = event
            .get("timestamp")
            .or_else(|| event.get("ts"))
            .and_then(|v| v.as_str());
        if since.is_some_and(|since| {
            !ts.is_some_and(|s| {
                DateTime::parse_from_rfc3339(s).is_ok_and(|t| t.with_timezone(&Utc) >= *since)
            })
        }) {
            continue;
        }
        let kind = event
            .get("event_type")
            .or_else(|| event.get("type"))
            .and_then(|v| v.as_str())
            .unwrap_or_default();
        if event.get("severity").and_then(|v| v.as_str()) != Some("blocking")
            || !matches!(kind, "command_error" | "command_end" | "prd_queue_stopped")
        {
            continue;
        }
        let Some(message) = event
            .get("message")
            .or_else(|| event.pointer("/payload/message"))
            .and_then(|v| v.as_str())
        else {
            continue;
        };
        let text = |key| {
            event
                .get(key)
                .and_then(|v| v.as_str())
                .filter(|s| !s.is_empty() && *s != "-")
                .map(str::to_owned)
        };
        return Ok(Some(build_failure_report(
            message.into(),
            text("task_id"),
            text("phase"),
            "scoped_event_log",
            true,
            Some(kind.into()),
        )));
    }
    Ok(None)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn current_run_failure_surfaces_e902_without_attributing_previous_runs() {
        let tmp = tempfile::tempdir().unwrap();
        let paths = ProjectPaths::from_root(tmp.path());
        std::fs::create_dir_all(tmp.path().join(".macc/log")).unwrap();
        std::fs::write(tmp.path().join(".macc/log/events.jsonl"),serde_json::json!({
            "timestamp":"2026-10-19T14:16:02Z","event_type":"prd_queue_stopped","severity":"blocking",
            "message":"Coordinator cannot continue: ROOT blocked E902; retained work preserved","task_id":"ROOT","phase":"run"
        }).to_string()+"\n").unwrap();
        let current = DateTime::parse_from_rfc3339("2026-10-19T14:16:01Z")
            .unwrap()
            .with_timezone(&Utc);
        assert!(failure(&paths, Some(&current))
            .unwrap()
            .unwrap()
            .message
            .contains("E902"));
        let later = current + chrono::Duration::seconds(2);
        assert!(failure(&paths, Some(&later)).unwrap().is_none());
    }
}
