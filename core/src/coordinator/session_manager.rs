use crate::{MaccError, Result};
use serde::{Deserialize, Serialize};
use serde_json::{json, Map, Value};
use std::fs;
use std::path::{Path, PathBuf};
use std::time::Duration;

const TOOL_SESSIONS_REL_PATH: &str = ".macc/state/tool-sessions.json";

#[derive(Debug, Clone, Default)]
pub struct SessionSealOutcome {
    pub sealed: bool,
    pub session_id: Option<String>,
}

pub fn seal_worktree_scoped_session(
    repo_root: &Path,
    tool_id: &str,
    worktree_path: &Path,
    task_id: &str,
    now_iso: &str,
) -> Result<SessionSealOutcome> {
    if tool_id.trim().is_empty() {
        return Ok(SessionSealOutcome::default());
    }

    let scope = read_session_scope(worktree_path).unwrap_or_else(|| "worktree".to_string());
    if scope != "worktree" {
        return Ok(SessionSealOutcome::default());
    }

    let sessions_path = repo_root.join(TOOL_SESSIONS_REL_PATH);
    if !sessions_path.exists() {
        return Ok(SessionSealOutcome::default());
    }
    let lock_dir = sessions_path.with_extension("json.lock");
    acquire_lock(&lock_dir)?;

    let result = (|| {
        let raw = fs::read_to_string(&sessions_path).map_err(|e| MaccError::Io {
            path: sessions_path.to_string_lossy().into(),
            action: "read tool sessions state".into(),
            source: e,
        })?;
        let mut root: Value = serde_json::from_str(&raw).map_err(|e| {
            MaccError::Validation(format!(
                "Failed to parse sessions file '{}': {}",
                sessions_path.display(),
                e
            ))
        })?;

        let tools = root
            .get_mut("tools")
            .and_then(Value::as_object_mut)
            .ok_or_else(|| {
                MaccError::Validation("sessions file missing root .tools object".into())
            })?;
        let Some(tool) = tools.get_mut(tool_id).and_then(Value::as_object_mut) else {
            return Ok(SessionSealOutcome::default());
        };
        let Some(sessions_obj) = tool.get_mut("sessions").and_then(Value::as_object_mut) else {
            return Ok(SessionSealOutcome::default());
        };

        let key_candidates = worktree_key_candidates(worktree_path);
        let mut selected_key: Option<String> = None;
        let mut session_id: Option<String> = None;
        for key in key_candidates {
            let Some(candidate) = sessions_obj.get(&key) else {
                continue;
            };
            let sid = candidate
                .get("session_id")
                .and_then(Value::as_str)
                .unwrap_or_default();
            if sid.is_empty() {
                continue;
            }
            selected_key = Some(key);
            session_id = Some(sid.to_string());
            break;
        }

        let (Some(selected_key), Some(session_id)) = (selected_key, session_id) else {
            return Ok(SessionSealOutcome::default());
        };

        sessions_obj.remove(&selected_key);

        let archived = tool
            .entry("archived")
            .or_insert_with(|| Value::Object(Map::new()))
            .as_object_mut()
            .ok_or_else(|| MaccError::Validation("sessions .archived must be an object".into()))?;
        let archive_key = format!("{}-{}", task_id, now_iso.replace(':', ""));
        archived.insert(
            archive_key,
            json!({
                "task_id": task_id,
                "session_id": session_id,
                "sealed_at": now_iso,
                "scope": "worktree",
                "session_key": selected_key,
                "worktree_path": worktree_path.to_string_lossy().to_string(),
                "reason": "task_commit_completed",
            }),
        );

        if let Some(leases_obj) = tool.get_mut("leases").and_then(Value::as_object_mut) {
            if let Some(lease) = leases_obj
                .get_mut(&session_id)
                .and_then(Value::as_object_mut)
            {
                lease.insert("status".to_string(), Value::String("sealed".to_string()));
                lease.insert("updated_at".to_string(), Value::String(now_iso.to_string()));
            }
        }

        persist_sessions_file(&sessions_path, &root)?;

        Ok(SessionSealOutcome {
            sealed: true,
            session_id: Some(session_id),
        })
    })();

    release_lock(&lock_dir);
    result
}

