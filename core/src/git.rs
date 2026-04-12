use crate::{MaccError, Result};
use std::path::Path;
use std::process::{ExitStatus, Output};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GitGraphCommit {
    pub sha: String,
    pub parents: Vec<String>,
    pub subject: String,
    pub body: String,
    pub author: String,
    pub timestamp: i64,
    pub refs: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CommitInfo {
    pub hash: String,
    pub subject: String,
    pub author_date: String,
}

async fn run_git_output_async(current_dir: &Path, args: &[&str], action: &str) -> Result<Output> {
    tokio::process::Command::new("git")
        .args(args)
        .current_dir(current_dir)
        .output()
        .await
        .map_err(|e| MaccError::Io {
            path: current_dir.to_string_lossy().into(),
            action: action.to_string(),
            source: e,
        })
}

async fn run_git_status_async(
    current_dir: &Path,
    args: &[&str],
    action: &str,
) -> Result<ExitStatus> {
    tokio::process::Command::new("git")
        .args(args)
        .current_dir(current_dir)
        .status()
        .await
        .map_err(|e| MaccError::Io {
            path: current_dir.to_string_lossy().into(),
            action: action.to_string(),
            source: e,
        })
}

fn run_git_output(current_dir: &Path, args: &[&str], action: &str) -> Result<Output> {
    std::process::Command::new("git")
        .args(args)
        .current_dir(current_dir)
        .output()
        .map_err(|e| MaccError::Io {
            path: current_dir.to_string_lossy().into(),
            action: action.to_string(),
            source: e,
        })
}

fn run_git_output_with_env(
    current_dir: &Path,
    args: &[&str],
    envs: &[(&str, &str)],
    action: &str,
) -> Result<Output> {
    let mut command = std::process::Command::new("git");
    command.args(args).current_dir(current_dir);
    for (key, value) in envs {
        command.env(key, value);
    }
    command.output().map_err(|e| MaccError::Io {
        path: current_dir.to_string_lossy().into(),
        action: action.to_string(),
        source: e,
    })
}

fn run_git_status(current_dir: &Path, args: &[&str], action: &str) -> Result<ExitStatus> {
    std::process::Command::new("git")
        .args(args)
        .current_dir(current_dir)
        .status()
        .map_err(|e| MaccError::Io {
            path: current_dir.to_string_lossy().into(),
            action: action.to_string(),
            source: e,
        })
}

pub fn run_git_output_mapped(current_dir: &Path, args: &[&str], action: &str) -> Result<Output> {
    run_git_output(current_dir, args, action)
}

pub fn run_git_output_with_env_mapped(
    current_dir: &Path,
    args: &[&str],
    envs: &[(&str, &str)],
    action: &str,
) -> Result<Output> {
    run_git_output_with_env(current_dir, args, envs, action)
}

pub fn worktree_list_porcelain(repo_root: &Path) -> Result<String> {
    let output = run_git_output(
        repo_root,
        &["worktree", "list", "--porcelain"],
        "run git worktree list",
    )?;
    if !output.status.success() {
        return Err(MaccError::Git {
            operation: "worktree_list".to_string(),
            message: format!(
                "git worktree list failed: {}",
                String::from_utf8_lossy(&output.stderr)
            ),
        });
    }
    Ok(String::from_utf8_lossy(&output.stdout).to_string())
}

pub fn worktree_add(repo_root: &Path, branch: &str, path: &Path, base: &str) -> Result<()> {
    let output = run_git_output(
        repo_root,
        &[
            "worktree",
            "add",
            "-b",
            branch,
            path.to_string_lossy().as_ref(),
            base,
        ],
        "run git worktree add",
    )?;
    if !output.status.success() {
        return Err(MaccError::Git {
            operation: "worktree_add".to_string(),
            message: format!(
                "git worktree add failed: {}",
                String::from_utf8_lossy(&output.stderr)
            ),
        });
    }
    Ok(())
}

pub fn worktree_remove(repo_root: &Path, path: &Path, force: bool) -> Result<()> {
    let path_str = path.to_string_lossy().to_string();
    let args = if force {
        vec!["worktree", "remove", "--force", path_str.as_str()]
    } else {
        vec!["worktree", "remove", path_str.as_str()]
    };
    let output = run_git_output(repo_root, &args, "run git worktree remove")?;
    if !output.status.success() {
        return Err(MaccError::Git {
            operation: "worktree_remove".to_string(),
            message: format!(
                "git worktree remove failed: {}",
                String::from_utf8_lossy(&output.stderr)
            ),
        });
    }
    Ok(())
}

pub fn worktree_prune(repo_root: &Path) -> Result<()> {
    let output = run_git_output(repo_root, &["worktree", "prune"], "run git worktree prune")?;
    if !output.status.success() {
        return Err(MaccError::Git {
            operation: "worktree_prune".to_string(),
            message: format!(
                "git worktree prune failed: {}",
                String::from_utf8_lossy(&output.stderr)
            ),
        });
    }
    Ok(())
}

pub fn status_porcelain(repo_or_worktree: &Path) -> Result<String> {
    let output = run_git_output(
        repo_or_worktree,
        &["status", "--porcelain"],
        "read git status --porcelain",
    )?;
    if !output.status.success() {
        return Ok(String::new());
    }
    Ok(String::from_utf8_lossy(&output.stdout).to_string())
}

pub fn is_dirty(repo_or_worktree: &Path) -> Result<bool> {
    Ok(!status_porcelain(repo_or_worktree)?.trim().is_empty())
}

pub async fn is_dirty_async(repo_or_worktree: &Path) -> Result<bool> {
    let output = run_git_output_async(
        repo_or_worktree,
        &["status", "--porcelain"],
        "read git status --porcelain",
    )
    .await?;
    if !output.status.success() {
        return Ok(false);
    }
    Ok(!String::from_utf8_lossy(&output.stdout).trim().is_empty())
}

pub fn reset_hard(repo_or_worktree: &Path, target: &str) -> Result<bool> {
    Ok(run_git_status(
        repo_or_worktree,
        &["reset", "--hard", target],
        "run git reset --hard",
    )?
    .success())
}

pub async fn reset_hard_async(repo_or_worktree: &Path, target: &str) -> Result<bool> {
    Ok(run_git_status_async(
        repo_or_worktree,
        &["reset", "--hard", target],
        "run git reset --hard",
    )
    .await?
    .success())
}

pub fn clean_fd(repo_or_worktree: &Path) -> Result<bool> {
    Ok(run_git_status(repo_or_worktree, &["clean", "-fd"], "run git clean -fd")?.success())
}

pub async fn clean_fd_async(repo_or_worktree: &Path) -> Result<bool> {
    Ok(
        run_git_status_async(repo_or_worktree, &["clean", "-fd"], "run git clean -fd")
            .await?
            .success(),
    )
}

pub fn checkout(repo_or_worktree: &Path, branch: &str, force: bool) -> Result<bool> {
    let args = if force {
        vec!["checkout", "-f", branch]
    } else {
        vec!["checkout", branch]
    };
    Ok(run_git_status(repo_or_worktree, &args, "run git checkout")?.success())
}

pub async fn checkout_async(repo_or_worktree: &Path, branch: &str, force: bool) -> Result<bool> {
    let args = if force {
        vec!["checkout", "-f", branch]
    } else {
        vec!["checkout", branch]
    };
    Ok(
        run_git_status_async(repo_or_worktree, &args, "run git checkout")
            .await?
            .success(),
    )
}

pub fn checkout_reset_branch(repo_or_worktree: &Path, branch: &str, force: bool) -> Result<bool> {
    let args = if force {
        vec!["checkout", "-f", "-B", branch, branch]
    } else {
        vec!["checkout", "-B", branch, branch]
    };
    Ok(run_git_status(repo_or_worktree, &args, "run git checkout -B")?.success())
}

pub async fn checkout_reset_branch_async(
    repo_or_worktree: &Path,
    branch: &str,
    force: bool,
) -> Result<bool> {
    let args = if force {
        vec!["checkout", "-f", "-B", branch, branch]
    } else {
        vec!["checkout", "-B", branch, branch]
    };
    Ok(
        run_git_status_async(repo_or_worktree, &args, "run git checkout -B")
            .await?
            .success(),
    )
}

/// Detach HEAD in a worktree. This avoids the "branch already checked out"
/// error that occurs when two worktrees share the same branch name.
pub fn checkout_detach(repo_or_worktree: &Path) -> Result<bool> {
    Ok(
        run_git_status(repo_or_worktree, &["checkout", "--detach"], "run git checkout --detach")?
            .success(),
    )
}

pub async fn checkout_detach_async(repo_or_worktree: &Path) -> Result<bool> {
    Ok(
        run_git_status_async(
            repo_or_worktree,
            &["checkout", "--detach"],
            "run git checkout --detach",
        )
        .await?
        .success(),
    )
}

pub fn fetch(repo_or_worktree: &Path, remote: &str) -> Result<bool> {
    Ok(run_git_status(repo_or_worktree, &["fetch", remote], "run git fetch")?.success())
}

pub async fn fetch_async(repo_or_worktree: &Path, remote: &str) -> Result<bool> {
    Ok(
        run_git_status_async(repo_or_worktree, &["fetch", remote], "run git fetch")
            .await?
            .success(),
    )
}

pub fn merge_ff_only(repo_or_worktree: &Path, reference: &str) -> Result<bool> {
    Ok(run_git_status(
        repo_or_worktree,
        &["merge", "--ff-only", reference],
        "run git merge --ff-only",
    )?
    .success())
}

pub fn head_commit(repo_or_worktree: &Path) -> Result<String> {
    let output = run_git_output(repo_or_worktree, &["rev-parse", "HEAD"], "read git head")?;
    if !output.status.success() {
        return Err(MaccError::Git {
            operation: "log".to_string(),
            message: format!("Failed to resolve HEAD in {}", repo_or_worktree.display()),
        });
    }
    Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
}

pub fn current_branch(repo_or_worktree: &Path) -> Result<String> {
    let output = run_git_output(
        repo_or_worktree,
        &["rev-parse", "--abbrev-ref", "HEAD"],
        "read git current branch",
    )?;
    if !output.status.success() {
        return Err(MaccError::Git {
            operation: "status".to_string(),
            message: format!(
                "Failed to resolve current branch in {}",
                repo_or_worktree.display()
            ),
        });
    }
    Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
}

pub fn current_branch_name(repo_or_worktree: &Path) -> Result<String> {
    let output = run_git_output(
        repo_or_worktree,
        &["branch", "--show-current"],
        "read git current branch name",
    )?;
    if !output.status.success() {
        return Err(MaccError::Git {
            operation: "status".to_string(),
            message: format!(
                "Failed to resolve current branch in {}",
                repo_or_worktree.display()
            ),
        });
    }
    Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
}

pub fn git_log_graph(
    repo_path: &Path,
    limit: usize,
    since_sha: Option<&str>,
) -> Result<Vec<GitGraphCommit>> {
    let output = run_git_output(
        repo_path,
        &[
            "log",
            "--all",
            "--topo-order",
            "--decorate=short",
            "--parents",
            "--format=%H%x1f%P%x1f%d%x1f%s%x1f%b%x1f%an%x1f%at%x00",
        ],
        "read git graph log",
    )?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        if stderr.contains("does not have any commits yet")
            || stderr.contains("your current branch")
            || stderr.contains("bad default revision")
        {
            return Ok(Vec::new());
        }
        return Err(MaccError::Git {
            operation: "log_graph".to_string(),
            message: format!("git log graph failed: {}", stderr.trim()),
        });
    }

    let raw = String::from_utf8_lossy(&output.stdout);
    Ok(parse_git_graph_records(&raw, limit, since_sha))
}

