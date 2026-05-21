use crate::coordinator::helpers::{
    append_coordinator_event_with_severity, ensure_coordinator_run_id, now_iso_coordinator,
};
use crate::process_ownership::{ClientIdentity, ProcessHandle};
use serde_json::{Map, Value};
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::Path;

pub struct OwnershipEventBroadcaster;

impl OwnershipEventBroadcaster {
    pub fn emit(
        repo_root: &Path,
        event_type: &str,
        handle: &ProcessHandle,
        payload_fields: Map<String, Value>,
    ) {
        let message = serialize_message(handle, payload_fields);
        let raw_event = build_raw_event(event_type, &message);
        if let Err(err) = append_coordinator_event_with_severity(
            repo_root,
            event_type,
            "-",
            "ownership",
            "info",
            &message,
            "info",
        ) {
            tracing::warn!(
                event_type = event_type,
                process_kind = ?handle.kind,
                pid = handle.pid,
                "failed to append ownership event: {}",
                err
            );
        }
        if let Err(err) = append_event_jsonl(repo_root, &raw_event) {
            tracing::warn!(
                event_type = event_type,
                process_kind = ?handle.kind,
                pid = handle.pid,
                "failed to append ownership event jsonl: {}",
                err
            );
        }
    }
}

pub fn emit_ownership_claimed(repo_root: &Path, handle: &ProcessHandle, client: &ClientIdentity) {
    OwnershipEventBroadcaster::emit(
        repo_root,
        "ownership_claimed",
        handle,
        client_payload(client),
    );
}

pub fn emit_ownership_released(repo_root: &Path, handle: &ProcessHandle, client: &ClientIdentity) {
    OwnershipEventBroadcaster::emit(
        repo_root,
        "ownership_released",
        handle,
        client_payload(client),
    );
}

pub fn emit_viewer_joined(repo_root: &Path, handle: &ProcessHandle, client: &ClientIdentity) {
    OwnershipEventBroadcaster::emit(repo_root, "viewer_joined", handle, client_payload(client));
}

pub fn emit_viewer_left(repo_root: &Path, handle: &ProcessHandle, client: &ClientIdentity) {
    OwnershipEventBroadcaster::emit(repo_root, "viewer_left", handle, client_payload(client));
}

pub fn emit_takeover_requested(
    repo_root: &Path,
    handle: &ProcessHandle,
    requester: &ClientIdentity,
    request_id: &str,
) {
    let mut payload = client_payload(requester);
    payload.insert(
        "request_id".to_string(),
        Value::String(request_id.to_string()),
    );
    OwnershipEventBroadcaster::emit(repo_root, "takeover_requested", handle, payload);
}

pub fn emit_takeover_accepted(
    repo_root: &Path,
    handle: &ProcessHandle,
    requester: &ClientIdentity,
    request_id: &str,
) {
    let mut payload = client_payload(requester);
    payload.insert(
        "request_id".to_string(),
        Value::String(request_id.to_string()),
    );
    OwnershipEventBroadcaster::emit(repo_root, "takeover_accepted", handle, payload);
}

pub fn emit_takeover_rejected(
    repo_root: &Path,
    handle: &ProcessHandle,
    requester: &ClientIdentity,
    request_id: &str,
) {
    let mut payload = client_payload(requester);
    payload.insert(
        "request_id".to_string(),
        Value::String(request_id.to_string()),
    );
    OwnershipEventBroadcaster::emit(repo_root, "takeover_rejected", handle, payload);
}

pub fn emit_owner_stale(repo_root: &Path, handle: &ProcessHandle, client: &ClientIdentity) {
    OwnershipEventBroadcaster::emit(repo_root, "owner_stale", handle, client_payload(client));
}

fn client_payload(client: &ClientIdentity) -> Map<String, Value> {
    let mut payload = Map::new();
    payload.insert("client".to_string(), serde_json::json!(client));
    payload
}

