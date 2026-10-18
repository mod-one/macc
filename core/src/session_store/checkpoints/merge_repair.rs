//! Explicit checkpoint contract for the coordinator's detached integration slot.
//! Ordinary task worktrees can never opt into relaxed merge-state capture.
use super::*;
use std::{collections::BTreeMap, path::PathBuf};

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MergeRepair {
    pub worktree: PathBuf,
    pub source_worktree: PathBuf,
    pub task_id: String,
    pub base_revision: String,
    pub feature_revision: String,
}
fn revision(path: &Path, reference: &str) -> Result<String> {
    if reference.starts_with('-') || reference.is_empty() {
        return Err(error("Invalid merge repair reference"));
    }
    git_state::text(
        path,
        &["rev-parse", "--verify", &format!("{reference}^{{commit}}")],
    )
}
fn location(root: &Path, worktree: &Path) -> Result<PathBuf> {
    let common = git_state::text(
        root,
        &["rev-parse", "--path-format=absolute", "--git-common-dir"],
    )?;
    let expected = Path::new(&common).join("macc/integration");
    let actual = worktree.canonicalize().map_err(|e| error(e.to_string()))?;
    if actual != expected.canonicalize().map_err(|e| error(e.to_string()))? {
        return Err(error(
            "Merge repair requires the coordinator integration slot",
        ));
    }
    git_state::verify_repository(root, &actual)?;
    Ok(actual.join(".macc/merge-repair.json"))
}
pub fn capture(root: &Path, request: &MergeRepair) -> Result<Value> {
    let path = location(root, &request.worktree)?;
    git_state::verify_repository(root, &request.source_worktree)?;
    git_state::task_sources(&request.source_worktree, &request.task_id)?;
    let context = MergeRepair {
        worktree: request
            .worktree
            .canonicalize()
            .map_err(|e| error(e.to_string()))?,
        source_worktree: request
            .source_worktree
            .canonicalize()
            .map_err(|e| error(e.to_string()))?,
        task_id: request.task_id.clone(),
        base_revision: revision(root, &request.base_revision)?,
        feature_revision: revision(root, &request.feature_revision)?,
    };
    context.validate()?;
    let _guard = lock(root)?;
    let (db, _) = db::active(root)?;
    let active: bool = sql(db.query_row(
        "SELECT EXISTS(SELECT 1 FROM session_leases WHERE owner_worktree_id=?1)",
        [context.worktree.to_string_lossy().as_ref()],
        |r| r.get(0),
    ))?;
    if active {
        return Err(error("Merge repair slot has an active lease"));
    }
    write(
        root,
        &path,
        &serde_json::to_vec(&context).map_err(|e| error(e.to_string()))?,
    )?;
    db::event(
        &db,
        &context.task_id,
        "merge_repair_prepared",
        serde_json::to_value(&context).map_err(|e| error(e.to_string()))?,
    )?;
    Ok(json!({"prepared":true,"task_id":context.task_id}))
}
impl MergeRepair {
    fn validate(&self) -> Result<()> {
        if git_state::text(&self.worktree, &["rev-parse", "--abbrev-ref", "HEAD"])? != "HEAD" {
            return Err(error(
                "Merge repair requires detached HEAD; branch ownership changed",
            ));
        }
        let head = revision(&self.worktree, "HEAD")?;
        // Repair may retain the original conflicted merge, or complete it. Neither
        // an unrelated HEAD nor a different MERGE_HEAD is a valid continuation.
        if head != self.base_revision
            && !(git_state::descends_from(&self.worktree, &self.base_revision)
                && git_state::descends_from(&self.worktree, &self.feature_revision))
        {
            return Err(error(
                "Merge repair HEAD no longer matches its pinned inputs",
            ));
        }
        let merge_path = git_state::text(
            &self.worktree,
            &[
                "rev-parse",
                "--path-format=absolute",
                "--git-path",
                "MERGE_HEAD",
            ],
        )?;
        if Path::new(&merge_path).exists()
            && std::fs::read_to_string(merge_path)
                .map_err(|e| error(e.to_string()))?
                .trim()
                != self.feature_revision
        {
            return Err(error("Merge repair MERGE_HEAD differs from pinned feature"));
        }
        Ok(())
    }
}
pub(super) fn load(root: &Path, wt: &Path, task: &str) -> Result<MergeRepair> {
    let path = location(root, wt)?;
    let raw = bytes(&path)?;
    if raw.len() > 16384 {
        return Err(error("Merge repair context exceeds bound"));
    }
    let c: MergeRepair = json(&raw)?;
    if c.task_id != task || c.worktree != wt.canonicalize().map_err(|e| error(e.to_string()))? {
        return Err(error("Merge repair identity mismatch"));
    }
    git_state::verify_repository(root, &c.source_worktree)?;
    c.validate()?;
    Ok(c)
}
pub(super) fn sources(c: &MergeRepair) -> Result<(Value, BTreeMap<String, String>)> {
    let (task, mut hashes) = git_state::task_sources(&c.source_worktree, &c.task_id)?;
    hashes.insert(
        "macc:merge-repair".into(),
        digest(&serde_json::to_vec(c).map_err(|e| error(e.to_string()))?),
    );
    Ok((task, hashes))
}

#[cfg(test)]
#[path = "merge_repair_tests.rs"]
mod tests;
