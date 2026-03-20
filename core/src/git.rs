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
    use super::{parse_git_graph_record, parse_git_graph_records, parse_graph_refs};

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
}
