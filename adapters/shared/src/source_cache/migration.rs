//! Read old mutable caches without ever checking out or editing them.
use super::*;
pub(super) fn legacy_git(paths: &ProjectPaths, source: &Source) -> Option<(PathBuf, String)> {
    for base in bases(paths) {
        let Ok(entries) = fs::read_dir(base) else {
            continue;
        };
        for entry in entries.flatten() {
            let repo = entry.path().join("repo");
            if !repo.join(".git").is_dir() {
                continue;
            }
            let origin = Command::new("git")
                .args(["config", "--get", "remote.origin.url"])
                .current_dir(&repo)
                .output()
                .ok()?;
            if !origin.status.success()
                || String::from_utf8_lossy(&origin.stdout).trim() != source.url
            {
                continue;
            }
            if let Ok(sha) = fetch::resolve_ref_to_sha(
                &repo,
                if source.reference.is_empty() {
                    "HEAD"
                } else {
                    &source.reference
                },
            ) {
                return Some((repo, sha));
            }
        }
    }
    None
}
