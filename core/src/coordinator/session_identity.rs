//! Provider-neutral eligibility for the P0 JSON compatibility store.
//!
//! The adapter runner owns atomic claim/release. Rust only supplies a hint;
//! the runner rechecks this identity while holding the shared store lock.
use serde_json::{json, Value};
use std::path::Path;

/// Phases and semantic roles differ: developer corrections may retain their
/// own history, while an independent reviewer must never inherit it.
pub fn role_for_phase(phase: &str) -> &str {
    match phase {
        "dev" | "fix" => "performer",
        "review" => "reviewer",
        "merge" | "merge_fix" => "merge_fix",
        other => other,
    }
}

/// Effective routing participates in transcript compatibility, independently of
/// the configured defaults in the resolved adapter specification.
pub struct SessionRouting {
    pub tier: String,
    pub mode: String,
}

impl SessionRouting {
    pub fn from_environment() -> Self {
        Self {
            tier: std::env::var("MACC_MODEL_TIER").unwrap_or_default(),
            mode: std::env::var("MACC_MODEL_ROUTING_MODE")
                .ok()
                .filter(|value| !value.is_empty())
                .unwrap_or_else(|| "auto".into()),
        }
    }
}

/// Mirrored by adapters/shared/session_identity.sh. Preserve legacy scope names
/// as constraints, never as permission to reuse unrelated task transcripts.
pub fn identity(
    repo: &Path,
    worktree: &Path,
    task: &str,
    role: &str,
    tool: &str,
    spec: &Value,
    routing: &SessionRouting,
) -> Option<Value> {
    let scope = spec
        .pointer("/performer/session/scope")
        .and_then(Value::as_str)
        .unwrap_or("task_role_tool");
    if task.is_empty()
        || role.is_empty()
        || spec.get("id")?.as_str()? != tool
        || !matches!(scope, "task_role_tool" | "task" | "worktree" | "project")
        || spec
            .pointer("/performer/session/enabled")
            .and_then(Value::as_bool)
            != Some(true)
    {
        return None;
    }
    Some(json!({
        "version": 1,
        "project_root": repo.canonicalize().ok()?.to_str()?,
        "execution_root": worktree.canonicalize().ok()?.to_str()?,
        "task_id": task, "role": role, "tool_id": tool, "scope": scope,
        "execution_config": {
            "performer": spec.get("performer")?,
            "model_tiers": spec.get("model_tiers").unwrap_or(&Value::Null),
            "tier": routing.tier, "routing": routing.mode
        }
    }))
}

/// Select only a verified same-task/role candidate. Availability is not proof
/// of history ownership. Unknown legacy entries and terminal sessions fail closed.
pub fn read_session_id(
    repo: &Path,
    tool: &str,
    worktree: &Path,
    task: &str,
    role: &str,
) -> Option<String> {
    let spec: Value =
        serde_json::from_slice(&std::fs::read(worktree.join(".macc/tool.json")).ok()?).ok()?;
    let expected = identity(
        repo,
        worktree,
        task,
        role,
        tool,
        &spec,
        &SessionRouting::from_environment(),
    )?;
    if crate::session_store::is_sqlite(repo).ok()? {
        return crate::session_store::read_hint(repo, &expected)
            .ok()
            .flatten();
    }
    let state: Value =
        serde_json::from_slice(&std::fs::read(repo.join(".macc/state/tool-sessions.json")).ok()?)
            .ok()?;
    let max_age = std::env::var("SESSION_MAX_AGE_SECONDS")
        .ok()
        .and_then(|v| v.parse::<i64>().ok())
        .or_else(|| {
            spec.pointer("/performer/session/max_age_seconds")
                .and_then(Value::as_i64)
        })
        .unwrap_or(86400);
    state
        .get("tools")?
        .get(tool)?
        .get("sessions")?
        .as_object()?
        .iter()
        .filter(|(_, entry)| {
            entry.get("session_id").is_none() && entry.get("session_identity") == Some(&expected)
        })
        .filter(|(_, entry)| {
            let valid_age = entry
                .get("last_used_at")
                .or_else(|| entry.get("updated_at"))
                .or_else(|| entry.get("created_at"))
                .and_then(Value::as_str)
                .and_then(|s| chrono::DateTime::parse_from_rfc3339(s).ok())
                .map(|t| chrono::Utc::now().timestamp() - t.timestamp() <= max_age)
                .unwrap_or(false);
            valid_age && entry.get("status").and_then(Value::as_str) == Some("available")
        })
        .max_by_key(|(_, entry)| {
            entry
                .get("last_used_at")
                .or_else(|| entry.get("updated_at"))
                .and_then(Value::as_str)
                .unwrap_or_default()
        })
        .map(|(sid, _)| sid.clone())
}