fn serialize_message(handle: &ProcessHandle, payload_fields: Map<String, Value>) -> String {
    let mut message = Map::new();
    message.insert("process".to_string(), serde_json::json!(handle));
    message.extend(payload_fields);
    Value::Object(message).to_string()
}

fn build_raw_event(event_type: &str, message: &str) -> Value {
    let seq = chrono::Utc::now().timestamp_nanos_opt().unwrap_or_default();
    serde_json::json!({
        "schema_version": "1",
        "event_id": format!("evt-{}---{}", event_type, seq),
        "run_id": ensure_coordinator_run_id(),
        "seq": seq,
        "ts": now_iso_coordinator(),
        "source": "coordinator:native",
        "task_id": "-",
        "type": event_type,
        "phase": "ownership",
        "status": "info",
        "severity": "info",
        "payload": {
            "message": message
        }
    })
}

fn append_event_jsonl(repo_root: &Path, event: &Value) -> std::io::Result<()> {
    let events_path = repo_root
        .join(".macc")
        .join("log")
        .join("coordinator")
        .join("events.jsonl");
    if let Some(parent) = events_path.parent() {
        fs::create_dir_all(parent)?;
    }
    let mut file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(events_path)?;
    serde_json::to_writer(&mut file, event)?;
    file.write_all(b"\n")?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::emit_ownership_claimed;
    use crate::process_ownership::{ClientIdentity, ClientKind, ProcessHandle, ProcessKind};
    use serde_json::Value;
    use std::fs;
    use std::path::PathBuf;
    use std::time::{SystemTime, UNIX_EPOCH};

    #[test]
    fn emit_ownership_claimed_appends_valid_json_line() {
        let repo_root = temp_repo_root();
        let handle = ProcessHandle {
            kind: ProcessKind::Coordinator,
            project_root: repo_root.clone(),
            pid: Some(4242),
        };
        let client = ClientIdentity {
            client_id: "client-1".to_string(),
            kind: ClientKind::Cli,
            connected_at: "2026-05-18T12:00:00Z".to_string(),
        };

        emit_ownership_claimed(&repo_root, &handle, &client);

        let events_path = repo_root
            .join(".macc")
            .join("log")
            .join("coordinator")
            .join("events.jsonl");
        let raw = fs::read_to_string(&events_path).expect("read events.jsonl");
        let line = raw.lines().last().expect("event line");
        let event: Value = serde_json::from_str(line).expect("valid json line");
        assert_eq!(
            event.get("type").and_then(Value::as_str),
            Some("ownership_claimed")
        );
        assert_eq!(event.get("task_id").and_then(Value::as_str), Some("-"));
        assert_eq!(
            event.get("phase").and_then(Value::as_str),
            Some("ownership")
        );
        assert_eq!(event.get("severity").and_then(Value::as_str), Some("info"));

        let message = event
            .get("payload")
            .and_then(|payload| payload.get("message"))
            .and_then(Value::as_str)
            .expect("payload.message");
        let message_json: Value = serde_json::from_str(message).expect("message json");
        assert_eq!(
            message_json
                .get("client")
                .and_then(|value| value.get("client_id"))
                .and_then(Value::as_str),
            Some("client-1")
        );
        assert_eq!(
            message_json
                .get("process")
                .and_then(|value| value.get("kind"))
                .and_then(Value::as_str),
            Some("Coordinator")
        );
        assert_eq!(
            message_json
                .get("process")
                .and_then(|value| value.get("pid"))
                .and_then(Value::as_i64),
            Some(4242)
        );

        let _ = fs::remove_dir_all(repo_root);
    }

    fn temp_repo_root() -> PathBuf {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system time")
            .as_nanos();
        let root = std::env::temp_dir().join(format!(
            "macc-ownership-events-{}-{}",
            std::process::id(),
            unique
        ));
        fs::create_dir_all(&root).expect("create temp repo root");
        root
    }
}
