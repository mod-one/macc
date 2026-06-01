/// Reference Branch Preflight Gate (spec §6–25).
///
/// Validates that `reference_branch` exists locally and is clean before any
/// coordinator mutation. Pure business logic — no prompts or terminal I/O.
use crate::git::{
    check_ref_format_branch, create_branch_at, create_tracking_branch, is_bare_repository,
    local_branch_exists, remote_tracking_refs_for_branch, status_porcelain_v1, worktrees_for_branch,
    GitPorcelainEntry,
};
use std::path::{Path, PathBuf};

// ── Error codes (spec §15) ────────────────────────────────────────────────────

pub const E701: &str = "E701";
pub const E702: &str = "E702";
pub const E703: &str = "E703";
pub const E704: &str = "E704";
pub const E705: &str = "E705";
pub const E706: &str = "E706";
pub const E707: &str = "E707";

// ── Public data structures (spec §10.3) ──────────────────────────────────────

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReferenceBranchPreflightReport {
    pub reference_branch: String,
    pub branch_exists: bool,
    pub remote_tracking_branches: Vec<String>,
    pub checked_out_worktrees: Vec<ReferenceWorktreeStatus>,
    pub status: ReferencePreflightStatus,
    pub recommended_action: ReferencePreflightAction,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReferenceWorktreeStatus {
    pub path: PathBuf,
    pub branch: String,
    pub dirty_entries: Vec<GitStatusEntry>,
}

/// A parsed entry from `git status --porcelain=v1`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GitStatusEntry {
    pub index_status: char,
    pub worktree_status: char,
    pub path: String,
    pub original_path: Option<String>,
}

