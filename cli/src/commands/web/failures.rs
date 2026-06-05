use super::WebState;
use axum::extract::State;
use axum::Json;
use serde::Serialize;

#[derive(Serialize)]
pub(crate) struct RecentFailure {
    task_id: Option<String>,
    tool: Option<String>,
    worktree: Option<String>,
    error_code: Option<String>,
    retryable: bool,
    excerpt: Option<String>,
    ts: Option<String>,
}

pub(crate) async fn recent_failures_handler(
    State(state): State<WebState>,
) -> Json<Vec<RecentFailure>> {
    let events_path = state
        .paths
        .macc_dir
        .join("log")
        .join("coordinator")
        .join("events.jsonl");

    if !events_path.exists() {
        return Json(Vec::new());
    }

    let Ok(content) = std::fs::read_to_string(&events_path) else {
        return Json(Vec::new());
    };

    let failures: Vec<RecentFailure> = content
        .lines()
        .rev()
        .filter_map(|line| {
            let v: serde_json::Value = serde_json::from_str(line).ok()?;
            let event_type = v
                .get("event_type")
                .or_else(|| v.get("type"))
                .and_then(|x| x.as_str())
                .unwrap_or("");
            let status = v.get("status").and_then(|x| x.as_str()).unwrap_or("");

            if !event_type.contains("fail") && !event_type.contains("error") && status != "failed" {
                return None;
            }

            Some(RecentFailure {
                task_id: v
                    .get("task_id")
                    .and_then(|x| x.as_str())
                    .map(|s| s.to_string()),
                tool: v
                    .get("tool")
                    .and_then(|x| x.as_str())
                    .map(|s| s.to_string()),
                worktree: v
                    .get("worktree")
                    .and_then(|x| x.as_str())
                    .map(|s| s.to_string()),
                error_code: v
                    .get("error_code")
                    .or_else(|| v.get("last_error_code"))
                    .and_then(|x| x.as_str())
                    .map(|s| s.to_string()),
                retryable: v.get("retryable").and_then(|x| x.as_bool()).unwrap_or(true),
                excerpt: v
                    .get("message")
                    .and_then(|x| x.as_str())
                    .map(|s| s.chars().take(200).collect()),
                ts: v.get("ts").and_then(|x| x.as_str()).map(|s| s.to_string()),
            })
        })
        .take(20)
        .collect();

    Json(failures)
}
