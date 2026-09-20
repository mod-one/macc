use crate::tool::spec::{CheckSeverity, DoctorCheckKind, ToolSpec};
use serde::{Deserialize, Serialize};
use std::path::Path;
use std::process::Command;

// ── Shared diagnostic types (spec §14.2) ─────────────────────────────────────

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DiagnosticSeverity {
    Ok,
    Info,
    Warning,
    Error,
}

impl std::fmt::Display for DiagnosticSeverity {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            DiagnosticSeverity::Ok => write!(f, "ok"),
            DiagnosticSeverity::Info => write!(f, "info"),
            DiagnosticSeverity::Warning => write!(f, "warning"),
            DiagnosticSeverity::Error => write!(f, "error"),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DiagnosticFinding {
    pub id: String,
    pub title: String,
    pub severity: DiagnosticSeverity,
    pub category: String,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub recommended_action: Option<String>,
    pub fix_available: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub docs_ref: Option<String>,
}

impl DiagnosticFinding {
    pub fn ok(id: &str, category: &str, title: &str) -> Self {
        Self {
            id: id.to_string(),
            title: title.to_string(),
            severity: DiagnosticSeverity::Ok,
            category: category.to_string(),
            message: String::new(),
            recommended_action: None,
            fix_available: false,
            docs_ref: None,
        }
    }

    pub fn error(
        id: &str,
        category: &str,
        title: &str,
        message: &str,
        action: Option<&str>,
        fix_available: bool,
    ) -> Self {
        Self {
            id: id.to_string(),
            title: title.to_string(),
            severity: DiagnosticSeverity::Error,
            category: category.to_string(),
            message: message.to_string(),
            recommended_action: action.map(|s| s.to_string()),
            fix_available,
            docs_ref: None,
        }
    }

    pub fn warning(
        id: &str,
        category: &str,
        title: &str,
        message: &str,
        action: Option<&str>,
    ) -> Self {
        Self {
            id: id.to_string(),
            title: title.to_string(),
            severity: DiagnosticSeverity::Warning,
            category: category.to_string(),
            message: message.to_string(),
            recommended_action: action.map(|s| s.to_string()),
            fix_available: false,
            docs_ref: None,
        }
    }

