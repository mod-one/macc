//! Journaled relocation of a retained performer that occupied reserved merge scratch.
//! Caller holds the common role lock. No reset, clean or receipt rewriting is used.
use super::{
    model::{Task, TaskRegistry},
    repair_handoff::git,
    worktree_roles as roles,
};
use crate::{ProjectPaths, Result};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};

#[derive(Serialize, Deserialize)]
struct Recovery {
    task_id: String,
    claim_id: Option<String>,
    branch: String,
    old_path: PathBuf,
    new_path: PathBuf,
    head: String,
    branch_head: String,
    tree: String,
    index: String,
    recovery_ref: String,
}
pub fn journal_path(root: &Path, task_id: &str) -> PathBuf {
    root.join(".macc/persistent/worktree-recovery")
        .join(format!("{:x}.json", Sha256::digest(task_id.as_bytes())))
}
fn args(root: &Path) -> Result<std::collections::BTreeMap<String, String>> {
    let paths = ProjectPaths::from_root(root);
    let config = if paths.config_path.exists() {
        crate::load_canonical_config(&paths.config_path)?
    } else {
        Default::default()
    };
    Ok(config
        .automation
        .coordinator
        .and_then(|c| c.storage_mode)
        .map(|mode| std::collections::BTreeMap::from([("storage-mode".into(), mode)]))
        .unwrap_or_default())
}
fn registry(root: &Path) -> Result<TaskRegistry> {
    TaskRegistry::from_value(&super::state::coordinator_state_registry_load(
        root,
        &args(root)?,
    )?)
}
fn validate_binding(task: &Task, proof: &Recovery) -> Result<()> {
    if task.id != proof.task_id
        || task.branch() != Some(proof.branch.as_str())
        || task.task_runtime.claim_id != proof.claim_id
        || !task
            .worktree_path()
            .is_some_and(|p| Path::new(p) == proof.old_path || Path::new(p) == proof.new_path)
    {
        return Err(roles::conflict(
            "Retained task binding differs from recovery journal; all work preserved",
        ));
    }
    Ok(())
}
fn save_binding(root: &Path, task: &mut Task, proof: &Recovery) -> Result<()> {
    validate_binding(task, proof)?;
    let mut reg = registry(root)?;
    let stored = reg
        .find_task_mut(&task.id)
        .ok_or_else(|| roles::conflict("Recovery task is no longer registered"))?;
    validate_binding(stored, proof)?;
    stored.ensure_worktree().worktree_path = Some(proof.new_path.to_string_lossy().into());
    stored.task_runtime.last_worktree_path = Some(proof.new_path.to_string_lossy().into());
    super::state::coordinator_state_registry_save(root, &args(root)?, &reg.to_value()?)?;
    task.ensure_worktree().worktree_path = Some(proof.new_path.to_string_lossy().into());
    task.task_runtime.last_worktree_path = Some(proof.new_path.to_string_lossy().into());
    task.task_runtime.extra.insert("worktree_recovery".into(), serde_json::json!({"from":proof.old_path,"to":proof.new_path,"preserved_ref":proof.recovery_ref}));
    Ok(())
}

/// Relocate a proven legacy collision before checkpoint binding is evaluated.
pub fn recover_task(root: &Path, task: &mut Task) -> Result<()> {
    if !task
        .worktree_path()
        .is_some_and(|p| Path::new(p).file_name().and_then(|n| n.to_str()) == Some("integration"))
    {
        return Ok(());
    }
    let _locks = roles::retained_locks(root, task)?;
    recover_reserved(root, task)
}

