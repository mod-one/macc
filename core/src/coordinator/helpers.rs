use crate::coordinator::model::{PrdInput, TaskRegistry};
use crate::coordinator::runtime as coordinator_runtime;
use crate::coordinator_storage::append_event_sqlite;
use crate::{MaccError, Result};
use std::collections::HashMap;
use std::path::Path;

pub fn now_iso_coordinator() -> String {
    chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Secs, true)
}

pub fn set_registry_updated_at(registry: &mut serde_json::Value) {
    let mut typed = TaskRegistry::from_value(registry).unwrap_or_default();
    typed.set_updated_at(now_iso_coordinator());
    if let Ok(next) = typed.to_value() {
        *registry = next;
    }
}

pub fn recompute_resource_locks_from_tasks(registry: &mut serde_json::Value) {
    let mut typed = TaskRegistry::from_value(registry).unwrap_or_default();
    typed.recompute_resource_locks(&now_iso_coordinator());
    if let Ok(next) = typed.to_value() {
        *registry = next;
    }
}

fn sanitize_slug(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    for ch in input.chars() {
        if ch.is_ascii_alphanumeric() {
            out.push(ch.to_ascii_lowercase());
        } else if ch == '-' || ch == '_' || ch == ' ' {
            out.push('-');
        }
    }
    while out.contains("--") {
        out = out.replace("--", "-");
    }
    out.trim_matches('-').to_string()
}

