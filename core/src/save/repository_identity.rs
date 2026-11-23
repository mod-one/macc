use super::manifest::MatchStrength;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::path::Path;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct RepositoryIdentity {
    pub root_name: String,
    pub root_path_hash: String,
    pub git_remote_url_hash: String,
    pub git_default_branch: String,
    pub git_current_branch: String,
    pub git_head_sha: String,
    pub identity_strength: String,
}

fn hash_string(s: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(s.as_bytes());
    format!("sha256:{:x}", hasher.finalize())
}

// Only the scheme and host are case-insensitive. Keep the port, user and
// repository path distinct, including the .git suffix of local directories.
const REMOTE_HASH_VERSION: &str = "v2:";

impl RepositoryIdentity {
    pub(super) fn has_legacy_remote_hash(&self) -> bool {
        !self.git_remote_url_hash.is_empty()
            && !self.git_remote_url_hash.starts_with(REMOTE_HASH_VERSION)
    }
}

fn normalized_authority(authority: &str) -> String {
    match authority.rsplit_once('@') {
        Some((user, host)) => format!("{user}@{}", host.to_ascii_lowercase()),
        None => authority.to_ascii_lowercase(),
    }
}

fn normalize_git_url(root: &Path, remote: &str) -> String {
    if let Some((scheme, rest)) = remote.split_once("://") {
        if scheme.starts_with(|c: char| c.is_ascii_alphabetic())
            && scheme
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || matches!(c, '+' | '-' | '.'))
        {
            let end = rest.find(['/', '?', '#']).unwrap_or(rest.len());
            return format!(
                "url:{}://{}{}",
                scheme.to_ascii_lowercase(),
                normalized_authority(&rest[..end]),
                &rest[end..]
            );
        }
    }
    // In scp syntax the first colon outside an IPv6 literal separates the
    // authority from the path. A slash before it identifies a local path.
    let authority_end = remote.find(':').and_then(|first| {
        let prefix = remote[..first].rsplit('@').next().unwrap_or_default();
        if prefix.starts_with('[') {
            remote.find("]:").map(|i| i + 1)
        } else {
            Some(first)
        }
    });
    if let Some(i) = authority_end {
        let authority = &remote[..i];
        if !authority.is_empty() && !authority.contains(['/', '\\']) && !(cfg!(windows) && i == 1) {
            return format!("scp:{}{}", normalized_authority(authority), &remote[i..]);
        }
    }
    let path = root.join(remote);
    format!("local:{}", path.canonicalize().unwrap_or(path).display())
}

fn get_git_remote_url(repo_root: &Path) -> Option<String> {
    if let Ok(out) = crate::git::run_git_output_mapped(
        repo_root,
        &["remote", "get-url", "origin"],
        "get remote url",
    ) {
        let raw = String::from_utf8(out.stdout).ok()?;
        let url = raw.strip_suffix('\n').unwrap_or(&raw);
        if !url.is_empty() {
            return Some(url.to_owned());
        }
    }
    None
}

fn get_git_current_branch(repo_root: &Path) -> Option<String> {
    if let Ok(out) = crate::git::run_git_output_mapped(
        repo_root,
        &["rev-parse", "--abbrev-ref", "HEAD"],
        "get current branch",
    ) {
        let branch = String::from_utf8_lossy(&out.stdout).trim().to_string();
        if !branch.is_empty() {
            return Some(branch);
        }
    }
    None
}

fn get_git_default_branch(repo_root: &Path) -> Option<String> {
    if let Ok(out) = crate::git::run_git_output_mapped(
        repo_root,
        &["symbolic-ref", "refs/remotes/origin/HEAD"],
        "get default branch",
    ) {
        let ref_path = String::from_utf8_lossy(&out.stdout).trim().to_string();
        if let Some(branch) = ref_path.split('/').next_back() {
            return Some(branch.to_string());
        }
    }
    None
}

