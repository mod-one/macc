//! Dedicated integration worktree used for all coordinator branch merges.
//!
//! # Why this exists
//!
//! The coordinator used to integrate task branches by running `git checkout
//! <base>` directly in the operator's primary working tree and merging there.
//! That had three operator-visible consequences:
//!
//! * it silently moved the operator's HEAD to the base branch mid-session;
//! * any uncommitted work made the pre-merge cleanliness check fail, so *every*
//!   merge blocked with a reason that never mentioned the dirty tree;
//! * a conflicted merge left `MERGE_HEAD` and conflict markers in the
//!   operator's checkout.
//!
//! Merges now happen in a private worktree kept at `<git-common-dir>/macc/
//! integration`, detached at the tip of the base branch. The git *common* dir
//! is shared by every worktree and is never scanned by `git status`, so the
//! integration checkout cannot dirty any working tree regardless of what the
//! project's `.gitignore` says.
//!
//! # Publishing
//!
//! A merge produces a commit on the integration worktree's detached HEAD; that
//! commit still has to reach `refs/heads/<base>`. Which mechanism is safe
//! depends on whether a working tree currently has the base branch checked out:
//!
//! * **Nobody has it checked out** — compare-and-swap the ref with
//!   `git update-ref <ref> <new> <old>`. The `<old>` argument makes this atomic:
//!   git rejects the update if the branch moved under us.
//! * **A working tree has it checked out** — fast-forward *that* worktree with
//!   `git merge --ff-only <new>`. This advances the ref and the checkout
//!   together. Moving the ref behind a live checkout's back is what we must not
//!   do: HEAD would jump forward while the files stayed behind, and every
//!   merged change would show up as an uncommitted deletion.
//!
//! In the second case git itself decides whether the operation is safe. It
//! fast-forwards when the operator's uncommitted changes don't collide with the
//! merge, and refuses — touching nothing — when they do. That is a strictly
//! finer-grained check than the old "is the tree clean at all" precondition, so
//! ordinary WIP no longer blocks unrelated merges.
//!
//! # Exclusion
//!
//! An advisory lock (`flock` on unix) serialises integration against other
//! `macc` processes. Unlike a lockfile guarded by `create_new`, an `flock` is
//! released by the kernel when the holder dies, so a killed coordinator cannot
//! wedge merges permanently.

use crate::{git, MaccError, Result};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

/// Directory (under the git common dir) holding all integration scratch state.
const INTEGRATION_DIR: &str = "macc";
/// Worktree directory name within [`INTEGRATION_DIR`].
const WORKTREE_NAME: &str = "integration";
/// Lock file name within [`INTEGRATION_DIR`].
const LOCK_NAME: &str = "integration.lock";
/// How long to wait for another process to finish integrating before giving up.
const LOCK_TIMEOUT: Duration = Duration::from_secs(120);
/// Delay between lock acquisition attempts.
const LOCK_RETRY_DELAY: Duration = Duration::from_millis(100);

/// Result of publishing the integration worktree's HEAD to the base branch.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PublishOutcome {
    /// The base branch now points at the integrated commit.
    Published { new_sha: String },
    /// The merge produced no new commit (base already contained the branch).
    UpToDate,
    /// A working tree has the base branch checked out and git refused to
    /// fast-forward it because the operator's uncommitted changes overlap the
    /// merged files. Nothing was modified.
    BlockedByCheckout {
        worktree: PathBuf,
        git_output: String,
    },
    /// The base branch moved while we were integrating, so the merge result is
    /// stale. The caller should retry from the new tip.
    BaseMoved { expected_sha: String },
}

/// An exclusive advisory lock over the integration worktree.
struct IntegrationLock {
    #[cfg(unix)]
    file: std::fs::File,
    #[cfg(not(unix))]
    _path: PathBuf,
}

