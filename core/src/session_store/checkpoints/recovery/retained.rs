//! Preserve unfinished task files without staging, committing, deleting or overwriting.
use super::*;
use std::{collections::BTreeSet, fs, io::Write, path::PathBuf};

fn bytes(root: &Path, name: &str) -> Result<Option<(Vec<u8>, bool)>> {
    if name.chars().any(char::is_control) {
        return Err(error("Invalid retained filename"));
    }
    crate::project_ownership::validate_path(&crate::ProjectPaths::from_root(root), name)?;
    let path = root.join(name);
    let meta = match fs::symlink_metadata(&path) {
        Ok(meta) => meta,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(error(e.to_string())),
    };
    if !meta.is_file() || meta.len() > 64 * 1024 * 1024 {
        return Err(error("Retained work must be a bounded regular file"));
    }
    #[cfg(unix)]
    let executable = {
        use std::os::unix::fs::PermissionsExt;
        meta.permissions().mode() & 0o111 != 0
    };
    #[cfg(not(unix))]
    let executable = false;
    Ok(Some((
        fs::read(path).map_err(|e| error(e.to_string()))?,
        executable,
    )))
}

pub(super) fn matches(source: &Path, target: &Path, changed: &[String]) -> Result<bool> {
    for name in changed {
        if bytes(source, name)? != bytes(target, name)? {
            return Ok(false);
        }
    }
    Ok(true)
}

pub(super) struct Transfer {
    source: PathBuf,
    target: PathBuf,
    name: String,
    content: Vec<u8>,
    executable: bool,
}

pub(super) fn prepare(source: &Path, target: &Path) -> Result<Vec<Transfer>> {
    let untracked = git_state::text(
        source,
        &["ls-files", "--others", "--exclude-standard", "-z"],
    )?;
    let untracked: BTreeSet<_> = untracked.split('\0').filter(|p| !p.is_empty()).collect();
    let tracked_target = git_state::text(target, &["ls-files", "-z"])?;
    let tracked_target: BTreeSet<_> = tracked_target
        .split('\0')
        .filter(|p| !p.is_empty())
        .collect();
    let state = git_state::snapshot(source)?;
    let mut plan = Vec::new();
    let mut total_bytes = 0usize;
    for name in state.changed {
        let previous = bytes(source, &name)?;
        let current = bytes(target, &name)?;
        if previous == current {
            continue;
        }
        if current.is_some()
            || tracked_target.contains(name.as_str())
            || !untracked.contains(name.as_str())
            || name
                .split('/')
                .any(|p| matches!(p, ".macc" | ".git" | ".env") || p.starts_with(".env."))
            || matches!(name.as_str(), "performer.sh" | "worktree.prd.json")
        {
            return Err(error(format!("{}: retained file {name} conflicts or requires reviewed integration; both worktrees preserved",super::CODE)));
        }
        let (content, executable) = previous.ok_or_else(|| error("Missing retained source"))?;
        total_bytes = total_bytes
            .checked_add(content.len())
            .ok_or_else(|| error("Retained transfer size overflow"))?;
        if total_bytes > 64 * 1024 * 1024 {
            return Err(error(
                "Retained transfer exceeds 64 MiB; reviewed integration required",
            ));
        }
        plan.push(Transfer {
            source: source.into(),
            target: target.into(),
            name,
            content,
            executable,
        });
    }
    Ok(plan)
}

impl Transfer {
    pub(super) fn copy(self) -> Result<Value> {
        if bytes(&self.source, &self.name)? != Some((self.content.clone(), self.executable)) {
            return Err(error(
                "Retained source changed during recovery; retry after inspection",
            ));
        }
        if let Some(current) = bytes(&self.target, &self.name)? {
            if current != (self.content.clone(), self.executable) {
                return Err(error(
                    "Retained destination changed; no overwrite permitted",
                ));
            }
        } else {
            let destination = self.target.join(&self.name);
            fs::create_dir_all(destination.parent().unwrap()).map_err(|e| error(e.to_string()))?;
            crate::project_ownership::validate_path(
                &crate::ProjectPaths::from_root(&self.target),
                &self.name,
            )?;
            let mut temp = tempfile::NamedTempFile::new_in(destination.parent().unwrap())
                .map_err(|e| error(e.to_string()))?;
            temp.write_all(&self.content)
                .map_err(|e| error(e.to_string()))?;
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                temp.as_file()
                    .set_permissions(fs::Permissions::from_mode(if self.executable {
                        0o700
                    } else {
                        0o600
                    }))
                    .map_err(|e| error(e.to_string()))?;
            }
            temp.as_file()
                .sync_all()
                .map_err(|e| error(e.to_string()))?;
            temp.persist_noclobber(&destination)
                .map_err(|e| error(format!("Retained destination was not overwritten: {e}")))?;
        }
        Ok(
            json!({"source":self.source,"destination":self.target,"path":self.name,"sha256":digest(&self.content),"source_preserved":true,"staged":false,"committed":false}),
        )
    }
}