fn parse_git_graph_records(
    raw: &str,
    limit: usize,
    since_sha: Option<&str>,
) -> Vec<GitGraphCommit> {
    let commits: Vec<GitGraphCommit> = raw.split('\0').filter_map(parse_git_graph_record).collect();

    let start = since_sha
        .and_then(|cursor| {
            commits
                .iter()
                .position(|commit| commit.sha == cursor || commit.sha.starts_with(cursor))
                .map(|index| index + 1)
        })
        .unwrap_or(0);

    commits.into_iter().skip(start).take(limit).collect()
}

fn parse_git_graph_record(record: &str) -> Option<GitGraphCommit> {
    let record = record.trim();
    if record.is_empty() {
        return None;
    }

    let parts: Vec<&str> = record.splitn(7, '\x1f').collect();
    if parts.len() != 7 {
        return None;
    }

    let timestamp = parts[6].trim().parse::<i64>().ok()?;
    Some(GitGraphCommit {
        sha: parts[0].trim().to_string(),
        parents: parts[1]
            .split_whitespace()
            .map(|parent| parent.to_string())
            .collect(),
        refs: parse_graph_refs(parts[2]),
        subject: parts[3].trim().to_string(),
        body: parts[4].trim().to_string(),
        author: parts[5].trim().to_string(),
        timestamp,
    })
}

