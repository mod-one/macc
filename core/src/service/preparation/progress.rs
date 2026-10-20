use super::*;
use std::io::{Read, Seek, SeekFrom};
pub(super) fn output(root: &Path) -> Result<Option<String>> {
    let pointer = root.join(".macc/state/apply-current-log");
    let Ok(name) = std::fs::read_to_string(pointer) else {
        return Ok(None);
    };
    let relative = Path::new(name.trim());
    if !(1..=2).contains(&relative.components().count())
        || relative
            .components()
            .any(|c| !matches!(c, std::path::Component::Normal(_)))
    {
        return Ok(None);
    }
    let path = root.join(".macc/log/apply").join(relative);
    let Ok(meta) = std::fs::symlink_metadata(&path) else {
        return Ok(None);
    };
    if !meta.is_file()
        || meta.file_type().is_symlink()
        || path
            .parent()
            .is_some_and(|p| std::fs::symlink_metadata(p).is_ok_and(|m| m.file_type().is_symlink()))
    {
        return Ok(None);
    }
    let mut file = std::fs::File::open(path).map_err(|e| MaccError::Validation(e.to_string()))?;
    file.seek(SeekFrom::Start(meta.len().saturating_sub(8192)))
        .map_err(|e| MaccError::Validation(e.to_string()))?;
    let mut raw = vec![];
    file.take(8192)
        .read_to_end(&mut raw)
        .map_err(|e| MaccError::Validation(e.to_string()))?;
    Ok(Some(crate::save::redact_secrets_in_text(
        &String::from_utf8_lossy(&raw),
    )))
}

pub(super) fn clear(root: &Path) -> Result<()> {
    let paths = ProjectPaths::from_root(root);
    crate::atomic_write(&paths, &paths.macc_dir.join("state/apply-current-log"), b"")
}

pub(super) fn kind(root: &Path) -> Option<String> {
    let name = std::fs::read_to_string(root.join(".macc/state/apply-current-log")).ok()?;
    if name.trim().is_empty() {
        return None;
    }
    Some(
        if name.trim().ends_with("-account.log") {
            "interaction"
        } else {
            "diagnostic"
        }
        .into(),
    )
}
pub(super) fn directory(paths: &ProjectPaths) -> std::path::PathBuf {
    let run = std::fs::read(paths.macc_dir.join("state/apply-preparation.json"))
        .ok()
        .and_then(|r| serde_json::from_slice::<Report>(&r).ok())
        .filter(|r| r.status == "preparing" && uuid::Uuid::parse_str(&r.run_id).is_ok())
        .map(|r| r.run_id)
        .unwrap_or_else(|| uuid::Uuid::new_v4().to_string());
    paths.macc_dir.join("log/apply").join(run)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn new_phase_cannot_display_the_previous_tool_output_or_an_escaping_pointer() {
        let tmp = tempfile::tempdir().unwrap();
        let paths = ProjectPaths::from_root(tmp.path());
        std::fs::create_dir_all(paths.macc_dir.join("log/apply/run")).unwrap();
        std::fs::create_dir_all(paths.macc_dir.join("state")).unwrap();
        std::fs::write(
            paths.macc_dir.join("log/apply/run/check.log"),
            "previous output",
        )
        .unwrap();
        let pointer = paths.macc_dir.join("state/apply-current-log");
        std::fs::write(&pointer, "run/check.log").unwrap();
        assert_eq!(
            output(tmp.path()).unwrap().as_deref(),
            Some("previous output")
        );
        clear(tmp.path()).unwrap();
        assert!(output(tmp.path()).unwrap().is_none());
        std::fs::write(pointer, "../../secret").unwrap();
        assert!(output(tmp.path()).unwrap().is_none());
    }
}
