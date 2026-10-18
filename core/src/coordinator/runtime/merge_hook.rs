//! Bounded, independently terminated merge-hook execution and completion checks.
use super::{git, MaccError, MergeTaskContext, Path, PerformerCompletionKind, Result};
use std::io::{Read, Seek};
use std::path::PathBuf;
/// Truncate a diff to a maximum number of lines to avoid overwhelming the
/// merge-fix prompt with huge diffs.
fn truncate_diff(text: &str, max_lines: usize) -> String {
    let lines: Vec<&str> = text.lines().collect();
    if lines.len() <= max_lines {
        text.to_string()
    } else {
        let mut result: String = lines[..max_lines].join("\n");
        result.push_str(&format!(
            "\n... ({} more lines truncated)",
            lines.len() - max_lines
        ));
        result
    }
}

pub(super) fn coordinator_log_dir(repo_root: &Path) -> PathBuf {
    repo_root.join(".macc").join("log").join("coordinator")
}

pub(super) enum HookRunResult {
    Completed {
        output: String,
        timed_out: bool,
        succeeded: bool,
    },
}

/// Run the AI merge-fix hook.
///
/// `repo_root` is used for read-only history queries and for resolving the
/// hook's own tooling; `merge_dir` is the integration worktree that actually
/// holds the conflicted merge, and is where the hook (and the AI performer it
/// launches) must do its work.
#[allow(clippy::too_many_arguments)]
pub(super) fn run_merge_hook_with_timeout(
    repo_root: &Path,
    merge_dir: &Path,
    hook: &Path,
    task_id: &str,
    branch: &str,
    base: &str,
    conflicts: &str,
    timeout_seconds: u64,
    ctx: &MergeTaskContext,
) -> Result<HookRunResult> {
    use std::process::{Command, Stdio};

    // Build three-way diff context so the AI understands both sides of the
    // conflict: what the base branch gained since the fork point, and what the
    // feature branch changed.
    let merge_base_sha = git::run_git_output_mapped(
        repo_root,
        &["merge-base", base, branch],
        "compute merge-base for merge-fix context",
    )
    .ok()
    .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
    .unwrap_or_default();

    let (base_commits, branch_commits, base_diff_stat, branch_diff_stat) =
        if !merge_base_sha.is_empty() {
            let base_log = git::run_git_output_mapped(
                repo_root,
                &["log", "--oneline", &format!("{}..{}", merge_base_sha, base)],
                "base commits since fork",
            )
            .ok()
            .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
            .unwrap_or_default();

            let branch_log = git::run_git_output_mapped(
                repo_root,
                &[
                    "log",
                    "--oneline",
                    &format!("{}..{}", merge_base_sha, branch),
                ],
                "branch commits since fork",
            )
            .ok()
            .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
            .unwrap_or_default();

            let base_stat = git::run_git_output_mapped(
                repo_root,
                &["diff", "--stat", &merge_base_sha, base],
                "base diff stat since fork",
            )
            .ok()
            .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
            .unwrap_or_default();

            let branch_stat = git::run_git_output_mapped(
                repo_root,
                &["diff", "--stat", &merge_base_sha, branch],
                "branch diff stat since fork",
            )
            .ok()
            .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
            .unwrap_or_default();

            (base_log, branch_log, base_stat, branch_stat)
        } else {
            (String::new(), String::new(), String::new(), String::new())
        };

    // Build the targeted diff for only the conflicting files so the AI can see
    // exactly what each side changed in those files.
    let conflict_diff = if !merge_base_sha.is_empty() && !conflicts.is_empty() {
        let conflict_files: Vec<&str> = conflicts.split(',').map(|s| s.trim()).collect();
        let mut diff_sections = String::new();
        for file in &conflict_files {
            let base_side = git::run_git_output_mapped(
                repo_root,
                &["diff", &merge_base_sha, base, "--", file],
                "base-side diff for conflict file",
            )
            .ok()
            .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
            .unwrap_or_default();

            let branch_side = git::run_git_output_mapped(
                repo_root,
                &["diff", &merge_base_sha, branch, "--", file],
                "branch-side diff for conflict file",
            )
            .ok()
            .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
            .unwrap_or_default();

            if !base_side.is_empty() || !branch_side.is_empty() {
                diff_sections.push_str(&format!("### {}\n", file));
                diff_sections.push_str(&format!(
                    "Base side ({}):\n```diff\n{}\n```\n",
                    base,
                    truncate_diff(&base_side, 200)
                ));
                diff_sections.push_str(&format!(
                    "Branch side ({}):\n```diff\n{}\n```\n\n",
                    branch,
                    truncate_diff(&branch_side, 200)
                ));
            }
        }
        diff_sections
    } else {
        String::new()
    };

    // Files avoid deadlocking a verbose tool on a full stdout/stderr pipe while
    // the parent waits for exit. Capture remains bounded and private.
    let (mut stdout, stdout_path) = capture_file(repo_root, "stdout")?;
    let (mut stderr, stderr_path) = capture_file(repo_root, "stderr")?;
    let mut command = Command::new(hook);
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        command.process_group(0);
    }
    let mut child = command
        .current_dir(merge_dir)
        .arg("--repo")
        .arg(repo_root)
        .arg("--merge-dir")
        .arg(merge_dir)
        .arg("--task-id")
        .arg(task_id)
        .arg("--branch")
        .arg(branch)
        .arg("--base-branch")
        .arg(base)
        .arg("--failure-step")
        .arg("merge")
        .arg("--failure-reason")
        .arg("git merge reported conflicts")
        .arg("--conflicts")
        .arg(conflicts)
        .arg("--tool")
        .arg(&ctx.tool)
        .arg("--worktree-path")
        .arg(&ctx.worktree_path)
        .arg("--task-title")
        .arg(&ctx.title)
        .arg("--task-description")
        .arg(&ctx.description)
        .arg("--task-objective")
        .arg(&ctx.objective)
        .arg("--merge-base")
        .arg(&merge_base_sha)
        .arg("--base-commits")
        .arg(&base_commits)
        .arg("--branch-commits")
        .arg(&branch_commits)
        .arg("--base-diff-stat")
        .arg(&base_diff_stat)
        .arg("--branch-diff-stat")
        .arg(&branch_diff_stat)
        .arg("--conflict-diff")
        .arg(&conflict_diff)
        .stdin(Stdio::null())
        .stdout(Stdio::from(
            stdout
                .try_clone()
                .map_err(|e| MaccError::Validation(e.to_string()))?,
        ))
        .stderr(Stdio::from(
            stderr
                .try_clone()
                .map_err(|e| MaccError::Validation(e.to_string()))?,
        ))
        .spawn()
        .map_err(|e| MaccError::Io {
            path: hook.to_string_lossy().into(),
            action: "spawn merge-fix hook".into(),
            source: e,
        })?;
    let started = std::time::Instant::now();
    let mut timed_out = false;
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) => {
                let size = stdout
                    .metadata()
                    .map(|m| m.len())
                    .unwrap_or(u64::MAX)
                    .saturating_add(stderr.metadata().map(|m| m.len()).unwrap_or(u64::MAX));
                if (timeout_seconds > 0 && started.elapsed().as_secs() >= timeout_seconds)
                    || size > 8 * 1024 * 1024
                {
                    timed_out = true;
                    terminate(&mut child);
                    break child
                        .wait()
                        .map_err(|e| MaccError::Validation(e.to_string()))?;
                }
                std::thread::sleep(std::time::Duration::from_millis(50));
            }
            Err(e) => {
                terminate(&mut child);
                return Err(MaccError::Validation(format!(
                    "merge-fix hook wait error: {e}"
                )));
            }
        }
    };

    stdout
        .rewind()
        .map_err(|e| MaccError::Validation(e.to_string()))?;
    stderr
        .rewind()
        .map_err(|e| MaccError::Validation(e.to_string()))?;
    let mut out = String::new();
    stdout
        .take(8 * 1024 * 1024)
        .read_to_string(&mut out)
        .map_err(|e| MaccError::Validation(e.to_string()))?;
    let mut err = String::new();
    stderr
        .take(8 * 1024 * 1024)
        .read_to_string(&mut err)
        .map_err(|e| MaccError::Validation(e.to_string()))?;
    let bounded = stdout_path
        .metadata()
        .map(|m| m.len())
        .unwrap_or(u64::MAX)
        .saturating_add(stderr_path.metadata().map(|m| m.len()).unwrap_or(u64::MAX))
        <= 8 * 1024 * 1024;
    let output = format!(
        "Full merge-hook captures: {} {}\n{out}\n{err}\n{}",
        stdout_path.display(),
        stderr_path.display(),
        if bounded {
            ""
        } else {
            "Merge-hook output exceeded capture bound"
        }
    );
    let verdict = output
        .lines()
        .rev()
        .filter_map(|l| l.trim().strip_prefix("MACC_TASK_RESULT:"))
        .find_map(|v| v.trim().parse::<PerformerCompletionKind>().ok());
    let succeeded = status.success()
        && bounded
        && !timed_out
        && verdict.is_none_or(|v| {
            matches!(
                v,
                PerformerCompletionKind::SuccessWithChanges
                    | PerformerCompletionKind::SuccessWithoutChanges
                    | PerformerCompletionKind::AlreadySatisfied
            )
        });
    Ok(HookRunResult::Completed {
        output,
        timed_out,
        succeeded,
    })
}
fn terminate(child: &mut std::process::Child) {
    #[cfg(unix)]
    unsafe {
        libc::kill(-(child.id() as i32), libc::SIGTERM);
    }
    #[cfg(not(unix))]
    let _ = child.kill();
    let until = std::time::Instant::now() + std::time::Duration::from_secs(2);
    while std::time::Instant::now() < until {
        if child.try_wait().ok().flatten().is_some() {
            return;
        }
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
    #[cfg(unix)]
    unsafe {
        libc::kill(-(child.id() as i32), libc::SIGKILL);
    }
    let _ = child.kill();
    let _ = child.wait();
}

fn capture_file(root: &Path, stream: &str) -> Result<(std::fs::File, PathBuf)> {
    let dir = coordinator_log_dir(root);
    std::fs::create_dir_all(&dir).map_err(|e| MaccError::Validation(e.to_string()))?;
    let path = dir.join(format!("merge-hook-{}-{stream}.log", uuid::Uuid::new_v4()));
    let mut options = std::fs::OpenOptions::new();
    options.read(true).write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let file = options
        .open(&path)
        .map_err(|e| MaccError::Validation(e.to_string()))?;
    Ok((file, path))
}