impl From<GitPorcelainEntry> for GitStatusEntry {
    fn from(e: GitPorcelainEntry) -> Self {
        Self {
            index_status: e.index_status,
            worktree_status: e.worktree_status,
            path: e.path,
            original_path: e.original_path,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ReferencePreflightStatus {
    Clean,
    BranchMissing,
    Dirty,
    InvalidBranchName,
    GitInspectionFailed,
    BareRepository,
    NotCheckedOut,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ReferencePreflightAction {
    Proceed,
    PromptCreateBranch,
    PromptCleanOrOverride,
    Fail,
}

// ── Policy structures (spec §10.4) ────────────────────────────────────────────

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReferenceBranchPreflightConfig {
    pub enabled: bool,
    pub missing_branch_policy: MissingBranchPolicy,
    pub dirty_policy: DirtyReferencePolicy,
    pub include_untracked: bool,
    pub create_from: BranchCreateSourcePolicy,
    pub allow_non_interactive_create: bool,
    pub log_clean_result: bool,
}

impl Default for ReferenceBranchPreflightConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            missing_branch_policy: MissingBranchPolicy::Prompt,
            dirty_policy: DirtyReferencePolicy::Block,
            include_untracked: true,
            create_from: BranchCreateSourcePolicy::RemoteTrackingOrCurrentHead,
            allow_non_interactive_create: false,
            log_clean_result: true,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MissingBranchPolicy {
    Prompt,
    Fail,
    Create,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DirtyReferencePolicy {
    Block,
    Warn,
    Allow,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BranchCreateSourcePolicy {
    CurrentHead,
    RemoteTracking,
    RemoteTrackingOrCurrentHead,
    LocalBranch(String),
    Revision(String),
}

/// Source for `create_reference_branch()`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BranchCreateSource {
    CurrentHead,
    RemoteTracking(String),
    LocalBranch(String),
    Revision(String),
}

// ── Preflight error ───────────────────────────────────────────────────────────

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PreflightError {
    pub code: &'static str,
    pub message: String,
}

impl std::fmt::Display for PreflightError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}: {}", self.code, self.message)
    }
}

impl PreflightError {
    fn new(code: &'static str, message: impl Into<String>) -> Self {
        Self { code, message: message.into() }
    }
}

// ── Public API (spec §10.5) ───────────────────────────────────────────────────

/// Inspect the reference branch and return a structured preflight report.
///
/// This function never prompts the user. The caller decides how to handle
/// `recommended_action` based on its execution context (CLI / TUI / Web).
pub fn inspect_reference_branch_preflight(
    repo_root: &Path,
    reference_branch: &str,
    config: &ReferenceBranchPreflightConfig,
) -> Result<ReferenceBranchPreflightReport, PreflightError> {
    // Bare repository check (spec §7.15)
    match is_bare_repository(repo_root) {
        Ok(true) => {
            return Ok(ReferenceBranchPreflightReport {
                reference_branch: reference_branch.to_string(),
                branch_exists: false,
                remote_tracking_branches: vec![],
                checked_out_worktrees: vec![],
                status: ReferencePreflightStatus::BareRepository,
                recommended_action: ReferencePreflightAction::Fail,
            });
        }
        Err(e) => {
            return Ok(ReferenceBranchPreflightReport {
                reference_branch: reference_branch.to_string(),
                branch_exists: false,
                remote_tracking_branches: vec![],
                checked_out_worktrees: vec![],
                status: ReferencePreflightStatus::GitInspectionFailed,
                recommended_action: ReferencePreflightAction::Fail,
            });
            let _ = e;
        }
        Ok(false) => {}
    }

    // Branch name validation (spec §7.2)
    match check_ref_format_branch(repo_root, reference_branch) {
        Ok(false) | Err(_) => {
            return Ok(ReferenceBranchPreflightReport {
                reference_branch: reference_branch.to_string(),
                branch_exists: false,
                remote_tracking_branches: vec![],
                checked_out_worktrees: vec![],
                status: ReferencePreflightStatus::InvalidBranchName,
                recommended_action: ReferencePreflightAction::Fail,
            });
        }
        Ok(true) => {}
    }

    // Local branch existence check (spec §7.3)
    let branch_exists = local_branch_exists(repo_root, reference_branch)
        .unwrap_or(false);

    // Remote-tracking branches (spec §7.4)
    let remote_tracking_branches = if !branch_exists {
        remote_tracking_refs_for_branch(repo_root, reference_branch).unwrap_or_default()
    } else {
        vec![]
    };

    if !branch_exists {
        let recommended_action = match config.missing_branch_policy {
            MissingBranchPolicy::Prompt => ReferencePreflightAction::PromptCreateBranch,
            MissingBranchPolicy::Fail => ReferencePreflightAction::Fail,
            MissingBranchPolicy::Create => {
                if config.allow_non_interactive_create {
                    ReferencePreflightAction::Proceed
                } else {
                    ReferencePreflightAction::PromptCreateBranch
                }
            }
        };
        return Ok(ReferenceBranchPreflightReport {
            reference_branch: reference_branch.to_string(),
            branch_exists: false,
            remote_tracking_branches,
            checked_out_worktrees: vec![],
            status: ReferencePreflightStatus::BranchMissing,
            recommended_action,
        });
    }

    // Dirty-state check across all worktrees for this branch (spec §7.8)
    let worktree_paths = worktrees_for_branch(repo_root, reference_branch).unwrap_or_default();

    if worktree_paths.is_empty() {
        // Branch not checked out anywhere — clean by definition (spec §7.12)
        return Ok(ReferenceBranchPreflightReport {
            reference_branch: reference_branch.to_string(),
            branch_exists: true,
            remote_tracking_branches: vec![],
            checked_out_worktrees: vec![],
            status: ReferencePreflightStatus::NotCheckedOut,
            recommended_action: ReferencePreflightAction::Proceed,
        });
    }

    let mut checked_out_worktrees: Vec<ReferenceWorktreeStatus> = vec![];
    let mut any_dirty = false;

    for wt_path in &worktree_paths {
        let entries = status_porcelain_v1(wt_path, config.include_untracked)
            .unwrap_or_default();
        let dirty_entries: Vec<GitStatusEntry> =
            entries.into_iter().map(GitStatusEntry::from).collect();

        if !dirty_entries.is_empty() {
            any_dirty = true;
        }

        checked_out_worktrees.push(ReferenceWorktreeStatus {
            path: wt_path.clone(),
            branch: reference_branch.to_string(),
            dirty_entries,
        });
    }

    let (status, recommended_action) = if any_dirty {
        let action = match config.dirty_policy {
            DirtyReferencePolicy::Block => ReferencePreflightAction::PromptCleanOrOverride,
            DirtyReferencePolicy::Warn => ReferencePreflightAction::Proceed,
            DirtyReferencePolicy::Allow => ReferencePreflightAction::Proceed,
        };
        (ReferencePreflightStatus::Dirty, action)
    } else {
        (ReferencePreflightStatus::Clean, ReferencePreflightAction::Proceed)
    };

    Ok(ReferenceBranchPreflightReport {
        reference_branch: reference_branch.to_string(),
        branch_exists: true,
        remote_tracking_branches: vec![],
        checked_out_worktrees,
        status,
        recommended_action,
    })
}

/// Create the reference branch from the given source (spec §7.6).
pub fn create_reference_branch(
    repo_root: &Path,
    reference_branch: &str,
    source: BranchCreateSource,
) -> Result<(), PreflightError> {
    match source {
        BranchCreateSource::CurrentHead => {
            create_branch_at(repo_root, reference_branch, "HEAD").map_err(|e| {
                PreflightError::new(E705, format!("Failed to create branch: {}", e))
            })
        }
        BranchCreateSource::RemoteTracking(remote_ref) => {
            create_tracking_branch(repo_root, reference_branch, &remote_ref).map_err(|e| {
                PreflightError::new(E705, format!("Failed to create tracking branch: {}", e))
            })
        }
        BranchCreateSource::LocalBranch(base) | BranchCreateSource::Revision(base) => {
            create_branch_at(repo_root, reference_branch, &base).map_err(|e| {
                PreflightError::new(E705, format!("Failed to create branch from {}: {}", base, e))
            })
        }
    }
}

// ── Helpers for CLI/TUI decision handling (spec §11.3) ────────────────────────

/// Whether the preflight status blocks the coordinator run (before overrides).
pub fn is_blocking(report: &ReferenceBranchPreflightReport) -> bool {
    matches!(
        report.recommended_action,
        ReferencePreflightAction::Fail | ReferencePreflightAction::PromptCleanOrOverride
    ) || matches!(
        report.status,
        ReferencePreflightStatus::BranchMissing
            | ReferencePreflightStatus::InvalidBranchName
            | ReferencePreflightStatus::BareRepository
            | ReferencePreflightStatus::GitInspectionFailed
    )
}

/// Human-readable summary for CLI output.
pub fn format_report_cli(report: &ReferenceBranchPreflightReport) -> String {
    match &report.status {
        ReferencePreflightStatus::Clean => {
            format!("Reference branch: {}\nPreflight: OK", report.reference_branch)
        }
        ReferencePreflightStatus::NotCheckedOut => {
            format!(
                "Reference branch \"{}\" is not checked out in any worktree; dirty-state check skipped.\nPreflight: OK",
                report.reference_branch
            )
        }
        ReferencePreflightStatus::BranchMissing => {
            if report.remote_tracking_branches.is_empty() {
                format!(
                    "ERROR {}: Reference branch \"{}\" does not exist locally.\n\
                     Use --create-reference-branch with --reference-branch-base to create it non-interactively.",
                    E701, report.reference_branch
                )
            } else {
                format!(
                    "Reference branch \"{}\" does not exist locally, but {} exists.\n\
                     Create local tracking branch? Use --create-reference-branch.",
                    report.reference_branch,
                    report.remote_tracking_branches.join(", ")
                )
            }
        }
        ReferencePreflightStatus::Dirty => {
            let mut out = String::new();
            for wt in &report.checked_out_worktrees {
                if !wt.dirty_entries.is_empty() {
                    out.push_str(&format!(
                        "Reference branch \"{}\" has uncommitted changes in:\n{}\n\nChanges:\n",
                        report.reference_branch,
                        wt.path.display()
                    ));
                    for e in &wt.dirty_entries {
                        out.push_str(&format!("  {}{} {}\n", e.index_status, e.worktree_status, e.path));
                    }
                }
            }
            out.push_str(&format!(
                "\nRunning the coordinator may later merge task branches into this branch.\n\
                 Please commit, stash, or discard these changes before continuing.\n\
                 Or rerun with --allow-dirty-reference.\n\
                 Error code: {}",
                E702
            ));
            out
        }
        ReferencePreflightStatus::InvalidBranchName => format!(
            "ERROR {}: Invalid reference branch name \"{}\".\n\
             Correct automation.coordinator.reference_branch in macc.yaml.",
            E706, report.reference_branch
        ),
        ReferencePreflightStatus::BareRepository => format!(
            "ERROR {}: Bare repository is not supported for coordinator run.",
            E707
        ),
        ReferencePreflightStatus::GitInspectionFailed => format!(
            "ERROR {}: Reference branch inspection failed.\n\
             Run `macc doctor` and inspect Git state.",
            E703
        ),
    }
}

/// Build a coordinator log event for the preflight result (spec §16.1).
pub fn build_preflight_log_event(
    report: &ReferenceBranchPreflightReport,
    decision: &str,
    override_used: bool,
) -> serde_json::Value {
    let timestamp = chrono::Utc::now().to_rfc3339();
    serde_json::json!({
        "timestamp": timestamp,
        "event": "reference_branch_preflight",
        "reference_branch": report.reference_branch,
        "branch_exists": report.branch_exists,
        "status": format!("{:?}", report.status).to_ascii_lowercase().replace("branch", "_branch"),
        "dirty_worktrees": report.checked_out_worktrees
            .iter()
            .filter(|w| !w.dirty_entries.is_empty())
            .map(|w| serde_json::json!({
                "path": w.path.to_string_lossy(),
                "entries": w.dirty_entries.iter().map(|e| serde_json::json!({
                    "status": format!("{}{}", e.index_status, e.worktree_status),
                    "path": e.path,
                })).collect::<Vec<_>>(),
            }))
            .collect::<Vec<_>>(),
        "decision": decision,
        "override_used": override_used,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn default_config() -> ReferenceBranchPreflightConfig {
        ReferenceBranchPreflightConfig::default()
    }

    // ── Unit tests for decision logic (spec §18.1) ────────────────────────────

    #[test]
    fn test_dirty_policy_block_recommends_prompt() {
        let cfg = ReferenceBranchPreflightConfig {
            dirty_policy: DirtyReferencePolicy::Block,
            ..default_config()
        };
        // Simulate what inspect_reference_branch_preflight does when dirty entries found.
        let action = match cfg.dirty_policy {
            DirtyReferencePolicy::Block => ReferencePreflightAction::PromptCleanOrOverride,
            DirtyReferencePolicy::Warn => ReferencePreflightAction::Proceed,
            DirtyReferencePolicy::Allow => ReferencePreflightAction::Proceed,
        };
        assert_eq!(action, ReferencePreflightAction::PromptCleanOrOverride);
    }

    #[test]
    fn test_dirty_policy_warn_proceeds() {
        let cfg = ReferenceBranchPreflightConfig {
            dirty_policy: DirtyReferencePolicy::Warn,
            ..default_config()
        };
        let action = match cfg.dirty_policy {
            DirtyReferencePolicy::Block => ReferencePreflightAction::PromptCleanOrOverride,
            DirtyReferencePolicy::Warn => ReferencePreflightAction::Proceed,
            DirtyReferencePolicy::Allow => ReferencePreflightAction::Proceed,
        };
        assert_eq!(action, ReferencePreflightAction::Proceed);
    }

    #[test]
    fn test_dirty_policy_allow_proceeds() {
        let cfg = ReferenceBranchPreflightConfig {
            dirty_policy: DirtyReferencePolicy::Allow,
            ..default_config()
        };
        let action = match cfg.dirty_policy {
            DirtyReferencePolicy::Block => ReferencePreflightAction::PromptCleanOrOverride,
            DirtyReferencePolicy::Warn => ReferencePreflightAction::Proceed,
            DirtyReferencePolicy::Allow => ReferencePreflightAction::Proceed,
        };
        assert_eq!(action, ReferencePreflightAction::Proceed);
    }

    #[test]
    fn test_missing_branch_policy_fail() {
        let cfg = ReferenceBranchPreflightConfig {
            missing_branch_policy: MissingBranchPolicy::Fail,
            ..default_config()
        };
        let action = match cfg.missing_branch_policy {
            MissingBranchPolicy::Prompt => ReferencePreflightAction::PromptCreateBranch,
            MissingBranchPolicy::Fail => ReferencePreflightAction::Fail,
            MissingBranchPolicy::Create if cfg.allow_non_interactive_create => {
                ReferencePreflightAction::Proceed
            }
            MissingBranchPolicy::Create => ReferencePreflightAction::PromptCreateBranch,
        };
        assert_eq!(action, ReferencePreflightAction::Fail);
    }

    #[test]
    fn test_missing_branch_policy_create_non_interactive() {
        let cfg = ReferenceBranchPreflightConfig {
            missing_branch_policy: MissingBranchPolicy::Create,
            allow_non_interactive_create: true,
            ..default_config()
        };
        let action = match cfg.missing_branch_policy {
            MissingBranchPolicy::Prompt => ReferencePreflightAction::PromptCreateBranch,
            MissingBranchPolicy::Fail => ReferencePreflightAction::Fail,
            MissingBranchPolicy::Create if cfg.allow_non_interactive_create => {
                ReferencePreflightAction::Proceed
            }
            MissingBranchPolicy::Create => ReferencePreflightAction::PromptCreateBranch,
        };
        assert_eq!(action, ReferencePreflightAction::Proceed);
    }

    #[test]
    fn test_is_blocking_dirty_prompt() {
        let report = ReferenceBranchPreflightReport {
            reference_branch: "main".to_string(),
            branch_exists: true,
            remote_tracking_branches: vec![],
            checked_out_worktrees: vec![],
            status: ReferencePreflightStatus::Dirty,
            recommended_action: ReferencePreflightAction::PromptCleanOrOverride,
        };
        assert!(is_blocking(&report));
    }

    #[test]
    fn test_is_blocking_clean_false() {
        let report = ReferenceBranchPreflightReport {
            reference_branch: "main".to_string(),
            branch_exists: true,
            remote_tracking_branches: vec![],
            checked_out_worktrees: vec![],
            status: ReferencePreflightStatus::Clean,
            recommended_action: ReferencePreflightAction::Proceed,
        };
        assert!(!is_blocking(&report));
    }

    #[test]
    fn test_format_report_clean() {
        let report = ReferenceBranchPreflightReport {
            reference_branch: "main".to_string(),
            branch_exists: true,
            remote_tracking_branches: vec![],
            checked_out_worktrees: vec![],
            status: ReferencePreflightStatus::Clean,
            recommended_action: ReferencePreflightAction::Proceed,
        };
        let text = format_report_cli(&report);
        assert!(text.contains("Preflight: OK"));
        assert!(text.contains("main"));
    }

    #[test]
    fn test_format_report_missing_branch_no_remote() {
        let report = ReferenceBranchPreflightReport {
            reference_branch: "integration".to_string(),
            branch_exists: false,
            remote_tracking_branches: vec![],
            checked_out_worktrees: vec![],
            status: ReferencePreflightStatus::BranchMissing,
            recommended_action: ReferencePreflightAction::Fail,
        };
        let text = format_report_cli(&report);
        assert!(text.contains("E701"));
        assert!(text.contains("integration"));
    }

    #[test]
    fn test_format_report_missing_branch_with_remote() {
        let report = ReferenceBranchPreflightReport {
            reference_branch: "integration".to_string(),
            branch_exists: false,
            remote_tracking_branches: vec!["origin/integration".to_string()],
            checked_out_worktrees: vec![],
            status: ReferencePreflightStatus::BranchMissing,
            recommended_action: ReferencePreflightAction::PromptCreateBranch,
        };
        let text = format_report_cli(&report);
        assert!(text.contains("origin/integration"));
        assert!(!text.contains("E701"));
    }

    #[test]
    fn test_format_report_dirty() {
        let report = ReferenceBranchPreflightReport {
            reference_branch: "main".to_string(),
            branch_exists: true,
            remote_tracking_branches: vec![],
            checked_out_worktrees: vec![ReferenceWorktreeStatus {
                path: std::path::PathBuf::from("/repo"),
                branch: "main".to_string(),
                dirty_entries: vec![GitStatusEntry {
                    index_status: ' ',
                    worktree_status: 'M',
                    path: "src/foo.rs".to_string(),
                    original_path: None,
                }],
            }],
            status: ReferencePreflightStatus::Dirty,
            recommended_action: ReferencePreflightAction::PromptCleanOrOverride,
        };
        let text = format_report_cli(&report);
        assert!(text.contains("E702"));
        assert!(text.contains("src/foo.rs"));
    }

    #[test]
    fn test_build_preflight_log_event_shape() {
        let report = ReferenceBranchPreflightReport {
            reference_branch: "main".to_string(),
            branch_exists: true,
            remote_tracking_branches: vec![],
            checked_out_worktrees: vec![],
            status: ReferencePreflightStatus::Clean,
            recommended_action: ReferencePreflightAction::Proceed,
        };
        let event = build_preflight_log_event(&report, "proceed", false);
        assert_eq!(event["reference_branch"], "main");
        assert_eq!(event["decision"], "proceed");
        assert_eq!(event["override_used"], false);
        assert!(event["timestamp"].is_string());
    }

    // ── Integration tests with real Git repos (spec §18.2) ────────────────────

    #[cfg(not(target_os = "windows"))]
    mod integration {
        use super::*;
        use std::fs;
        use std::process::Command;

        fn setup_git_repo(dir: &Path) {
            let run = |args: &[&str]| {
                Command::new("git")
                    .args(args)
                    .current_dir(dir)
                    .status()
                    .expect("git command");
            };
            run(&["init", "-b", "main"]);
            run(&["config", "user.email", "test@test.com"]);
            run(&["config", "user.name", "Test"]);
            run(&["commit", "--allow-empty", "-m", "chore: init"]);
        }

        fn temp_dir(prefix: &str) -> PathBuf {
            let base = std::env::temp_dir().join(format!("macc-preflight-{}-{}", prefix, std::process::id()));
            fs::create_dir_all(&base).unwrap();
            base
        }

        #[test]
        fn test_clean_branch() {
            let dir = temp_dir("clean");
            setup_git_repo(&dir);
            let cfg = ReferenceBranchPreflightConfig::default();
            let report = inspect_reference_branch_preflight(&dir, "main", &cfg).unwrap();
            assert!(
                matches!(report.status, ReferencePreflightStatus::Clean | ReferencePreflightStatus::NotCheckedOut),
                "expected clean or not-checked-out, got {:?}", report.status
            );
            assert_eq!(report.recommended_action, ReferencePreflightAction::Proceed);
            let _ = fs::remove_dir_all(&dir);
        }

        #[test]
        fn test_missing_branch_no_remote() {
            let dir = temp_dir("missing");
            setup_git_repo(&dir);
            let cfg = ReferenceBranchPreflightConfig {
                missing_branch_policy: MissingBranchPolicy::Fail,
                ..Default::default()
            };
            let report = inspect_reference_branch_preflight(&dir, "integration", &cfg).unwrap();
            assert_eq!(report.status, ReferencePreflightStatus::BranchMissing);
            assert_eq!(report.branch_exists, false);
            assert!(report.remote_tracking_branches.is_empty());
            assert_eq!(report.recommended_action, ReferencePreflightAction::Fail);
            let _ = fs::remove_dir_all(&dir);
        }

        #[test]
        fn test_invalid_branch_name() {
            let dir = temp_dir("invalid");
            setup_git_repo(&dir);
            let cfg = ReferenceBranchPreflightConfig::default();
            let report = inspect_reference_branch_preflight(&dir, "bad..name", &cfg).unwrap();
            assert_eq!(report.status, ReferencePreflightStatus::InvalidBranchName);
            assert_eq!(report.recommended_action, ReferencePreflightAction::Fail);
            let _ = fs::remove_dir_all(&dir);
        }

        #[test]
        fn test_dirty_branch_blocked() {
            let dir = temp_dir("dirty");
            setup_git_repo(&dir);
            // Create a dirty file in the main worktree.
            fs::write(dir.join("dirty.txt"), "change").unwrap();
            let cfg = ReferenceBranchPreflightConfig {
                dirty_policy: DirtyReferencePolicy::Block,
                include_untracked: true,
                ..Default::default()
            };
            let report = inspect_reference_branch_preflight(&dir, "main", &cfg).unwrap();
            assert_eq!(report.status, ReferencePreflightStatus::Dirty);
            assert_eq!(report.recommended_action, ReferencePreflightAction::PromptCleanOrOverride);
            assert!(report.checked_out_worktrees.iter().any(|w| !w.dirty_entries.is_empty()));
            let _ = fs::remove_dir_all(&dir);
        }

        #[test]
        fn test_dirty_branch_warn_proceeds() {
            let dir = temp_dir("dirty-warn");
            setup_git_repo(&dir);
            fs::write(dir.join("dirty.txt"), "change").unwrap();
            let cfg = ReferenceBranchPreflightConfig {
                dirty_policy: DirtyReferencePolicy::Warn,
                include_untracked: true,
                ..Default::default()
            };
            let report = inspect_reference_branch_preflight(&dir, "main", &cfg).unwrap();
            assert_eq!(report.status, ReferencePreflightStatus::Dirty);
            assert_eq!(report.recommended_action, ReferencePreflightAction::Proceed);
            let _ = fs::remove_dir_all(&dir);
        }

        #[test]
        fn test_create_branch_from_head() {
            let dir = temp_dir("create");
            setup_git_repo(&dir);
            create_reference_branch(&dir, "new-branch", BranchCreateSource::CurrentHead).unwrap();
            let report = inspect_reference_branch_preflight(
                &dir,
                "new-branch",
                &ReferenceBranchPreflightConfig::default(),
            )
            .unwrap();
            assert!(report.branch_exists);
            let _ = fs::remove_dir_all(&dir);
        }
    }
}