/// Unclean finished-job recovery must match every supplied constraint and may
/// only quarantine a dead owner's lease. It never grants resume eligibility.
pub(super) fn finished_job_can_release(
    entry: &Value,
    worktree: Option<&Path>,
    task_id: Option<&str>,
) -> bool {
    let same_task = task_id
        .map(|task| entry.get("owner_task_id").and_then(Value::as_str) == Some(task))
        .unwrap_or(true);
    let same_worktree = worktree
        .map(|path| {
            entry.get("owner_worktree").and_then(Value::as_str)
                == Some(path.to_string_lossy().as_ref())
        })
        .unwrap_or(true);
    same_task
        && same_worktree
        && owner_pid(entry)
            .map(|pid| !crate::coordinator::helpers::is_pid_running(pid))
            .unwrap_or(false)
}

pub(crate) fn owner_pid(entry: &Value) -> Option<i64> {
    let pid = entry.get("owner_pid")?;
    pid.as_i64()
        .or_else(|| pid.as_str()?.parse().ok())
        .filter(|p| *p > 0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn phases_preserve_correction_but_isolate_review() {
        assert_eq!(role_for_phase("dev"), role_for_phase("fix"));
        assert_ne!(role_for_phase("dev"), role_for_phase("review"));
        assert_eq!(role_for_phase("audit"), "audit");
    }

    #[test]
    fn selection_requires_complete_identity_and_compatible_execution() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        std::fs::create_dir_all(root.join(".macc/state")).unwrap();
        let spec = json!({"id":"future-tool", "performer": {
            "command":"stub", "session":{"enabled":true, "scope":"task_role_tool"}
        }});
        std::fs::write(root.join(".macc/tool.json"), spec.to_string()).unwrap();
        let expected = identity(
            root,
            root,
            "A",
            "performer",
            "future-tool",
            &spec,
            &SessionRouting::from_environment(),
        )
        .unwrap();
        let base = json!({"status":"available", "updated_at":chrono::Utc::now().to_rfc3339(),
            "session_identity":expected});
        let select = |entry: Value, task: &str, role: &str| {
            std::fs::write(
                root.join(".macc/state/tool-sessions.json"),
                json!({"tools":{"future-tool":{"sessions":{"sid":entry}}}}).to_string(),
            )
            .unwrap();
            read_session_id(root, "future-tool", root, task, role)
        };
        assert_eq!(select(base.clone(), "A", "performer"), Some("sid".into()));
        assert_eq!(select(base.clone(), "B", "performer"), None);
        assert_eq!(select(base.clone(), "A", "reviewer"), None);
        for field in [
            "project_root",
            "execution_root",
            "tool_id",
            "scope",
            "execution_config",
        ] {
            let mut other = base.clone();
            other["session_identity"][field] = json!("incompatible");
            assert_eq!(select(other, "A", "performer"), None, "{field}");
        }
        let mut legacy = base.clone();
        legacy.as_object_mut().unwrap().remove("session_identity");
        assert_eq!(select(legacy, "A", "performer"), None);
        for status in ["closed", "failed", "archived", "abandoned", "unknown"] {
            let mut closed = base.clone();
            closed["status"] = json!(status);
            assert_eq!(select(closed, "A", "performer"), None);
        }
        let mut active = base.clone();
        active["status"] = json!("active");
        active["owner_pid"] = json!(std::process::id());
        assert_eq!(select(active, "A", "performer"), None);
        let mut stale = base;
        stale["updated_at"] = json!("2000-01-01T00:00:00Z");
        assert_eq!(select(stale, "A", "performer"), None);
    }
}
