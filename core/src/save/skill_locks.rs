//! Version pins travel with configuration saves; source caches remain excluded.
use crate::{skills_catalog::SkillsLockFile, MaccError, ProjectPaths, Result};
use std::path::Path;
pub(super) fn capture(paths: &ProjectPaths, bundle: &Path) -> Result<()> {
    if !paths.skills_lock_path().is_file() {
        return Ok(());
    }
    let lock = SkillsLockFile::load(&paths.skills_lock_path())?;
    let bytes =
        serde_json::to_vec_pretty(&lock).map_err(|e| MaccError::Validation(e.to_string()))?;
    if crate::security::scan_bytes("config/skills.lock.json", &bytes)
        .iter()
        .any(|finding| finding.severity == crate::security::Severity::Error)
    {
        return Err(MaccError::Validation(
            "Remove credentials from skill source URLs before saving the lockfile".into(),
        ));
    }
    crate::atomic_write(paths, &bundle.join("config/skills.lock.json"), &bytes)
}
pub(super) fn load(bundle: &Path) -> Result<Option<SkillsLockFile>> {
    let path = bundle.join("config/skills.lock.json");
    if !path.exists() {
        return Ok(None);
    }
    let checked = super::sessions::payload(bundle, "config/skills.lock.json")?;
    SkillsLockFile::load(&checked).map(Some)
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn pins_round_trip_with_configuration_without_copying_cache() {
        let dir = tempfile::tempdir().unwrap();
        let source = ProjectPaths::from_root(dir.path().join("source"));
        let bundle = dir.path().join("bundle");
        let target = ProjectPaths::from_root(dir.path().join("target"));
        let mut lock = SkillsLockFile::load(&source.skills_lock_path()).unwrap();
        lock.generated_at = "fixture".into();
        lock.save(&source.skills_lock_path()).unwrap();
        capture(&source, &bundle).unwrap();
        let restored = load(&bundle).unwrap().unwrap();
        restored.save(&target.skills_lock_path()).unwrap();
        assert_eq!(
            SkillsLockFile::load(&target.skills_lock_path()).unwrap(),
            lock
        );
        assert!(!bundle.join("cache").exists());
        std::fs::write(bundle.join("config/skills.lock.json"), "invalid").unwrap();
        assert!(load(&bundle).is_err());
    }
}
