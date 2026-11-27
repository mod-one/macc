//! Provenance checks for the single-task contract handed to a worker.
use super::model::{Task, TaskRegistry};
use crate::{persistent::error, project_ownership, MaccError, ProjectPaths, Result};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::{
    fs,
    path::{Path, PathBuf},
};

mod recovery;
pub(crate) use recovery::recover_dispatch;
pub use recovery::{is_dispatch_failure, requeued};

pub const NAME: &str = "worktree.prd.json";
pub const RECOVERY_KEY: &str = "scoped_prd_recovery";
pub const INCIDENT_KEY: &str = "scoped_prd_incident";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Cause {
    LegacyPerformerMarker,
    UnverifiedFile,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Incident {
    pub cause: Cause,
    pub worker: String,
    pub previous_task: Option<String>,
    pub incoming_task: String,
    pub path: PathBuf,
    pub before_launch: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub allocation_error: Option<String>,
}

impl Incident {
    pub(super) fn unverified(worktree: &Path, incoming: &str) -> Self {
        Self {
            cause: Cause::UnverifiedFile,
            worker: worktree
                .file_name()
                .unwrap_or_default()
                .to_string_lossy()
                .into_owned(),
            previous_task: None,
            incoming_task: incoming.into(),
            path: worktree.join(NAME),
            before_launch: true,
            allocation_error: None,
        }
    }
    fn refused(self) -> MaccError {
        MaccError::ScopedPrd(Box::new(self))
    }
}

impl std::fmt::Display for Incident {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self.cause {
            Cause::LegacyPerformerMarker => write!(
                f,
                "The previous performer marked the generated PRD for {} as passed.",
                self.previous_task.as_deref().unwrap_or("an earlier task")
            )?,
            Cause::UnverifiedFile => {
                write!(f, "The existing PRD has unverified ownership or edits.")?
            }
        }
        write!(
            f,
            " {} did not start on {}. File: {}",
            self.incoming_task,
            self.worker,
            self.path.display()
        )?;
        if let Some(error) = &self.allocation_error {
            write!(f, ". An alternative worker could not be created: {error}")?;
        }
        Ok(())
    }
}

