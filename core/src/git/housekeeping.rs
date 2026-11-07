//! Captured, noninteractive housekeeping; optional network failures are warnings.
use crate::{MaccError, Result};
use std::path::Path;
pub async fn fetch_housekeeping(worktree: &Path, remote: &str) -> Result<bool> {
    let output = tokio::process::Command::new("git")
        .current_dir(worktree)
        .args(["fetch", remote])
        .env("GIT_TERMINAL_PROMPT", "0")
        .output()
        .await
        .map_err(|source| MaccError::Io {
            path: worktree.display().to_string(),
            action: "optional git fetch".into(),
            source,
        })?;
    if !output.status.success() {
        let detail = String::from_utf8_lossy(&output.stderr);
        let class = if detail.contains("Username")
            || detail.contains("Authentication")
            || detail.contains("Permission denied")
        {
            "authentication"
        } else if detail.contains("resolve host") || detail.contains("connect") {
            "network"
        } else {
            "git_exit"
        };
        tracing::warn!(operation="optional_fetch",remote,path=%worktree.display(),class,exit=?output.status.code(),cause=%detail.trim(),"Optional remote refresh failed; local delivery remains available");
    }
    Ok(output.status.success())
}