fn is_worktree_clean(worktree_path: &Path) -> Result<bool> {
    Ok(crate::git::status_porcelain_v1(worktree_path, true)?.is_empty())
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SessionWarmth {
    Warm(u64),
    Cold,
}

impl SessionWarmth {
    fn sort_key(self) -> (u8, u64) {
        match self {
            SessionWarmth::Warm(age_secs) => (0, age_secs),
            SessionWarmth::Cold => (1, u64::MAX),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SlotActivityRecency {
    Recent(u64),
    Stale,
}

impl SlotActivityRecency {
    fn sort_key(self) -> (u8, u64) {
        match self {
            SlotActivityRecency::Recent(age_secs) => (0, age_secs),
            SlotActivityRecency::Stale => (1, u64::MAX),
        }
    }
}

fn load_tool_sessions_state(repo_root: &Path) -> Option<serde_json::Value> {
    // Legacy pool warmth is not evidence of an eligible SQLite transcript.
    if crate::session_store::is_sqlite(repo_root).ok()? {
        return None;
    }
    let path = repo_root.join(".macc/state/tool-sessions.json");
    let raw = std::fs::read_to_string(path).ok()?;
    serde_json::from_str(&raw).ok()
}

fn score_worktree_session_warmth_from_state(
    state: Option<&serde_json::Value>,
    _worktree_path: &Path,
    tool_id: &str,
    ttl_seconds: u64,
    now_epoch: i64,
) -> SessionWarmth {
    let Some(root) = state else {
        return SessionWarmth::Cold;
    };
    let Some(sessions) = root
        .get("tools")
        .and_then(|v| v.get(tool_id))
        .and_then(|v| v.get("sessions"))
        .and_then(serde_json::Value::as_object)
    else {
        return SessionWarmth::Cold;
    };
    // Pool model: sessions are keyed by session_id. Find the freshest available
    // one and use its age as the tool-level warmth score.  Old-format entries
    // (keyed by worktree path, carrying a nested "session_id" sub-field) are
    // ignored — they predate the pool model and are not reused.
    let mut best_age: Option<u64> = None;
    for (_session_id, entry) in sessions {
        if entry.get("session_id").is_some() {
            continue; // old-format entry, skip
        }
        let status = entry
            .get("status")
            .and_then(serde_json::Value::as_str)
            .unwrap_or("available");
        if status == "active" {
            let pid = entry.get("owner_pid").and_then(|v| {
                if let Some(n) = v.as_i64() {
                    Some(n)
                } else if let Some(s) = v.as_str() {
                    s.parse::<i64>().ok()
                } else {
                    None
                }
            });
            let is_alive = pid.map(is_pid_running).unwrap_or(false);
            if is_alive {
                continue; // in use by a real live process
            }
        }
        let ts_str = entry
            .get("last_used_at")
            .or_else(|| entry.get("updated_at"))
            .and_then(serde_json::Value::as_str);
        let age = match ts_str {
            Some(s) => chrono::DateTime::parse_from_rfc3339(s)
                .ok()
                .map(|ts| {
                    now_epoch.saturating_sub(ts.with_timezone(&chrono::Utc).timestamp()) as u64
                })
                .unwrap_or(0),
            None => 0, // no timestamp — freshly created
        };
        best_age = Some(best_age.map(|b| b.min(age)).unwrap_or(age));
    }
    match best_age {
        Some(age) if age <= ttl_seconds => SessionWarmth::Warm(age),
        _ => SessionWarmth::Cold,
    }
}

pub fn score_worktree_session_warmth(
    repo_root: &Path,
    worktree_path: &Path,
    tool_id: &str,
    ttl_seconds: u64,
) -> SessionWarmth {
    let state = load_tool_sessions_state(repo_root);
    let now_epoch = chrono::Utc::now().timestamp();
    score_worktree_session_warmth_from_state(
        state.as_ref(),
        worktree_path,
        tool_id,
        ttl_seconds,
        now_epoch,
    )
}

fn score_worktree_activity_recency_from_state(
    last_session_activity_at: &HashMap<String, i64>,
    worktree_path: &Path,
    ttl_seconds: u64,
    now_epoch: i64,
) -> SlotActivityRecency {
    let key_plain = worktree_path.to_string_lossy().to_string();
    let key_canon = std::fs::canonicalize(worktree_path)
        .ok()
        .map(|p| p.to_string_lossy().to_string());
    let updated_epoch = std::iter::once(key_plain.as_str())
        .chain(key_canon.as_deref())
        .find_map(|key| last_session_activity_at.get(key).copied());
    let Some(updated_epoch) = updated_epoch else {
        return SlotActivityRecency::Stale;
    };
    if updated_epoch > now_epoch {
        return SlotActivityRecency::Recent(0);
    }
    let age_secs = now_epoch.saturating_sub(updated_epoch) as u64;
    if age_secs <= ttl_seconds {
        SlotActivityRecency::Recent(age_secs)
    } else {
        SlotActivityRecency::Stale
    }
}

pub fn is_worktree_activity_recent(
    last_session_activity_at: &HashMap<String, i64>,
    worktree_path: &Path,
    ttl_seconds: u64,
) -> bool {
    let now_epoch = chrono::Utc::now().timestamp();
    matches!(
        score_worktree_activity_recency_from_state(
            last_session_activity_at,
            worktree_path,
            ttl_seconds,
            now_epoch
        ),
        SlotActivityRecency::Recent(_)
    )
}

pub fn build_non_task_worker_slug(worker_number: usize) -> String {
    format!("worker-{:02}", worker_number)
}

/// The lowest `worker-NN` slot a new pool worktree can take: no directory at
/// its path, no worktree registered there, and no `ai/<tool>/worker-NN`
/// branch left over (`git worktree add -b` refuses an existing branch).
///
/// Numbering from the pool's size is not enough: once a slot is gone (a run
/// that died mid-dispatch, a removed worktree), `size + 1` lands on a slot
/// still in use.
pub fn next_free_worker_slug(repo_root: &Path, tool: &str) -> Result<String> {
    let pool_root = repo_root.join(".macc").join("worktree");
    let registered: Vec<std::path::PathBuf> = crate::list_worktrees(repo_root)?
        .into_iter()
        .map(|entry| entry.path)
        .collect();
    let mut number = 1;
    loop {
        let slug = build_non_task_worker_slug(number);
        let path = pool_root.join(&slug);
        let taken = path.exists()
            || registered.iter().any(|p| p == &path)
            || crate::git::local_branch_exists(repo_root, &format!("ai/{tool}/{slug}"))?;
        if !taken {
            return Ok(slug);
        }
        number += 1;
    }
}

fn build_reuse_branch_name(tool: &str, worktree_path: &Path) -> String {
    let slot = sanitize_slug(
        worktree_path
            .file_name()
            .and_then(|s| s.to_str())
            .unwrap_or("worker"),
    );
    let ts = chrono::Utc::now().format("%Y%m%d%H%M%S");
    format!(
        "ai/{}/{}-{}",
        tool,
        if slot.is_empty() { "worker" } else { &slot },
        ts
    )
}

/// Check the worker's actual HEAD, including a detached or renamed branch.
pub(crate) fn integrity_pause_reason(
    worker: &Path,
    base: &str,
    code: Option<&str>,
    cause: &str,
) -> Option<String> {
    if code != Some("E901") {
        return None;
    }
    let retained = match crate::git::commits_ahead_of_base(worker, base) {
        Ok(commits) if commits.is_empty() => return None,
        Ok(commits) => format!("{} unmerged commit(s) relative to {base}", commits.len()),
        Err(error) => format!("could not verify integration into {base}: {error}"),
    };
    Some(format!(
        "E901: {cause}. Worktree {} retained for review ({retained}).",
        worker.display()
    ))
}

type ReusableWorktree = (std::path::PathBuf, String, String, bool, bool);
type ReusableWorktreePrepareError = (String, String);

pub fn find_reusable_worktree_native(
    repo_root: &Path,
    registry: &serde_json::Value,
    tool: &str,
    base_branch: &str,
    session_cache_ttl_seconds: u64,
    last_session_activity_at: &HashMap<String, i64>,
) -> Result<(
    Option<ReusableWorktree>,
    Option<ReusableWorktreePrepareError>,
)> {
    match find_reusable_worktree_for_task(
        repo_root,
        registry,
        tool,
        base_branch,
        session_cache_ttl_seconds,
        last_session_activity_at,
        "-",
    ) {
        Err(MaccError::ScopedPrd(incident)) => Ok((
            None,
            Some(("scoped_prd_preserved".into(), incident.to_string())),
        )),
        result => result,
    }
}

pub(super) fn find_reusable_worktree_for_task(
    repo_root: &Path,
    registry: &serde_json::Value,
    tool: &str,
    base_branch: &str,
    session_cache_ttl_seconds: u64,
    last_session_activity_at: &HashMap<String, i64>,
    incoming: &str,
) -> Result<(
    Option<ReusableWorktree>,
    Option<ReusableWorktreePrepareError>,
)> {
    let _roles = super::worktree_roles::lock(repo_root)?;
    let registry = TaskRegistry::from_value(registry)?;
    let entries = crate::list_worktrees(repo_root)?;
    let sessions_state = load_tool_sessions_state(repo_root);
    let now_epoch = chrono::Utc::now().timestamp();
    let mut ranked_entries = entries
        .into_iter()
        .enumerate()
        .filter(|(_, entry)| {
            !entry.locked && !entry.prunable && !super::worktree_roles::reserved(&entry.path)
        })
        .map(|(idx, entry)| {
            let warmth = score_worktree_session_warmth_from_state(
                sessions_state.as_ref(),
                &entry.path,
                tool,
                session_cache_ttl_seconds,
                now_epoch,
            );
            let recency = score_worktree_activity_recency_from_state(
                last_session_activity_at,
                &entry.path,
                session_cache_ttl_seconds,
                now_epoch,
            );
            (idx, entry, warmth, recency)
        })
        .collect::<Vec<_>>();
    ranked_entries.sort_by(|a, b| {
        a.2.sort_key()
            .cmp(&b.2.sort_key())
            .then_with(|| a.3.sort_key().cmp(&b.3.sort_key()))
            .then_with(|| a.0.cmp(&b.0))
    });
    let mut last_prepare_error: Option<(String, String)> = None;
    let mut preserved_prd = None;
    for (_, entry, _, _) in ranked_entries {
        let key = entry.path.to_string_lossy().to_string();
        if !registry.can_reuse_worktree_slot(&key) {
            last_prepare_error = Some((
                "worktree_assigned".into(),
                format!("{key} is reserved by a task"),
            ));
            continue;
        }
        let Some(metadata) = super::worktree_roles::performer_metadata(repo_root, &entry.path)
        else {
            continue;
        };
        let previous_branch = crate::git::current_branch_name(&entry.path)?;
        let released = previous_branch.is_empty()
            && entry.branch.is_none()
            && metadata.base == base_branch
            && (metadata
                .released_head
                .as_ref()
                .is_some_and(|head| entry.head.as_ref() == Some(head))
                || registry.tasks.iter().any(|t| {
                    t.state == "merged"
                        && t.worktree_path() == Some(key.as_str())
                        && t.branch() == Some(metadata.branch.as_str())
                }));
        if (!released && previous_branch != metadata.branch)
            || !super::engine::is_worktree_healthy(&entry.path)
        {
            last_prepare_error = Some((
                "worktree_not_available".into(),
                format!(
                    "{}: ownership changed or Git operation in progress",
                    entry.path.display()
                ),
            ));
            continue;
        }
        let base_ok = crate::git::rev_parse_verify(&entry.path, base_branch).unwrap_or(false);
        if !base_ok {
            last_prepare_error = Some((
                "base_branch_missing".to_string(),
                format!(
                    "worktree {} cannot resolve base branch {}",
                    entry.path.display(),
                    base_branch
                ),
            ));
            continue;
        }

        if !crate::git::merge_base_is_ancestor(&entry.path, "HEAD", base_branch)? {
            last_prepare_error = Some((
                "previous_branch_not_merged".into(),
                format!(
                    "worktree {} branch {} retains unmerged work; slot preserved",
                    entry.path.display(),
                    previous_branch
                ),
            ));
            continue;
        }
        if super::scoped_prd::worker_busy(&registry, &entry.path) {
            last_prepare_error = Some(("worktree_busy".into(), key));
            continue;
        }
        match super::scoped_prd::prepare(&entry.path, incoming) {
            Ok(Some(incident)) => {
                super::scoped_prd::record_event(repo_root, &incident, "scoped_prd_restored")?
            }
            Ok(None) => {}
            Err(MaccError::ScopedPrd(incident)) => {
                super::scoped_prd::record_event(repo_root, &incident, "scoped_prd_preserved")?;
                preserved_prd = Some(incident);
                continue;
            }
            Err(error) => return Err(error),
        }
        if !is_worktree_clean(&entry.path)?
            || (!released
                && !crate::project_ownership::unowned_ignored_files(&entry.path)?.is_empty())
        {
            last_prepare_error = Some((
                "retained_local_work".into(),
                format!(
                    "worktree {} contains local work; slot preserved",
                    entry.path.display()
                ),
            ));
            continue;
        }

        // A normal checkout carries ignored files forward and refuses collisions.
        // Never reset or clean a released slot to make room for another task.
        if crate::git::git_remote_exists(&entry.path, "origin")? {
            crate::git::fetch_housekeeping_sync(&entry.path, "origin")?;
        }
        let mut branch = build_reuse_branch_name(tool, &entry.path);
        let mut i = 0usize;
        loop {
            let exists = crate::git::rev_parse_verify(repo_root, &branch).unwrap_or(false);
            if !exists {
                break;
            }
            i += 1;
            branch = format!("{}-{}", build_reuse_branch_name(tool, &entry.path), i);
        }
        if !crate::git::checkout_new_branch_from_base(&entry.path, &branch, base_branch)? {
            last_prepare_error = Some((
                "checkout_new_branch_failed".to_string(),
                format!(
                    "failed to create branch {} in reused worktree {}",
                    branch,
                    entry.path.display()
                ),
            ));
            continue;
        }
        crate::project_ownership::record_branch(repo_root, &branch)?;
        if !previous_branch.is_empty()
            && previous_branch != base_branch
            && previous_branch != branch
        {
            coordinator_runtime::report_branch_cleanup_outcome(
                repo_root,
                None,
                "dispatch",
                &previous_branch,
                base_branch,
                "reused_worktree_switch",
                coordinator_runtime::cleanup_merged_local_branch(
                    repo_root,
                    &previous_branch,
                    base_branch,
                ),
                |event_type, task_id, phase, status, message, severity| {
                    let _ = append_coordinator_event_with_severity(
                        repo_root, event_type, task_id, phase, status, message, severity,
                    );
                },
                |msg| tracing::warn!("{}", msg),
            );
        }
        let last_commit = crate::git::head_commit(&entry.path).unwrap_or_default();

        let updated = crate::WorktreeMetadata {
            tool: tool.to_string(),
            base: base_branch.to_string(),
            branch: branch.clone(),
            released_head: None,
            ..metadata
        };
        crate::worktree::write_worktree_metadata(&entry.path, updated)?;
        return Ok((Some((entry.path, branch, last_commit, true, false)), None));
    }
    match preserved_prd {
        Some(incident) => Err(MaccError::ScopedPrd(incident)),
        None => Ok((None, last_prepare_error)),
    }
}

#[cfg(test)]
mod tests {
    use super::{
        score_worktree_activity_recency_from_state, score_worktree_session_warmth_from_state,
        SessionWarmth, SlotActivityRecency,
    };
    use serde_json::json;
    use std::collections::HashMap;
    use std::path::Path;

    #[test]
    fn a_new_worker_takes_the_lowest_free_slot() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path();
        let git = |args: &[&str]| {
            let output =
                crate::git::run_git_output_mapped(root, args, "worker slot fixture").unwrap();
            assert!(
                output.status.success(),
                "{}",
                String::from_utf8_lossy(&output.stderr)
            );
        };
        git(&["init", "-q", "-b", "main"]);
        git(&["config", "user.name", "Test"]);
        git(&["config", "user.email", "test@example.invalid"]);
        std::fs::write(root.join("source.txt"), "base").unwrap();
        git(&["add", "."]);
        git(&["commit", "-qm", "base"]);
        let pool = root.join(".macc/worktree");
        std::fs::create_dir_all(&pool).unwrap();
        // Workers 01, 02 and 04 exist: three workers, but `worker-04` is taken.
        for (slug, tool) in [
            ("worker-01", "codex"),
            ("worker-02", "claude"),
            ("worker-04", "codex"),
        ] {
            let path = pool.join(slug);
            git(&[
                "worktree",
                "add",
                "-b",
                &format!("ai/{tool}/{slug}"),
                path.to_str().unwrap(),
                "main",
            ]);
        }
        assert_eq!(
            super::next_free_worker_slug(root, "claude").unwrap(),
            "worker-03"
        );
        // A branch left by an earlier `worker-03` blocks that slot for its tool.
        git(&["branch", "ai/claude/worker-03", "main"]);
        assert_eq!(
            super::next_free_worker_slug(root, "claude").unwrap(),
            "worker-05"
        );
        assert_eq!(
            super::next_free_worker_slug(root, "codex").unwrap(),
            "worker-03"
        );
    }

    #[test]
    fn supervisor_slots_are_not_recycled_or_removed_by_worker_pool_cleanup() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path();
        let git = |args: &[&str]| {
            let output =
                crate::git::run_git_output_mapped(root, args, "supervisor pool fixture").unwrap();
            assert!(
                output.status.success(),
                "{}",
                String::from_utf8_lossy(&output.stderr)
            );
        };
        git(&["init", "-q", "-b", "main"]);
        git(&["config", "user.name", "Test"]);
        git(&["config", "user.email", "test@example.invalid"]);
        std::fs::write(root.join("source.txt"), "base").unwrap();
        git(&["add", "."]);
        git(&["commit", "-qm", "base"]);
        let slot = root.join(".macc/worktree/supervisor-01");
        std::fs::create_dir_all(slot.parent().unwrap()).unwrap();
        git(&[
            "worktree",
            "add",
            "--detach",
            slot.to_str().unwrap(),
            "HEAD",
        ]);
        std::fs::write(slot.join("source.txt"), "unfinished repair").unwrap();
        let (candidate, _) = super::find_reusable_worktree_native(
            root,
            &json!({"tasks":[]}),
            "test-tool",
            "main",
            3600,
            &HashMap::new(),
        )
        .unwrap();
        assert!(candidate.is_none());
        assert_eq!(
            crate::domain::worktree::remove_all_worktrees(root, false).unwrap(),
            0
        );
        assert_eq!(
            std::fs::read_to_string(slot.join("source.txt")).unwrap(),
            "unfinished repair"
        );
    }

    #[test]
    fn warm_session_scored_when_within_ttl() {
        let updated_at = "2026-04-13T00:00:00Z";
        let now_epoch = chrono::DateTime::parse_from_rfc3339(updated_at)
            .expect("valid ts")
            .with_timezone(&chrono::Utc)
            .timestamp()
            + 300;
        let state = json!({
            "tools": {
                "codex": {
                    "sessions": {
                        "sid-1": {
                            "status": "available",
                            "updated_at": updated_at
                        }
                    }
                }
            }
        });
        let warmth = score_worktree_session_warmth_from_state(
            Some(&state),
            Path::new("/tmp/wt-1"),
            "codex",
            300,
            now_epoch,
        );
        assert_eq!(warmth, SessionWarmth::Warm(300));
    }

    #[test]
    fn expired_session_scored_cold() {
        let updated_at = "2026-04-13T00:00:00Z";
        let now_epoch = chrono::DateTime::parse_from_rfc3339(updated_at)
            .expect("valid ts")
            .with_timezone(&chrono::Utc)
            .timestamp()
            + 301;
        let state = json!({
            "tools": {
                "codex": {
                    "sessions": {
                        "sid-1": {
                            "status": "available",
                            "updated_at": updated_at
                        }
                    }
                }
            }
        });
        let warmth = score_worktree_session_warmth_from_state(
            Some(&state),
            Path::new("/tmp/wt-1"),
            "codex",
            300,
            now_epoch,
        );
        assert_eq!(warmth, SessionWarmth::Cold);
    }

    #[test]
    fn missing_session_scored_cold() {
        let state = json!({
            "tools": {
                "codex": {
                    "sessions": {}
                }
            }
        });
        let warmth = score_worktree_session_warmth_from_state(
            Some(&state),
            Path::new("/tmp/wt-1"),
            "codex",
            300,
            1_744_505_100,
        );
        assert_eq!(warmth, SessionWarmth::Cold);
    }

    #[test]
    fn old_format_session_not_scored_as_warm() {
        // Old-format entries (worktree-path key + nested session_id sub-field) must
        // be ignored by the pool scorer so they don't falsely appear as warm.
        let updated_at = "2026-04-13T00:00:00Z";
        let now_epoch = chrono::DateTime::parse_from_rfc3339(updated_at)
            .expect("valid ts")
            .with_timezone(&chrono::Utc)
            .timestamp()
            + 10;
        let state = json!({
            "tools": {
                "codex": {
                    "sessions": {
                        "/tmp/wt-1": {
                            "session_id": "sid-old",
                            "updated_at": updated_at
                        }
                    },
                    "leases": {
                        "sid-old": { "status": "active" }
                    }
                }
            }
        });
        let warmth = score_worktree_session_warmth_from_state(
            Some(&state),
            Path::new("/tmp/wt-1"),
            "codex",
            300,
            now_epoch,
        );
        assert_eq!(warmth, SessionWarmth::Cold);
    }

    #[test]
    fn recent_activity_scored_within_ttl() {
        let mut activity = HashMap::new();
        activity.insert("/tmp/wt-1".to_string(), 1_744_505_100);
        let recency = score_worktree_activity_recency_from_state(
            &activity,
            Path::new("/tmp/wt-1"),
            300,
            1_744_505_400,
        );
        assert_eq!(recency, SlotActivityRecency::Recent(300));
    }

    #[test]
    fn stale_activity_scored_outside_ttl() {
        let mut activity = HashMap::new();
        activity.insert("/tmp/wt-1".to_string(), 1_744_505_100);
        let recency = score_worktree_activity_recency_from_state(
            &activity,
            Path::new("/tmp/wt-1"),
            300,
            1_744_505_401,
        );
        assert_eq!(recency, SlotActivityRecency::Stale);
    }

    #[test]
    fn test_structured_event_emission_jsonl() {
        let temp_dir = tempfile::tempdir().unwrap();
        let repo_root = temp_dir.path();

        let event = crate::coordinator::CoordinatorEventRecord {
            schema_version: "1".to_string(),
            event_id: "evt-test-123".to_string(),
            run_id: Some("run-test-xyz".to_string()),
            coordinator_epoch: Some(0),
            claim_id: Some("claim-test-abc".to_string()),
            seq: 12345,
            ts: "2026-05-31T12:00:00Z".to_string(),
            source: "performer:test".to_string(),
            task_id: Some("TASK-123".to_string()),
            event_type: "phase_progress".to_string(),
            phase: Some("dev".to_string()),
            status: "running".to_string(),
            detail: Some("Doing task stuff".to_string()),
            msg: None,
            payload: serde_json::Value::Null,
            extra: std::collections::BTreeMap::new(),
        };

        super::append_structured_event_record(repo_root, &event).unwrap();

        // Check global events.jsonl
        let global_log = repo_root.join(".macc").join("log").join("events.jsonl");
        assert!(global_log.exists());
        let global_content = std::fs::read_to_string(&global_log).unwrap();
        let parsed_global: serde_json::Value = serde_json::from_str(global_content.trim()).unwrap();
        assert_eq!(parsed_global["id"], "evt-test-123");
        assert_eq!(parsed_global["task_id"], "TASK-123");
        assert_eq!(parsed_global["run_id"], "run-test-xyz");
        assert_eq!(parsed_global["source"], "performer");
        assert_eq!(parsed_global["event_type"], "phase_progress");
        assert_eq!(parsed_global["phase"], "dev");
        assert_eq!(parsed_global["message"], "Doing task stuff");

        // Check performer events.jsonl
        let task_log = repo_root
            .join(".macc")
            .join("log")
            .join("performer")
            .join("TASK-123")
            .join("run-test-xyz.events.jsonl");
        assert!(task_log.exists());
        let task_content = std::fs::read_to_string(&task_log).unwrap();
        let parsed_task: serde_json::Value = serde_json::from_str(task_content.trim()).unwrap();
        assert_eq!(parsed_task["id"], "evt-test-123");
    }
}

pub fn append_coordinator_event(
    repo_root: &Path,
    event_type: &str,
    task_id: &str,
    phase: &str,
    status: &str,
    message: &str,
) -> Result<()> {
    let severity = if status.eq_ignore_ascii_case("failed") || status.eq_ignore_ascii_case("error")
    {
        "blocking"
    } else {
        "info"
    };
    append_coordinator_event_with_severity(
        repo_root, event_type, task_id, phase, status, message, severity,
    )
}

pub fn append_coordinator_event_with_severity(
    repo_root: &Path,
    event_type: &str,
    task_id: &str,
    phase: &str,
    status: &str,
    message: &str,
    severity: &str,
) -> Result<()> {
    append_coordinator_event_payload(
        repo_root,
        event_type,
        task_id,
        phase,
        status,
        severity,
        serde_json::json!({"message":message}),
    )
}

pub(super) fn append_coordinator_event_payload(
    repo_root: &Path,
    event_type: &str,
    task_id: &str,
    phase: &str,
    status: &str,
    severity: &str,
    details: serde_json::Value,
) -> Result<()> {
    let message = details["message"].as_str().unwrap_or_default();
    let run_id = ensure_coordinator_run_id();
    let now = now_iso_coordinator();
    let seq = chrono::Utc::now().timestamp_nanos_opt().unwrap_or_default() as u64;
    let payload = serde_json::json!({
        "schema_version":"1",
        "event_id": format!("evt-{}-{}-{}", event_type, task_id, seq),
        "run_id": run_id,
        "seq": seq,
        "ts": now,
        "source": "coordinator:native",
        "task_id": task_id,
        "type": event_type,
        "phase": phase,
        "status": status,
        "severity": severity,
        "payload": details
    });
    let project_paths = crate::ProjectPaths::from_root(repo_root);
    let _ = append_event_sqlite(&project_paths, &payload)?;
    let _ = write_structured_event_jsonl(repo_root, event_type, task_id, phase, message, severity);
    Ok(())
}

/// Emit the final coordinator run-complete event.
///
/// Unlike the generic helper, this includes a top-level `"result"` field so
/// that the supervisor's `read_last_coordinator_result` can identify a clean
/// exit without ambiguity, even if the `"type"/"status"` mapping changes.
pub fn append_coordinator_run_complete_event(repo_root: &Path) -> Result<()> {
    let run_id = ensure_coordinator_run_id();
    let now = now_iso_coordinator();
    let seq = chrono::Utc::now().timestamp_nanos_opt().unwrap_or_default() as u64;
    let payload = serde_json::json!({
        "schema_version": "1",
        "event_id": format!("evt-command_end--{}", seq),
        "run_id": run_id,
        "seq": seq,
        "ts": now,
        "source": "coordinator:native",
        "task_id": "-",
        "type": "command_end",
        "phase": "run",
        "status": "done",
        "result": "success",
        "severity": "info",
        "payload": {"message": "Coordinator run complete"}
    });
    let project_paths = crate::ProjectPaths::from_root(repo_root);
    let _ = append_event_sqlite(&project_paths, &payload)?;
    let _ = write_structured_event_jsonl(
        repo_root,
        "command_end",
        "-",
        "run",
        "Coordinator run complete",
        "info",
    );
    Ok(())
}

pub fn append_phase_skipped_event(
    repo_root: &Path,
    task_id: &str,
    phase: &str,
    reason: &str,
    message: &str,
) -> Result<()> {
    let run_id = ensure_coordinator_run_id();
    let now = now_iso_coordinator();
    let seq = chrono::Utc::now().timestamp_nanos_opt().unwrap_or_default() as u64;
    let payload = serde_json::json!({
        "schema_version": "1",
        "event_id": format!("evt-phase_skipped-{}-{}", task_id, seq),
        "run_id": run_id,
        "seq": seq,
        "ts": now,
        "source": "coordinator:native",
        "task_id": task_id,
        "type": "phase_skipped",
        "phase": phase,
        "status": "skipped",
        "reason": reason,
        "severity": "info",
        "payload": {
            "message": message
        }
    });
    let project_paths = crate::ProjectPaths::from_root(repo_root);
    let _ = append_event_sqlite(&project_paths, &payload)?;
    let msg_with_reason = format!("{} (reason: {})", message, reason);
    let _ = write_structured_event_jsonl(
        repo_root,
        "phase_skipped",
        task_id,
        phase,
        &msg_with_reason,
        "info",
    );
    Ok(())
}

pub fn append_session_event(
    repo_root: &Path,
    event_type: &str,
    task_id: &str,
    tool_id: &str,
    session_id: &str,
    owner_pid: Option<i64>,
    details: &str,
) -> Result<()> {
    let run_id = ensure_coordinator_run_id();
    let epoch = std::env::var("COORDINATOR_EPOCH")
        .ok()
        .and_then(|s| s.parse::<i64>().ok())
        .unwrap_or(0);
    let now = now_iso_coordinator();
    let seq = chrono::Utc::now().timestamp_nanos_opt().unwrap_or_default() as u64;
    let message = format!(
        "session {} tool={} task={} pid={}: {}",
        session_id,
        tool_id,
        if task_id.is_empty() { "-" } else { task_id },
        owner_pid
            .map(|p| p.to_string())
            .unwrap_or_else(|| "-".to_string()),
        details
    );
    let payload = serde_json::json!({
        "schema_version": "1",
        "event_id": format!("evt-{}-{}-{}", event_type, if task_id.is_empty() { "session" } else { task_id }, seq),
        "run_id": run_id,
        "coordinator_epoch": epoch,
        "claim_id": session_id,
        "seq": seq,
        "ts": now,
        "source": "coordinator:native",
        "task_id": if task_id.is_empty() { "-" } else { task_id },
        "type": event_type,
        "phase": "session",
        "status": "ok",
        "severity": "info",
        "payload": {
            "session_id": session_id,
            "tool": tool_id,
            "task_id": if task_id.is_empty() { serde_json::Value::Null } else { serde_json::json!(task_id) },
            "owner_pid": owner_pid,
            "message": message,
            "detail": details
        }
    });
    let project_paths = crate::ProjectPaths::from_root(repo_root);
    let _ = append_event_sqlite(&project_paths, &payload)?;
    let _ =
        write_structured_event_jsonl(repo_root, event_type, task_id, "session", &message, "info");
    Ok(())
}

pub fn write_structured_event_jsonl(
    repo_root: &Path,
    event_type: &str,
    task_id: &str,
    phase: &str,
    message: &str,
    severity: &str,
) -> Result<()> {
    let run_id = ensure_coordinator_run_id();
    let now = now_iso_coordinator();
    let seq = chrono::Utc::now().timestamp_nanos_opt().unwrap_or_default() as u64;
    let id = format!("evt_{}", seq);
    let event = crate::coordinator::CoordinatorEventRecord {
        schema_version: "1".to_string(),
        event_id: id,
        run_id: Some(run_id),
        coordinator_epoch: None,
        claim_id: None,
        seq: seq as i64,
        ts: now,
        source: "coordinator".to_string(),
        task_id: if task_id.is_empty() {
            None
        } else {
            Some(task_id.to_string())
        },
        event_type: event_type.to_string(),
        phase: if phase.is_empty() {
            None
        } else {
            Some(phase.to_string())
        },
        status: severity.to_string(),
        detail: Some(message.to_string()),
        msg: None,
        payload: serde_json::Value::Null,
        extra: std::collections::BTreeMap::new(),
    };
    append_structured_event_record(repo_root, &event)
}

pub fn append_structured_event_record(
    repo_root: &Path,
    event: &crate::coordinator::CoordinatorEventRecord,
) -> Result<()> {
    let run_id = event
        .run_id
        .clone()
        .unwrap_or_else(ensure_coordinator_run_id);
    let now = if event.ts.is_empty() {
        now_iso_coordinator()
    } else {
        event.ts.clone()
    };

    let task_id = event.task_id.as_deref().unwrap_or("-");
    let phase = event.phase.as_deref().unwrap_or("-");
    let severity = if event.status.eq_ignore_ascii_case("failed")
        || event.status.eq_ignore_ascii_case("error")
        || event.status.eq_ignore_ascii_case("blocking")
    {
        "blocking"
    } else if event.status.eq_ignore_ascii_case("warning")
        || event.status.eq_ignore_ascii_case("warn")
    {
        "warning"
    } else {
        "info"
    };

    let project_paths = crate::ProjectPaths::from_root(repo_root);
    let storage_paths =
        crate::coordinator_storage::CoordinatorStoragePaths::from_project_paths(&project_paths);
    let mut worker_id = String::new();
    let mut tool = String::new();
    let mut run_id_val = run_id;

    if let Ok(registry) =
        crate::coordinator_storage::SqliteStorage::new(storage_paths).load_registry()
    {
        if let Some(task) = registry.tasks.iter().find(|t| t.id == task_id) {
            worker_id = task.task_runtime.worker_id.clone().unwrap_or_default();
            tool = task
                .tool
                .clone()
                .or_else(|| task.task_runtime.tool.clone())
                .unwrap_or_default();
            if let Some(ref r_id) = task.task_runtime.run_id {
                if !r_id.is_empty() {
                    run_id_val = r_id.clone();
                }
            }
        }
    }

    let source = if event.source.starts_with("coordinator") {
        "coordinator"
    } else if event.source.starts_with("performer") {
        "performer"
    } else if event.source.starts_with("tester") {
        "tester"
    } else if event.source.starts_with("reviewer") {
        "reviewer"
    } else {
        match phase {
            "planning" | "plan" => "performer",
            "implementing" | "dev" | "editing" | "edit" | "fixing" | "fix" => "performer",
            "testing" | "test" => "tester",
            "reviewing" | "review" => "reviewer",
            "merging" | "merge" => "merge_worker",
            "committing" | "commit" | "opening_pr" | "pr" => "git",
            _ => "coordinator",
        }
    };

    let id = if event.event_id.is_empty() {
        let seq = chrono::Utc::now().timestamp_nanos_opt().unwrap_or_default() as u64;
        format!("evt_{}", seq)
    } else {
        event.event_id.clone()
    };

    let message = event.message().unwrap_or("");

    let structured_event = serde_json::json!({
        "id": id,
        "task_id": task_id,
        "run_id": run_id_val,
        "worker_id": worker_id,
        "timestamp": now,
        "severity": severity.to_ascii_lowercase(),
        "source": source,
        "event_type": event.event_type,
        "phase": phase,
        "message": message,
        "metadata": {
            "tool": tool
        }
    });

    let log_dir = repo_root.join(".macc").join("log");
    let _ = std::fs::create_dir_all(&log_dir);

    // Append to global events.jsonl
    let global_file = log_dir.join("events.jsonl");
    if let Ok(mut file) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&global_file)
    {
        use std::io::Write;
        let serialized = serde_json::to_string(&structured_event).unwrap_or_default();
        let _ = writeln!(file, "{}", serialized);
    }

    // Append to task/run events.jsonl
    if !task_id.is_empty() && task_id != "-" && !run_id_val.is_empty() {
        let performer_log_dir = log_dir.join("performer").join(task_id);
        let _ = std::fs::create_dir_all(&performer_log_dir);
        let task_file = performer_log_dir.join(format!("{}.events.jsonl", run_id_val));
        if let Ok(mut file) = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&task_file)
        {
            use std::io::Write;
            let serialized = serde_json::to_string(&structured_event).unwrap_or_default();
            let _ = writeln!(file, "{}", serialized);
        }
    }

    Ok(())
}