impl IntegrationLock {
    #[cfg(unix)]
    fn acquire(path: &Path) -> Result<Self> {
        use std::os::unix::io::AsRawFd;

        let file = std::fs::OpenOptions::new()
            .create(true)
            .truncate(false)
            .write(true)
            .open(path)
            .map_err(|e| MaccError::Io {
                path: path.to_string_lossy().into(),
                action: "open integration lock file".into(),
                source: e,
            })?;

        let started = Instant::now();
        loop {
            // SAFETY: `file` owns a valid fd for the duration of the call.
            let rc = unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) };
            if rc == 0 {
                return Ok(Self { file });
            }
            let err = std::io::Error::last_os_error();
            let would_block = matches!(
                err.raw_os_error(),
                Some(code) if code == libc::EWOULDBLOCK || code == libc::EINTR
            );
            if !would_block {
                return Err(MaccError::Io {
                    path: path.to_string_lossy().into(),
                    action: "lock integration worktree".into(),
                    source: err,
                });
            }
            if started.elapsed() >= LOCK_TIMEOUT {
                return Err(MaccError::Git {
                    operation: "integration_lock".into(),
                    message: format!(
                        "timed out after {}s waiting for another macc process to finish integrating (lock: {})",
                        LOCK_TIMEOUT.as_secs(),
                        path.display()
                    ),
                });
            }
            std::thread::sleep(LOCK_RETRY_DELAY);
        }
    }

    /// Non-unix platforms have no portable advisory lock here. Integration is
    /// still serialised within a process by the coordinator's single merge
    /// worker; cross-process exclusion is best-effort.
    #[cfg(not(unix))]
    fn acquire(path: &Path) -> Result<Self> {
        if let Some(parent) = path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        let _ = std::fs::OpenOptions::new()
            .create(true)
            .truncate(false)
            .write(true)
            .open(path);
        Ok(Self {
            _path: path.to_path_buf(),
        })
    }
}

#[cfg(unix)]
impl Drop for IntegrationLock {
    fn drop(&mut self) {
        use std::os::unix::io::AsRawFd;
        // SAFETY: `file` is still open here; the fd is closed after this runs.
        unsafe {
            libc::flock(self.file.as_raw_fd(), libc::LOCK_UN);
        }
    }
}

/// A private worktree, detached at the base branch tip, in which the
/// coordinator performs merges without touching any operator checkout.
pub struct IntegrationWorktree {
    repo_root: PathBuf,
    path: PathBuf,
    base: String,
    base_sha: String,
    _lock: IntegrationLock,
}

impl IntegrationWorktree {
    /// Take the integration lock and prepare a worktree detached at the current
    /// tip of `base`.
    ///
    /// Any state left by a previous merge (conflicts, stray files, an
    /// in-progress merge) is discarded — the worktree is scratch space owned
    /// entirely by the coordinator.
    pub fn acquire(repo_root: &Path, base: &str) -> Result<Self> {
        let common_dir = git::git_common_dir(repo_root)?;
        let integration_dir = common_dir.join(INTEGRATION_DIR);
        std::fs::create_dir_all(&integration_dir).map_err(|e| MaccError::Io {
            path: integration_dir.to_string_lossy().into(),
            action: "create integration directory".into(),
            source: e,
        })?;

        let lock = IntegrationLock::acquire(&integration_dir.join(LOCK_NAME))?;
        let path = integration_dir.join(WORKTREE_NAME);

        let base_sha = git::resolve_ref(repo_root, base).ok_or_else(|| MaccError::Git {
            operation: "resolve_base".into(),
            message: format!("base branch '{}' could not be resolved", base),
        })?;

        // Drop registrations for worktree directories that no longer exist, so
        // a manually deleted integration dir can be recreated.
        let _ = git::worktree_prune(repo_root);

        if path.join(".git").exists() {
            // Reuse: discard whatever the previous merge left behind.
            let _ = git::merge_abort(&path);
            git::checkout_detach_force(&path, &base_sha)?;
            let _ = git::clean_untracked(&path);
        } else {
            if path.exists() {
                std::fs::remove_dir_all(&path).map_err(|e| MaccError::Io {
                    path: path.to_string_lossy().into(),
                    action: "remove stale integration worktree directory".into(),
                    source: e,
                })?;
            }
            git::worktree_add_detached(repo_root, &path, &base_sha)?;
        }

        Ok(Self {
            repo_root: repo_root.to_path_buf(),
            path,
            base: base.to_string(),
            base_sha,
            _lock: lock,
        })
    }

