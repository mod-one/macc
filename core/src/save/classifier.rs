use crate::ProjectPaths;
use std::fs;
use std::path::Path;

pub fn copy_dir_all(
    src: impl AsRef<Path>,
    dst: impl AsRef<Path>,
    paths: &ProjectPaths,
) -> std::io::Result<()> {
    fs::create_dir_all(&dst)?;
    for entry in fs::read_dir(src)? {
        let entry = entry?;
        let path = entry.path();

        let kind = crate::classify_path(&path, paths);
        match kind {
            crate::ManagedPathKind::Cache
            | crate::ManagedPathKind::Worktree
            | crate::ManagedPathKind::RuntimeState
            | crate::ManagedPathKind::Generated
            | crate::ManagedPathKind::Secret => {
                continue;
            }
            _ => {}
        }

        let ty = entry.file_type()?;
        if ty.is_dir() {
            copy_dir_all(&path, dst.as_ref().join(entry.file_name()), paths)?;
        } else {
            fs::copy(&path, dst.as_ref().join(entry.file_name()))?;
        }
    }
    Ok(())
}