fn parse_graph_refs(raw: &str) -> Vec<String> {
    let trimmed = raw.trim();
    let inner = trimmed
        .strip_prefix('(')
        .and_then(|value| value.strip_suffix(')'))
        .unwrap_or(trimmed)
        .trim();
    if inner.is_empty() {
        return Vec::new();
    }

    inner
        .split(", ")
        .filter_map(|entry| {
            let entry = entry.trim();
            if entry.is_empty() || entry == "HEAD" || entry.starts_with("tag: ") {
                return None;
            }
            if let Some((_, target)) = entry.split_once(" -> ") {
                let target = target.trim();
                if target.is_empty() {
                    return None;
                }
                return Some(target.to_string());
            }
            Some(entry.to_string())
        })
        .collect()
}

pub fn rev_parse_verify(repo_or_worktree: &Path, reference: &str) -> Result<bool> {
    Ok(run_git_status(
        repo_or_worktree,
        &["rev-parse", "--verify", reference],
        "run git rev-parse --verify",
    )?
    .success())
}

pub fn merge_base_is_ancestor(
    repo_or_worktree: &Path,
    ancestor: &str,
    descendant: &str,
) -> Result<bool> {
    Ok(run_git_status(
        repo_or_worktree,
        &["merge-base", "--is-ancestor", ancestor, descendant],
        "run git merge-base --is-ancestor",
    )?
    .success())
}