pub fn ensure_coordinator_run_id() -> String {
    if let Ok(existing) = std::env::var("COORDINATOR_RUN_ID") {
        let trimmed = existing.trim();
        if !trimmed.is_empty() {
            return trimmed.to_string();
        }
    }
    let generated = format!(
        "run-{}-{}",
        chrono::Utc::now().timestamp_nanos_opt().unwrap_or_default(),
        std::process::id()
    );
    std::env::set_var("COORDINATOR_RUN_ID", &generated);
    generated
}

/// The exact scoped execution contract, shared by dispatch and recovery previews.
pub fn scoped_prd_for_task(prd_file: &Path, task_id: &str) -> Result<serde_json::Value> {
    let prd_raw = std::fs::read_to_string(prd_file).map_err(|e| MaccError::Io {
        path: prd_file.to_string_lossy().into(),
        action: "read prd for worktree".into(),
        source: e,
    })?;
    let prd: serde_json::Value = serde_json::from_str(&prd_raw).map_err(|e| {
        MaccError::Validation(format!(
            "Failed to parse PRD {} for worktree: {}",
            prd_file.display(),
            e
        ))
    })?;
    let typed_prd = serde_json::from_value::<PrdInput>(prd.clone()).map_err(|e| {
        MaccError::Validation(format!(
            "Failed to parse typed PRD {} for worktree: {}",
            prd_file.display(),
            e
        ))
    })?;
    let selected = typed_prd
        .tasks
        .into_iter()
        .find(|task| task.id == task_id)
        .ok_or_else(|| {
            MaccError::Validation(format!(
                "Task '{}' not found in PRD {}",
                task_id,
                prd_file.display()
            ))
        })?;
    let selected = serde_json::to_value(selected).map_err(|e| {
        MaccError::Validation(format!(
            "Failed to serialize typed PRD task '{}' for worktree: {}",
            task_id, e
        ))
    })?;
    let payload = serde_json::json!({
        "source_repositories": prd.get("source_repositories").cloned().unwrap_or_else(|| serde_json::json!({})),
        "integration_policy": prd.get("integration_policy").cloned().unwrap_or_else(|| serde_json::json!({})),
        "global_constraints": prd.get("global_constraints").cloned().unwrap_or_else(|| serde_json::json!([])),
        "lot": prd.get("lot").cloned().unwrap_or(serde_json::Value::Null),
        "tasks": [selected],
    });
    Ok(payload)
}

