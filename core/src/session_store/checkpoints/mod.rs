//! Durable task context, independent of provider conversations and task delivery.
mod git_state;
mod lifecycle;
mod memory_context;
pub mod merge_repair;
mod model;
mod schema;
#[cfg(test)]
mod tests;
use super::*;
pub use lifecycle::{finish, prepare, reconcile, rotation_guard};
pub use model::*;
use rusqlite::{params, OptionalExtension};
pub(super) use schema::ready;
pub use schema::upgrade;
use serde_json::{json, Value};
use std::path::Path;
pub(super) fn make(
    root: &Path,
    a: &Authority,
    request: &ContextRequest,
    reason: &str,
) -> Result<Checkpoint> {
    super::leases::validate_import_identity(root, &request.identity)?;
    if !matches!(
        request.phase.as_str(),
        "dev" | "fix" | "testing" | "test" | "review" | "merge" | "implementation"
    ) {
        return Err(error("Unknown checkpoint phase"));
    }
    let v = &request.identity;
    let wt = Path::new(
        v["execution_root"]
            .as_str()
            .ok_or_else(|| error("Worktree identity required"))?,
    );
    git_state::verify_repository(root, wt)?;
    let task_id = v["task_id"].as_str().unwrap_or_default();
    let repair = if v["role"] == "merge_fix" && request.phase == "merge" {
        Some(merge_repair::load(root, wt, task_id)?)
    } else {
        None
    };
    let git = if repair.is_some() {
        git_state::snapshot_for(wt, repair.as_ref())?
    } else {
        git_state::snapshot(wt)?
    };
    let (task, source_hashes) = if let Some(repair) = &repair {
        merge_repair::sources(repair)?
    } else {
        git_state::task_sources(wt, task_id)?
    };
    let criteria = task["tasks"][0]["acceptance_criteria"]
        .as_array()
        .map(|rows| {
            rows.iter()
                .enumerate()
                .map(|(i, c)| {
                    c["id"]
                        .as_str()
                        .map(str::to_string)
                        .unwrap_or_else(|| format!("AC-{:02}", i + 1))
                })
                .collect()
        })
        .unwrap_or_default();
    let mut c = Checkpoint {
        schema_version: 1,
        id: uuid::Uuid::new_v4().to_string(),
        project_id: a.project_id.clone(),
        task_id: v["task_id"].as_str().unwrap_or_default().into(),
        role: v["role"].as_str().unwrap_or_default().into(),
        tool_id: v["tool_id"].as_str().unwrap_or_default().into(),
        phase: request.phase.clone(),
        worktree: wt.display().to_string(),
        branch: git.branch,
        source_revision: git.revision,
        worktree_dirty_digest: git.digest,
        worktree_content_digest: git.content_digest,
        source_hashes,
        acceptance_criteria_ids: criteria,
        mandatory_task: task,
        completed_steps: vec![],
        remaining_steps: vec![
            "Revalidate all mandatory acceptance criteria against current code".into(),
        ],
        changed_files: git.changed,
        decisions: vec![],
        test_results: vec![json!({"command":null,"status":"not_observed","artifact_ref":null})],
        blocking_issues: vec![],
        review_objections: Value::Null,
        memory_refs: vec![],
        source_refs: vec!["worktree.prd.json".into()],
        next_action: format!(
            "Continue phase {}; verify current task and criteria before making changes",
            request.phase
        ),
        reason: reason.into(),
        validation: json!({"schema_ok":true,"sources_verified":true,"narrative":"deterministic; no completion inferred"}),
    };
    // Runtime is evidence only, never an instruction source or completion authority.
    let paths = crate::ProjectPaths::from_root(root);
    if root.join(".macc/state/coordinator.sqlite").exists() {
        let storage = crate::coordinator_storage::SqliteStorage::new(
            crate::coordinator_storage::CoordinatorStoragePaths::from_project_paths(&paths),
        );
        let registry = storage.load_registry()?;
        if let Some(task) = registry.tasks.iter().find(|t| t.id == c.task_id) {
            c.review_objections = json!({"review_cycles":task.task_runtime.review_cycles,"review_feedback":task.review,"last_error":task.task_runtime.last_error_message,"workflow_state":task.state});
        }
    }
    let mut value = serde_json::to_value(c).map_err(|e| error(e.to_string()))?;
    redact_evidence(&mut value);
    c = serde_json::from_value(value).map_err(|e| error(e.to_string()))?;
    Ok(c)
}
pub(crate) fn redact_evidence(v: &mut Value) {
    match v {
        Value::String(text) => {
            let mut findings =
                crate::security::secret_scan::scan_bytes("checkpoint", text.as_bytes());
            findings.sort_by_key(|f| f.range.start);
            let mut boundary = text.len();
            for f in findings.into_iter().rev() {
                if f.range.end <= boundary {
                    boundary = f.range.start;
                    text.replace_range(f.range, "[REDACTED]");
                }
            }
        }
        Value::Array(rows) => {
            for row in rows {
                redact_evidence(row)
            }
        }
        Value::Object(object) => {
            for (key, value) in object {
                if matches!(
                    key.to_ascii_lowercase().as_str(),
                    "password" | "secret" | "token" | "api_key" | "authorization" | "lease_token"
                ) {
                    *value = Value::String("[REDACTED]".into());
                } else {
                    redact_evidence(value)
                }
            }
        }
        _ => {}
    }
}
pub(crate) fn load(db: &rusqlite::Connection, id: &str) -> Result<Checkpoint> {
    let (raw, sha): (String, String) = sql(db.query_row(
        "SELECT content_json,content_sha256 FROM task_checkpoints WHERE id=?1",
        [id],
        |r| Ok((r.get(0)?, r.get(1)?)),
    ))?;
    if digest(raw.as_bytes()) != sha {
        return Err(error("Checkpoint checksum mismatch"));
    }
    let c: Checkpoint = json(raw.as_bytes())?;
    if c.schema_version != 1 || c.id != id {
        return Err(error("Unsupported or inconsistent checkpoint schema"));
    }
    Ok(c)
}
pub fn get(root: &Path, id: &str) -> Result<Checkpoint> {
    let _guard = lock(root)?;
    let (db, a) = db::active(root)?;
    let c = load(&db, id)?;
    if c.project_id != a.project_id {
        return Err(error("Foreign checkpoint project"));
    }
    Ok(c)
}
pub(super) fn list(db: &rusqlite::Connection) -> Result<Vec<CheckpointView>> {
    if !ready(db)? {
        return Ok(vec![]);
    }
    let mut q=sql(db.prepare("SELECT id,task_id,role,tool_id,phase,reason,validation_state,created_at,content_sha256 FROM task_checkpoints ORDER BY created_at DESC,rowid DESC LIMIT 200"))?;
    let rows = sql(q.query_map([], |r| {
        Ok(CheckpointView {
            id: r.get(0)?,
            task_id: r.get(1)?,
            role: r.get(2)?,
            tool_id: r.get(3)?,
            phase: r.get(4)?,
            reason: r.get(5)?,
            validation_state: r.get(6)?,
            created_at: r.get(7)?,
            content_sha256: r.get(8)?,
        })
    }))?;
    rows.map(sql).collect()
}
pub(crate) fn latest(
    db: &rusqlite::Connection,
    project: &str,
    task: &str,
    role: &str,
) -> Result<Option<Checkpoint>> {
    let id:Option<String>=sql(db.query_row("SELECT id FROM task_checkpoints WHERE project_id=?1 AND task_id=?2 AND role=?3 ORDER BY created_at DESC,rowid DESC LIMIT 1",params![project,task,role],|r|r.get(0)).optional())?;
    id.map(|id| load(db, &id)).transpose()
}
pub(super) fn fingerprint(c: &Checkpoint) -> String {
    digest(
        json!([
            c.project_id,
            c.task_id,
            c.role,
            c.phase,
            c.worktree,
            c.branch,
            c.source_revision,
            c.worktree_dirty_digest,
            c.source_hashes
        ])
        .to_string()
        .as_bytes(),
    )
}
pub(super) fn same_source(a: &Checkpoint, b: &Checkpoint) -> bool {
    a.project_id == b.project_id
        && a.task_id == b.task_id
        && a.worktree == b.worktree
        && a.branch == b.branch
        && a.source_hashes == b.source_hashes
}

pub(super) fn initial_schema() -> &'static str {
    schema::SCHEMA
}

/// Preserve the existing E903 contract across native synchronous phases.
pub fn precondition_reason(output: &str) -> Option<String> {
    let kind = output
        .lines()
        .rev()
        .find_map(|line| line.trim().strip_prefix("MACC_TASK_RESULT:").map(str::trim))?;
    if kind != "precondition_unmet" {
        return None;
    }
    let conditions = output
        .lines()
        .filter_map(|line| {
            line.trim()
                .strip_prefix("MACC_TASK_PRECONDITION:")
                .map(str::trim)
        })
        .collect::<Vec<_>>();
    Some(format!(
        "E903: {}",
        if conditions.is_empty() {
            "Validated checkpoint requires reconciliation".into()
        } else {
            conditions.join("; ")
        }
    ))
}