fn read_session_scope(worktree_path: &Path) -> Option<String> {
    let tool_json_path = worktree_path.join(".macc/tool.json");
    let raw = fs::read_to_string(tool_json_path).ok()?;
    let parsed: Value = serde_json::from_str(&raw).ok()?;
    Some(
        parsed
            .get("performer")
            .and_then(|v| v.get("session"))
            .and_then(|v| v.get("scope"))
            .and_then(Value::as_str)
            .unwrap_or("worktree")
            .to_string(),
    )
}

fn persist_sessions_file(path: &Path, value: &Value) -> Result<()> {
    let mut body = serde_json::to_string_pretty(value).map_err(|e| {
        MaccError::Validation(format!(
            "serialize sessions file '{}': {}",
            path.display(),
            e
        ))
    })?;
    body.push('\n');
    let tmp = path.with_extension("tmp");
    fs::write(&tmp, body).map_err(|e| MaccError::Io {
        path: tmp.to_string_lossy().into(),
        action: "write sessions temp".into(),
        source: e,
    })?;
    fs::rename(&tmp, path).map_err(|e| MaccError::Io {
        path: path.to_string_lossy().into(),
        action: "persist sessions state".into(),
        source: e,
    })?;
    Ok(())
}

fn worktree_key_candidates(worktree_path: &Path) -> Vec<String> {
    let mut keys = Vec::new();
    keys.push(worktree_path.to_string_lossy().to_string());
    if let Ok(canon) = fs::canonicalize(worktree_path) {
        let canon_s = canon.to_string_lossy().to_string();
        if !keys.iter().any(|k| k == &canon_s) {
            keys.push(canon_s);
        }
    }
    keys
}

fn acquire_lock(lock_dir: &PathBuf) -> Result<()> {
    for _ in 0..80 {
        match fs::create_dir(lock_dir) {
            Ok(()) => return Ok(()),
            Err(err) if err.kind() == std::io::ErrorKind::AlreadyExists => {
                std::thread::sleep(Duration::from_millis(100));
            }
            Err(err) => {
                return Err(MaccError::Io {
                    path: lock_dir.to_string_lossy().into(),
                    action: "acquire tool session lock".into(),
                    source: err,
                });
            }
        }
    }
    Err(MaccError::Validation(format!(
        "Timed out acquiring tool session lock '{}'",
        lock_dir.display()
    )))
}

fn release_lock(lock_dir: &PathBuf) {
    let _ = fs::remove_dir(lock_dir);
}

// ---------------------------------------------------------------------------
// Session save / restore / list / delete
// ---------------------------------------------------------------------------

/// Metadata for a saved session snapshot.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SavedSessionMeta {
    pub name: String,
    pub saved_at: String,
    pub project_root: String,
    pub tool_count: usize,
    pub active_session_count: usize,
    pub archived_session_count: usize,
}

fn user_sessions_dir() -> Result<PathBuf> {
    let home = crate::find_user_home().ok_or(MaccError::HomeDirNotFound)?;
    Ok(home.join(".macc").join("sessions"))
}

fn project_slug(repo_root: &Path) -> String {
    let name = repo_root
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_else(|| "unknown".to_string());
    let sanitized: String = name
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.') {
                c
            } else {
                '_'
            }
        })
        .collect();
    sanitized
}

fn project_sessions_dir(repo_root: &Path) -> Result<PathBuf> {
    let base = user_sessions_dir()?;
    Ok(base.join(project_slug(repo_root)))
}