/// Resolve a reference to its full SHA.  Returns `None` when the reference
/// does not exist.
pub fn resolve_ref(repo_or_worktree: &Path, reference: &str) -> Option<String> {
    let output = run_git_output(
        repo_or_worktree,
        &["rev-parse", "--verify", "--quiet", reference],
        "resolve git ref",
    )
    .ok()?;
    if !output.status.success() {
        return None;
    }
    let sha = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if sha.is_empty() {
        None
    } else {
        Some(sha)
    }
}

/// Returns `true` when the worktree HEAD has at least one commit that is not
/// reachable from `base_ref` (i.e. committed work exists ahead of the base).
pub fn has_commits_ahead(repo_or_worktree: &Path, base_ref: &str) -> bool {
    let output = run_git_output(
        repo_or_worktree,
        &["rev-list", "--count", &format!("{}..HEAD", base_ref)],
        "count commits ahead of base",
    );
    match output {
        Ok(out) if out.status.success() => {
            let count_str = String::from_utf8_lossy(&out.stdout).trim().to_string();
            count_str.parse::<usize>().unwrap_or(0) > 0
        }
        _ => false,
    }
}

pub fn commits_ahead_of_base(repo_or_worktree: &Path, base_branch: &str) -> Result<Vec<CommitInfo>> {
    let range = format!("{base_branch}..HEAD");
    let output = run_git_output(
        repo_or_worktree,
        &["log", "--format=%H%x1f%s%x1f%aI", &range],
        "read commits ahead of base",
    )?;
    if !output.status.success() {
        return Err(MaccError::Git {
            operation: "log".to_string(),
            message: format!(
                "Failed to read commits ahead of base in {}: {}",
                repo_or_worktree.display(),
                String::from_utf8_lossy(&output.stderr).trim()
            ),
        });
    }
    Ok(parse_commit_info_records(&String::from_utf8_lossy(
        &output.stdout,
    )))
}

