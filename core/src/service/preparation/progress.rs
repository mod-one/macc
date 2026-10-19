use super::*;
use std::io::{Read, Seek, SeekFrom};
pub(super) fn output(root: &Path) -> Result<Option<String>> {
    let pointer = root.join(".macc/state/apply-current-log");
    let Ok(name) = std::fs::read_to_string(pointer) else {
        return Ok(None);
    };
    let relative = Path::new(name.trim());
    if relative.components().count() != 1
        || !matches!(
            relative.components().next(),
            Some(std::path::Component::Normal(_))
        )
    {
        return Ok(None);
    }
    let path = root.join(".macc/log/apply").join(relative);
    let Ok(meta) = std::fs::symlink_metadata(&path) else {
        return Ok(None);
    };
    if !meta.is_file() || meta.file_type().is_symlink() {
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