fn get_git_head_sha(repo_root: &Path) -> Option<String> {
    if let Ok(out) =
        crate::git::run_git_output_mapped(repo_root, &["rev-parse", "HEAD"], "get HEAD sha")
    {
        let sha = String::from_utf8_lossy(&out.stdout).trim().to_string();
        if !sha.is_empty() {
            return Some(sha);
        }
    }
    None
}

pub fn get_repository_identity(repo_root: &Path) -> RepositoryIdentity {
    let root_name = repo_root
        .file_name()
        .map(|s| s.to_string_lossy().to_string())
        .unwrap_or_else(|| "unknown".to_string());

    let root_path_hash = hash_string(&repo_root.to_string_lossy());

    let remote_url = get_git_remote_url(repo_root);
    let git_remote_url_hash = remote_url
        .as_ref()
        .map(|url| {
            format!(
                "{REMOTE_HASH_VERSION}{}",
                hash_string(&normalize_git_url(repo_root, url))
            )
        })
        .unwrap_or_default();

    let git_current_branch =
        get_git_current_branch(repo_root).unwrap_or_else(|| "main".to_string());
    let git_default_branch =
        get_git_default_branch(repo_root).unwrap_or_else(|| "main".to_string());
    let git_head_sha = get_git_head_sha(repo_root).unwrap_or_default();

    let identity_strength = if !git_remote_url_hash.is_empty() {
        "strong".to_string()
    } else if !git_head_sha.is_empty() {
        "medium".to_string()
    } else {
        "weak".to_string()
    };

    RepositoryIdentity {
        root_name,
        root_path_hash,
        git_remote_url_hash,
        git_default_branch,
        git_current_branch,
        git_head_sha,
        identity_strength,
    }
}

pub fn compute_match_strength(
    current: &RepositoryIdentity,
    manifest: &RepositoryIdentity,
) -> MatchStrength {
    let same_checkout = current.root_name == manifest.root_name
        && current.root_path_hash == manifest.root_path_hash;
    if !manifest.git_remote_url_hash.is_empty() {
        if manifest.has_legacy_remote_hash() {
            // The original path case cannot be recovered from a legacy hash.
            return if same_checkout {
                MatchStrength::Medium
            } else {
                MatchStrength::None
            };
        }
        if !current.git_remote_url_hash.is_empty() {
            return if current.git_remote_url_hash == manifest.git_remote_url_hash {
                MatchStrength::Strong
            } else {
                MatchStrength::None
            };
        }
    }
    if same_checkout {
        MatchStrength::Medium
    } else if current.root_name == manifest.root_name {
        MatchStrength::Weak
    } else {
        MatchStrength::None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn host_case_is_insignificant_and_path_case_is_significant() {
        let key = |url| normalize_git_url(Path::new("/checkout"), url);
        for (first, same, different) in [
            (
                "HTTPS://Git.Example/A.git",
                "https://git.example/A.git",
                "https://git.example/a.git",
            ),
            (
                "ssh://git@HOST:2222/A",
                "ssh://git@host:2222/A",
                "ssh://git@host:2222/a",
            ),
            (
                "git@[ABCD::1]:Team/Repo",
                "git@[abcd::1]:Team/Repo",
                "git@[abcd::1]:team/Repo",
            ),
            ("HOST:Team]:v1", "host:Team]:v1", "host:team]:v1"),
            (
                "file:///Repos/Systems.git",
                "FILE:///Repos/Systems.git",
                "file:///Repos/systems.git",
            ),
        ] {
            assert_eq!(key(first), key(same));
            assert_ne!(key(first), key(different));
        }
    }

    #[test]
    fn legacy_hash_requires_independent_checkout_identity() {
        let temp = tempfile::tempdir().unwrap();
        let mut current = get_repository_identity(temp.path());
        current.git_remote_url_hash = "v2:sha256:current".into();
        let mut legacy = current.clone();
        legacy.git_remote_url_hash = "sha256:case-folded".into();
        assert_eq!(
            compute_match_strength(&current, &legacy),
            MatchStrength::Medium
        );
        legacy.root_path_hash = "another checkout".into();
        assert_eq!(
            compute_match_strength(&current, &legacy),
            MatchStrength::None
        );
    }
}