pub(crate) fn recover_reserved(root: &Path, task: &mut Task) -> Result<()> {
    let Some(old_path) = task.worktree_path().map(PathBuf::from) else {
        return Ok(());
    };
    // Supervisor slots are never recoverable as performers by inference.
    if old_path.file_name().and_then(|n| n.to_str()) != Some("integration") {
        return Ok(());
    }
    if task
        .task_runtime
        .pid
        .is_some_and(super::helpers::is_pid_running)
    {
        return Err(roles::conflict(format!(
            "Task {} still has a live performer; ownership recovery deferred",
            task.id
        )));
    }
    let paths = ProjectPaths::from_root(root);
    let record = journal_path(root, &task.id);
    crate::project_ownership::validate_path(
        &paths,
        &crate::relative_to_root(&paths, &record).unwrap(),
    )?;
    let prior: Option<Recovery> = if record.exists() {
        Some(
            serde_json::from_slice(&std::fs::read(&record).map_err(crate::persistent::error)?)
                .map_err(crate::persistent::error)?,
        )
    } else {
        None
    };
    let mut worktrees = vec![old_path.as_path()];
    if let Some(proof) = &prior {
        worktrees.push(proof.new_path.as_path());
    }
    let _sessions = crate::session_store::checkpoints::recovery::idle_worktrees(root, &worktrees)?;
    crate::project_ownership::validate_path(
        &paths,
        &crate::relative_to_root(&paths, &old_path.join(".macc/worktree.json"))
            .ok_or_else(|| roles::conflict("Worker metadata is outside the project"))?,
    )?;
    let proof: Recovery = if let Some(proof) = prior {
        proof
    } else {
        let branch = task
            .branch()
            .ok_or_else(|| roles::conflict("Retained performer has no expected branch"))?
            .to_owned();
        if !roles::same_path(&old_path, &crate::persistent::integration_path(root)?) {
            return Err(roles::conflict(
                "Reserved performer is outside the primary integration slot",
            ));
        }
        if old_path
            .symlink_metadata()
            .is_ok_and(|m| m.file_type().is_symlink())
        {
            return Err(roles::conflict("Reserved performer path is a symlink"));
        }
        let metadata = crate::read_worktree_metadata(&old_path)?.ok_or_else(|| {
            roles::conflict("Reserved slot lacks performer ownership metadata; not adopted")
        })?;
        if metadata.branch != branch || metadata.tool != task.tool.as_deref().unwrap_or("") {
            return Err(roles::conflict(
                "Reserved slot metadata differs from its retained task",
            ));
        }
        let expected = registry(root)?
            .find_task(&task.id)
            .cloned()
            .ok_or_else(|| roles::conflict("Recovery requires a durable task assignment"))?;
        if expected.worktree_path() != task.worktree_path()
            || expected.branch() != task.branch()
            || expected.task_runtime.claim_id != task.task_runtime.claim_id
        {
            return Err(roles::conflict(
                "Recovery task ownership changed; no files moved",
            ));
        }
        let head = git(&old_path, &["rev-parse", "HEAD"])?;
        let branch_head = git(
            root,
            &["rev-parse", "--verify", &format!("refs/heads/{branch}")],
        )?;
        let actual = crate::git::current_branch_name(&old_path)?;
        if !actual.is_empty() && actual != branch {
            return Err(roles::conflict(format!(
                "Reserved slot belongs to branch {actual}, expected {branch}; preserved"
            )));
        }
        if crate::git::git_common_dir(&old_path)?
            .canonicalize()
            .map_err(crate::persistent::error)?
            != crate::git::git_common_dir(root)?
                .canonicalize()
                .map_err(crate::persistent::error)?
        {
            return Err(roles::conflict(
                "Reserved slot belongs to another repository",
            ));
        }
        if head != branch_head {
            let parents = git(&old_path, &["show", "-s", "--format=%P", "HEAD"])?;
            let parents: Vec<_> = parents.split_whitespace().collect();
            let subject = git(&old_path, &["show", "-s", "--format=%s", "HEAD"])?;
            if !actual.is_empty()
                || parents.len() != 2
                || parents[1] != branch_head
                || subject
                    != crate::commit_message::merge_commit(&task.id)
                        .format()
                        .lines()
                        .next()
                        .unwrap_or("")
                || git(&old_path, &["rev-parse", "HEAD^{tree}"])?
                    != git(root, &["rev-parse", &format!("{branch_head}^{{tree}}")])?
            {
                return Err(roles::conflict("Detached content is not the unchanged MACC merge of the retained branch; independent ownership recovery required"));
            }
        }
        if crate::git::rev_parse_verify(&old_path, "MERGE_HEAD")?
            || !crate::git::worktrees_for_branch(root, &branch)?
                .iter()
                .all(|p| roles::same_path(p, &old_path))
        {
            return Err(roles::conflict(
                "Retained branch is busy or checked out elsewhere; preserved",
            ));
        }
        let tree = crate::git::worktree_content_tree(&old_path)?;
        let index = git(&old_path, &["write-tree"])?;
        let token = uuid::Uuid::new_v4().to_string();
        let recovery_ref = format!("refs/macc/recovery/ownership-{token}");
        for (suffix, tree) in [("content", tree.as_str()), ("index", index.as_str())] {
            let commit = git(
                &old_path,
                &[
                    "commit-tree",
                    tree,
                    "-p",
                    &head,
                    "-m",
                    "wip: preserve retained performer ownership recovery",
                ],
            )?;
            git(
                root,
                &["update-ref", &format!("{recovery_ref}-{suffix}"), &commit],
            )?;
        }
        git(
            root,
            &["update-ref", &format!("{recovery_ref}-head"), &head],
        )?;
        git(
            root,
            &[
                "update-ref",
                &format!("{recovery_ref}-branch"),
                &branch_head,
            ],
        )?;
        let tool = task.tool.as_deref().unwrap_or("performer");
        let destination = root
            .join(".macc/worktree")
            .join(super::helpers::next_free_worker_slug(root, tool)?);
        let proof = Recovery {
            task_id: task.id.clone(),
            claim_id: task.task_runtime.claim_id.clone(),
            branch,
            old_path: old_path.clone(),
            new_path: destination,
            head,
            branch_head,
            tree,
            index,
            recovery_ref,
        };
        crate::atomic_write(
            &paths,
            &record,
            &serde_json::to_vec_pretty(&proof).map_err(crate::persistent::error)?,
        )?;
        proof
    };
    validate_binding(task, &proof)?;
    if !roles::same_path(&proof.old_path, &crate::persistent::integration_path(root)?) {
        return Err(roles::conflict(
            "Recovery source is not the reserved integration path",
        ));
    }
    crate::project_ownership::validate_path(
        &paths,
        &crate::relative_to_root(&paths, &proof.new_path)
            .ok_or_else(|| roles::conflict("Recovery destination is outside the project"))?,
    )?;
    let pool = root
        .join(".macc/worktree")
        .canonicalize()
        .map_err(crate::persistent::error)?;
    if proof
        .new_path
        .parent()
        .and_then(|p| p.canonicalize().ok())
        .as_ref()
        != Some(&pool)
        || roles::reserved(&proof.new_path)
    {
        return Err(roles::conflict("Invalid ownership recovery destination"));
    }
    if proof.old_path.exists() && !proof.new_path.exists() {
        if git(&proof.old_path, &["rev-parse", "HEAD"])? != proof.head
            || crate::git::worktree_content_tree(&proof.old_path)? != proof.tree
            || git(&proof.old_path, &["write-tree"])? != proof.index
        {
            return Err(roles::conflict(
                "Worker changed after recovery was prepared; original work and refs preserved",
            ));
        }
        git(
            root,
            &[
                "worktree",
                "move",
                proof.old_path.to_str().unwrap(),
                proof.new_path.to_str().unwrap(),
            ],
        )?;
    }
    if !proof.new_path.join(".git").exists() || proof.old_path.exists() {
        return Err(roles::conflict(
            "Recovery paths are ambiguous; inspect the journal and preserved refs",
        ));
    }
    if crate::git::git_common_dir(&proof.new_path)?
        .canonicalize()
        .map_err(crate::persistent::error)?
        != crate::git::git_common_dir(root)?
            .canonicalize()
            .map_err(crate::persistent::error)?
    {
        return Err(roles::conflict(
            "Recovered checkout belongs to a different repository",
        ));
    }
    let branch = crate::git::current_branch(&proof.new_path)?;
    if branch != "HEAD" && branch != proof.branch {
        return Err(roles::conflict(format!(
            "Recovery destination now belongs to branch {branch}; preserved"
        )));
    }
    let head = git(&proof.new_path, &["rev-parse", "HEAD"])?;
    if (head != proof.head && head != proof.branch_head)
        || git(
            root,
            &["rev-parse", &format!("refs/heads/{}", proof.branch)],
        )? != proof.branch_head
        || crate::git::worktree_content_tree(&proof.new_path)? != proof.tree
        || git(&proof.new_path, &["write-tree"])? != proof.index
    {
        return Err(roles::conflict(
            "Recovered worker changed; refs and local work preserved for inspection",
        ));
    }
    if !crate::git::worktrees_for_branch(root, &proof.branch)?
        .iter()
        .all(|p| roles::same_path(p, &proof.new_path))
    {
        return Err(roles::conflict(
            "Recovered branch acquired by another checkout; preserved",
        ));
    }
    crate::project_ownership::validate_path(
        &paths,
        &crate::relative_to_root(&paths, &proof.new_path.join(".macc/worktree.json"))
            .ok_or_else(|| roles::conflict("Recovery metadata is outside the project"))?,
    )?;
    git(
        &proof.new_path,
        &[
            "symbolic-ref",
            "HEAD",
            &format!("refs/heads/{}", proof.branch),
        ],
    )?;
    let mut metadata = crate::read_worktree_metadata(&proof.new_path)?
        .ok_or_else(|| roles::conflict("Recovered performer metadata disappeared"))?;
    metadata.id = proof.new_path.file_name().unwrap().to_string_lossy().into();
    metadata.slug = metadata.id.clone();
    crate::atomic_write(
        &paths,
        &proof.new_path.join(".macc/worktree.json"),
        &serde_json::to_vec_pretty(&metadata).map_err(crate::persistent::error)?,
    )?;
    save_binding(root, task, &proof)?;
    tracing::info!(task_id=%task.id,from=%proof.old_path.display(),to=%proof.new_path.display(),recovery_ref=%proof.recovery_ref,"Retained performer moved out of reserved integration slot");
    Ok(())
}
