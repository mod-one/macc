//! Recovery must not grant transcript ownership from a stale job or snapshot.
use super::*;

#[test]
fn finished_job_requires_all_constraints_and_cannot_release_a_live_owner() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    fs::create_dir_all(root.join(".macc/state")).unwrap();
    let path = root.join(TOOL_SESSIONS_REL_PATH);
    let entry = |task: &str, worktree: &str, pid: i64| {
        json!({
            "status":"active", "owner_task_id":task, "owner_worktree":worktree,
            "owner_pid":pid, "session_identity":{"task_id":task,"role":"performer"}
        })
    };
    persist_sessions_file(
        &path,
        &json!({"tools":{"future-ai":{"sessions":{
            "finished":entry("A", "/worker-01", 99999999),
            "another-task-same-slot":entry("B", "/worker-01", 99999999),
            "same-task-other-slot":entry("A", "/worker-02", 99999999),
            "live":entry("A", "/worker-01", std::process::id() as i64),
            "ambiguous":entry("A", "/worker-01", 0)
        }}}}),
    )
    .unwrap();
    let released = release_job_sessions(
        root,
        Some("future-ai"),
        Some(Path::new("/worker-01")),
        Some("A"),
    )
    .unwrap();
    assert_eq!(released.len(), 1);
    assert_eq!(released[0].session_id, "finished");
    let after: Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
    let sessions = &after["tools"]["future-ai"]["sessions"];
    assert_eq!(sessions["finished"]["status"], "abandoned");
    assert_eq!(sessions["finished"]["session_identity"]["task_id"], "A");
    for sid in [
        "another-task-same-slot",
        "same-task-other-slot",
        "live",
        "ambiguous",
    ] {
        assert_eq!(sessions[sid]["status"], "active", "{sid}");
    }
}

#[test]
fn stale_recovery_retains_semantic_identity_and_quarantines_unclean_history() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    fs::create_dir_all(root.join(".macc/state")).unwrap();
    let path = root.join(TOOL_SESSIONS_REL_PATH);
    let identity = json!({"task_id":"A", "role":"reviewer", "tool_id":"future-ai"});
    persist_sessions_file(
        &path,
        &json!({"tools":{"future-ai":{"sessions":{
            "orphan":{"status":"active", "owner_pid":99999999, "heartbeat_epoch":0,
                      "owner_task_id":"A", "session_identity":identity},
            "unknown":{"status":"active", "heartbeat_epoch":0, "session_identity":identity}
        }}}}),
    )
    .unwrap();
    assert_eq!(reset_stale_active_sessions(root, 0).unwrap(), 1);
    let after: Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
    assert_eq!(
        after["tools"]["future-ai"]["sessions"]["orphan"]["status"],
        "abandoned"
    );
    assert_eq!(
        after["tools"]["future-ai"]["sessions"]["orphan"]["session_identity"],
        identity
    );
    assert_eq!(
        after["tools"]["future-ai"]["sessions"]["unknown"]["status"],
        "active"
    );
}
