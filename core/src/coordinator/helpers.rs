use crate::coordinator::model::{PrdInput, TaskRegistry};
use crate::coordinator::runtime as coordinator_runtime;
use crate::coordinator_storage::append_event_sqlite;
use crate::{MaccError, Result};
use std::collections::{HashMap, HashSet};
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
    Ok(!crate::git::is_dirty(worktree_path)?)
}

fn active_task_worktree_paths(registry: &serde_json::Value) -> HashSet<String> {
    TaskRegistry::from_value(registry)
        .map(|typed| typed.active_task_worktree_paths())
        .unwrap_or_default()
}

fn can_reuse_worktree_slot(registry: &serde_json::Value, worktree_path: &Path) -> bool {
    TaskRegistry::from_value(registry)
        .map(|typed| typed.can_reuse_worktree_slot(&worktree_path.to_string_lossy()))
        .unwrap_or(false)
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
            continue; // in use right now
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

fn has_in_progress_or_queued_on_worktree(
    registry: &serde_json::Value,
    worktree_path: &Path,
) -> bool {
    TaskRegistry::from_value(registry)
        .map(|typed| typed.has_in_progress_or_queued_on_worktree(&worktree_path.to_string_lossy()))
        .unwrap_or(false)
}

fn write_worktree_metadata_file(
    worktree_path: &Path,
    metadata: &crate::WorktreeMetadata,
) -> Result<()> {
    let macc_dir = worktree_path.join(".macc");
    std::fs::create_dir_all(&macc_dir).map_err(|e| MaccError::Io {
        path: macc_dir.to_string_lossy().into(),
        action: "create worktree .macc dir".into(),
        source: e,
    })?;
    let path = macc_dir.join("worktree.json");
    let data = serde_json::to_string_pretty(metadata).map_err(|e| {
        MaccError::Validation(format!("Failed to serialize worktree metadata: {}", e))
    })?;
    std::fs::write(&path, data).map_err(|e| MaccError::Io {
        path: path.to_string_lossy().into(),
        action: "write worktree metadata".into(),
        source: e,
    })
}

pub fn build_non_task_worker_slug(worker_count: usize) -> String {
    format!("worker-{:02}", worker_count + 1)
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

fn git_current_branch_name(worktree_path: &Path) -> Option<String> {
    crate::git::current_branch(worktree_path).ok()
}

fn task_id_for_worktree(registry: &serde_json::Value, worktree_path: &Path) -> Option<String> {
    let key = worktree_path.to_string_lossy();
    TaskRegistry::from_value(registry)
        .ok()
        .and_then(|typed| {
            typed
                .tasks
                .iter()
                .find(|task| task.worktree_path().is_some_and(|path| path == key))
                .map(|task| task.id.clone())
        })
        .filter(|id| !id.trim().is_empty())
}

fn extract_task_id_from_text(input: &str) -> Option<String> {
    let mut best = String::new();
    let mut current = String::new();
    for ch in input.chars() {
        if ch.is_ascii_alphanumeric() || ch == '-' {
            current.push(ch);
        } else {
            if current.matches('-').count() >= 2
                && current.chars().any(|c| c.is_ascii_alphabetic())
                && current.chars().any(|c| c.is_ascii_digit())
                && current.len() > best.len()
            {
                best = current.clone();
            }
            current.clear();
        }
    }
    if current.matches('-').count() >= 2
        && current.chars().any(|c| c.is_ascii_alphabetic())
        && current.chars().any(|c| c.is_ascii_digit())
        && current.len() > best.len()
    {
        best = current;
    }
    if best.is_empty() {
        None
    } else {
        Some(best)
    }
}

fn sanitize_tag_component(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    for ch in input.chars() {
        if ch.is_ascii_alphanumeric() || ch == '-' || ch == '_' || ch == '.' {
            out.push(ch);
        } else {
            out.push('-');
        }
    }
    while out.contains("--") {
        out = out.replace("--", "-");
    }
    out.trim_matches('-').trim_matches('.').to_string()
}

fn resolve_abandon_task_id(
    worktree_path: &Path,
    branch_hint: Option<&str>,
    task_id_hint: Option<&str>,
) -> String {
    if let Some(id) = task_id_hint {
        let trimmed = id.trim();
        if !trimmed.is_empty() {
            return sanitize_tag_component(trimmed);
        }
    }
    if let Some(branch) = branch_hint {
        if let Some(id) = extract_task_id_from_text(branch) {
            return sanitize_tag_component(&id);
        }
    }
    if let Some(branch) = git_current_branch_name(worktree_path) {
        if let Some(id) = extract_task_id_from_text(&branch) {
            return sanitize_tag_component(&id);
        }
    }
    if let Ok(Some(metadata)) = crate::read_worktree_metadata(worktree_path) {
        if let Some(id) = extract_task_id_from_text(&metadata.branch) {
            return sanitize_tag_component(&id);
        }
    }
    "unknown-task".to_string()
}

fn create_abandonment_tag_if_needed(
    repo_root: &Path,
    worktree_path: &Path,
    base_branch: &str,
    branch_hint: Option<&str>,
    task_id_hint: Option<&str>,
) -> Result<Option<String>> {
    let commits = crate::git::commits_ahead_of_base(worktree_path, base_branch)?;
    if commits.is_empty() {
        return Ok(None);
    }

    let task_id = resolve_abandon_task_id(worktree_path, branch_hint, task_id_hint);
    let head = crate::git::head_commit(worktree_path)?;
    let timestamp = chrono::Utc::now().format("%Y%m%d-%H%M%S");
    let base_tag = format!("macc/abandoned/{}-{}", task_id, timestamp);
    let mut tag = base_tag.clone();
    let mut index = 0usize;
    while crate::git::rev_parse_verify(repo_root, &format!("refs/tags/{}", tag)).unwrap_or(false) {
        index += 1;
        tag = format!("{}-{}", base_tag, index);
    }
    crate::git::create_tag(repo_root, &tag, &head)?;

    let branch_for_log = branch_hint
        .map(|s| s.to_string())
        .or_else(|| git_current_branch_name(worktree_path))
        .unwrap_or_else(|| "<detached>".to_string());
    let message = format!(
        "created abandonment tag {} for branch {} at {}",
        tag, branch_for_log, head
    );
    if let Err(err) = append_coordinator_event_with_severity(
        repo_root, "progress", &task_id, "dispatch", "ok", &message, "info",
    ) {
        tracing::warn!("failed to append abandonment tag event: {}", err);
    }
    Ok(Some(tag))
}

fn prepare_reused_worktree_base(
    repo_root: &Path,
    worktree_path: &Path,
    base_branch: &str,
    branch_hint: Option<&str>,
    task_id_hint: Option<&str>,
) -> Result<(bool, bool)> {
    let _ = create_abandonment_tag_if_needed(
        repo_root,
        worktree_path,
        base_branch,
        branch_hint,
        task_id_hint,
    )?;
    if !crate::git::reset_hard(worktree_path, "HEAD")? {
        return Ok((false, false));
    }
    if !crate::git::clean_fd(worktree_path)? {
        return Ok((false, false));
    }
    // Try checkout base_branch directly first. If that fails (e.g. because
    // the branch is already checked out in another worktree), detach HEAD
    // and reset to the base commit instead.
    if !crate::git::checkout(worktree_path, base_branch, false)?
        && !crate::git::checkout_reset_branch(worktree_path, base_branch, false)?
        && !crate::git::checkout_detach(worktree_path)?
    {
        return Ok((false, false));
    }
    // Fetch is best-effort: a network failure should not permanently block
    // worktree reuse.  This mirrors the behaviour of prepare_clean_worktree
    // which only hard-fails on fetch when fail_on_fetch_error is set.
    if crate::git::git_remote_exists(worktree_path, "origin")?
        && !crate::git::fetch(worktree_path, "origin")?
    {
        tracing::warn!(
            "prepare_reused_worktree_base: fetch failed for {}, continuing",
            worktree_path.display()
        );
    }
    if !crate::git::reset_hard(worktree_path, base_branch)? {
        return Ok((false, false));
    }
    if !crate::git::reset_hard(worktree_path, "HEAD")? {
        return Ok((false, false));
    }
    if !crate::git::clean_fd(worktree_path)? {
        return Ok((false, false));
    }
    Ok((true, false))
}

fn is_branch_merged_into_base(worktree_path: &Path, branch: &str, base_branch: &str) -> bool {
    if branch.is_empty() || branch == base_branch {
        return true;
    }
    let exists = crate::git::rev_parse_verify(worktree_path, branch).unwrap_or(false);
    if !exists {
        return true;
    }
    crate::git::merge_base_is_ancestor(worktree_path, branch, base_branch).unwrap_or(false)
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
    let active_paths = active_task_worktree_paths(registry);
    let pool_root = repo_root.join(".macc").join("worktree");
    let entries = crate::list_worktrees(repo_root)?;
    let sessions_state = load_tool_sessions_state(repo_root);
    let now_epoch = chrono::Utc::now().timestamp();
    let mut ranked_entries = entries
        .into_iter()
        .enumerate()
        .filter(|(_, entry)| entry.path.starts_with(&pool_root))
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
    for (_, entry, _, _) in ranked_entries {
        let key = entry.path.to_string_lossy().to_string();
        if active_paths.contains(&key) {
            continue;
        }
        if !can_reuse_worktree_slot(registry, &entry.path) {
            continue;
        }
        let dirty_before = !is_worktree_clean(&entry.path)?;
        if dirty_before && has_in_progress_or_queued_on_worktree(registry, &entry.path) {
            last_prepare_error = Some((
                "dirty_inflight_guard".to_string(),
                format!(
                    "worktree {} is dirty and still assigned to an in_progress/queued task",
                    entry.path.display()
                ),
            ));
            continue;
        }
        let merge_head = crate::git::rev_parse_verify(&entry.path, "MERGE_HEAD").unwrap_or(false);
        if merge_head {
            last_prepare_error = Some((
                "merge_head_present".to_string(),
                format!(
                    "worktree {} has unresolved MERGE_HEAD",
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

        let previous_branch = git_current_branch_name(&entry.path).unwrap_or_default();
        let task_id_hint = task_id_for_worktree(registry, &entry.path);
        if !is_branch_merged_into_base(&entry.path, &previous_branch, base_branch) {
            // If the task that owns this branch is permanently stuck (blocked/failed/abandoned),
            // it will never merge autonomously. Force-checkout to base to reclaim the slot
            // rather than letting the coordinator deadlock.
            let stuck = TaskRegistry::from_value(registry)
                .map(|r| r.task_on_worktree_is_permanently_stuck(&entry.path.to_string_lossy()))
                .unwrap_or(false);
            if stuck {
                let _ = create_abandonment_tag_if_needed(
                    repo_root,
                    &entry.path,
                    base_branch,
                    Some(&previous_branch),
                    task_id_hint.as_deref(),
                )?;
                // Abandon the unmerged branch. We cannot `git checkout <base>` directly
                // because git forbids checking out a branch that is already used by
                // another worktree (the main repo). Instead we detach HEAD first, then
                // reset to the base branch commit. prepare_reused_worktree_base will
                // also handle the checkout-to-base via the same detach+reset pattern.
                let detached = crate::git::checkout_detach(&entry.path).unwrap_or(false);
                if detached {
                    let _ = crate::git::reset_hard(&entry.path, base_branch);
                    // Fall through — prepare_reused_worktree_base will finish the reset.
                } else {
                    last_prepare_error = Some((
                        "stuck_branch_checkout_failed".to_string(),
                        format!(
                            "worktree {} stuck branch {} could not be abandoned (detach failed)",
                            entry.path.display(),
                            previous_branch,
                        ),
                    ));
                    continue;
                }
            } else {
                last_prepare_error = Some((
                    "previous_branch_not_merged".to_string(),
                    format!(
                        "worktree {} branch {} is not merged into {}",
                        entry.path.display(),
                        previous_branch,
                        base_branch
                    ),
                ));
                continue;
            }
        }

        let (prepared, skipped_reset) = prepare_reused_worktree_base(
            repo_root,
            &entry.path,
            base_branch,
            Some(&previous_branch),
            task_id_hint.as_deref(),
        )?;
        if !prepared {
            last_prepare_error = Some((
                "sanitize_failed".to_string(),
                format!(
                    "sanitize failed for worktree {} on base {}",
                    entry.path.display(),
                    base_branch
                ),
            ));
            continue;
        }
        if !is_worktree_clean(&entry.path)? {
            last_prepare_error = Some((
                "sanitize_dirty_after".to_string(),
                format!("sanitize left worktree {} dirty", entry.path.display()),
            ));
            continue;
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

        let existing = crate::read_worktree_metadata(&entry.path)?.unwrap_or_else(|| {
            let folder_name = entry
                .path
                .file_name()
                .and_then(|v| v.to_str())
                .unwrap_or("worker")
                .to_string();
            crate::WorktreeMetadata {
                id: folder_name.clone(),
                slug: folder_name,
                tool: tool.to_string(),
                scope: None,
                feature: None,
                base: base_branch.to_string(),
                branch: branch.clone(),
            }
        });
        let updated = crate::WorktreeMetadata {
            id: existing.id,
            slug: existing.slug,
            tool: tool.to_string(),
            scope: existing.scope,
            feature: existing.feature,
            base: base_branch.to_string(),
            branch: branch.clone(),
        };
        write_worktree_metadata_file(&entry.path, &updated)?;
        return Ok((
            Some((entry.path, branch, last_commit, skipped_reset, dirty_before)),
            None,
        ));
    }
    Ok((None, last_prepare_error))
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
}

pub fn count_pool_worktrees(repo_root: &Path) -> Result<usize> {
    let pool_root = repo_root.join(".macc").join("worktree");
    let entries = crate::list_worktrees(repo_root)?;
    Ok(entries
        .into_iter()
        .filter(|e| e.path.starts_with(&pool_root))
        .count())
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
        "payload": {"message": message}
    });
    let project_paths = crate::ProjectPaths::from_root(repo_root);
    let _ = append_event_sqlite(&project_paths, &payload)?;
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

pub fn write_worktree_prd_for_task(
    prd_file: &Path,
    task_id: &str,
    worktree_path: &Path,
) -> Result<()> {
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
        "lot": prd.get("lot").cloned().unwrap_or(serde_json::Value::Null),
        "version": prd.get("version").cloned().unwrap_or(serde_json::Value::Null),
        "generated_at": prd.get("generated_at").cloned().unwrap_or(serde_json::Value::Null),
        "timezone": prd.get("timezone").cloned().unwrap_or(serde_json::Value::String("UTC".to_string())),
        "priority_mapping": prd.get("priority_mapping").cloned().unwrap_or_else(|| serde_json::json!({})),
        "assumptions": prd.get("assumptions").cloned().unwrap_or_else(|| serde_json::json!([])),
        "tasks": [selected],
    });
    let out_path = worktree_path.join("worktree.prd.json");
    std::fs::write(
        &out_path,
        serde_json::to_string_pretty(&payload).map_err(|e| {
            MaccError::Validation(format!(
                "Failed to serialize worktree.prd.json payload: {}",
                e
            ))
        })?,
    )
    .map_err(|e| MaccError::Io {
        path: out_path.to_string_lossy().into(),
        action: "write worktree.prd.json".into(),
        source: e,
    })
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
    let repo_canon = repo_root.canonicalize().unwrap_or_else(|_| repo_root.to_path_buf());
    cwd.starts_with(repo_canon)
}