fn hash(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

/// Reverse only the two historical values (missing or false). The inventory
/// covers the entire original document, including every other task property.
fn original(bytes: &[u8], document: &Value, expected: &str) -> Option<Vec<u8>> {
    // serde_json::Value collapses duplicate object members. Such a file
    // cannot establish that only the historical marker changed.
    if member_count(bytes) != member_count(&serde_json::to_vec(document).ok()?) {
        return None;
    }
    let mut candidate = document.clone();
    let tasks = candidate.get_mut("tasks")?.as_array_mut()?;
    if tasks.len() != 1 || tasks[0].get("passes")? != true {
        return None;
    }
    tasks[0].as_object_mut()?.remove("passes");
    let bytes = serde_json::to_vec_pretty(&candidate).ok()?;
    if hash(&bytes) == expected {
        return Some(bytes);
    }
    candidate["tasks"][0]["passes"] = false.into();
    let bytes = serde_json::to_vec_pretty(&candidate).ok()?;
    (hash(&bytes) == expected).then_some(bytes)
}

/// Count member separators in an already parsed JSON document. Colons in
/// quoted strings, including escaped quotes, are ordinary data.
fn member_count(document: &[u8]) -> usize {
    let mut bytes = document.iter();
    let mut quoted = false;
    let mut count = 0;
    while let Some(byte) = bytes.next() {
        match *byte {
            b'\\' if quoted => {
                bytes.next();
            }
            b'"' => quoted = !quoted,
            b':' if !quoted => count += 1,
            _ => {}
        }
    }
    count
}

pub(super) fn tracked(worktree: &Path) -> Result<bool> {
    let out = crate::git::run_git_output_mapped(
        worktree,
        &["ls-files", "-z", "--", NAME],
        "check tracked scoped PRD",
    )?;
    if !out.status.success() {
        return Err(error("Cannot inspect scoped PRD ownership in Git"));
    }
    Ok(!out.stdout.is_empty())
}

/// Every new worker would inherit this file. Detect it once before touching
/// any checkout or allocating a string of unusable worker directories.
pub(super) fn check_base(root: &Path, base: &str, id: &str) -> Result<()> {
    let result = crate::git::run_git_output_mapped(
        root,
        &["ls-tree", "-z", base, "--", NAME],
        "check worker base for a tracked scoped PRD",
    )?;
    if !result.status.success() {
        return Err(error(format!("Cannot inspect worker base {base}")));
    }
    if !result.stdout.is_empty() {
        return Err(MaccError::ScopedPrd(Box::new(Incident {
            cause: Cause::UnverifiedFile,
            worker: format!("any worker based on {base}"),
            previous_task: None,
            incoming_task: id.into(),
            path: root.join(NAME),
            before_launch: true,
            allocation_error: None,
        })));
    }
    Ok(())
}

pub(crate) fn worker_busy(registry: &TaskRegistry, path: &Path) -> bool {
    let registered = registry
        .tasks
        .iter()
        .filter(|t| {
            t.worktree_path()
                .is_some_and(|p| super::worktree_roles::same_path(Path::new(p), path))
        })
        .any(|t| {
            let rt = &t.task_runtime;
            let live = [rt.pid, rt.merge_worker_pid]
                .into_iter()
                .flatten()
                .any(super::helpers::is_pid_running);
            #[cfg(unix)]
            if let Some(group) = rt
                .process_group_id
                .and_then(|n| i32::try_from(n).ok())
                .filter(|n| *n > 1)
            {
                // Signal zero probes whether the process group still exists.
                if unsafe { libc::killpg(group, 0) } == 0
                    || std::io::Error::last_os_error().raw_os_error() == Some(libc::EPERM)
                {
                    return true;
                }
            }
            live
        });
    if registered {
        return true;
    }
    #[cfg(target_os = "linux")]
    {
        // Older runs can leave children after the registered worker exits.
        let Ok(processes) = fs::read_dir("/proc") else {
            return true;
        };
        let Ok(path) = path.canonicalize() else {
            return true;
        };
        return processes
            .flatten()
            .filter_map(|e| e.file_name().to_str()?.parse::<u32>().ok())
            .filter(|pid| *pid != std::process::id())
            .any(|pid| {
                fs::read_link(format!("/proc/{pid}/cwd")).is_ok_and(|cwd| cwd.starts_with(&path))
            });
    }
    #[cfg(not(target_os = "linux"))]
    false
}

/// Caller holds the repository's worktree role lock and has verified inactivity.
/// Restore the original bytes first: a crash after this step leaves an ordinary
/// owned file which dispatch can safely regenerate on the next attempt.
pub(super) fn prepare(worktree: &Path, incoming: &str) -> Result<Option<Incident>> {
    let paths = ProjectPaths::from_root(worktree);
    let _lock = project_ownership::lock(&paths)?;
    inspect(&paths, incoming, true)
}

fn inspect(paths: &ProjectPaths, incoming: &str, restore: bool) -> Result<Option<Incident>> {
    let worktree = &paths.root;
    let path = worktree.join(NAME);
    let mut incident = Incident::unverified(worktree, incoming);
    if tracked(worktree)? {
        return Err(incident.refused());
    }
    let metadata = match path.symlink_metadata() {
        Ok(meta) => meta,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(error(e)),
    };
    if !metadata.is_file() {
        return Err(incident.refused());
    }
    let inventory = project_ownership::read(paths)?;
    let bytes = fs::read(&path).map_err(error)?;
    let document: Value = serde_json::from_slice(&bytes).unwrap_or(Value::Null);
    incident.previous_task = document
        .pointer("/tasks/0/id")
        .and_then(Value::as_str)
        .map(str::to_owned);
    let Some(expected) = inventory
        .files
        .get(NAME)
        .filter(|_| !inventory.protected.contains(NAME))
    else {
        return Err(incident.refused());
    };
    let digest = hash(&bytes);
    if &digest == expected {
        return Ok(None);
    }
    let Some(baseline) = original(&bytes, &document, expected) else {
        return Err(incident.refused());
    };
    incident.cause = Cause::LegacyPerformerMarker;
    if !restore {
        return Ok(Some(incident));
    }
    let archive = format!(".macc/persistent/scoped-prd-archive/{digest}.json");
    let project = ProjectPaths::from_root(&crate::persistent::primary_root(worktree)?);
    project_ownership::validate_path(&project, &archive)?;
    let file = project.root.join(archive);
    match fs::read(&file) {
        Ok(saved) if saved == bytes => {}
        Ok(_) => return Err(error("Scoped PRD archive conflicts with previous evidence")),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            crate::atomic_write(&project, &file, &bytes)?
        }
        Err(e) => return Err(error(e)),
    }
    project_ownership::validate_path(paths, NAME)?;
    if fs::read(&path).map_err(error)? != bytes {
        return Err(Incident {
            cause: Cause::UnverifiedFile,
            ..incident
        }
        .refused());
    }
    crate::atomic_write(paths, &path, &baseline)?;
    Ok(Some(incident))
}