pub async fn commits_ahead_of_base_async(
    repo_or_worktree: &Path,
    base_branch: &str,
) -> Result<Vec<CommitInfo>> {
    let range = format!("{base_branch}..HEAD");
    let output = run_git_output_async(
        repo_or_worktree,
        &["log", "--format=%H%x1f%s%x1f%aI", &range],
        "read commits ahead of base",
    )
    .await?;
    if !output.status.success() {
        return Err(MaccError::Git {
            operation: "log".to_string(),
            message: format!(
                "Failed to read commits ahead of base in {}: {}",
                repo_or_worktree.display(),
                String::from_utf8_lossy(&output.stderr).trim()
            ),
        });
    }
    Ok(parse_commit_info_records(&String::from_utf8_lossy(
        &output.stdout,
    )))
}

pub fn commits_containing_pattern(
    repo_or_worktree: &Path,
    base_branch: &str,
    pattern: &str,
) -> Result<Vec<CommitInfo>> {
    let range = format!("{base_branch}..HEAD");
    let grep = format!("--grep={pattern}");
    let output = run_git_output(
        repo_or_worktree,
        &["log", "--format=%H%x1f%s%x1f%aI", &grep, &range],
        "read commits matching pattern",
    )?;
    if !output.status.success() {
        return Err(MaccError::Git {
            operation: "log".to_string(),
            message: format!(
                "Failed to read commits matching pattern in {}: {}",
                repo_or_worktree.display(),
                String::from_utf8_lossy(&output.stderr).trim()
            ),
        });
    }
    Ok(parse_commit_info_records(&String::from_utf8_lossy(
        &output.stdout,
    )))
}

pub async fn commits_containing_pattern_async(
    repo_or_worktree: &Path,
    base_branch: &str,
    pattern: &str,
) -> Result<Vec<CommitInfo>> {
    let range = format!("{base_branch}..HEAD");
    let grep = format!("--grep={pattern}");
    let output = run_git_output_async(
        repo_or_worktree,
        &["log", "--format=%H%x1f%s%x1f%aI", &grep, &range],
        "read commits matching pattern",
    )
    .await?;
    if !output.status.success() {
        return Err(MaccError::Git {
            operation: "log".to_string(),
            message: format!(
                "Failed to read commits matching pattern in {}: {}",
                repo_or_worktree.display(),
                String::from_utf8_lossy(&output.stderr).trim()
            ),
        });
    }
    Ok(parse_commit_info_records(&String::from_utf8_lossy(
        &output.stdout,
    )))
}

pub fn create_tag(repo_root: &Path, tag_name: &str, commit_ref: &str) -> Result<()> {
    let output = run_git_output(repo_root, &["tag", tag_name, commit_ref], "create git tag")?;
    if !output.status.success() {
        return Err(MaccError::Git {
            operation: "tag".to_string(),
            message: format!(
                "Failed to create tag {} at {}: {}",
                tag_name,
                commit_ref,
                String::from_utf8_lossy(&output.stderr).trim()
            ),
        });
    }
    Ok(())
}

pub async fn create_tag_async(repo_root: &Path, tag_name: &str, commit_ref: &str) -> Result<()> {
    let output = run_git_output_async(repo_root, &["tag", tag_name, commit_ref], "create git tag")
        .await?;
    if !output.status.success() {
        return Err(MaccError::Git {
            operation: "tag".to_string(),
            message: format!(
                "Failed to create tag {} at {}: {}",
                tag_name,
                commit_ref,
                String::from_utf8_lossy(&output.stderr).trim()
            ),
        });
    }
    Ok(())
}

