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

/// Generates paired sync/async git helpers for operations where both variants
/// share the same command and result mapping.
macro_rules! git_command {
    (
        $sync_name:ident,
        $async_name:ident,
        ($($param:ident : $param_ty:ty),* $(,)?),
        $args:expr,
        $action:expr
    ) => {
        pub fn $sync_name(repo_or_worktree: &Path, $($param: $param_ty),*) -> Result<bool> {
            let args = $args;
            Ok(run_git_status(repo_or_worktree, &args, $action)?.success())
        }

        pub async fn $async_name(repo_or_worktree: &Path, $($param: $param_ty),*) -> Result<bool> {
            let args = $args;
            Ok(
                run_git_status_async(repo_or_worktree, &args, $action)
                    .await?
                    .success(),
            )
        }
    };
    (
        output,
        $sync_name:ident,
        $async_name:ident,
        ($($param:ident : $param_ty:ty),* $(,)?),
        -> $ret:ty,
        $args:expr,
        $action:expr,
        |$repo:ident, $output:ident| $body:block
    ) => {
        pub fn $sync_name(repo_root: &Path, $($param: $param_ty),*) -> Result<$ret> {
            let args = $args;
            let $output = run_git_output(repo_root, &args, $action)?;
            let $repo = repo_root;
            $body
        }

        pub async fn $async_name(repo_root: &Path, $($param: $param_ty),*) -> Result<$ret> {
            let args = $args;
            let $output = run_git_output_async(repo_root, &args, $action).await?;
            let $repo = repo_root;
            $body
        }
    };
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

git_command!(
    reset_hard,
    reset_hard_async,
    (target: &str),
    ["reset", "--hard", target],
    "run git reset --hard"
);

git_command!(
    clean_fd,
    clean_fd_async,
    (),
    ["clean", "-fd"],
    "run git clean -fd"
);

git_command!(
    checkout,
    checkout_async,
    (branch: &str, force: bool),
    if force {
        vec!["checkout", "-f", branch]
    } else {
        vec!["checkout", branch]
    },
    "run git checkout"
);

git_command!(
    checkout_reset_branch,
    checkout_reset_branch_async,
    (branch: &str, force: bool),
    if force {
        vec!["checkout", "-f", "-B", branch, branch]
    } else {
        vec!["checkout", "-B", branch, branch]
    },
    "run git checkout -B"
);

// Detach HEAD in a worktree. This avoids the "branch already checked out"
// error that occurs when two worktrees share the same branch name.
git_command!(
    checkout_detach,
    checkout_detach_async,
    (),
    ["checkout", "--detach"],
    "run git checkout --detach"
);

git_command!(
    fetch,
    fetch_async,
    (remote: &str),
    ["fetch", remote],
    "run git fetch"
);

pub fn git_remote_exists(repo_or_worktree: &Path, remote: &str) -> Result<bool> {
    Ok(run_git_status(
        repo_or_worktree,
        &["remote", "get-url", remote],
        "check git remote exists",
    )?
    .success())
}

pub async fn git_remote_exists_async(repo_or_worktree: &Path, remote: &str) -> Result<bool> {
    Ok(run_git_status_async(
        repo_or_worktree,
        &["remote", "get-url", remote],
        "check git remote exists",
    )
    .await?
    .success())
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

pub fn commits_ahead_of_base(
    repo_or_worktree: &Path,
    base_branch: &str,
) -> Result<Vec<CommitInfo>> {
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

git_command!(
    output,
    create_tag,
    create_tag_async,
    (tag_name: &str, commit_ref: &str),
    -> (),
    ["tag", tag_name, commit_ref],
    "create git tag",
    |_repo_root, output| {
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
);

git_command!(
    output,
    list_branches_by_prefix,
    list_branches_by_prefix_async,
    (prefix: &str),
    -> Vec<String>,
    ["branch", "--list"],
    "list branches by prefix",
    |repo_root, output| {
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
            .filter(|line| line.starts_with(prefix))
            .filter(|line| !line.is_empty())
            .collect();
        Ok(branches)
    }
);

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

// ── Preflight gate helpers ────────────────────────────────────────────────────

/// Validate a branch name using `git check-ref-format --branch <name>`.
/// Returns `true` when the name is valid, `false` otherwise.
pub fn check_ref_format_branch(repo_root: &Path, branch: &str) -> Result<bool> {
    let output = run_git_output(
        repo_root,
        &["check-ref-format", "--branch", branch],
        "check-ref-format",
    )?;
    Ok(output.status.success())
}

/// Return `true` when `refs/heads/<branch>` exists in the repository.
pub fn local_branch_exists(repo_root: &Path, branch: &str) -> Result<bool> {
    let refspec = format!("refs/heads/{}", branch);
    let output = run_git_output(
        repo_root,
        &["show-ref", "--verify", "--quiet", &refspec],
        "show-ref branch",
    )?;
    Ok(output.status.success())
}

/// Return remote-tracking refs that match `<remote>/<branch>` for all remotes.
/// MVP checks `origin/<branch>` plus any entry returned by `git for-each-ref`.
pub fn remote_tracking_refs_for_branch(repo_root: &Path, branch: &str) -> Result<Vec<String>> {
    let format = "%(refname:short)";
    let pattern = format!("refs/remotes/*/{}", branch);
    let output = run_git_output(
        repo_root,
        &["for-each-ref", &format!("--format={}", format), &pattern],
        "for-each-ref remotes",
    )?;
    if !output.status.success() {
        return Ok(Vec::new());
    }
    let refs: Vec<String> = String::from_utf8_lossy(&output.stdout)
        .lines()
        .filter(|l| !l.trim().is_empty())
        .map(|l| l.trim().to_string())
        .collect();
    Ok(refs)
}

/// Return the paths of all Git worktrees where `refs/heads/<branch>` is checked out.
pub fn worktrees_for_branch(repo_root: &Path, branch: &str) -> Result<Vec<std::path::PathBuf>> {
    let raw = worktree_list_porcelain(repo_root)?;
    let target_ref = format!("refs/heads/{}", branch);
    let mut paths = Vec::new();
    let mut current_path: Option<std::path::PathBuf> = None;

    for line in raw.lines() {
        if let Some(rest) = line.strip_prefix("worktree ") {
            current_path = Some(std::path::PathBuf::from(rest.trim()));
        } else if let Some(rest) = line.strip_prefix("branch ") {
            if rest.trim() == target_ref {
                if let Some(p) = current_path.take() {
                    paths.push(p);
                }
            } else {
                current_path = None;
            }
        } else if line.trim().is_empty() {
            current_path = None;
        }
    }
    Ok(paths)
}

/// Run `git -C <path> status --porcelain=v1` and return parsed entries.
/// When `include_untracked` is `false`, `??`-prefixed entries are excluded.
pub fn status_porcelain_v1(
    worktree_path: &Path,
    include_untracked: bool,
) -> Result<Vec<GitPorcelainEntry>> {
    let untracked_flag = if include_untracked {
        "--untracked-files=all"
    } else {
        "--untracked-files=no"
    };
    let output = run_git_output(
        worktree_path,
        &["status", "--porcelain=v1", untracked_flag],
        "status porcelain",
    )?;
    if !output.status.success() {
        return Err(MaccError::Git {
            operation: "status_porcelain".to_string(),
            message: format!(
                "git status failed: {}",
                String::from_utf8_lossy(&output.stderr)
            ),
        });
    }
    let entries = String::from_utf8_lossy(&output.stdout)
        .lines()
        .filter(|l| l.len() >= 3)
        .map(parse_porcelain_v1_line)
        .collect();
    Ok(entries)
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GitPorcelainEntry {
    pub index_status: char,
    pub worktree_status: char,
    pub path: String,
    pub original_path: Option<String>,
}

fn parse_porcelain_v1_line(line: &str) -> GitPorcelainEntry {
    let mut chars = line.chars();
    let index_status = chars.next().unwrap_or(' ');
    let worktree_status = chars.next().unwrap_or(' ');
    // skip the space separator
    let rest = &line[3..];
    // Renames: "R old -> new" or "old\0new" (v1 uses space-arrow in some modes)
    if let Some((original, path)) = rest.split_once(" -> ") {
        GitPorcelainEntry {
            index_status,
            worktree_status,
            path: path.to_string(),
            original_path: Some(original.to_string()),
        }
    } else {
        GitPorcelainEntry {
            index_status,
            worktree_status,
            path: rest.to_string(),
            original_path: None,
        }
    }
}

/// Create a local branch at `start_point` (branch name, tag, or rev).
pub fn create_branch_at(repo_root: &Path, branch: &str, start_point: &str) -> Result<()> {
    let output = run_git_output(repo_root, &["branch", branch, start_point], "create branch")?;
    if !output.status.success() {
        return Err(MaccError::Git {
            operation: "create_branch".to_string(),
            message: format!(
                "git branch {} {} failed: {}",
                branch,
                start_point,
                String::from_utf8_lossy(&output.stderr)
            ),
        });
    }
    Ok(())
}

/// Create a local tracking branch from a remote-tracking ref.
pub fn create_tracking_branch(repo_root: &Path, branch: &str, remote_ref: &str) -> Result<()> {
    let output = run_git_output(
        repo_root,
        &["branch", "--track", branch, remote_ref],
        "create tracking branch",
    )?;
    if !output.status.success() {
        return Err(MaccError::Git {
            operation: "create_tracking_branch".to_string(),
            message: format!(
                "git branch --track {} {} failed: {}",
                branch,
                remote_ref,
                String::from_utf8_lossy(&output.stderr)
            ),
        });
    }
    Ok(())
}

/// Return `true` when the repository is a bare repository.
pub fn is_bare_repository(repo_root: &Path) -> Result<bool> {
    let output = run_git_output(
        repo_root,
        &["rev-parse", "--is-bare-repository"],
        "is-bare-repository",
    )?;
    Ok(output.status.success() && String::from_utf8_lossy(&output.stdout).trim() == "true")
}

#[cfg(test)]
mod tests {
    use super::{
        commits_ahead_of_base, commits_ahead_of_base_async, commits_containing_pattern,
        commits_containing_pattern_async, create_tag, create_tag_async, git_remote_exists,
        git_remote_exists_async, list_branches_by_prefix, list_branches_by_prefix_async,
        parse_git_graph_record, parse_git_graph_records, parse_graph_refs,
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
        assert!(commits
            .iter()
            .any(|commit| commit.subject == "feat: add one"));
        assert!(commits
            .iter()
            .any(|commit| commit.subject == "fix: add two"));

        let filtered = commits_containing_pattern(&repo, "main", "feat").expect("filtered commits");
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
        assert!(commits
            .iter()
            .any(|commit| commit.subject == "feat: add one"));

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

    #[test]
    fn git_remote_exists_returns_true_when_remote_exists() {
        let repo = make_test_repo();
        run_git(
            &repo,
            &["remote", "add", "origin", "https://example.com/repo.git"],
        );

        assert!(git_remote_exists(&repo, "origin").expect("remote check succeeds"));
    }

    #[test]
    fn git_remote_exists_returns_false_when_remote_missing() {
        let repo = make_test_repo();
        assert!(!git_remote_exists(&repo, "origin").expect("remote check succeeds"));
    }

    #[tokio::test]
    async fn git_remote_exists_async_matches_sync_result() {
        let repo = make_test_repo();
        run_git(
            &repo,
            &["remote", "add", "origin", "https://example.com/repo.git"],
        );

        let sync_exists = git_remote_exists(&repo, "origin").expect("sync remote check succeeds");
        let async_exists = git_remote_exists_async(&repo, "origin")
            .await
            .expect("async remote check succeeds");
        assert_eq!(async_exists, sync_exists);

        let sync_missing =
            git_remote_exists(&repo, "upstream").expect("sync remote check succeeds");
        let async_missing = git_remote_exists_async(&repo, "upstream")
            .await
            .expect("async remote check succeeds");
        assert_eq!(async_missing, sync_missing);
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
