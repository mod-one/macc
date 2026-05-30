use std::path::Path;
use sha2::{Digest, Sha256};
use serde::{Deserialize, Serialize};
use super::manifest::MatchStrength;

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

fn normalize_git_url(url: &str) -> String {
    let mut s = url.trim().to_ascii_lowercase();
    if s.starts_with("https://") {
        s = s["https://".len()..].to_string();
    } else if s.starts_with("http://") {
        s = s["http://".len()..].to_string();
    } else if s.starts_with("ssh://") {
        s = s["ssh://".len()..].to_string();
    }
    if s.starts_with("git@") {
        s = s["git@".len()..].to_string();
    }
    s = s.replace(':', "/");
    if s.ends_with(".git") {
        s = s[..s.len() - 4].to_string();
    }
    s
}

fn get_git_remote_url(repo_root: &Path) -> Option<String> {
    if let Ok(out) = crate::git::run_git_output_mapped(repo_root, &["remote", "get-url", "origin"], "get remote url") {
        let url = String::from_utf8_lossy(&out.stdout).trim().to_string();
        if !url.is_empty() {
            return Some(url);
        }
    }
    None
}

fn get_git_current_branch(repo_root: &Path) -> Option<String> {
    if let Ok(out) = crate::git::run_git_output_mapped(repo_root, &["rev-parse", "--abbrev-ref", "HEAD"], "get current branch") {
        let branch = String::from_utf8_lossy(&out.stdout).trim().to_string();
        if !branch.is_empty() {
            return Some(branch);
        }
    }
    None
}

fn get_git_default_branch(repo_root: &Path) -> Option<String> {
    if let Ok(out) = crate::git::run_git_output_mapped(repo_root, &["symbolic-ref", "refs/remotes/origin/HEAD"], "get default branch") {
        let ref_path = String::from_utf8_lossy(&out.stdout).trim().to_string();
        if let Some(branch) = ref_path.split('/').last() {
            return Some(branch.to_string());
        }
    }
    None
}

fn get_git_head_sha(repo_root: &Path) -> Option<String> {
    if let Ok(out) = crate::git::run_git_output_mapped(repo_root, &["rev-parse", "HEAD"], "get HEAD sha") {
        let sha = String::from_utf8_lossy(&out.stdout).trim().to_string();
        if !sha.is_empty() {
            return Some(sha);
        }
    }
    None
}

pub fn get_repository_identity(repo_root: &Path) -> RepositoryIdentity {
    let root_name = repo_root.file_name()
        .map(|s| s.to_string_lossy().to_string())
        .unwrap_or_else(|| "unknown".to_string());
    
    let root_path_hash = hash_string(&repo_root.to_string_lossy());
    
    let remote_url = get_git_remote_url(repo_root);
    let git_remote_url_hash = remote_url
        .as_ref()
        .map(|url| hash_string(&normalize_git_url(url)))
        .unwrap_or_else(|| "".to_string());
        
    let git_current_branch = get_git_current_branch(repo_root).unwrap_or_else(|| "main".to_string());
    let git_default_branch = get_git_default_branch(repo_root).unwrap_or_else(|| "main".to_string());
    let git_head_sha = get_git_head_sha(repo_root).unwrap_or_else(|| "".to_string());
    
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

pub fn compute_match_strength(current: &RepositoryIdentity, manifest: &RepositoryIdentity) -> MatchStrength {
    if !current.git_remote_url_hash.is_empty() && current.git_remote_url_hash == manifest.git_remote_url_hash {
        MatchStrength::Strong
    } else if current.root_name == manifest.root_name && current.root_path_hash == manifest.root_path_hash {
        MatchStrength::Medium
    } else if current.root_name == manifest.root_name {
        MatchStrength::Weak
    } else {
        MatchStrength::None
    }
}
