use super::WebState;
use axum::body::{to_bytes, Body, Bytes};
use axum::extract::{Request, State};
use axum::http::{header::CONTENT_TYPE, Method, Uri};
use axum::middleware::Next;
use axum::response::Response;
use chrono::Utc;
use serde::Serialize;
use serde_json::Value;
use std::path::PathBuf;
use std::time::Instant;
use tokio::fs::{self, OpenOptions};
use tokio::io::AsyncWriteExt;

const AUDIT_LOG_PATH: &str = ".macc/log/ops.jsonl";
const REQUEST_BODY_LIMIT_BYTES: usize = 1024 * 1024;
const SUMMARY_TEXT_LIMIT: usize = 240;

#[derive(Serialize)]
struct AuditRecord {
    timestamp: String,
    actor: String,
    action: String,
    method: String,
    path: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    inputs_summary: Option<Value>,
    result: AuditResult,
    duration_ms: u64,
    log_path: &'static str,
}

#[derive(Serialize)]
struct AuditResult {
    status_code: u16,
}

pub(super) async fn audit_middleware(
    State(state): State<WebState>,
    request: Request,
    next: Next,
) -> Response {
    let method = request.method().clone();
    let path = request.uri().path().to_string();
    if should_skip_request(&method, &path) {
        return next.run(request).await;
    }

    let content_type = request
        .headers()
        .get(CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .map(str::to_owned);
    let uri = request.uri().clone();
    let (parts, body) = request.into_parts();
    let body_bytes = match to_bytes(body, REQUEST_BODY_LIMIT_BYTES).await {
        Ok(bytes) => bytes,
        Err(err) => {
            tracing::warn!("audit middleware could not read request body: {}", err);
            Bytes::new()
        }
    };
    let summary = summarize_inputs(&uri, content_type.as_deref(), &body_bytes);
    let request = Request::from_parts(parts, Body::from(body_bytes));

    let started_at = Instant::now();
    let response = next.run(request).await;
    let duration_ms = started_at.elapsed().as_millis().min(u128::from(u64::MAX)) as u64;
    let status_code = response.status().as_u16();

    let record = AuditRecord {
        timestamp: Utc::now().to_rfc3339(),
        actor: local_user(),
        action: format!("{} {}", method, path),
        method: method.to_string(),
        path,
        inputs_summary: summary,
        result: AuditResult { status_code },
        duration_ms,
        log_path: AUDIT_LOG_PATH,
    };
    if let Err(err) = append_record(&state, &record).await {
        tracing::warn!("audit log write failed: {}", err);
    }

    response
}

fn should_skip_request(method: &Method, path: &str) -> bool {
    !matches!(method.as_str(), "POST" | "PUT" | "DELETE")
        || matches!(path, "/api/v1/health" | "/api/v1/events")
}

fn summarize_inputs(uri: &Uri, content_type: Option<&str>, body_bytes: &Bytes) -> Option<Value> {
    let mut summary = serde_json::Map::new();
    if let Some(query) = uri.query() {
        summary.insert(
            "query".to_string(),
            Value::String(truncate(query, SUMMARY_TEXT_LIMIT)),
        );
    }
    if let Some(content_type) = content_type {
        summary.insert(
            "content_type".to_string(),
            Value::String(content_type.to_string()),
        );
    }
    if !body_bytes.is_empty() {
        summary.insert(
            "body_bytes".to_string(),
            Value::Number(serde_json::Number::from(body_bytes.len() as u64)),
        );
        summary.insert("body".to_string(), summarize_body(body_bytes));
    }
    (!summary.is_empty()).then_some(Value::Object(summary))
}

fn summarize_body(body_bytes: &Bytes) -> Value {
    match serde_json::from_slice::<Value>(body_bytes) {
        Ok(Value::Object(map)) => Value::Object(
            [
                ("kind".to_string(), Value::String("json-object".to_string())),
                (
                    "keys".to_string(),
                    Value::Array(map.keys().cloned().map(Value::String).collect()),
                ),
            ]
            .into_iter()
            .collect(),
        ),
        Ok(Value::Array(items)) => Value::Object(
            [
                ("kind".to_string(), Value::String("json-array".to_string())),
                (
                    "items".to_string(),
                    Value::Number(serde_json::Number::from(items.len() as u64)),
                ),
            ]
            .into_iter()
            .collect(),
        ),
        Ok(_) => Value::Object(
            [("kind".to_string(), Value::String("json-scalar".to_string()))]
                .into_iter()
                .collect(),
        ),
        Err(_) => Value::Object(
            [
                ("kind".to_string(), Value::String("text".to_string())),
                (
                    "preview".to_string(),
                    Value::String(truncate(
                        &String::from_utf8_lossy(body_bytes),
                        SUMMARY_TEXT_LIMIT,
                    )),
                ),
            ]
            .into_iter()
            .collect(),
        ),
    }
}

fn truncate(value: &str, limit: usize) -> String {
    let mut chars = value.chars();
    let truncated: String = chars.by_ref().take(limit).collect();
    if chars.next().is_some() {
        format!("{}...", truncated)
    } else {
        truncated
    }
}

fn local_user() -> String {
    std::env::var("USER")
        .or_else(|_| std::env::var("USERNAME"))
        .unwrap_or_else(|_| "unknown".to_string())
}

async fn append_record(state: &WebState, record: &AuditRecord) -> std::io::Result<()> {
    let path = audit_log_path(state);
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).await?;
    }

    let mut file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)
        .await?;
    let mut line = serde_json::to_vec(record)
        .map_err(|err| std::io::Error::other(format!("serialize audit record: {}", err)))?;
    line.push(b'\n');
    file.write_all(&line).await?;
    file.flush().await
}

fn audit_log_path(state: &WebState) -> PathBuf {
    state.paths.root.join(AUDIT_LOG_PATH)
}