    /// Path of the integration worktree. All merge commands must run here.
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// The base branch being integrated into.
    pub fn base(&self) -> &str {
        &self.base
    }

    /// The base branch tip observed when the worktree was acquired.
    pub fn base_sha(&self) -> &str {
        &self.base_sha
    }

    /// Publish the integration worktree's current HEAD to the base branch.
    ///
    /// See the module docs for why the mechanism depends on whether a working
    /// tree holds the base branch.
    pub fn publish(&self) -> Result<PublishOutcome> {
        let new_sha = git::head_commit(&self.path)?;
        if new_sha == self.base_sha {
            return Ok(PublishOutcome::UpToDate);
        }

        let holders = git::worktrees_for_branch(&self.repo_root, &self.base)?;
        let Some(holder) = holders.into_iter().next() else {
            // Nobody has the branch checked out: atomic compare-and-swap.
            if git::update_branch_ref_checked(
                &self.repo_root,
                &self.base,
                &new_sha,
                &self.base_sha,
            )? {
                return Ok(PublishOutcome::Published { new_sha });
            }
            return Ok(PublishOutcome::BaseMoved {
                expected_sha: self.base_sha.clone(),
            });
        };

        // The branch is checked out somewhere. If it already moved past the tip
        // we integrated from, our result is stale — retry rather than merge a
        // divergent history into the operator's checkout.
        let holder_head = git::head_commit(&holder)?;
        if holder_head != self.base_sha {
            return Ok(PublishOutcome::BaseMoved {
                expected_sha: self.base_sha.clone(),
            });
        }

        // Fast-forward the holding worktree. Git advances the ref and the files
        // together, and refuses outright if that would clobber local changes.
        let output = git::run_git_output_mapped(
            &holder,
            &["merge", "--ff-only", &new_sha],
            "fast-forward base worktree to integrated commit",
        )?;
        if output.status.success() {
            return Ok(PublishOutcome::Published { new_sha });
        }
        Ok(PublishOutcome::BlockedByCheckout {
            worktree: holder,
            git_output: format!(
                "{}\n{}",
                String::from_utf8_lossy(&output.stdout).trim(),
                String::from_utf8_lossy(&output.stderr).trim()
            )
            .trim()
            .to_string(),
        })
    }

    /// Abort any in-progress merge and return the worktree to the base tip.
    pub fn reset(&self) -> Result<()> {
        let _ = git::merge_abort(&self.path);
        git::checkout_detach_force(&self.path, &self.base_sha)?;
        let _ = git::clean_untracked(&self.path);
        Ok(())
    }

    /// Whether a merge is currently in progress in the integration worktree.
    pub fn merge_in_progress(&self) -> bool {
        git::rev_parse_verify(&self.path, "MERGE_HEAD").unwrap_or(false)
    }
}

