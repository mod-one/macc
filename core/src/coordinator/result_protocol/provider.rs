//! Only provider-owned error records reach adapter error normalizers.
use super::{Channel, State};
use serde_json::Value;
fn structured_error(line: &str) -> bool {
    // Adapter error envelopes may prefix a structured record, e.g.
    // FUTURE_ERROR: {...}. Recognition is independent of the adapter ID.
    let envelope = line.split_once(':').filter(|(prefix, _)| {
        prefix.ends_with("_ERROR") && prefix.chars().all(|c| c.is_ascii_uppercase() || c == '_')
    });
    let raw = envelope.map(|(_, value)| value.trim()).unwrap_or(line);
    let Ok(v) = serde_json::from_str::<Value>(raw) else {
        return false;
    };
    v["type"] == "error"
        || v["is_error"] == true
        || v["type"] == "result"
            && v["subtype"]
                .as_str()
                .is_some_and(|s| s.starts_with("error"))
        || !v["error"].is_null() && v.get("type").is_none()
        || envelope.is_some()
            && (v["error_code"].as_u64().is_some_and(|n| n >= 400)
                || v["status"].as_str().is_some_and(|s| s != "OK"))
}
fn plain_error(line: &str) -> bool {
    let text = line.to_lowercase();
    [
        "error:",
        "error ",
        "fatal:",
        "429",
        "529",
        "resource_exhausted",
        "insufficient_quota",
        "rate limit",
        "too many requests",
        "permission denied",
        "authentication failed",
        "invalid api key",
        "not authenticated",
        "you've hit your ",
        "you’ve hit your ",
        "you have reached your ",
        "your usage limit",
        "quota exceeded",
        "individual quota reached",
        "api error:",
    ]
    .iter()
    .any(|p| text.starts_with(p))
}
pub fn provider_output(raw: &str) -> String {
    let mut state = State::default();
    let mut out = String::new();
    for line in raw.lines() {
        if !state.line(line) {
            continue;
        }
        // Structured provider errors are explicit records; ordinary assistant
        // prose, command snippets and echoed limit strings are never evidence.
        if state.channel == Channel::Provider && (structured_error(line) || plain_error(line)) {
            out.push_str(line);
            out.push('\n');
        }
    }
    out
}
