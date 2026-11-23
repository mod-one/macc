//! Bundle names are single path components. Destructive operations also require
//! a real bundle whose manifest identifies that directory.
use super::SaveBundleManifest;
use crate::{MaccError, Result};
use std::{
    fmt, fs,
    path::{Path, PathBuf},
};

pub(super) struct SaveName<'a>(&'a str);

impl<'a> SaveName<'a> {
    pub(super) fn parse(value: &'a str) -> Result<Self> {
        if value.is_empty()
            || !value
                .chars()
                .all(|c| c.is_alphanumeric() || matches!(c, '-' | '_'))
        {
            return Err(MaccError::Validation(format!(
                "MACC-SAVE-1000: Invalid save name: {value}"
            )));
        }
        Ok(Self(value))
    }

    pub(super) fn path(&self, saves: &Path) -> PathBuf {
        saves.join(self.0)
    }

    pub(super) fn existing_bundle(&self, saves: &Path) -> Result<PathBuf> {
        let dir = self.path(saves);
        let invalid = |reason: &str| {
            MaccError::Validation(format!(
                "Invalid save bundle at {}: {reason}",
                dir.display()
            ))
        };
        let io = |source| MaccError::Io {
            path: dir.display().to_string(),
            action: "identify save bundle".into(),
            source,
        };
        let metadata = fs::symlink_metadata(&dir).map_err(|e| {
            if e.kind() == std::io::ErrorKind::NotFound {
                MaccError::Validation(format!("MACC-RESTORE-2000: Save not found: {self}"))
            } else {
                io(e)
            }
        })?;
        let expected = self.path(&fs::canonicalize(saves).map_err(io)?);
        if !metadata.is_dir() || fs::canonicalize(&dir).map_err(io)? != expected {
            return Err(invalid(
                "expected a direct child directory of the save root",
            ));
        }
        let path = expected.join("manifest.yaml");
        let manifest_io = |source| MaccError::Io {
            path: path.display().to_string(),
            action: "inspect save manifest".into(),
            source,
        };
        if !fs::symlink_metadata(&path).map_err(manifest_io)?.is_file() {
            return Err(invalid("manifest.yaml must be a regular file"));
        }
        let manifest: SaveBundleManifest =
            serde_yaml::from_str(&fs::read_to_string(&path).map_err(manifest_io)?)
                .map_err(|e| invalid(&format!("malformed manifest.yaml: {e}")))?;
        if !manifest.supported_format() || manifest.name != self.0 {
            return Err(invalid("manifest format or bundle name does not match"));
        }
        Ok(expected)
    }
}

impl fmt::Display for SaveName<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.0)
    }
}
