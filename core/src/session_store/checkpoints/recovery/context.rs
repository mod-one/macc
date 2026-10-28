//! Resolve the actual retained worker and the authoritative dispatch contract.
use super::*;

pub(super) struct Context {
    pub current: Checkpoint,
    pub payload: Option<Value>,
    pub binding: Value,
    pub materialization_required: bool,
}

pub(super) fn resolve(root: &Path, previous: &Checkpoint) -> Result<Context> {
    let mut target = Path::new(&previous.worktree).to_path_buf();
    let mut payload = None;
    let mut owner = Value::Null;
    let mut prd_binding = Value::Null;
    if previous.role != "merge_fix"
        && (root.join(".macc/state/coordinator.sqlite").exists()
            || root
                .join(".macc/automation/task/task_registry.json")
                .exists())
    {
        let paths = crate::ProjectPaths::from_root(root);
        let config = if paths.config_path.exists() {
            crate::load_canonical_config(&paths.config_path)?
        } else {
            crate::config::CanonicalConfig::default()
        };
        let mut args = std::collections::BTreeMap::new();
        if let Some(mode) = config
            .automation
            .coordinator
            .as_ref()
            .and_then(|c| c.storage_mode.as_ref())
        {
            args.insert("storage-mode".into(), mode.clone());
        }
        let value = crate::coordinator::state::coordinator_state_registry_load(root, &args)?;
        let registry = crate::coordinator::model::TaskRegistry::from_value(&value)?;
        if let Some(task) = registry.find_task(&previous.task_id) {
            if task
                .task_runtime
                .pid
                .is_some_and(crate::coordinator::helpers::is_pid_running)
            {
                return Err(error("Checkpoint recovery requires a stopped task worker"));
            }
            if matches!(task.state.as_str(), "merged" | "done") {
                return Err(error("Checkpoint recovery cannot reopen a delivered task"));
            }
            if let Some(path) = task.worktree_path() {
                target = Path::new(path)
                    .canonicalize()
                    .map_err(|e| error(e.to_string()))?;
                let pool = root
                    .join(".macc/worktree")
                    .canonicalize()
                    .map_err(|e| error(e.to_string()))?;
                if target.parent() != Some(pool.as_path()) {
                    return Err(error(
                        "Checkpoint recovery target is outside the managed worker pool",
                    ));
                }
                if task.branch()
                    != Some(
                        git_state::text(&target, &["symbolic-ref", "--short", "HEAD"])?.as_str(),
                    )
                {
                    return Err(error(
                        "Checkpoint recovery worker branch differs from registry ownership",
                    ));
                }
                owner = serde_json::to_value(task).map_err(|e| error(e.to_string()))?;
                let prd = crate::prd_queue::active_path(
                    root,
                    config.automation.coordinator.as_ref(),
                    task.task_runtime
                        .extra
                        .get("checkpoint_prd_file")
                        .and_then(Value::as_str),
                )?;
                let raw = std::fs::read(&prd).map_err(|e| error(e.to_string()))?;
                prd_binding = json!({"path":prd,"sha256":digest(&raw)});
                payload = Some(crate::coordinator::helpers::scoped_prd_for_task(
                    &prd,
                    &previous.task_id,
                )?);
            }
        }
    }
    git_state::verify_repository(root, &target)?;
    let repair = if previous.role == "merge_fix" && previous.phase == "merge" {
        Some(merge_repair::load(root, &target, &previous.task_id)?)
    } else {
        None
    };
    let git = git_state::snapshot_for(&target, repair.as_ref())?;
    let prior_git = if target == Path::new(&previous.worktree) {
        git.clone()
    } else {
        git_state::snapshot_for(Path::new(&previous.worktree), repair.as_ref())?
    };
    if target != Path::new(&previous.worktree) {
        git_state::verify_repository(root, Path::new(&previous.worktree))?;
        if (!prior_git.changed.is_empty() && prior_git.content_digest != git.content_digest)
            || !git_state::descends_from(&target, &prior_git.revision)
        {
            return Err(error(format!("{CODE}: retained work in {} is not integrated into {}; preserve and integrate it before moving the checkpoint",previous.worktree,target.display())));
        }
    }
    let (task, hashes) = if let Some(repair) = &repair {
        merge_repair::sources(repair)?
    } else if let Some(payload) = &payload {
        git_state::task_sources_from(&target, &previous.task_id, payload.clone())?
    } else {
        git_state::task_sources(&target, &previous.task_id)?
    };
    let local_prd = if repair.is_some() {
        None
    } else {
        match std::fs::read(target.join("worktree.prd.json")) {
            Ok(bytes) => Some(digest(&bytes)),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound && payload.is_some() => None,
            Err(e) => return Err(error(e.to_string())),
        }
    };
    let materialization_required = payload.is_some()
        && git_state::task_sources(&target, &previous.task_id)
            .map(|(_, local_hashes)| local_hashes != hashes)
            .unwrap_or(true);
    let mut current = previous.clone();
    current.worktree = target.display().to_string();
    current.branch = git.branch;
    current.source_revision = git.revision;
    current.worktree_dirty_digest = git.digest;
    current.worktree_content_digest = git.content_digest;
    current.changed_files = git.changed;
    current.mandatory_task = task;
    current.source_hashes = hashes;
    Ok(Context {
        current,
        payload,
        materialization_required,
        binding: json!({"owner":owner,"authoritative_prd":prd_binding,"worker_prd_sha256":local_prd,"previous_worker_revision":prior_git.revision,"previous_worker_digest":prior_git.digest}),
    })
}