fn snapshot_path(repo_root: &Path, name: &str) -> Result<PathBuf> {
    let dir = project_sessions_dir(repo_root)?;
    Ok(dir.join(format!("{}.json", name)))
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct SessionSnapshot {
    saved_at: String,
    project_root: String,
    name: String,
    tools: Value,
}

/// Save current tool-sessions.json to user-level storage.
///
/// If `name` is None, a timestamp-based name is generated.
pub fn save_sessions(repo_root: &Path, name: Option<&str>) -> Result<SavedSessionMeta> {
    let sessions_path = repo_root.join(TOOL_SESSIONS_REL_PATH);
    if !sessions_path.exists() {
        return Err(MaccError::Validation(
            "No tool-sessions.json found. Nothing to save.".into(),
        ));
    }

    let lock_dir = sessions_path.with_extension("json.lock");
    acquire_lock(&lock_dir)?;
    let raw = fs::read_to_string(&sessions_path).map_err(|e| {
        release_lock(&lock_dir);
        MaccError::Io {
            path: sessions_path.to_string_lossy().into(),
            action: "read tool sessions for save".into(),
            source: e,
        }
    })?;
    release_lock(&lock_dir);

    let root: Value = serde_json::from_str(&raw).map_err(|e| {
        MaccError::Validation(format!(
            "Failed to parse sessions file '{}': {}",
            sessions_path.display(),
            e
        ))
    })?;

    let now = chrono::Utc::now().format("%Y-%m-%dT%H:%M:%SZ").to_string();
    let snapshot_name = name
        .map(|n| n.to_string())
        .unwrap_or_else(|| format!("auto-{}", now.replace(':', "")));

    let tools_obj = root
        .get("tools")
        .cloned()
        .unwrap_or(Value::Object(Map::new()));

    // Count sessions
    let (mut active_count, mut archived_count, mut tool_count) = (0usize, 0usize, 0usize);
    if let Some(tools) = tools_obj.as_object() {
        for (_tool_id, tool_val) in tools {
            tool_count += 1;
            if let Some(sessions) = tool_val.get("sessions").and_then(Value::as_object) {
                active_count += sessions.len();
            }
            if let Some(archived) = tool_val.get("archived").and_then(Value::as_object) {
                archived_count += archived.len();
            }
        }
    }

    let snapshot = SessionSnapshot {
        saved_at: now.clone(),
        project_root: repo_root.to_string_lossy().to_string(),
        name: snapshot_name.clone(),
        tools: tools_obj,
    };

    let dest = snapshot_path(repo_root, &snapshot_name)?;
    if let Some(parent) = dest.parent() {
        fs::create_dir_all(parent).map_err(|e| MaccError::Io {
            path: parent.to_string_lossy().into(),
            action: "create sessions snapshot directory".into(),
            source: e,
        })?;
    }

    let body = serde_json::to_string_pretty(&snapshot).map_err(|e| {
        MaccError::Validation(format!("Failed to serialize session snapshot: {}", e))
    })?;
    fs::write(&dest, format!("{}\n", body)).map_err(|e| MaccError::Io {
        path: dest.to_string_lossy().into(),
        action: "write session snapshot".into(),
        source: e,
    })?;

    Ok(SavedSessionMeta {
        name: snapshot_name,
        saved_at: now,
        project_root: repo_root.to_string_lossy().to_string(),
        tool_count,
        active_session_count: active_count,
        archived_session_count: archived_count,
    })
}

/// Restore sessions from a saved snapshot into the current tool-sessions.json.
///
/// Active sessions from the snapshot are merged by tool ID. Existing active
/// sessions in tool-sessions.json for the same tool are preserved (snapshot
/// entries do not overwrite them). This allows new coordinator runs to pick up
/// saved sessions for tools/worktrees that don't already have one.
pub fn restore_sessions(repo_root: &Path, name: &str, dry_run: bool) -> Result<SavedSessionMeta> {
    let src = snapshot_path(repo_root, name)?;
    if !src.exists() {
        return Err(MaccError::Validation(format!(
            "Session snapshot '{}' not found at '{}'",
            name,
            src.display()
        )));
    }

    let snap_raw = fs::read_to_string(&src).map_err(|e| MaccError::Io {
        path: src.to_string_lossy().into(),
        action: "read session snapshot".into(),
        source: e,
    })?;
    let snapshot: SessionSnapshot = serde_json::from_str(&snap_raw).map_err(|e| {
        MaccError::Validation(format!(
            "Failed to parse session snapshot '{}': {}",
            src.display(),
            e
        ))
    })?;

    // Count what we'd restore
    let snap_tools = snapshot.tools.as_object().cloned().unwrap_or_default();
    let (mut active_count, mut archived_count, mut tool_count) = (0usize, 0usize, 0usize);
    for (_tool_id, tool_val) in &snap_tools {
        tool_count += 1;
        if let Some(sessions) = tool_val.get("sessions").and_then(Value::as_object) {
            active_count += sessions.len();
        }
        if let Some(archived) = tool_val.get("archived").and_then(Value::as_object) {
            archived_count += archived.len();
        }
    }

    let meta = SavedSessionMeta {
        name: snapshot.name.clone(),
        saved_at: snapshot.saved_at.clone(),
        project_root: snapshot.project_root.clone(),
        tool_count,
        active_session_count: active_count,
        archived_session_count: archived_count,
    };

    if dry_run {
        return Ok(meta);
    }

    let sessions_path = repo_root.join(TOOL_SESSIONS_REL_PATH);
    let lock_dir = sessions_path.with_extension("json.lock");
    acquire_lock(&lock_dir)?;

    let result = (|| {
        // Load or initialize current sessions file
        let mut root: Value = if sessions_path.exists() {
            let raw = fs::read_to_string(&sessions_path).map_err(|e| MaccError::Io {
                path: sessions_path.to_string_lossy().into(),
                action: "read tool sessions for restore".into(),
                source: e,
            })?;
            serde_json::from_str(&raw).map_err(|e| {
                MaccError::Validation(format!(
                    "Failed to parse sessions file '{}': {}",
                    sessions_path.display(),
                    e
                ))
            })?
        } else {
            // Create minimal structure
            if let Some(parent) = sessions_path.parent() {
                let _ = fs::create_dir_all(parent);
            }
            json!({ "tools": {} })
        };

        let current_tools = root
            .as_object_mut()
            .unwrap()
            .entry("tools")
            .or_insert_with(|| Value::Object(Map::new()))
            .as_object_mut()
            .ok_or_else(|| {
                MaccError::Validation("sessions file .tools must be an object".into())
            })?;

        for (tool_id, snap_tool_val) in &snap_tools {
            let snap_tool = match snap_tool_val.as_object() {
                Some(o) => o,
                None => continue,
            };

            let current_tool = current_tools
                .entry(tool_id)
                .or_insert_with(|| Value::Object(Map::new()))
                .as_object_mut()
                .ok_or_else(|| {
                    MaccError::Validation(format!(
                        "sessions file .tools.{} must be an object",
                        tool_id
                    ))
                })?;

            // Merge active sessions: snapshot entries fill in gaps (don't overwrite)
            if let Some(snap_sessions) = snap_tool.get("sessions").and_then(Value::as_object) {
                let current_sessions = current_tool
                    .entry("sessions")
                    .or_insert_with(|| Value::Object(Map::new()))
                    .as_object_mut()
                    .ok_or_else(|| {
                        MaccError::Validation(format!(
                            "sessions .tools.{}.sessions must be an object",
                            tool_id
                        ))
                    })?;

                for (key, val) in snap_sessions {
                    if !current_sessions.contains_key(key) {
                        current_sessions.insert(key.clone(), val.clone());
                    }
                }
            }

            // Merge leases: snapshot leases fill in gaps
            if let Some(snap_leases) = snap_tool.get("leases").and_then(Value::as_object) {
                let current_leases = current_tool
                    .entry("leases")
                    .or_insert_with(|| Value::Object(Map::new()))
                    .as_object_mut()
                    .ok_or_else(|| {
                        MaccError::Validation(format!(
                            "sessions .tools.{}.leases must be an object",
                            tool_id
                        ))
                    })?;

                for (key, val) in snap_leases {
                    if !current_leases.contains_key(key) {
                        // Reset lease status to "available" so performers can claim it
                        let mut lease = val.clone();
                        if let Some(obj) = lease.as_object_mut() {
                            obj.insert(
                                "status".to_string(),
                                Value::String("available".to_string()),
                            );
                            obj.insert(
                                "restored_at".to_string(),
                                Value::String(
                                    chrono::Utc::now().format("%Y-%m-%dT%H:%M:%SZ").to_string(),
                                ),
                            );
                        }
                        current_leases.insert(key.clone(), lease);
                    }
                }
            }
        }

        persist_sessions_file(&sessions_path, &root)?;
        Ok(meta)
    })();

    release_lock(&lock_dir);
    result
}

/// List all saved session snapshots for the current project.
pub fn list_saved_sessions(repo_root: &Path) -> Result<Vec<SavedSessionMeta>> {
    let dir = project_sessions_dir(repo_root)?;
    if !dir.exists() {
        return Ok(Vec::new());
    }

    let mut results = Vec::new();
    let entries = fs::read_dir(&dir).map_err(|e| MaccError::Io {
        path: dir.to_string_lossy().into(),
        action: "list session snapshots".into(),
        source: e,
    })?;

    for entry in entries {
        let entry = match entry {
            Ok(e) => e,
            Err(_) => continue,
        };
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) != Some("json") {
            continue;
        }
        let raw = match fs::read_to_string(&path) {
            Ok(r) => r,
            Err(_) => continue,
        };
        let snapshot: SessionSnapshot = match serde_json::from_str(&raw) {
            Ok(s) => s,
            Err(_) => continue,
        };

        let snap_tools = snapshot.tools.as_object().cloned().unwrap_or_default();
        let (mut active_count, mut archived_count, mut tool_count) = (0usize, 0usize, 0usize);
        for (_tool_id, tool_val) in &snap_tools {
            tool_count += 1;
            if let Some(sessions) = tool_val.get("sessions").and_then(Value::as_object) {
                active_count += sessions.len();
            }
            if let Some(archived) = tool_val.get("archived").and_then(Value::as_object) {
                archived_count += archived.len();
            }
        }

        results.push(SavedSessionMeta {
            name: snapshot.name,
            saved_at: snapshot.saved_at,
            project_root: snapshot.project_root,
            tool_count,
            active_session_count: active_count,
            archived_session_count: archived_count,
        });
    }

    results.sort_by(|a, b| b.saved_at.cmp(&a.saved_at));
    Ok(results)
}