pub fn list_branches_by_prefix(repo_root: &Path, prefix: &str) -> Result<Vec<String>> {
    let branch_glob = format!("{prefix}*");
    let output = run_git_output(
        repo_root,
        &["branch", "--list", &branch_glob],
        "list branches by prefix",
    )?;
    if !output.status.success() {
        return Err(MaccError::Git {
            operation: "branch_list".to_string(),
            message: format!(
                "Failed to list branches by prefix in {}: {}",
                repo_root.display(),
                String::from_utf8_lossy(&output.stderr).trim()
            ),
        });
    }

    let branches = String::from_utf8_lossy(&output.stdout)
        .lines()
        .map(str::trim)
        .map(|line| line.trim_start_matches("* ").to_string())
        .filter(|line| !line.is_empty())
        .collect();
    Ok(branches)
}

pub async fn list_branches_by_prefix_async(repo_root: &Path, prefix: &str) -> Result<Vec<String>> {
    let branch_glob = format!("{prefix}*");
    let output = run_git_output_async(
        repo_root,
        &["branch", "--list", &branch_glob],
        "list branches by prefix",
    )
    .await?;
    if !output.status.success() {
        return Err(MaccError::Git {
            operation: "branch_list".to_string(),
            message: format!(
                "Failed to list branches by prefix in {}: {}",
                repo_root.display(),
                String::from_utf8_lossy(&output.stderr).trim()
            ),
        });
    }

    let branches = String::from_utf8_lossy(&output.stdout)
        .lines()
        .map(str::trim)
        .map(|line| line.trim_start_matches("* ").to_string())
        .filter(|line| !line.is_empty())
        .collect();
    Ok(branches)
}

fn parse_commit_info_records(raw: &str) -> Vec<CommitInfo> {
    raw.lines().filter_map(parse_commit_info_line).collect()
}

fn parse_commit_info_line(line: &str) -> Option<CommitInfo> {
    let trimmed = line.trim();
    if trimmed.is_empty() {
        return None;
    }
    let parts: Vec<&str> = trimmed.splitn(3, '\x1f').collect();
    if parts.len() != 3 {
        return None;
    }
    Some(CommitInfo {
        hash: parts[0].trim().to_string(),
        subject: parts[1].trim().to_string(),
        author_date: parts[2].trim().to_string(),
    })
}

/// Returns the list of git identity fields that are not configured.
/// Checks `user.email` and `user.name` via `git config` (respects global, local, and system).
/// An empty vec means all required fields are set.
pub fn missing_git_identity_fields(repo_root: &Path) -> Vec<&'static str> {
    let mut missing = Vec::new();
    for key in ["user.email", "user.name"] {
        let configured = std::process::Command::new("git")
            .args(["config", key])
            .current_dir(repo_root)
            .output()
            .ok()
            .map(|out| {
                out.status.success() && !String::from_utf8_lossy(&out.stdout).trim().is_empty()
            })
            .unwrap_or(false);
        if !configured {
            missing.push(key);
        }
    }
    missing
}

pub fn checkout_new_branch_from_base(
    repo_or_worktree: &Path,
    branch: &str,
    base_branch: &str,
) -> Result<bool> {
    Ok(run_git_status(
        repo_or_worktree,
        &["checkout", "-B", branch, base_branch],
        "create branch from base",
    )?
    .success())
}

pub async fn merge_ff_only_async(repo_or_worktree: &Path, reference: &str) -> Result<bool> {
    Ok(run_git_status_async(
        repo_or_worktree,
        &["merge", "--ff-only", reference],
        "run git merge --ff-only",
    )
    .await?
    .success())
}

