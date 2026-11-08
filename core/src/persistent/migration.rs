//! Atomic same-filesystem relocation with a consistent SQLite backup and legacy fences.
use super::*;
use crate::fs_lock::AdvisoryLock;
use std::{fs, time::Duration};

pub(super) fn prepare(root: &Path) -> Result<PathBuf> {
    let primary = primary_root(root)?;
    let common = crate::git::git_common_dir(root)?;
    let locator = common.join("macc-primary-worktree.json");
    no_symlink(&locator)?;
    let primary_bytes = serde_json::to_vec(&primary).map_err(error)?;
    let locator_current = fs::read(&locator).ok().as_deref() == Some(primary_bytes.as_slice());
    let legacy = common.join("macc");
    let destination = primary.join(".macc/persistent");
    for p in [
        destination.join("layout.json"),
        legacy.join("control.sqlite"),
        legacy.join("MOVED"),
    ] {
        no_symlink(&p)?;
    }
    for p in [primary.join(".macc"), destination.clone(), legacy.clone()] {
        no_symlink(&p)?;
    }
    // Steady-state access must work while the coordinator owns its controller lock.
    if fs::read_to_string(destination.join("layout.json"))
        .ok()
        .as_deref()
        == Some(LAYOUT)
        && locator_current
        && is_fence(&legacy.join("control.sqlite"))
        && fs::read_to_string(legacy.join("MOVED")).ok().as_deref() == Some(LAYOUT)
    {
        return Ok(destination);
    }
    let _migration = AdvisoryLock::acquire(
        &common.join("macc-layout.lock"),
        Duration::from_secs(5),
        "MACC layout migration",
    )?;
    if !locator_current {
        io(fs::write(&locator, &primary_bytes))?;
    }
    if fs::read_to_string(destination.join("layout.json"))
        .ok()
        .as_deref()
        == Some(LAYOUT)
    {
        // Recover a crash after atomic publication, but never accept a second legacy DB.
        if legacy.join("control.sqlite").exists() && !is_fence(&legacy.join("control.sqlite")) {
            return Err(error(
                "Both legacy and current authority exist; stop MACC and reconcile explicitly",
            ));
        }
        if legacy.join("integration").exists() && !is_fence(&legacy.join("integration")) {
            return Err(error(
                "A legacy integration slot reappeared after migration; reconcile explicitly",
            ));
        }
        fences(&legacy)?;
        return Ok(destination);
    }
    if destination.exists() && io(fs::read_dir(&destination))?.next().is_some() {
        return Err(error("Destination already contains state without a layout receipt; no automatic merge is permitted"));
    }
    if legacy.join("control.sqlite").exists() || legacy.join("integration").exists() {
        crate::clear_policy::require_stopped(&primary)?;
    }
    let mut guards = Vec::new();
    for name in ["controller.lock", "mutation.lock", "integration.lock"] {
        no_symlink(&legacy.join(name))?;
        guards.push(AdvisoryLock::acquire(
            &legacy.join(name),
            Duration::ZERO,
            "legacy MACC state (stop active processes before migration)",
        )?);
    }
    // Exclude technical state even before Apply updates the project's managed .gitignore.
    crate::gitignore_policy::ensure_local_exclusions(&common)?;
    backup(&legacy)?;
    move_integration(&primary, &legacy)?;
    io(fs::write(legacy.join("layout.json"), LAYOUT))?;
    if destination.exists() {
        io(fs::remove_dir(&destination))?;
    }
    io(fs::create_dir_all(destination.parent().unwrap()))?;
    // Rename preserves all auxiliary files, WAL/SHM, broker state and lock inodes.
    // Cross-filesystem moves fail safely; no partial recursive copy is attempted.
    fs::rename(&legacy, &destination).map_err(|cause| {
        error(format!(
            "Cannot atomically relocate {} to {}: {cause}; legacy state and backup are retained",
            legacy.display(),
            destination.display()
        ))
    })?;
    fences(&legacy)?;
    tracing::info!(source=%legacy.display(),destination=%destination.display(),"MACC persistent state layout prepared");
    Ok(destination)
}
fn fences(legacy: &Path) -> Result<()> {
    io(fs::create_dir_all(legacy))?;
    for name in ["control.sqlite", "MOVED", "integration"] {
        no_symlink(&legacy.join(name))?;
    }
    io(fs::write(legacy.join("control.sqlite"), FENCE))?;
    // A regular file makes old integration-worktree creation fail instead of forking state.
    if !legacy.join("integration").exists() {
        io(fs::write(legacy.join("integration"), FENCE))?;
    }
    io(fs::write(legacy.join("MOVED"), LAYOUT))
}
fn backup(legacy: &Path) -> Result<()> {
    let source = legacy.join("control.sqlite");
    if !source.exists() {
        return Ok(());
    }
    no_symlink(&source)?;
    let db =
        rusqlite::Connection::open_with_flags(&source, rusqlite::OpenFlags::SQLITE_OPEN_READ_WRITE)
            .map_err(error)?;
    db.busy_timeout(Duration::ZERO).map_err(error)?;
    let check: String = db
        .query_row("PRAGMA integrity_check", [], |r| r.get(0))
        .map_err(error)?;
    if check != "ok" {
        return Err(error("Legacy SQLite integrity check failed"));
    }
    // Also reject rollback-journal readers/writers, not only WAL contention.
    db.execute_batch("BEGIN EXCLUSIVE; ROLLBACK")
        .map_err(error)?;
    // Checkpoint must actually finish; a live reader/writer must not be relocated.
    let (busy, _, _): (i64, i64, i64) = db
        .query_row("PRAGMA wal_checkpoint(TRUNCATE)", [], |r| {
            Ok((r.get(0)?, r.get(1)?, r.get(2)?))
        })
        .map_err(error)?;
    if busy != 0 {
        return Err(error(
            "Legacy SQLite is busy; stop all readers/runners before migration",
        ));
    }
    let folder = legacy
        .join("migration-backups")
        .join(uuid::Uuid::new_v4().to_string());
    io(fs::create_dir_all(&folder))?;
    let target = folder.join("control.sqlite");
    db.execute("VACUUM INTO ?1", [target.to_string_lossy().as_ref()])
        .map_err(error)?;
    let snapshot =
        rusqlite::Connection::open_with_flags(&target, rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY)
            .map_err(error)?;
    let check: String = snapshot
        .query_row("PRAGMA integrity_check", [], |r| r.get(0))
        .map_err(error)?;
    if check != "ok" {
        return Err(error("SQLite backup verification failed"));
    }
    use sha2::{Digest, Sha256};
    io(fs::write(
        folder.join("sha256.txt"),
        format!("{:x}\n", Sha256::digest(io(fs::read(&target))?)),
    ))?;
    Ok(())
}
fn move_integration(primary: &Path, legacy: &Path) -> Result<()> {
    let old = legacy.join("integration");
    let new = primary.join(".macc/worktree/integration");
    no_symlink(&primary.join(".macc/worktree"))?;
    no_symlink(&old)?;
    no_symlink(&new)?;
    if !old.exists() {
        return Ok(());
    }
    if new.exists() {
        return Err(error(
            "Both integration slots exist; preserve and reconcile them explicitly",
        ));
    }
    if !old.join(".git").is_file() {
        return Err(error(
            "Legacy integration slot is not a registered worktree",
        ));
    }
    if crate::git::is_dirty(&old)? {
        return Err(error(
            "Legacy integration worktree has uncommitted work; preserve it before migration",
        ));
    }
    io(fs::create_dir_all(new.parent().unwrap()))?;
    let output = crate::git::run_git_output_mapped(
        primary,
        &[
            "worktree",
            "move",
            old.to_str()
                .ok_or_else(|| error("Invalid legacy worktree path"))?,
            new.to_str()
                .ok_or_else(|| error("Invalid integration path"))?,
        ],
        "relocate MACC integration worktree",
    )?;
    if !output.status.success() {
        return Err(error(String::from_utf8_lossy(&output.stderr)));
    }
    Ok(())
}

fn is_fence(path: &Path) -> bool {
    fs::metadata(path).is_ok_and(|m| m.len() == FENCE.len() as u64)
        && fs::read_to_string(path).ok().as_deref() == Some(FENCE)
}