/// Delete a saved session snapshot.
pub fn delete_saved_session(repo_root: &Path, name: &str) -> Result<()> {
    let path = snapshot_path(repo_root, name)?;
    if !path.exists() {
        return Err(MaccError::Validation(format!(
            "Session snapshot '{}' not found at '{}'",
            name,
            path.display()
        )));
    }
    fs::remove_file(&path).map_err(|e| MaccError::Io {
        path: path.to_string_lossy().into(),
        action: "delete session snapshot".into(),
        source: e,
    })?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_dir(prefix: &str) -> PathBuf {
        let id = format!(
            "{}_{}_{}",
            prefix,
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or(0)
        );
        std::env::temp_dir().join(id)
    }

    #[test]
    fn seal_session_archives_only_matching_tool() {
        let root = temp_dir("macc_session_manager");
        let worktree = root.join(".macc/worktree/worker-01");
        fs::create_dir_all(worktree.join(".macc")).expect("create worktree");
        fs::create_dir_all(root.join(".macc/state")).expect("create state dir");
        fs::write(
            worktree.join(".macc/tool.json"),
            r#"{"id":"codex","performer":{"session":{"scope":"worktree"}}}"#,
        )
        .expect("write tool json");

        let sessions_path = root.join(TOOL_SESSIONS_REL_PATH);
        let sessions = json!({
            "tools": {
                "codex": {
                    "sessions": {
                        worktree.to_string_lossy().to_string(): {
                            "session_id": "codex-sid",
                            "updated_at": "2026-02-01T00:00:00Z"
                        }
                    },
                    "leases": {
                        "codex-sid": { "status": "active", "owner_worktree": worktree.to_string_lossy().to_string() }
                    }
                },
                "gemini": {
                    "sessions": {
                        worktree.to_string_lossy().to_string(): {
                            "session_id": "gemini-sid",
                            "updated_at": "2026-02-01T00:00:00Z"
                        }
                    },
                    "leases": {
                        "gemini-sid": { "status": "active", "owner_worktree": worktree.to_string_lossy().to_string() }
                    }
                }
            }
        });
        persist_sessions_file(&sessions_path, &sessions).expect("persist sessions seed");

        let outcome = seal_worktree_scoped_session(
            &root,
            "codex",
            &worktree,
            "TASK-1",
            "2026-02-23T00:00:00Z",
        )
        .expect("seal session");
        assert!(outcome.sealed);
        assert_eq!(outcome.session_id.as_deref(), Some("codex-sid"));

        let updated: Value = serde_json::from_str(&fs::read_to_string(&sessions_path).unwrap())
            .expect("updated sessions json");
        assert!(
            updated["tools"]["codex"]["sessions"]
                .as_object()
                .map(|o| o.is_empty())
                .unwrap_or(false),
            "codex session should be removed from active sessions"
        );
        assert_eq!(
            updated["tools"]["gemini"]["sessions"][worktree.to_string_lossy().to_string()]
                ["session_id"]
                .as_str(),
            Some("gemini-sid"),
            "gemini session must stay untouched"
        );

        let _ = fs::remove_dir_all(&root);
    }

    fn seed_sessions_file(root: &Path) -> PathBuf {
        let sessions_path = root.join(TOOL_SESSIONS_REL_PATH);
        fs::create_dir_all(sessions_path.parent().unwrap()).expect("create state dir");
        let sessions = json!({
            "tools": {
                "claude": {
                    "sessions": {
                        "/tmp/worker-01": {
                            "session_id": "claude-s1",
                            "updated_at": "2026-04-01T00:00:00Z"
                        },
                        "/tmp/worker-02": {
                            "session_id": "claude-s2",
                            "updated_at": "2026-04-01T00:00:00Z"
                        }
                    },
                    "leases": {
                        "claude-s1": { "status": "active" },
                        "claude-s2": { "status": "active" }
                    },
                    "archived": {
                        "TASK-1-20260401T000000Z": {
                            "task_id": "TASK-1",
                            "session_id": "claude-old",
                            "sealed_at": "2026-04-01T00:00:00Z"
                        }
                    }
                },
                "codex": {
                    "sessions": {
                        "/tmp/worker-03": {
                            "session_id": "codex-s1",
                            "updated_at": "2026-04-01T00:00:00Z"
                        }
                    },
                    "leases": {}
                }
            }
        });
        persist_sessions_file(&sessions_path, &sessions).expect("seed");
        sessions_path
    }

    #[test]
    fn save_sessions_creates_snapshot() {
        let root = temp_dir("macc_sess_save");
        seed_sessions_file(&root);

        let meta = save_sessions(&root, Some("test-snap")).expect("save");
        assert_eq!(meta.name, "test-snap");
        assert_eq!(meta.tool_count, 2);
        assert_eq!(meta.active_session_count, 3);
        assert_eq!(meta.archived_session_count, 1);

        // Verify snapshot file exists
        let snap = snapshot_path(&root, "test-snap").expect("snap path");
        assert!(snap.exists(), "snapshot file should exist");

        // Verify content round-trips
        let raw = fs::read_to_string(&snap).unwrap();
        let parsed: SessionSnapshot = serde_json::from_str(&raw).unwrap();
        assert_eq!(parsed.name, "test-snap");
        assert!(
            parsed.tools["claude"]["sessions"]
                .as_object()
                .unwrap()
                .len()
                == 2
        );

        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn save_sessions_auto_generates_name() {
        let root = temp_dir("macc_sess_save_auto");
        seed_sessions_file(&root);

        let meta = save_sessions(&root, None).expect("save auto");
        assert!(meta.name.starts_with("auto-"));

        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn save_sessions_no_file_returns_error() {
        let root = temp_dir("macc_sess_save_missing");
        let result = save_sessions(&root, Some("nope"));
        assert!(result.is_err());

        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn list_saved_sessions_returns_all() {
        let root = temp_dir("macc_sess_list");
        seed_sessions_file(&root);

        save_sessions(&root, Some("snap-a")).expect("save a");
        save_sessions(&root, Some("snap-b")).expect("save b");

        let list = list_saved_sessions(&root).expect("list");
        assert_eq!(list.len(), 2);
        let names: Vec<&str> = list.iter().map(|s| s.name.as_str()).collect();
        assert!(names.contains(&"snap-a"));
        assert!(names.contains(&"snap-b"));

        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn list_saved_sessions_empty_dir() {
        let root = temp_dir("macc_sess_list_empty");
        let list = list_saved_sessions(&root).expect("list");
        assert!(list.is_empty());

        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn delete_saved_session_removes_file() {
        let root = temp_dir("macc_sess_delete");
        seed_sessions_file(&root);
        save_sessions(&root, Some("to-delete")).expect("save");

        let snap = snapshot_path(&root, "to-delete").expect("path");
        assert!(snap.exists());

        delete_saved_session(&root, "to-delete").expect("delete");
        assert!(!snap.exists());

        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn delete_saved_session_missing_returns_error() {
        let root = temp_dir("macc_sess_delete_missing");
        let result = delete_saved_session(&root, "nonexistent");
        assert!(result.is_err());

        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn restore_sessions_merges_into_empty() {
        let root = temp_dir("macc_sess_restore_empty");
        seed_sessions_file(&root);
        save_sessions(&root, Some("snap")).expect("save");

        // Remove the sessions file to simulate a fresh project
        let sessions_path = root.join(TOOL_SESSIONS_REL_PATH);
        fs::remove_file(&sessions_path).expect("remove sessions");

        let meta = restore_sessions(&root, "snap", false).expect("restore");
        assert_eq!(meta.active_session_count, 3);

        // Verify sessions file was recreated
        assert!(sessions_path.exists());
        let restored: Value =
            serde_json::from_str(&fs::read_to_string(&sessions_path).unwrap()).unwrap();
        assert_eq!(
            restored["tools"]["claude"]["sessions"]["/tmp/worker-01"]["session_id"].as_str(),
            Some("claude-s1")
        );

        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn restore_sessions_does_not_overwrite_existing() {
        let root = temp_dir("macc_sess_restore_merge");
        seed_sessions_file(&root);
        save_sessions(&root, Some("snap")).expect("save");

        // Modify an existing session — restore should NOT overwrite it
        let sessions_path = root.join(TOOL_SESSIONS_REL_PATH);
        let raw = fs::read_to_string(&sessions_path).unwrap();
        let mut current: Value = serde_json::from_str(&raw).unwrap();
        current["tools"]["claude"]["sessions"]["/tmp/worker-01"]["session_id"] =
            Value::String("claude-s1-NEW".to_string());
        persist_sessions_file(&sessions_path, &current).expect("update");

        restore_sessions(&root, "snap", false).expect("restore");

        let after: Value =
            serde_json::from_str(&fs::read_to_string(&sessions_path).unwrap()).unwrap();
        // Should keep the NEW value, not overwrite with snapshot
        assert_eq!(
            after["tools"]["claude"]["sessions"]["/tmp/worker-01"]["session_id"].as_str(),
            Some("claude-s1-NEW"),
            "existing session should not be overwritten by restore"
        );

        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn restore_sessions_dry_run_does_not_write() {
        let root = temp_dir("macc_sess_restore_dry");
        seed_sessions_file(&root);
        save_sessions(&root, Some("snap")).expect("save");

        // Remove sessions file
        let sessions_path = root.join(TOOL_SESSIONS_REL_PATH);
        fs::remove_file(&sessions_path).expect("remove");

        let meta = restore_sessions(&root, "snap", true).expect("dry run");
        assert_eq!(meta.active_session_count, 3);
        assert!(!sessions_path.exists(), "dry run should not create files");

        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn restore_sessions_resets_lease_status() {
        let root = temp_dir("macc_sess_restore_lease");
        seed_sessions_file(&root);
        save_sessions(&root, Some("snap")).expect("save");

        // Remove sessions file to force a clean restore
        let sessions_path = root.join(TOOL_SESSIONS_REL_PATH);
        fs::remove_file(&sessions_path).expect("remove");

        restore_sessions(&root, "snap", false).expect("restore");

        let restored: Value =
            serde_json::from_str(&fs::read_to_string(&sessions_path).unwrap()).unwrap();
        // Leases should be reset to "available"
        assert_eq!(
            restored["tools"]["claude"]["leases"]["claude-s1"]["status"].as_str(),
            Some("available"),
            "restored lease should be 'available'"
        );
        assert!(
            restored["tools"]["claude"]["leases"]["claude-s1"]["restored_at"]
                .as_str()
                .is_some(),
            "restored lease should have restored_at"
        );

        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn project_slug_sanitizes() {
        let p = Path::new("/home/user/my project (test)");
        let slug = project_slug(p);
        assert_eq!(slug, "my_project__test_");
    }
}