    pub fn is_blocking(&self) -> bool {
        matches!(self.severity, DiagnosticSeverity::Error)
    }
}

// ── New diagnostic checks (spec §5.3) ────────────────────────────────────────

pub fn check_git_identity(project_root: &Path) -> Vec<DiagnosticFinding> {
    let mut findings = Vec::new();

    let has_local_name = git_config_value(project_root, "user.name", false);
    let has_global_name = git_config_value(project_root, "user.name", true);
    let has_local_email = git_config_value(project_root, "user.email", false);
    let has_global_email = git_config_value(project_root, "user.email", true);

    let has_name = has_local_name || has_global_name;
    let has_email = has_local_email || has_global_email;

    if has_name && has_email {
        findings.push(DiagnosticFinding::ok(
            "MACC-GIT-IDENTITY",
            "git",
            "Git identity configured",
        ));
    } else {
        let missing: Vec<&str> = [
            (!has_name).then_some("user.name"),
            (!has_email).then_some("user.email"),
        ]
        .into_iter()
        .flatten()
        .collect();
        findings.push(DiagnosticFinding::error(
            "MACC-GIT-IDENTITY-MISSING",
            "git",
            "Git identity is missing",
            &format!(
                "user.name and/or user.email are not configured (missing: {}).",
                missing.join(", ")
            ),
            Some(
                "Configure Git identity:\n  git config --global user.name \"Your Name\"\n  git config --global user.email \"you@example.com\"\n\nOr run:\n  macc doctor --fix --git-name \"Your Name\" --git-email \"you@example.com\"",
            ),
            true,
        ));
    }

    findings
}

pub fn check_disk_space(project_root: &Path, max_parallel: u32) -> Vec<DiagnosticFinding> {
    let repo_size_bytes = estimate_directory_size(project_root);
    let recommended =
        (repo_size_bytes as f64 * max_parallel as f64 * 1.25).max(2.0 * 1024.0 * 1024.0 * 1024.0);
    let available = free_space_bytes(project_root);

    let ratio = if recommended > 0.0 {
        available as f64 / recommended
    } else {
        1.0
    };

    if ratio >= 1.0 {
        vec![DiagnosticFinding::ok(
            "MACC-WORKTREE-DISK-SPACE",
            "worktrees",
            "Worktree disk space OK",
        )]
    } else if ratio >= 0.5 {
        vec![DiagnosticFinding::warning(
            "MACC-WORKTREE-DISK-LOW",
            "worktrees",
            "Low disk space for worktrees",
            &format!(
                "Available: {}. Recommended: {} for max_parallel={}. Consider freeing disk or reducing max_parallel.",
                fmt_bytes(available),
                fmt_bytes(recommended as u64),
                max_parallel
            ),
            Some("Free disk space or reduce max_parallel in macc.yaml."),
        )]
    } else {
        vec![DiagnosticFinding::error(
            "MACC-WORKTREE-DISK-LOW",
            "worktrees",
            "Insufficient disk space for worktrees",
            &format!(
                "Available: {}. Recommended: {} for max_parallel={}.",
                fmt_bytes(available),
                fmt_bytes(recommended as u64),
                max_parallel
            ),
            Some("Free disk space or reduce max_parallel in macc.yaml."),
            false,
        )]
    }
}

pub fn check_coordinator_ipc(macc_dir: &Path) -> Vec<DiagnosticFinding> {
    let sqlite_path = macc_dir.join("state/coordinator.sqlite");
    if !sqlite_path.exists() {
        return vec![DiagnosticFinding::error(
            "MACC-COORDINATOR-IPC-MISSING",
            "coordinator",
            "No coordinator is running",
            "Coordinator database not found; no coordinator has been started.",
            Some("Start a coordinator:\n  macc coordinator run"),
            false,
        )];
    }

    // Check if an active run exists by reading the SQLite file.
    match check_coordinator_active_run(&sqlite_path) {
        CoordinatorState::Running { pid } => {
            if is_pid_alive(pid as u32) {
                vec![DiagnosticFinding::ok(
                    "MACC-COORDINATOR-IPC",
                    "coordinator",
                    "Coordinator is running",
                )]
            } else {
                vec![DiagnosticFinding::error(
                    "MACC-COORDINATOR-IPC-STALE",
                    "coordinator",
                    "Coordinator socket appears stale",
                    &format!(
                        "Coordinator PID {} is no longer alive but is still marked as running in the database.",
                        pid
                    ),
                    Some(
                        "Run:\n  macc doctor --fix --coordinator\n\nOr start a new coordinator:\n  macc coordinator run",
                    ),
                    true,
                )]
            }
        }
        CoordinatorState::NotRunning => vec![DiagnosticFinding::error(
            "MACC-COORDINATOR-IPC-MISSING",
            "coordinator",
            "No coordinator is running",
            "No active coordinator run found in the database.",
            Some("Start a coordinator:\n  macc coordinator run"),
            false,
        )],
        CoordinatorState::Unknown => vec![DiagnosticFinding::warning(
            "MACC-COORDINATOR-IPC-UNKNOWN",
            "coordinator",
            "Coordinator state could not be determined",
            "Could not read the coordinator database.",
            Some("Run macc doctor or macc coordinator status for details."),
        )],
    }
}

pub fn check_task_readiness(macc_dir: &Path) -> Vec<DiagnosticFinding> {
    let prd_path = macc_dir.join("prd.json");
    if !prd_path.exists() {
        return vec![DiagnosticFinding::error(
            "MACC-TASK-NONE-READY",
            "tasks",
            "No PRD file found",
            "No prd.json was found in .macc/.",
            Some(
                "Create a starter task:\n  macc quickstart --starter-task\n\nOr initialize from a PRD:\n  macc coordinator sync-prd",
            ),
            false,
        )];
    }

    let ready_count = count_ready_tasks(&prd_path);
    if ready_count == 0 {
        vec![DiagnosticFinding::warning(
            "MACC-TASK-NONE-READY",
            "tasks",
            "No ready task found",
            "PRD exists but no task is in a dispatchable state.",
            Some(
                "Create a starter task:\n  macc quickstart --starter-task\n\nOr sync from PRD:\n  macc coordinator sync-prd",
            ),
        )]
    } else {
        vec![DiagnosticFinding::ok(
            "MACC-TASK-READY",
            "tasks",
            &format!("{} ready task(s) found", ready_count),
        )]
    }
}

/// Collect all new-style diagnostic findings for the current project.
pub fn collect_all_findings(
    paths: &crate::ProjectPaths,
    max_parallel: u32,
) -> Vec<DiagnosticFinding> {
    let mut findings = Vec::new();
    findings.extend(check_git_identity(&paths.root));
    findings.extend(check_disk_space(&paths.root, max_parallel));
    findings.extend(check_coordinator_ipc(&paths.macc_dir));
    findings.extend(check_task_readiness(&paths.macc_dir));
    findings.extend(check_tool_login_states(paths));
    findings.extend(check_reference_branch(paths));
    findings.extend(check_coordinator_config(paths));
    findings
}

/// Report coordinator settings the runtime accepts but silently ignores.
///
/// The canonical case: `max_review_cycles: 0` alongside `phases.review.enabled:
/// true` (often with `mode: required`). The config loads, the coordinator runs,
/// and review never executes -- there is no way to notice from behaviour alone.
pub fn check_coordinator_config(paths: &crate::ProjectPaths) -> Vec<DiagnosticFinding> {
    let Ok(canonical) = crate::config::load_canonical_config(&paths.config_path) else {
        // Config problems that stop it loading are reported by other checks.
        return Vec::new();
    };
    let warnings =
        crate::config::coordinator_config_warnings(canonical.automation.coordinator.as_ref());
    if warnings.is_empty() {
        return vec![DiagnosticFinding::ok(
            "MACC-CONFIG-COORDINATOR",
            "config",
            "Coordinator phase settings are consistent",
        )];
    }
    warnings
        .into_iter()
        .map(|warning| {
            DiagnosticFinding::warning(
                "MACC-CONFIG-COORDINATOR",
                "config",
                "Coordinator setting has no effect",
                &format!("{} {}", warning.setting, warning.message),
                Some("Edit .macc/macc.yaml so the setting matches the intended behaviour."),
            )
        })
        .collect()
}

/// Check that the configured reference branch exists locally and is clean (spec §19.2 item 10).
///
/// Reuses the existing preflight inspection logic so the check is consistent
/// with what `macc coordinator run` validates at startup.
pub fn check_reference_branch(paths: &crate::ProjectPaths) -> Vec<DiagnosticFinding> {
    use crate::coordinator::preflight::{
        inspect_reference_branch_preflight, ReferenceBranchPreflightConfig,
        ReferencePreflightStatus,
    };

    // Resolve the reference branch from project config (same resolution order as coordinator).
    let reference_branch = resolve_reference_branch(paths);

    let cfg = ReferenceBranchPreflightConfig::default();

    let report = match inspect_reference_branch_preflight(&paths.root, &reference_branch, &cfg) {
        Ok(r) => r,
        Err(_) => {
            return vec![DiagnosticFinding::warning(
                "MACC-REFERENCE-BRANCH-UNKNOWN",
                "git",
                "Reference branch could not be inspected",
                "Could not run Git commands to check the reference branch.",
                Some("Run `git status` and `macc doctor --coordinator` for details."),
            )];
        }
    };

    match report.status {
        ReferencePreflightStatus::Clean | ReferencePreflightStatus::NotCheckedOut => {
            vec![DiagnosticFinding::ok(
                "MACC-REFERENCE-BRANCH",
                "git",
                &format!(
                    "Reference branch \"{}\" exists and is clean",
                    reference_branch
                ),
            )]
        }

        ReferencePreflightStatus::BranchMissing => {
            let remote_hint = if report.remote_tracking_branches.is_empty() {
                format!(
                    "No local branch \"{}\" found and no matching remote-tracking branch detected.",
                    reference_branch
                )
            } else {
                format!(
                    "No local branch \"{}\" found; remote {} exists.",
                    reference_branch,
                    report.remote_tracking_branches.join(", ")
                )
            };
            vec![DiagnosticFinding::error(
                "MACC-REFERENCE-BRANCH-MISSING",
                "git",
                &format!("Reference branch \"{}\" does not exist locally", reference_branch),
                &remote_hint,
                Some(&format!(
                    "Create it:\n  git checkout -b {branch}\n\nOr run:\n  macc coordinator run --create-reference-branch --reference-branch-base main",
                    branch = reference_branch
                )),
                true,
            )]
        }

        ReferencePreflightStatus::Dirty => {
            let dirty_paths: Vec<String> = report
                .checked_out_worktrees
                .iter()
                .filter(|w| !w.dirty_entries.is_empty())
                .map(|w| w.path.to_string_lossy().to_string())
                .collect();
            vec![DiagnosticFinding::warning(
                "MACC-REFERENCE-BRANCH-DIRTY",
                "git",
                &format!(
                    "Reference branch \"{}\" has uncommitted changes",
                    reference_branch
                ),
                &format!(
                    "Uncommitted changes detected in: {}",
                    dirty_paths.join(", ")
                ),
                Some("Commit, stash, or discard changes before running the coordinator."),
            )]
        }

        ReferencePreflightStatus::InvalidBranchName => vec![DiagnosticFinding::error(
            "MACC-REFERENCE-BRANCH-INVALID",
            "git",
            &format!("Reference branch name \"{}\" is invalid", reference_branch),
            "The branch name fails `git check-ref-format` validation.",
            Some("Correct `automation.coordinator.reference_branch` in macc.yaml."),
            false,
        )],

        ReferencePreflightStatus::BareRepository => vec![DiagnosticFinding::warning(
            "MACC-REFERENCE-BRANCH-BARE",
            "git",
            "Bare repository detected",
            "Coordinator worktree operations require a non-bare repository.",
            None,
        )],

        ReferencePreflightStatus::GitInspectionFailed => vec![DiagnosticFinding::warning(
            "MACC-REFERENCE-BRANCH-UNKNOWN",
            "git",
            "Reference branch inspection failed",
            "Could not inspect the reference branch state.",
            Some("Run `macc doctor --coordinator` for details."),
        )],
    }
}

/// Resolve the coordinator reference_branch from project config, falling back to "main".
fn resolve_reference_branch(paths: &crate::ProjectPaths) -> String {
    crate::load_canonical_config(&paths.config_path)
        .ok()
        .and_then(|c| c.automation.coordinator)
        .and_then(|c| c.reference_branch)
        .unwrap_or_else(|| "main".to_string())
}

/// Check tool login and capability state for all enabled tools (spec §5.3.5).
///
/// Distinguishes: installed → configured → runnable → performer-available.
/// Emits one `DiagnosticFinding` per enabled tool plus a config-not-applied finding
/// when `macc apply` has not been run.
pub fn check_tool_login_states(paths: &crate::ProjectPaths) -> Vec<DiagnosticFinding> {
    use crate::tool::ToolSpecLoader;

    let search_paths = ToolSpecLoader::default_search_paths(&paths.root);
    let loader = ToolSpecLoader::new(search_paths);
    let (specs, _) = loader.load_all_with_embedded();

    let enabled_ids = load_enabled_tool_ids(paths);

    // MACC-CONFIG-NOT-APPLIED: tools enabled but apply never run.
    let config_applied = paths.managed_paths_state_path().exists();

    let mut findings = Vec::new();

    for spec in &specs {
        if !enabled_ids.contains(&spec.id) {
            continue;
        }

        let tool_name = &spec.display_name;

        // Sub-check 1: binary installed.
        let binary_ok = check_binary_in_path(&spec.id);
        if !binary_ok {
            findings.push(DiagnosticFinding::error(
                "MACC-TOOL-NOT-RUNNABLE",
                "tools",
                &format!("{} — binary not found", tool_name),
                &format!("'{}' is not in PATH.", spec.id),
                Some(&format!(
                    "Install {} then retry macc doctor.\nOr: macc tool install {}",
                    tool_name, spec.id
                )),
                false,
            ));
            continue;
        }

        // Sub-check 2: adapter configured (ToolSpec loaded + in enabled list — always true here).

        // Sub-check 3: runnable — probe via version_check if available.
        let (runnable, run_detail) = probe_tool_runnable(spec);

        // Sub-check 4: performer runner available.
        let has_performer = spec.performer.is_some();

        if runnable && has_performer {
            findings.push(DiagnosticFinding::ok(
                &format!(
                    "MACC-TOOL-{}",
                    spec.id.to_ascii_uppercase().replace('-', "_")
                ),
                "tools",
                &format!("{} — ready", tool_name),
            ));
        } else if !runnable {
            findings.push(DiagnosticFinding::warning(
                "MACC-TOOL-NOT-RUNNABLE",
                "tools",
                &format!("{} — authentication not confirmed", tool_name),
                &run_detail,
                Some(&format!(
                    "Run the {} login flow, then retry:\n  macc doctor",
                    tool_name
                )),
            ));
        } else {
            // runnable but no performer: warn
            findings.push(DiagnosticFinding::warning(
                "MACC-TOOL-NOT-RUNNABLE",
                "tools",
                &format!("{} — performer runner unavailable", tool_name),
                "No performer spec found for this tool. Coordinator execution will fail.",
                Some("Check tool configuration: macc doctor"),
            ));
        }
    }

    if !config_applied && !enabled_ids.is_empty() {
        findings.push(DiagnosticFinding::warning(
            "MACC-CONFIG-NOT-APPLIED",
            "project",
            "Config has not been applied",
            "Tools are enabled but no MACC-managed files have been written to the project.",
            Some("Run:\n  macc apply"),
        ));
    }

    findings
}

// ── Tool login helpers ────────────────────────────────────────────────────────

fn load_enabled_tool_ids(paths: &crate::ProjectPaths) -> Vec<String> {
    crate::load_canonical_config(&paths.config_path)
        .map(|c| c.tools.enabled)
        .unwrap_or_default()
}

fn check_binary_in_path(binary: &str) -> bool {
    let cmd = if cfg!(windows) { "where" } else { "which" };
    matches!(
        Command::new(cmd).arg(binary).output(),
        Ok(out) if out.status.success()
    )
}

/// Probe whether a tool is runnable by running its `version_check.current` command.
///
/// Returns `(true, "")` on success, `(false, reason)` on failure.
/// Falls back to `<binary> --version` when no `version_check` is declared.
fn probe_tool_runnable(spec: &ToolSpec) -> (bool, String) {
    let (cmd_str, args): (&str, &[String]) = if let Some(vc) = &spec.version_check {
        (&vc.current.command, &vc.current.args)
    } else {
        // Fallback: try `<id> --version`
        return probe_binary_version(&spec.id);
    };

    match Command::new(cmd_str).args(args).output() {
        Ok(out) if out.status.success() => (true, String::new()),
        Ok(out) => {
            let stderr = String::from_utf8_lossy(&out.stderr);
            let first_line = stderr.lines().next().unwrap_or("unknown error").trim();
            (
                false,
                format!("'{}' exited non-zero: {}", cmd_str, first_line),
            )
        }
        Err(e) => (false, format!("Could not run '{}': {}", cmd_str, e)),
    }
}

fn probe_binary_version(binary: &str) -> (bool, String) {
    match Command::new(binary).arg("--version").output() {
        Ok(out) if out.status.success() => (true, String::new()),
        Ok(_) => (
            false,
            format!(
                "Authentication status unknown — '{}' returned an error. \
                 Run the tool login flow.",
                binary
            ),
        ),
        Err(_) => (
            false,
            format!(
                "Authentication status unknown — could not run '{}'. \
                 Run the tool login flow.",
                binary
            ),
        ),
    }
}

/// Apply a git-identity fix locally in the project.
pub fn fix_git_identity(project_root: &Path, name: &str, email: &str) -> Result<(), String> {
    let status = Command::new("git")
        .args(["config", "--local", "user.name", name])
        .current_dir(project_root)
        .status()
        .map_err(|e| format!("Failed to run git config: {}", e))?;
    if !status.success() {
        return Err("git config user.name failed".to_string());
    }
    let status = Command::new("git")
        .args(["config", "--local", "user.email", email])
        .current_dir(project_root)
        .status()
        .map_err(|e| format!("Failed to run git config: {}", e))?;
    if !status.success() {
        return Err("git config user.email failed".to_string());
    }
    Ok(())
}

// ── Internal helpers ──────────────────────────────────────────────────────────

fn git_config_value(project_root: &Path, key: &str, global: bool) -> bool {
    let mut cmd = Command::new("git");
    if global {
        cmd.args(["config", "--global", key]);
    } else {
        cmd.args(["config", "--local", key]);
        cmd.current_dir(project_root);
    }
    matches!(
        cmd.output(),
        Ok(out) if out.status.success() && !String::from_utf8_lossy(&out.stdout).trim().is_empty()
    )
}

#[derive(Debug)]
enum CoordinatorState {
    Running { pid: i64 },
    NotRunning,
    Unknown,
}

fn check_coordinator_active_run(sqlite_path: &Path) -> CoordinatorState {
    let conn = match rusqlite::Connection::open(sqlite_path) {
        Ok(c) => c,
        Err(_) => return CoordinatorState::Unknown,
    };
    let result = conn.query_row(
        "SELECT pid FROM coordinator_runs WHERE status = 'running' OR status = 'draining' ORDER BY started_at DESC LIMIT 1",
        [],
        |row| row.get::<_, i64>(0),
    );
    match result {
        Ok(pid) => CoordinatorState::Running { pid },
        Err(rusqlite::Error::QueryReturnedNoRows) => CoordinatorState::NotRunning,
        Err(_) => CoordinatorState::Unknown,
    }
}

fn is_pid_alive(pid: u32) -> bool {
    if pid == 0 {
        return false;
    }
    // Send signal 0 to check liveness without affecting the process.
    #[cfg(unix)]
    {
        let result = unsafe { libc::kill(pid as libc::pid_t, 0) };
        result == 0
    }
    #[cfg(not(unix))]
    {
        // On non-Unix platforms, fall back to checking /proc or assuming alive.
        std::path::Path::new(&format!("/proc/{}", pid)).exists()
    }
}

fn estimate_directory_size(dir: &Path) -> u64 {
    // Fast approximation: check .git size only as a proxy for repo size.
    let git_dir = dir.join(".git");
    dir_size_bytes(&git_dir).unwrap_or(0)
}

fn dir_size_bytes(dir: &Path) -> Option<u64> {
    let mut total = 0u64;
    let entries = std::fs::read_dir(dir).ok()?;
    for entry in entries.flatten() {
        let meta = entry.metadata().ok()?;
        if meta.is_file() {
            total += meta.len();
        } else if meta.is_dir() {
            total += dir_size_bytes(&entry.path()).unwrap_or(0);
        }
    }
    Some(total)
}

fn free_space_bytes(path: &Path) -> u64 {
    #[cfg(unix)]
    {
        use std::ffi::CString;
        let path_str = match path.to_str() {
            Some(s) => s,
            None => return u64::MAX,
        };
        let c_path = match CString::new(path_str) {
            Ok(s) => s,
            Err(_) => return u64::MAX,
        };
        let mut stat: libc::statvfs = unsafe { std::mem::zeroed() };
        if unsafe { libc::statvfs(c_path.as_ptr(), &mut stat) } == 0 {
            return (stat.f_bavail as u64) * (stat.f_frsize as u64);
        }
        u64::MAX
    }
    #[cfg(not(unix))]
    {
        u64::MAX
    }
}

fn fmt_bytes(bytes: u64) -> String {
    const GB: u64 = 1024 * 1024 * 1024;
    const MB: u64 = 1024 * 1024;
    if bytes >= GB {
        format!("{:.1} GB", bytes as f64 / GB as f64)
    } else if bytes >= MB {
        format!("{:.0} MB", bytes as f64 / MB as f64)
    } else {
        format!("{} B", bytes)
    }
}

/// Public re-export for use in onboarding module.
pub fn count_ready_tasks_public(prd_path: &Path) -> usize {
    count_ready_tasks(prd_path)
}

/// Public re-export for use in onboarding module.
pub fn is_pid_alive_pub(pid: u32) -> bool {
    is_pid_alive(pid)
}

fn count_ready_tasks(prd_path: &Path) -> usize {
    let Ok(content) = std::fs::read_to_string(prd_path) else {
        return 0;
    };
    let Ok(value) = serde_json::from_str::<serde_json::Value>(&content) else {
        return 0;
    };
    let tasks = match value.get("tasks").and_then(|t| t.as_array()) {
        Some(t) => t,
        None => return 0,
    };
    tasks
        .iter()
        .filter(|t| {
            matches!(
                t.get("state").and_then(|s| s.as_str()),
                Some("todo") | Some("ready")
            )
        })
        .count()
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ToolStatus {
    Installed,
    Missing,
    Error(String),
}

pub trait CheckRunner {
    fn which(&self, binary: &str) -> bool;
    fn path_exists(&self, path: &str) -> bool;
    fn git_config_key(&self, key: &str) -> bool;
}

pub struct SystemRunner;

impl CheckRunner for SystemRunner {
    fn which(&self, binary: &str) -> bool {
        let cmd = if cfg!(windows) { "where" } else { "which" };
        let output = Command::new(cmd).arg(binary).output();
        matches!(output, Ok(out) if out.status.success())
    }

    fn path_exists(&self, path: &str) -> bool {
        Path::new(path).exists()
    }

    fn git_config_key(&self, key: &str) -> bool {
        matches!(
            Command::new("git").args(["config", key]).output(),
            Ok(out) if out.status.success() && !String::from_utf8_lossy(&out.stdout).trim().is_empty()
        )
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ToolCheck {
    pub name: String,
    pub tool_id: Option<String>,
    pub check_target: String,
    pub kind: DoctorCheckKind,
    pub status: ToolStatus,
    pub severity: CheckSeverity,
    /// Optional human-readable fix hint shown when the check fails.
    pub fix_hint: Option<String>,
}

pub fn check_tool(runner: &dyn CheckRunner, kind: &DoctorCheckKind, value: &str) -> ToolStatus {
    match kind {
        DoctorCheckKind::Which => {
            if runner.which(value) {
                ToolStatus::Installed
            } else {
                ToolStatus::Missing
            }
        }
        DoctorCheckKind::PathExists => {
            if runner.path_exists(value) {
                ToolStatus::Installed
            } else {
                ToolStatus::Missing
            }
        }
        DoctorCheckKind::GitConfigKey => {
            if runner.git_config_key(value) {
                ToolStatus::Installed
            } else {
                ToolStatus::Missing
            }
        }
        DoctorCheckKind::Custom => ToolStatus::Error("Custom checks not supported yet".to_string()),
    }
}

pub fn checks_for_enabled_tools(specs: &[ToolSpec]) -> Vec<ToolCheck> {
    let mut checks = Vec::new();

    // Baseline checks
    checks.push(ToolCheck {
        name: "Git".to_string(),
        tool_id: None,
        check_target: "git".to_string(),
        kind: DoctorCheckKind::Which,
        status: ToolStatus::Missing,
        severity: CheckSeverity::Error,
        fix_hint: None,
    });

    // Git identity checks — required for commits to succeed.
    checks.push(ToolCheck {
        name: "Git user.email".to_string(),
        tool_id: None,
        check_target: "user.email".to_string(),
        kind: DoctorCheckKind::GitConfigKey,
        status: ToolStatus::Missing,
        severity: CheckSeverity::Error,
        fix_hint: Some("git config --global user.email \"you@example.com\"".to_string()),
    });
    checks.push(ToolCheck {
        name: "Git user.name".to_string(),
        tool_id: None,
        check_target: "user.name".to_string(),
        kind: DoctorCheckKind::GitConfigKey,
        status: ToolStatus::Missing,
        severity: CheckSeverity::Error,
        fix_hint: Some("git config --global user.name \"Your Name\"".to_string()),
    });

    for spec in specs {
        if let Some(doctor_specs) = &spec.doctor {
            for check_spec in doctor_specs {
                checks.push(ToolCheck {
                    name: spec.display_name.clone(),
                    tool_id: Some(spec.id.clone()),
                    check_target: check_spec.value.clone(),
                    kind: check_spec.kind.clone(),
                    status: ToolStatus::Missing,
                    severity: check_spec.severity.clone(),
                    fix_hint: None,
                });
            }
        } else {
            // Heuristic fallback: check for binary with same ID
            checks.push(ToolCheck {
                name: spec.display_name.clone(),
                tool_id: Some(spec.id.clone()),
                check_target: spec.id.clone(),
                kind: DoctorCheckKind::Which,
                status: ToolStatus::Missing,
                severity: CheckSeverity::Warning,
                fix_hint: None,
            });
        }
    }

    checks
}

pub fn run_checks(checks: &mut [ToolCheck]) {
    let runner = SystemRunner;
    for check in checks {
        check.status = check_tool(&runner, &check.kind, &check.check_target);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct MockRunner {
        installed: Vec<String>,
        paths: Vec<String>,
    }

    impl CheckRunner for MockRunner {
        fn which(&self, binary: &str) -> bool {
            self.installed.contains(&binary.to_string())
        }
        fn path_exists(&self, path: &str) -> bool {
            self.paths.contains(&path.to_string())
        }
        fn git_config_key(&self, key: &str) -> bool {
            self.installed.contains(&key.to_string())
        }
    }

    #[test]
    fn test_check_tool_availability_with_mock() {
        let tool_id = format!("tool-{}", uuid_v4_like());
        let runner = MockRunner {
            installed: vec![tool_id.clone()],
            paths: vec!["/tmp/foo".to_string()],
        };

        assert_eq!(
            check_tool(&runner, &DoctorCheckKind::Which, &tool_id),
            ToolStatus::Installed
        );
        assert_eq!(
            check_tool(&runner, &DoctorCheckKind::Which, "missing-tool"),
            ToolStatus::Missing
        );
        assert_eq!(
            check_tool(&runner, &DoctorCheckKind::PathExists, "/tmp/foo"),
            ToolStatus::Installed
        );
    }

    #[test]
    fn test_checks_generation() {
        let spec = ToolSpec {
            api_version: "v1".to_string(),
            id: format!("tool-{}", uuid_v4_like()),
            display_name: "Test Tool".to_string(),
            description: None,
            capabilities: vec![],
            fields: vec![],
            doctor: Some(vec![crate::tool::spec::DoctorCheckSpec {
                kind: DoctorCheckKind::Which,
                value: "test-bin".to_string(),
                severity: CheckSeverity::Error,
            }]),
            gitignore: Vec::new(),
            performer: None,
            install: None,
            update: None,
            version_check: None,
            defaults: None,
            model_tiers: Default::default(),
        };

        let checks = checks_for_enabled_tools(&[spec]);
        // Git + Git user.email + Git user.name + Test Tool
        assert_eq!(checks.len(), 4);
        assert_eq!(checks[1].check_target, "user.email");
        assert_eq!(checks[1].kind, DoctorCheckKind::GitConfigKey);
        assert!(checks[1].fix_hint.is_some());
        assert_eq!(checks[2].check_target, "user.name");
        assert_eq!(checks[2].kind, DoctorCheckKind::GitConfigKey);
        assert!(checks[2].fix_hint.is_some());
        assert_eq!(checks[3].check_target, "test-bin");
        assert_eq!(checks[3].kind, DoctorCheckKind::Which);
    }

    fn uuid_v4_like() -> String {
        use std::time::{SystemTime, UNIX_EPOCH};
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        format!("{:x}", nanos)
    }
}
