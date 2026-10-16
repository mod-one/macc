//! Graphify namespace facade over shared provider ownership proofs.
use super::*;
pub(super) fn owned(root: &Path, path: &str) -> Result<std::path::PathBuf> {
    super::super::generation::owned(root, "graphify", path)
}
pub(super) fn seal(root: &Path, path: &str) -> Result<String> {
    super::super::generation::seal(root, "graphify", path)
}
pub(super) fn verify(root: &Path, path: &str, hash: &str) -> Result<()> {
    super::super::generation::verify(root, "graphify", path, hash)
}
pub(super) fn remove(root: &Path, path: &str, hash: &str) -> Result<bool> {
    super::super::generation::remove(root, "graphify", path, hash)
}
pub(super) use super::super::generation::attempt;