pub async fn head_commit_async(repo_or_worktree: &Path) -> Result<String> {
    let output =
        run_git_output_async(repo_or_worktree, &["rev-parse", "HEAD"], "read git head").await?;
    if !output.status.success() {
        return Err(MaccError::Git {
            operation: "log".to_string(),
            message: format!("Failed to resolve HEAD in {}", repo_or_worktree.display()),
        });
    }
    Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
}

pub fn delete_local_branch(repo_root: &Path, branch: &str, force: bool) -> Result<()> {
    let normalized = branch.strip_prefix("refs/heads/").unwrap_or(branch);
    if normalized.is_empty() {
        return Ok(());
    }
    let args = if force {
        vec!["branch", "-D", normalized]
    } else {
        vec!["branch", "-d", normalized]
    };
    let output = run_git_output(repo_root, &args, "run git branch delete")?;
    if !output.status.success() {
        return Err(MaccError::Git {
            operation: "branch_delete".to_string(),
            message: format!(
                "git branch delete failed: {}",
                String::from_utf8_lossy(&output.stderr)
            ),
        });
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{
        commits_ahead_of_base, commits_ahead_of_base_async, commits_containing_pattern,
        commits_containing_pattern_async, create_tag, create_tag_async, list_branches_by_prefix,
        list_branches_by_prefix_async, parse_git_graph_record, parse_git_graph_records,
        parse_graph_refs,
    };
    use std::path::{Path, PathBuf};
    use std::process::Command;
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::{fs, time::SystemTime};

    static TEST_COUNTER: AtomicU64 = AtomicU64::new(0);

    fn run_git(repo: &Path, args: &[&str]) {
        let output = Command::new("git")
            .args(args)
            .current_dir(repo)
            .output()
            .expect("run git command");
        assert!(
            output.status.success(),
            "git {:?} failed: {}",
            args,
            String::from_utf8_lossy(&output.stderr)
        );
    }

    fn create_commit(repo: &Path, file: &str, content: &str, message: &str) {
        let path = repo.join(file);
        fs::write(&path, content).expect("write file");
        run_git(repo, &["add", file]);
        run_git(repo, &["commit", "-m", message]);
    }

    fn make_test_repo() -> PathBuf {
        let suffix = TEST_COUNTER.fetch_add(1, Ordering::Relaxed);
        let nanos = SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("time")
            .as_nanos();
        let repo = std::env::temp_dir().join(format!(
            "macc-git-tests-{}-{}-{}",
            std::process::id(),
            nanos,
            suffix
        ));
        fs::create_dir_all(&repo).expect("create temp repo");
        run_git(&repo, &["init"]);
        run_git(&repo, &["checkout", "-b", "main"]);
        run_git(&repo, &["config", "user.email", "tests@example.com"]);
        run_git(&repo, &["config", "user.name", "MACC Tests"]);
        create_commit(&repo, "base.txt", "base\n", "chore: base");
        repo
    }

    #[test]
    fn parse_graph_refs_strips_head_and_tags() {
        assert_eq!(
            parse_graph_refs("(HEAD -> main, origin/HEAD -> origin/main, tag: v1.0.0, feature)"),
            vec![
                "main".to_string(),
                "origin/main".to_string(),
                "feature".to_string()
            ]
        );
    }

    #[test]
    fn parse_git_graph_record_reads_parents_and_body() {
        let record = "abc\x1fdef ghi\x1f(HEAD -> main, feature)\x1fsubject\x1fbody line\x1fAlice\x1f1710000000";
        let commit = parse_git_graph_record(record).expect("commit");
        assert_eq!(commit.sha, "abc");
        assert_eq!(commit.parents, vec!["def", "ghi"]);
        assert_eq!(commit.refs, vec!["main", "feature"]);
        assert_eq!(commit.subject, "subject");
        assert_eq!(commit.body, "body line");
        assert_eq!(commit.author, "Alice");
        assert_eq!(commit.timestamp, 1710000000);
    }

    #[test]
    fn parse_git_graph_records_applies_since_cursor() {
        let raw = concat!(
            "sha-1\x1f\x1f(HEAD -> main)\x1fone\x1f\x1fA\x1f1\x00",
            "sha-2\x1fsha-1\x1f(feature)\x1ftwo\x1f\x1fB\x1f2\x00",
            "sha-3\x1fsha-2\x1f\x1fthree\x1f\x1fC\x1f3\x00"
        );
        let commits = parse_git_graph_records(raw, 2, Some("sha-1"));
        let shas: Vec<String> = commits.into_iter().map(|commit| commit.sha).collect();
        assert_eq!(shas, vec!["sha-2".to_string(), "sha-3".to_string()]);
    }

    #[test]
    fn commits_helpers_filter_by_base_and_pattern() {
        let repo = make_test_repo();
        run_git(&repo, &["checkout", "-b", "task/demo"]);
        create_commit(&repo, "one.txt", "one\n", "feat: add one");
        create_commit(&repo, "two.txt", "two\n", "fix: add two");

        let commits = commits_ahead_of_base(&repo, "main").expect("commits ahead");
        assert_eq!(commits.len(), 2);
        assert!(!commits[0].hash.is_empty());
        assert!(!commits[0].author_date.is_empty());
        assert!(commits.iter().any(|commit| commit.subject == "feat: add one"));
        assert!(commits.iter().any(|commit| commit.subject == "fix: add two"));

        let filtered =
            commits_containing_pattern(&repo, "main", "feat").expect("filtered commits");
        assert_eq!(filtered.len(), 1);
        assert_eq!(filtered[0].subject, "feat: add one");
    }

    #[tokio::test]
    async fn commits_helpers_async_filter_by_base_and_pattern() {
        let repo = make_test_repo();
        run_git(&repo, &["checkout", "-b", "task/demo-async"]);
        create_commit(&repo, "one.txt", "one\n", "feat: add one");
        create_commit(&repo, "two.txt", "two\n", "chore: add two");

        let commits = commits_ahead_of_base_async(&repo, "main")
            .await
            .expect("commits ahead");
        assert_eq!(commits.len(), 2);
        assert!(commits.iter().any(|commit| commit.subject == "feat: add one"));

        let filtered = commits_containing_pattern_async(&repo, "main", "feat")
            .await
            .expect("filtered commits");
        assert_eq!(filtered.len(), 1);
        assert_eq!(filtered[0].subject, "feat: add one");
    }

    #[test]
    fn create_tag_and_list_branches_by_prefix_work() {
        let repo = make_test_repo();
        run_git(&repo, &["checkout", "-b", "task/one"]);
        run_git(&repo, &["checkout", "main"]);
        run_git(&repo, &["checkout", "-b", "task/two"]);
        run_git(&repo, &["checkout", "main"]);
        run_git(&repo, &["checkout", "-b", "feature/nope"]);
        run_git(&repo, &["checkout", "main"]);

        let mut task_branches = list_branches_by_prefix(&repo, "task/").expect("task branches");
        task_branches.sort();
        assert_eq!(
            task_branches,
            vec!["task/one".to_string(), "task/two".to_string()]
        );

        create_tag(&repo, "v-test-1", "HEAD").expect("create tag");
        run_git(&repo, &["rev-parse", "--verify", "refs/tags/v-test-1"]);
    }

    #[tokio::test]
    async fn create_tag_and_list_branches_by_prefix_async_work() {
        let repo = make_test_repo();
        run_git(&repo, &["checkout", "-b", "task/async-one"]);
        run_git(&repo, &["checkout", "main"]);
        run_git(&repo, &["checkout", "-b", "misc/branch"]);
        run_git(&repo, &["checkout", "main"]);

        let branches = list_branches_by_prefix_async(&repo, "task/")
            .await
            .expect("task branches");
        assert_eq!(branches, vec!["task/async-one".to_string()]);

        create_tag_async(&repo, "v-test-async", "HEAD")
            .await
            .expect("create tag");
        run_git(&repo, &["rev-parse", "--verify", "refs/tags/v-test-async"]);
    }
}