impl PublishOutcome {
    /// Render a `failure:local_merge`-style detail suffix for the outcomes that
    /// represent a failure to publish.
    pub fn failure_detail(&self) -> Option<String> {
        match self {
            PublishOutcome::Published { .. } | PublishOutcome::UpToDate => None,
            PublishOutcome::BlockedByCheckout {
                worktree,
                git_output,
            } => Some(format!(
                "step=publish reason=base_checked_out_dirty worktree=\"{}\" git_output=\"{}\"",
                worktree.display(),
                git_output.replace('"', "'").replace('\n', " ")
            )),
            PublishOutcome::BaseMoved { expected_sha } => Some(format!(
                "step=publish reason=base_moved expected={}",
                expected_sha
            )),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::process::Command;

    fn run_git(dir: &Path, args: &[&str]) {
        let out = Command::new("git")
            .args(args)
            .current_dir(dir)
            .output()
            .expect("run git");
        assert!(
            out.status.success(),
            "git {:?} failed: {}",
            args,
            String::from_utf8_lossy(&out.stderr)
        );
    }

    fn make_repo() -> tempfile::TempDir {
        let dir = tempfile::tempdir().expect("tempdir");
        let repo = dir.path();
        run_git(repo, &["init", "-q", "-b", "main", "."]);
        run_git(repo, &["config", "user.email", "test@example.com"]);
        run_git(repo, &["config", "user.name", "Test"]);
        std::fs::write(repo.join("base.txt"), "base\n").expect("write");
        run_git(repo, &["add", "."]);
        run_git(repo, &["commit", "-qm", "init"]);
        dir
    }

    /// Create `branch` off main adding `file`, and return to main.
    fn add_task_branch(repo: &Path, branch: &str, file: &str, contents: &str) {
        run_git(repo, &["checkout", "-q", "-b", branch]);
        std::fs::write(repo.join(file), contents).expect("write");
        run_git(repo, &["add", "."]);
        run_git(repo, &["commit", "-qm", "task work"]);
        run_git(repo, &["checkout", "-q", "main"]);
    }

    #[test]
    fn integration_worktree_lives_outside_every_checkout() {
        let dir = make_repo();
        let repo = dir.path();
        let wt = IntegrationWorktree::acquire(repo, "main").expect("acquire");

        assert!(wt.path().join(".git").exists(), "worktree should exist");
        assert!(
            git::status_porcelain(repo)
                .expect("status")
                .trim()
                .is_empty(),
            "integration worktree must not dirty the primary checkout"
        );
    }

    #[test]
    fn merge_publishes_into_checked_out_base_and_leaves_head_on_base() {
        let dir = make_repo();
        let repo = dir.path();
        add_task_branch(repo, "task/x", "work.txt", "work\n");

        let wt = IntegrationWorktree::acquire(repo, "main").expect("acquire");
        run_git(wt.path(), &["merge", "--no-ff", "-m", "merge", "task/x"]);
        let outcome = wt.publish().expect("publish");

        assert!(matches!(outcome, PublishOutcome::Published { .. }));
        assert_eq!(
            git::current_branch(repo).expect("branch"),
            "main",
            "operator checkout must stay on its own branch"
        );
        assert!(
            repo.join("work.txt").exists(),
            "base worktree fast-forwarded"
        );
    }

    #[test]
    fn merge_publishes_when_base_is_not_checked_out_anywhere() {
        let dir = make_repo();
        let repo = dir.path();
        add_task_branch(repo, "task/x", "work.txt", "work\n");
        // Operator moves off main entirely.
        run_git(repo, &["checkout", "-q", "-b", "operator/side"]);

        let wt = IntegrationWorktree::acquire(repo, "main").expect("acquire");
        run_git(wt.path(), &["merge", "--no-ff", "-m", "merge", "task/x"]);
        let outcome = wt.publish().expect("publish");

        match outcome {
            PublishOutcome::Published { new_sha } => {
                assert_eq!(git::resolve_ref(repo, "main").expect("main"), new_sha);
            }
            other => panic!("expected Published, got {:?}", other),
        }
        assert_eq!(
            git::current_branch(repo).expect("branch"),
            "operator/side",
            "operator checkout must be untouched"
        );
        assert!(
            !repo.join("work.txt").exists(),
            "operator's unrelated branch must not gain merged files"
        );
    }

    #[test]
    fn uncommitted_operator_work_does_not_block_unrelated_merges() {
        let dir = make_repo();
        let repo = dir.path();
        add_task_branch(repo, "task/x", "work.txt", "work\n");
        // Operator has WIP on a file the merge never touches.
        std::fs::write(repo.join("base.txt"), "operator wip\n").expect("write");

        let wt = IntegrationWorktree::acquire(repo, "main").expect("acquire");
        run_git(wt.path(), &["merge", "--no-ff", "-m", "merge", "task/x"]);
        let outcome = wt.publish().expect("publish");

        assert!(
            matches!(outcome, PublishOutcome::Published { .. }),
            "unrelated WIP must not block the merge, got {:?}",
            outcome
        );
        assert_eq!(
            std::fs::read_to_string(repo.join("base.txt")).expect("read"),
            "operator wip\n",
            "operator WIP must be preserved"
        );
    }

    #[test]
    fn overlapping_operator_work_blocks_publish_without_mutating_anything() {
        let dir = make_repo();
        let repo = dir.path();
        // Task branch edits the same file the operator is editing.
        add_task_branch(repo, "task/x", "base.txt", "task side\n");
        std::fs::write(repo.join("base.txt"), "operator wip\n").expect("write");
        let main_before = git::resolve_ref(repo, "main").expect("main");

        let wt = IntegrationWorktree::acquire(repo, "main").expect("acquire");
        run_git(wt.path(), &["merge", "--no-ff", "-m", "merge", "task/x"]);
        let outcome = wt.publish().expect("publish");

        match outcome {
            PublishOutcome::BlockedByCheckout { .. } => {}
            other => panic!("expected BlockedByCheckout, got {:?}", other),
        }
        assert_eq!(
            git::resolve_ref(repo, "main").expect("main"),
            main_before,
            "base ref must not move when publish is blocked"
        );
        assert_eq!(
            std::fs::read_to_string(repo.join("base.txt")).expect("read"),
            "operator wip\n",
            "operator WIP must survive a blocked publish"
        );
        assert!(
            outcome
                .failure_detail()
                .unwrap()
                .contains("base_checked_out_dirty"),
            "failure detail should name the real cause"
        );
    }

    #[test]
    fn publish_detects_base_moving_underneath() {
        let dir = make_repo();
        let repo = dir.path();
        add_task_branch(repo, "task/x", "work.txt", "work\n");
        run_git(repo, &["checkout", "-q", "-b", "operator/side"]);

        let wt = IntegrationWorktree::acquire(repo, "main").expect("acquire");
        run_git(wt.path(), &["merge", "--no-ff", "-m", "merge", "task/x"]);

        // Someone else advances main after we integrated.
        add_task_branch(repo, "task/y", "other.txt", "other\n");
        run_git(repo, &["checkout", "-q", "main"]);
        run_git(repo, &["merge", "--no-ff", "-m", "other merge", "task/y"]);
        run_git(repo, &["checkout", "-q", "operator/side"]);

        assert!(matches!(
            wt.publish().expect("publish"),
            PublishOutcome::BaseMoved { .. }
        ));
    }

    #[test]
    fn acquire_discards_state_left_by_a_previous_conflicted_merge() {
        let dir = make_repo();
        let repo = dir.path();
        add_task_branch(repo, "task/x", "base.txt", "task side\n");
        run_git(repo, &["checkout", "-q", "main"]);
        std::fs::write(repo.join("base.txt"), "main side\n").expect("write");
        run_git(repo, &["add", "."]);
        run_git(repo, &["commit", "-qm", "main side"]);

        {
            let wt = IntegrationWorktree::acquire(repo, "main").expect("acquire");
            // Leave a conflicted merge and a stray file behind.
            let _ = Command::new("git")
                .args(["merge", "--no-ff", "task/x"])
                .current_dir(wt.path())
                .output()
                .expect("merge");
            assert!(wt.merge_in_progress(), "expected a conflicted merge");
            std::fs::write(wt.path().join("stray.txt"), "junk").expect("write");
        }

        let wt = IntegrationWorktree::acquire(repo, "main").expect("re-acquire");
        assert!(!wt.merge_in_progress(), "stale merge must be aborted");
        assert!(
            !wt.path().join("stray.txt").exists(),
            "stray untracked files must be cleaned"
        );
        assert!(git::status_porcelain(wt.path())
            .expect("status")
            .trim()
            .is_empty());
    }

    #[test]
    fn up_to_date_merge_reports_no_publish() {
        let dir = make_repo();
        let repo = dir.path();
        let wt = IntegrationWorktree::acquire(repo, "main").expect("acquire");
        assert_eq!(wt.publish().expect("publish"), PublishOutcome::UpToDate);
    }
}
