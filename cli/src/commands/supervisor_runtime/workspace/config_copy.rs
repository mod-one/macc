//! The supervisor reads the project's current setup in its isolated checkout.
use super::{git, io};
use macc_core::{coordinator::decisions::evidence_digest, MaccError, ProjectPaths, Result};
use std::{fs, path::Path};

pub(super) const CONFIG: &str = ".macc/macc.yaml";
const RECEIPT: &str = ".macc/supervisor-config.sha256";

pub(super) fn sync(root: &Path, slot: &Path) -> Result<()> {
    let source = root.join(CONFIG);
    let bytes = match fs::read(&source) {
        Ok(bytes) => bytes,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(e) => return Err(io(&source, e)),
    };
    let dest = slot.join(CONFIG);
    if dest.is_symlink() || customized(slot) {
        return Err(MaccError::Validation(format!(
            "Supervisor configuration at {} was customized. Its contents are preserved.",
            dest.display()
        )));
    }
    if dest.is_file() && !matches_receipt(slot) {
        if fs::read(&dest).map_err(|e| io(&dest, e))? == bytes {
            return Ok(()); // An existing identical file needs no write or ownership receipt.
        }
        if git(slot, &["ls-files", "--", CONFIG])?.is_empty()
            || !git(slot, &["status", "--porcelain", "--", CONFIG])?.is_empty()
        {
            return Err(MaccError::Validation(format!(
                "Existing supervisor configuration at {} is preserved.",
                dest.display()
            )));
        }
    }
    let paths = ProjectPaths::from_root(slot);
    macc_core::atomic_write(&paths, &dest, &bytes)?;
    macc_core::atomic_write(
        &paths,
        &slot.join(RECEIPT),
        evidence_digest(&bytes).as_bytes(),
    )
}

fn matches_receipt(slot: &Path) -> bool {
    let file = slot.join(CONFIG);
    if file.is_symlink() || slot.join(RECEIPT).is_symlink() || !file.is_file() {
        return false;
    }
    fs::read_to_string(slot.join(RECEIPT))
        .ok()
        .is_some_and(|hash| fs::read(file).is_ok_and(|bytes| evidence_digest(&bytes) == hash))
}

pub(super) fn customized(slot: &Path) -> bool {
    slot.join(RECEIPT).symlink_metadata().is_ok() && !matches_receipt(slot)
}

/// Only a byte-identical copy backed by the primary checkout can be discarded.
pub(super) fn disposable(root: &Path, slot: &Path) -> Result<bool> {
    Ok(matches_receipt(slot)
        && fs::read(root.join(CONFIG))
            .ok()
            .zip(fs::read(slot.join(CONFIG)).ok())
            .is_some_and(|(source, local)| source == local)
        && git(slot, &["diff", "--cached", "--name-only", "--", CONFIG])?.is_empty())
}

pub(super) fn restore_before_removal(root: &Path, slot: &Path) -> Result<()> {
    if !disposable(root, slot)? {
        return Ok(());
    }
    if git(slot, &["ls-files", "--", CONFIG])?.is_empty() {
        fs::remove_file(slot.join(CONFIG)).map_err(|e| io(&slot.join(CONFIG), e))?;
    } else {
        git(slot, &["restore", "--worktree", "--", CONFIG])?;
    }
    Ok(())
}