pub(super) fn record_event(root: &Path, incident: &Incident, event: &str) -> Result<()> {
    super::helpers::append_coordinator_event_payload(
        root,
        event,
        &incident.incoming_task,
        "dispatch",
        "info",
        "info",
        serde_json::json!({"message":incident.to_string(),"code":"E901",INCIDENT_KEY:incident}),
    )
}

/// Wake a waiting supervisor when the operator restores the protected
/// contract. Only read the reserved file inside a registered performer slot.
pub fn recovery_revision(root: &Path, task: &Task) -> Option<Value> {
    if !is_dispatch_failure(task) {
        return None;
    }
    let mut revision = serde_json::json!({"before_launch": true});
    if let Some(worktree) = task.worktree_path().map(Path::new) {
        super::worktree_roles::performer_metadata(root, worktree)?;
        let path = worktree.join(NAME);
        if path.symlink_metadata().is_ok_and(|meta| meta.is_file()) {
            revision["file_sha256"] = fs::read(path).ok().map(|bytes| hash(&bytes)).into();
        }
        if let Ok(inventory) = project_ownership::read(&ProjectPaths::from_root(worktree)) {
            revision["original_sha256"] = inventory.files.get(NAME).cloned().into();
            revision["protected"] = inventory.protected.contains(NAME).into();
        }
    }
    Some(revision)
}

pub fn describe(root: &Path, task: &Task) -> Option<Incident> {
    if !matches!(
        task.task_runtime.last_error_origin.as_deref(),
        Some("worker_preflight" | "dispatch_preparation")
    ) {
        return None;
    }
    if let Some(value) = task.task_runtime.extra.get(INCIDENT_KEY) {
        return serde_json::from_value(value.clone()).ok();
    }
    if !is_dispatch_failure(task) {
        return None;
    }
    let path = Path::new(task.worktree_path()?);
    super::worktree_roles::performer_metadata(root, path)?;
    match inspect(&ProjectPaths::from_root(path), &task.id, false) {
        Ok(info) => info,
        Err(MaccError::ScopedPrd(info)) => Some(*info),
        Err(_) => None,
    }
}

pub(super) fn prepare_dispatch(
    root: &Path,
    registry: &TaskRegistry,
    path: &Path,
    prd: &Path,
    id: &str,
) -> Result<()> {
    if worker_busy(registry, path) {
        return Err(MaccError::Validation(format!(
            "Worker {} is still active",
            path.display()
        )));
    }
    if let Some(incident) = prepare(path, id)? {
        record_event(root, &incident, "scoped_prd_restored")?;
    }
    super::helpers::write_worktree_prd_for_task(prd, id, path)
}

/// A late edit between claim and launch is a local failure of that claim.
/// This final check must never replace a document another writer has changed.
pub(super) fn verify_dispatch(expected: &Value, id: &str, path: &Path) -> Result<()> {
    let paths = ProjectPaths::from_root(path);
    if !path
        .join(NAME)
        .symlink_metadata()
        .is_ok_and(|meta| meta.is_file())
    {
        return Err(Incident::unverified(path, id).refused());
    }
    project_ownership::validate_path(&paths, NAME)
        .map_err(|_| Incident::unverified(path, id).refused())?;
    let raw = fs::read(path.join(NAME)).map_err(error)?;
    if raw == serde_json::to_vec_pretty(expected).map_err(error)?
        && !tracked(path)?
        && project_ownership::owned_unchanged(&paths, NAME)?
    {
        return Ok(());
    }
    let info = inspect(&paths, id, false)?.unwrap_or_else(|| Incident::unverified(path, id));
    Err(MaccError::ScopedPrd(Box::new(info)))
}