pub fn write_worktree_prd_for_task(
    prd_file: &Path,
    task_id: &str,
    worktree_path: &Path,
) -> Result<()> {
    let payload = scoped_prd_for_task(prd_file, task_id)?;
    let name = "worktree.prd.json";
    let out_path = worktree_path.join(name);
    let paths = crate::ProjectPaths::from_root(worktree_path);
    let bytes = serde_json::to_vec_pretty(&payload)
        .map_err(|e| MaccError::Validation(format!("Failed to serialize scoped PRD: {e}")))?;
    crate::project_ownership::validate_path(&paths, name)?;
    let existing = crate::plan::read_existing(&out_path);
    if (out_path.try_exists().map_err(crate::persistent::error)?
        && !crate::project_ownership::owned_unchanged(&paths, name)?)
        || (worktree_path.join(".git").exists() && super::scoped_prd::tracked(worktree_path)?)
    {
        return Err(MaccError::ScopedPrd(Box::new(
            super::scoped_prd::Incident::unverified(worktree_path, task_id),
        )));
    }
    let status =
        crate::write_if_changed_with_existing(&paths, name, &out_path, &bytes, &existing, |_| {
            Ok(())
        })?;
    crate::local_exclusions::after_runtime_write(&paths, name, status)
}

pub fn is_pid_running(pid: i64) -> bool {
    if pid <= 0 {
        return false;
    }
    #[cfg(unix)]
    {
        unsafe { libc::kill(pid as libc::pid_t, 0) == 0 }
    }
    #[cfg(not(unix))]
    {
        std::process::Command::new("kill")
            .arg("-0")
            .arg(pid.to_string())
            .status()
            .map(|s| s.success())
            .unwrap_or(false)
    }
}

pub fn pgrep_pids(pattern: &str) -> Result<Vec<i32>> {
    let output = std::process::Command::new("pgrep")
        .arg("-f")
        .arg(pattern)
        .output()
        .map_err(|e| MaccError::Io {
            path: "pgrep".into(),
            action: "find performer/coordinator processes".into(),
            source: e,
        })?;
    if !output.status.success() {
        return Ok(Vec::new());
    }
    let text = String::from_utf8_lossy(&output.stdout);
    Ok(text
        .lines()
        .filter_map(|line| line.trim().parse::<i32>().ok())
        .collect())
}

pub fn pid_in_repo(pid: i32, repo_root: &std::path::Path) -> bool {
    let proc_cwd = std::path::PathBuf::from(format!("/proc/{}/cwd", pid));
    let Ok(cwd) = std::fs::read_link(proc_cwd) else {
        return false;
    };
    let cwd = cwd.canonicalize().unwrap_or(cwd);
    let repo_canon = repo_root
        .canonicalize()
        .unwrap_or_else(|_| repo_root.to_path_buf());
    cwd.starts_with(repo_canon)
}
