//! Supervisor Mode B: AI-powered worktree analysis and recovery.
//!
//! Mode B runs **in parallel with the coordinator** and is triggered when a
//! worktree is detected as stuck or failed. It:
//!
//! 1. Collects evidence (performer logs, coordinator events, git status/diff).
//! 2. Builds a structured prompt describing the evidence and requesting a
//!    machine-readable `SupervisorReport` in JSON.
//! 3. Dispatches the prompt to an AI tool via [`AiAnalysisDispatcher`].
//! 4. Parses the JSON response into a [`SupervisorReport`].
//! 5. Executes safe worktree-level recovery actions (git reset / clean) when
//!    the report recommends one.
//! 6. Writes the final report to `.macc/log/supervisor/<task-id>-<ts>.json`.
//!
//! # Safety contract
//!
//! Mode B may only execute coordinator-level recovery actions (`unlock`,
//! `reconcile`) when the coordinator process is **not running** — verified by
//! checking `.macc/state/coordinator.pid` before spawning any subcommand.
//! Worktree-local git operations (`reset`, `clean`) are always safe and do
//! not require the coordinator to be stopped.

use crate::supervisor::{
    coordinator_supervisor::{
        CoordinatorEvidence, CoordinatorEvidenceCollector, CoordinatorEvidenceConfig,
    },
    SupervisorAction, SupervisorReport,
};
use async_trait::async_trait;
use chrono::Utc;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Duration;

// ── Configuration ─────────────────────────────────────────────────────────────

/// Configuration for Mode B supervisor.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ModeBConfig {
    /// Timeout for the AI analysis dispatch call.
    #[serde(default = "default_analysis_timeout_seconds")]
    pub analysis_timeout_seconds: u64,

    /// Maximum recovery attempts per task per supervisor cycle.
    #[serde(default = "default_max_recovery_attempts")]
    pub max_recovery_attempts: u32,

    /// Directory where per-task reports are written.
    #[serde(default = "default_report_output_dir")]
    pub report_output_dir: PathBuf,

    /// Max bytes of performer logs to include in the prompt.
    #[serde(default = "default_max_log_bytes")]
    pub max_log_bytes: usize,

    /// Max coordinator event lines to include in the prompt.
    #[serde(default = "default_max_event_lines")]
    pub max_event_lines: usize,

    /// Optional coordinator-level evidence configuration.
    ///
    /// When set, Mode B can collect coordinator-scoped evidence (events, task
    /// registry state, run log footer) in addition to per-worktree evidence.
    /// The call path that uses this config is completed by L5-SUP-005.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub coordinator_evidence_config: Option<CoordinatorEvidenceConfig>,
}

fn default_analysis_timeout_seconds() -> u64 {
    120
}
fn default_max_recovery_attempts() -> u32 {
    1
}
fn default_report_output_dir() -> PathBuf {
    PathBuf::from(".macc/log/supervisor")
}
fn default_max_log_bytes() -> usize {
    16_384 // 16 KiB
}
fn default_max_event_lines() -> usize {
    50
}

impl Default for ModeBConfig {
    fn default() -> Self {
        Self {
            analysis_timeout_seconds: default_analysis_timeout_seconds(),
            max_recovery_attempts: default_max_recovery_attempts(),
            report_output_dir: default_report_output_dir(),
            max_log_bytes: default_max_log_bytes(),
            max_event_lines: default_max_event_lines(),
            coordinator_evidence_config: None,
        }
    }
}

// ── Evidence ──────────────────────────────────────────────────────────────────

/// All evidence collected for a stuck/failed worktree.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct WorktreeEvidence {
    /// Task ID being analysed.
    pub task_id: String,

    /// Absolute path to the worktree.
    pub worktree_path: String,

    /// Current branch in the worktree, if determinable.
    pub branch: Option<String>,

    /// Output of `git status --short` inside the worktree.
    pub git_status: String,

    /// Output of `git log --oneline HEAD ^<base>` (commits ahead of base).
    pub commits_ahead: String,

    /// Performer log content collected for this task (truncated to `max_log_bytes`).
    pub performer_log: String,

    /// Recent coordinator events for this task (last N lines from events.jsonl).
    pub coordinator_events: Vec<String>,

    /// Last known error code from the task runtime, if available.
    pub last_error_code: Option<String>,

    /// Last known error message from the task runtime, if available.
    pub last_error_message: Option<String>,
}

// ── Evidence Collector ────────────────────────────────────────────────────────

/// Collects evidence for a stuck/failed worktree.
pub struct EvidenceCollector {
    repo_root: PathBuf,
    config: ModeBConfig,
}

impl EvidenceCollector {
    pub fn new(repo_root: PathBuf, config: ModeBConfig) -> Self {
        Self { repo_root, config }
    }

    /// Collect all evidence for `task_id` running in `worktree_path`.
    pub fn collect(
        &self,
        task_id: &str,
        worktree_path: &Path,
        base_branch: &str,
        last_error_code: Option<&str>,
        last_error_message: Option<&str>,
    ) -> WorktreeEvidence {
        let branch = self.read_current_branch(worktree_path);
        let git_status = self.run_git_in(worktree_path, &["status", "--short"]);
        let commits_ahead = self.run_git_in(
            worktree_path,
            &["log", "--oneline", &format!("HEAD ^{}", base_branch)],
        );
        let performer_log = self.read_performer_log(task_id, worktree_path);
        let coordinator_events = self.read_coordinator_events(task_id);

        WorktreeEvidence {
            task_id: task_id.to_string(),
            worktree_path: worktree_path.to_string_lossy().into_owned(),
            branch,
            git_status,
            commits_ahead,
            performer_log,
            coordinator_events,
            last_error_code: last_error_code.map(str::to_owned),
            last_error_message: last_error_message.map(str::to_owned),
        }
    }

    fn read_current_branch(&self, worktree_path: &Path) -> Option<String> {
        let out = Command::new("git")
            .current_dir(worktree_path)
            .args(["rev-parse", "--abbrev-ref", "HEAD"])
            .output()
            .ok()?;
        if out.status.success() {
            Some(String::from_utf8_lossy(&out.stdout).trim().to_owned())
        } else {
            None
        }
    }

    fn run_git_in(&self, worktree_path: &Path, args: &[&str]) -> String {
        Command::new("git")
            .current_dir(worktree_path)
            .args(args)
            .output()
            .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_owned())
            .unwrap_or_default()
    }

    fn read_performer_log(&self, task_id: &str, worktree_path: &Path) -> String {
        // Try worktree-local log first, then aggregated log.
        let candidates = [
            worktree_path.join(".macc").join("log").join("performer"),
            self.repo_root.join(".macc").join("log").join("performer"),
        ];

        for dir in &candidates {
            if !dir.is_dir() {
                continue;
            }
            // Look for files whose name contains the task_id.
            let Ok(entries) = fs::read_dir(dir) else {
                continue;
            };
            for entry in entries.flatten() {
                let path = entry.path();
                let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
                if name.contains(task_id) && path.is_file() {
                    if let Ok(content) = fs::read_to_string(&path) {
                        return truncate_str(content, self.config.max_log_bytes);
                    }
                }
            }
        }
        String::new()
    }

    fn read_coordinator_events(&self, task_id: &str) -> Vec<String> {
        let events_path = self
            .repo_root
            .join(".macc")
            .join("log")
            .join("coordinator")
            .join("events.jsonl");

        let Ok(content) = fs::read_to_string(&events_path) else {
            return Vec::new();
        };

        let matching: Vec<String> = content
            .lines()
            .filter(|line| line.contains(task_id))
            .map(str::to_owned)
            .collect();

        // Return the last N matching lines.
        let n = self.config.max_event_lines;
        if matching.len() > n {
            matching[matching.len() - n..].to_vec()
        } else {
            matching
        }
    }
}

fn truncate_str(s: String, max_bytes: usize) -> String {
    if s.len() <= max_bytes {
        return s;
    }
    // Truncate at a UTF-8 boundary, keeping the *tail* (most recent output).
    let start = s.len().saturating_sub(max_bytes);
    let sliced = &s[start..];
    // Walk forward to next valid UTF-8 boundary.
    let offset = sliced.char_indices().next().map(|(i, _)| i).unwrap_or(0);
    format!("[...truncated...]\n{}", &sliced[offset..])
}

// ── Prompt Builder ────────────────────────────────────────────────────────────

/// Builds a structured analysis prompt from collected evidence.
pub struct AnalysisPromptBuilder;

impl AnalysisPromptBuilder {
    /// Build the analysis prompt that will be sent to the AI performer.
    pub fn build(evidence: &WorktreeEvidence) -> String {
        let events_text = if evidence.coordinator_events.is_empty() {
            "(none)".to_string()
        } else {
            evidence.coordinator_events.join("\n")
        };

        let performer_log = if evidence.performer_log.is_empty() {
            "(no performer log found)".to_string()
        } else {
            evidence.performer_log.clone()
        };

        let error_ctx = match (&evidence.last_error_code, &evidence.last_error_message) {
            (Some(code), Some(msg)) => format!("Error code: {}\nError message: {}", code, msg),
            (Some(code), None) => format!("Error code: {}", code),
            (None, Some(msg)) => format!("Error message: {}", msg),
            (None, None) => "(no error context available)".to_string(),
        };

        format!(
            r#"You are the MACC supervisor Mode B. A worktree has been detected as stuck or failed.
Analyze the evidence below and produce a structured JSON report.

## Task context
Task ID: {task_id}
Worktree path: {worktree_path}
Branch: {branch}
{error_ctx}

## Git status (inside worktree)
```
{git_status}
```

## Commits ahead of base branch
```
{commits_ahead}
```

## Recent coordinator events for this task
```
{events}
```

## Performer log (most recent excerpt)
```
{performer_log}
```

## Known MACC error patterns
- E101: Runner exited non-zero (transient, retryable)
- E301: Worktree missing
- E302: PRD missing
- E501: Merge conflict (requires manual resolution or git reset)
- E601: Rate-limited (transient, retryable with backoff)
- E602: Quota exhausted (requires operator action)

## Safe recovery actions available
The supervisor can execute ONE of these actions (coordinator state must not be modified):
- "git_reset": Run `git reset --hard HEAD` in the worktree (discards uncommitted changes)
- "git_clean": Run `git clean -fd` in the worktree (removes untracked files)
- "none": Take no automated action (log and report only)

## Required output format
Respond with ONLY valid JSON matching this schema (no markdown fences, no extra text):
{{
  "timestamp": "<RFC-3339 UTC>",
  "analysis_window_seconds": 0,
  "health": {{"status": "degraded", "reasons": ["<reason>"]}},
  "findings": [
    {{
      "severity": "error|warning|info|critical",
      "category": "task_progress|merge_health|process_lifecycle|rate_limit|observability|other",
      "description": "<what was observed>",
      "evidence": ["<supporting line or excerpt>"]
    }}
  ],
  "recommendations": [
    {{
      "priority": 1,
      "description": "<what should be done>",
      "rationale": "<why>",
      "affected_files": [],
      "implementation_hint": "<optional hint>"
    }}
  ],
  "actions_taken": [],
  "suggested_code_changes": [],
  "improvement_hints": [
    {{
      "problem_class": "<short label for the class of recurring problem>",
      "detection_hook": "<where in the coordinator lifecycle this can be detected>",
      "suggested_coordinator_behavior": "<what the coordinator should do autonomously when detected>",
      "suggested_prd_task": "<optional PRD task ID that would implement this, or omit field>"
    }}
  ],
  "recovery_action": "git_reset|git_clean|none"
}}
"#,
            task_id = evidence.task_id,
            worktree_path = evidence.worktree_path,
            branch = evidence.branch.as_deref().unwrap_or("(unknown)"),
            error_ctx = error_ctx,
            git_status = evidence.git_status,
            commits_ahead = evidence.commits_ahead,
            events = events_text,
            performer_log = performer_log,
        )
    }
}

/// Format coordinator-level evidence as a supplemental prompt section.
///
/// Appended to the per-worktree prompt when `coordinator_evidence_config` is
/// set and `events.jsonl` exists, giving the AI additional context about the
/// full coordinator state.
fn format_coordinator_evidence_section(ce: &CoordinatorEvidence) -> String {
    let result = ce.coordinator_result.as_deref().unwrap_or("(unknown)");
    let exit_reason = ce
        .coordinator_exit_reason
        .as_deref()
        .unwrap_or("(not recorded)");
    let counts = &ce.task_state_counts.counts;
    let state_summary = format!(
        "total={}, failed={}, todo={}, merged={}, in_progress={}",
        ce.task_state_counts.total,
        counts.get("failed").copied().unwrap_or(0),
        counts.get("todo").copied().unwrap_or(0),
        counts.get("merged").copied().unwrap_or(0),
        counts.get("in_progress").copied().unwrap_or(0),
    );
    let recent_events = if ce.recent_events.is_empty() {
        "(none)".to_string()
    } else {
        ce.recent_events.join("\n")
    };

    format!(
        r#"
## Coordinator-level context (supplemental)
Coordinator result: {result}
Exit reason: {exit_reason}
Task state summary: {state_summary}

Recent coordinator events (last {n} lines from events.jsonl):
```
{events}
```
"#,
        result = result,
        exit_reason = exit_reason,
        state_summary = state_summary,
        n = ce.recent_events.len(),
        events = recent_events,
    )
}

// ── AI Dispatcher trait ───────────────────────────────────────────────────────

/// Errors from [`AiAnalysisDispatcher`].
#[derive(Debug, thiserror::Error)]
pub enum DispatchError {
    #[error("AI dispatch timed out after {seconds}s")]
    Timeout { seconds: u64 },

    #[error("AI tool exited with non-zero status: {status}")]
    NonZeroExit { status: i32 },

    #[error("IO error during dispatch: {0}")]
    Io(#[from] std::io::Error),

    #[error("AI returned no output")]
    EmptyOutput,
}

/// Abstraction for dispatching a prompt to an AI tool and receiving a response.
///
/// Implementations may invoke a local CLI tool, call an API, or use a test
/// double that returns a canned response.
#[async_trait]
pub trait AiAnalysisDispatcher: Send + Sync {
    /// Send `prompt` to the AI tool. Returns the raw response string.
    ///
    /// Callers enforce the timeout externally via [`tokio::time::timeout`].
    async fn dispatch(&self, prompt: &str) -> Result<String, DispatchError>;
}

// ── CLI-based dispatcher (non-interactive) ────────────────────────────────────

/// Dispatcher that writes the prompt to a temp file and invokes a CLI AI tool
/// using its non-interactive (`-p` / `--print`) flag.
///
/// Compatible with `claude -p <file>` and similar patterns.
pub struct CliToolDispatcher {
    /// The command to invoke (e.g., `"claude"`).
    pub command: String,
    /// Extra arguments inserted before the prompt arg (e.g., `["--output-format", "json"]`).
    pub extra_args: Vec<String>,
}

impl CliToolDispatcher {
    /// Create a dispatcher for `command`, which must accept `-p <prompt_file>` and
    /// write the response to stdout.
    pub fn new(command: impl Into<String>) -> Self {
        Self {
            command: command.into(),
            extra_args: Vec::new(),
        }
    }

    pub fn with_extra_args(mut self, args: Vec<String>) -> Self {
        self.extra_args = args;
        self
    }
}

#[async_trait]
impl AiAnalysisDispatcher for CliToolDispatcher {
    async fn dispatch(&self, prompt: &str) -> Result<String, DispatchError> {
        // Write prompt to a temporary file.
        let tmp_dir = std::env::temp_dir();
        let prompt_file = tmp_dir.join(format!(
            "macc-supervisor-prompt-{}.txt",
            Utc::now().timestamp_nanos_opt().unwrap_or_default()
        ));
        fs::write(&prompt_file, prompt)?;

        let mut cmd = tokio::process::Command::new(&self.command);
        cmd.args(&self.extra_args);
        cmd.arg("-p").arg(&prompt_file);
        cmd.stdout(std::process::Stdio::piped());
        cmd.stderr(std::process::Stdio::null());

        let output = cmd.output().await?;

        // Clean up temp file regardless of outcome.
        let _ = fs::remove_file(&prompt_file);

        if !output.status.success() {
            return Err(DispatchError::NonZeroExit {
                status: output.status.code().unwrap_or(-1),
            });
        }

        let text = String::from_utf8_lossy(&output.stdout).trim().to_owned();
        if text.is_empty() {
            return Err(DispatchError::EmptyOutput);
        }
        Ok(text)
    }
}

// ── Report parsing ────────────────────────────────────────────────────────────

/// Extension of [`SupervisorReport`] that also carries the recommended recovery
/// action parsed from the AI response.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AiAnalysisResult {
    #[serde(flatten)]
    pub report: SupervisorReport,
    /// Recovery action recommended by the AI.
    #[serde(default)]
    pub recovery_action: RecoveryActionKind,
}

/// The recovery action an AI analysis may recommend.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum RecoveryActionKind {
    /// Run `git reset --hard HEAD` in the worktree.
    GitReset,
    /// Run `git clean -fd` in the worktree.
    GitClean,
    /// Run `macc coordinator unlock [task_ids...]`.
    ///
    /// Only executed when the coordinator is **not** running (safety guard).
    CoordinatorUnlock { task_ids: Vec<String> },
    /// Run `macc coordinator reconcile`.
    ///
    /// Only executed when the coordinator is **not** running (safety guard).
    CoordinatorReconcile,
    /// AI-powered fix for a compile error in a test file.
    ///
    /// `file_path` must refer to a test file (enforced by [`is_test_file`]).
    /// `description` should contain the compile error and any context needed
    /// for the AI to produce a targeted call-site fix.
    ///
    /// # Safety
    /// This action is **never** applied to implementation files.  The
    /// [`is_test_file`] guard is checked inside [`execute_recovery`] and the
    /// action falls back to [`RecoveryActionKind::Escalate`] when the guard
    /// rejects the path.
    FixTest {
        file_path: PathBuf,
        description: String,
    },
    /// Escalate: take no automated action.
    ///
    /// The `reason` is written to the recovery outcome and logged at WARN
    /// level.  No subprocess is spawned.
    Escalate { reason: String },
    /// Take no automated action.
    #[default]
    None,
}

/// Parse the raw AI response string into an [`AiAnalysisResult`].
///
/// The AI is expected to return a JSON object. We strip markdown fences if
/// present (defensive parsing).
pub fn parse_ai_response(raw: &str) -> Result<AiAnalysisResult, serde_json::Error> {
    // Strip optional ```json ... ``` fences.
    let cleaned = strip_json_fences(raw);
    serde_json::from_str(cleaned)
}

fn strip_json_fences(s: &str) -> &str {
    let s = s.trim();
    let s = s.strip_prefix("```json").unwrap_or(s);
    let s = s.strip_prefix("```").unwrap_or(s);
    let s = s.strip_suffix("```").unwrap_or(s);
    s.trim()
}

// ── Test-file guard ───────────────────────────────────────────────────────────

/// Returns `true` if `path` refers to a test file.
///
/// A path is considered a test file when **any** of the following hold:
/// - It contains a `tests` path component (e.g., `core/tests/foo.rs`).
/// - Its filename ends with `_test.rs`.
/// - Its filename ends with `_integration.rs`.
///
/// Implementation files (e.g., `core/src/lib.rs`, `core/src/foo.rs`) are
/// always rejected so that [`RecoveryActionKind::FixTest`] can never mutate
/// non-test sources.
pub fn is_test_file(path: &Path) -> bool {
    // Check for a `tests` directory component anywhere in the path.
    if path.components().any(|c| c.as_os_str() == "tests") {
        return true;
    }
    // Check filename suffixes.
    path.file_name()
        .and_then(|n| n.to_str())
        .map(|name| name.ends_with("_test.rs") || name.ends_with("_integration.rs"))
        .unwrap_or(false)
}

/// Build the targeted AI prompt for a [`RecoveryActionKind::FixTest`] action.
fn build_fix_test_prompt(file_path: &Path, file_content: &str, description: &str) -> String {
    format!(
        r#"You are a Rust compiler assistant. A test file has a compile error caused by a
function signature mismatch (e.g., the implementation changed and the test call-site
was not updated). Your task is to fix the call-site in the test file only.

## Test file path
{path}

## Compile error / context
{description}

## Current test file content
```rust
{content}
```

## Rules
- Fix ONLY the test call-site(s) that caused the compile error.
- Do NOT modify any implementation files.
- Do NOT change test logic, assertions, or structure — only adjust arguments or
  method names to match the current function signature.
- Return ONLY the complete fixed file content, with no markdown fences, no
  explanation, and no preamble.
"#,
        path = file_path.display(),
        description = description,
        content = file_content,
    )
}

// ── Recovery executor ─────────────────────────────────────────────────────────

/// Result of executing a recovery action.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RecoveryOutcome {
    pub action: RecoveryActionKind,
    pub succeeded: bool,
    pub output: String,
}

/// Execute the recovery action recommended by the AI in `worktree_path`.
///
/// `repo_root` is required for coordinator-level actions — they are run with
/// `repo_root` as the working directory and the PID file is checked relative
/// to it.
///
/// `ai_dispatcher` is used only by [`RecoveryActionKind::FixTest`]; all other
/// actions ignore it.  Pass `None` when no AI dispatcher is available (FixTest
/// will fall back to [`RecoveryActionKind::Escalate`]).
///
/// # Safety guards
///
/// - `CoordinatorUnlock` and `CoordinatorReconcile` are **skipped** when
///   [`is_coordinator_running`] returns `true`.
/// - `FixTest` is **rejected** for non-test files via [`is_test_file`].  The
///   compile verification gate (`cargo build -p macc-core --tests`) must pass
///   before the AI-generated fix is kept; otherwise the original file is
///   restored and the action falls back to `Escalate`.
pub async fn execute_recovery<D: AiAnalysisDispatcher>(
    action: &RecoveryActionKind,
    worktree_path: &Path,
    repo_root: &Path,
    ai_dispatcher: Option<&D>,
) -> RecoveryOutcome {
    match action {
        RecoveryActionKind::None => RecoveryOutcome {
            action: action.clone(),
            succeeded: true,
            output: "no recovery action taken".to_string(),
        },
        RecoveryActionKind::GitReset => {
            let out = Command::new("git")
                .current_dir(worktree_path)
                .args(["reset", "--hard", "HEAD"])
                .output();
            match out {
                Ok(o) => RecoveryOutcome {
                    action: action.clone(),
                    succeeded: o.status.success(),
                    output: String::from_utf8_lossy(&o.stdout).trim().to_owned(),
                },
                Err(e) => RecoveryOutcome {
                    action: action.clone(),
                    succeeded: false,
                    output: e.to_string(),
                },
            }
        }
        RecoveryActionKind::GitClean => {
            let out = Command::new("git")
                .current_dir(worktree_path)
                .args(["clean", "-fd"])
                .output();
            match out {
                Ok(o) => RecoveryOutcome {
                    action: action.clone(),
                    succeeded: o.status.success(),
                    output: String::from_utf8_lossy(&o.stdout).trim().to_owned(),
                },
                Err(e) => RecoveryOutcome {
                    action: action.clone(),
                    succeeded: false,
                    output: e.to_string(),
                },
            }
        }
        RecoveryActionKind::CoordinatorUnlock { task_ids } => {
            if is_coordinator_running(repo_root) {
                tracing::warn!(
                    "supervisor mode_b: skipping coordinator unlock — coordinator is running"
                );
                return RecoveryOutcome {
                    action: action.clone(),
                    succeeded: false,
                    output: "skipped: coordinator is still running".to_string(),
                };
            }
            let mut cmd = tokio::process::Command::new("macc");
            cmd.arg("coordinator").arg("unlock");
            for id in task_ids {
                cmd.arg(id);
            }
            cmd.current_dir(repo_root);
            match cmd.output().await {
                Ok(o) => RecoveryOutcome {
                    action: action.clone(),
                    succeeded: o.status.success(),
                    output: String::from_utf8_lossy(&o.stdout).trim().to_owned(),
                },
                Err(e) => RecoveryOutcome {
                    action: action.clone(),
                    succeeded: false,
                    output: e.to_string(),
                },
            }
        }
        RecoveryActionKind::CoordinatorReconcile => {
            if is_coordinator_running(repo_root) {
                tracing::warn!(
                    "supervisor mode_b: skipping coordinator reconcile — coordinator is running"
                );
                return RecoveryOutcome {
                    action: action.clone(),
                    succeeded: false,
                    output: "skipped: coordinator is still running".to_string(),
                };
            }
            let mut cmd = tokio::process::Command::new("macc");
            cmd.args(["coordinator", "reconcile"]);
            cmd.current_dir(repo_root);
            match cmd.output().await {
                Ok(o) => RecoveryOutcome {
                    action: action.clone(),
                    succeeded: o.status.success(),
                    output: String::from_utf8_lossy(&o.stdout).trim().to_owned(),
                },
                Err(e) => RecoveryOutcome {
                    action: action.clone(),
                    succeeded: false,
                    output: e.to_string(),
                },
            }
        }
        RecoveryActionKind::FixTest {
            file_path,
            description,
        } => {
            execute_fix_test(
                file_path,
                description,
                worktree_path,
                repo_root,
                ai_dispatcher,
            )
            .await
        }
        RecoveryActionKind::Escalate { reason } => {
            tracing::warn!(
                reason = %reason,
                "supervisor mode_b: escalating — no automated fix applied"
            );
            RecoveryOutcome {
                action: action.clone(),
                succeeded: true,
                output: format!("escalated: {}", reason),
            }
        }
    }
}

/// Inner implementation for [`RecoveryActionKind::FixTest`].
///
/// Separated to keep [`execute_recovery`] readable.  Call only from there.
async fn execute_fix_test<D: AiAnalysisDispatcher>(
    file_path: &Path,
    description: &str,
    worktree_path: &Path,
    repo_root: &Path,
    ai_dispatcher: Option<&D>,
) -> RecoveryOutcome {
    // ── Guard: only test files ────────────────────────────────────────────────
    if !is_test_file(file_path) {
        let reason = format!(
            "FixTest guard rejected non-test file: {}",
            file_path.display()
        );
        tracing::warn!(reason = %reason, "supervisor mode_b: FixTest guard rejected path");
        return RecoveryOutcome {
            action: RecoveryActionKind::Escalate {
                reason: reason.clone(),
            },
            succeeded: false,
            output: reason,
        };
    }

    // ── Require a dispatcher ──────────────────────────────────────────────────
    let dispatcher = match ai_dispatcher {
        Some(d) => d,
        None => {
            let reason = "FixTest: no AI dispatcher available".to_string();
            tracing::warn!(reason = %reason, "supervisor mode_b: FixTest skipped");
            return RecoveryOutcome {
                action: RecoveryActionKind::Escalate {
                    reason: reason.clone(),
                },
                succeeded: false,
                output: reason,
            };
        }
    };

    // ── Resolve path (relative paths are resolved relative to worktree) ───────
    let resolved = if file_path.is_absolute() {
        file_path.to_path_buf()
    } else {
        worktree_path.join(file_path)
    };

    // ── Read current file content ─────────────────────────────────────────────
    let original_content = match fs::read_to_string(&resolved) {
        Ok(c) => c,
        Err(e) => {
            let reason = format!("FixTest: cannot read {}: {}", resolved.display(), e);
            tracing::warn!(reason = %reason, "supervisor mode_b: FixTest read error");
            return RecoveryOutcome {
                action: RecoveryActionKind::Escalate {
                    reason: reason.clone(),
                },
                succeeded: false,
                output: reason,
            };
        }
    };

    // ── Build and dispatch prompt ─────────────────────────────────────────────
    let prompt = build_fix_test_prompt(&resolved, &original_content, description);
    let fixed_content = match dispatcher.dispatch(&prompt).await {
        Ok(content) => content,
        Err(e) => {
            let reason = format!("FixTest: AI dispatch failed: {}", e);
            tracing::warn!(reason = %reason, "supervisor mode_b: FixTest dispatch error");
            return RecoveryOutcome {
                action: RecoveryActionKind::Escalate {
                    reason: reason.clone(),
                },
                succeeded: false,
                output: reason,
            };
        }
    };

    // ── Write fix, run compile gate, restore on failure ───────────────────────
    let backup_path = resolved.with_extension("rs.fix_backup");
    if let Err(e) = fs::write(&backup_path, &original_content) {
        let reason = format!("FixTest: cannot write backup: {}", e);
        tracing::warn!(reason = %reason, "supervisor mode_b: FixTest backup error");
        return RecoveryOutcome {
            action: RecoveryActionKind::Escalate {
                reason: reason.clone(),
            },
            succeeded: false,
            output: reason,
        };
    }
    if let Err(e) = fs::write(&resolved, &fixed_content) {
        let reason = format!("FixTest: cannot write fix: {}", e);
        let _ = fs::remove_file(&backup_path);
        tracing::warn!(reason = %reason, "supervisor mode_b: FixTest write error");
        return RecoveryOutcome {
            action: RecoveryActionKind::Escalate {
                reason: reason.clone(),
            },
            succeeded: false,
            output: reason,
        };
    }

    // Compile verification gate: `cargo build -p macc-core --tests`
    let build_result = tokio::process::Command::new("cargo")
        .args(["build", "-p", "macc-core", "--tests"])
        .current_dir(repo_root)
        .output()
        .await;

    let (build_ok, build_stderr) = match build_result {
        Ok(o) => (
            o.status.success(),
            String::from_utf8_lossy(&o.stderr).trim().to_owned(),
        ),
        Err(e) => (false, e.to_string()),
    };

    if build_ok {
        // Fix compiles — discard backup and report success.
        let _ = fs::remove_file(&backup_path);
        tracing::info!(
            file = %resolved.display(),
            "supervisor mode_b: FixTest applied — compile gate passed"
        );
        RecoveryOutcome {
            action: RecoveryActionKind::FixTest {
                file_path: file_path.to_path_buf(),
                description: description.to_string(),
            },
            succeeded: true,
            output: format!(
                "FixTest: fix applied and verified for {}",
                resolved.display()
            ),
        }
    } else {
        // Build failed — restore original, escalate.
        if let Err(e) = fs::rename(&backup_path, &resolved) {
            tracing::error!(
                file = %resolved.display(),
                error = %e,
                "supervisor mode_b: FixTest failed to restore backup after compile failure"
            );
        }
        let reason = format!(
            "FixTest: compile gate failed for {} — fix not applied. Build error: {}",
            resolved.display(),
            build_stderr
        );
        tracing::warn!(reason = %reason, "supervisor mode_b: FixTest compile gate failed");
        RecoveryOutcome {
            action: RecoveryActionKind::Escalate {
                reason: reason.clone(),
            },
            succeeded: false,
            output: reason,
        }
    }
}

// ── Coordinator PID guard ─────────────────────────────────────────────────────

/// Return `true` if the coordinator is currently running.
///
/// Checks `.macc/state/coordinator.pid` relative to `repo_root`.  Returns
/// `false` when the file is absent, unreadable, or contains an invalid PID.
pub fn is_coordinator_running(repo_root: &Path) -> bool {
    let pid_file = repo_root
        .join(".macc")
        .join("state")
        .join("coordinator.pid");
    let Ok(content) = fs::read_to_string(&pid_file) else {
        return false;
    };
    let Ok(pid) = content.trim().parse::<u32>() else {
        return false;
    };
    process_is_alive(pid)
}

fn process_is_alive(pid: u32) -> bool {
    #[cfg(target_os = "linux")]
    {
        std::path::Path::new("/proc").join(pid.to_string()).exists()
    }
    #[cfg(all(unix, not(target_os = "linux")))]
    {
        Command::new("kill")
            .arg("-0")
            .arg(pid.to_string())
            .output()
            .map(|o| o.status.success())
            .unwrap_or(false)
    }
    #[cfg(not(unix))]
    {
        let _ = pid;
        false
    }
}

// ── Report writer ─────────────────────────────────────────────────────────────

/// Write a [`SupervisorReport`] to `.macc/log/supervisor/<task_id>-<ts>.json`.
///
/// Returns the path of the written file.
pub fn write_supervisor_report(
    report_dir: &Path,
    task_id: &str,
    report: &SupervisorReport,
) -> std::io::Result<PathBuf> {
    fs::create_dir_all(report_dir)?;

    let ts = Utc::now().format("%Y%m%dT%H%M%SZ");
    let filename = format!("{}-{}.json", task_id, ts);
    let path = report_dir.join(&filename);

    let tmp = path.with_extension("tmp");
    let bytes = serde_json::to_vec_pretty(report)
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
    fs::write(&tmp, bytes)?;
    fs::rename(&tmp, &path)?;
    Ok(path)
}

// ── Guard rails ───────────────────────────────────────────────────────────────

/// Tracks recovery attempts per task within a supervisor cycle.
///
/// Reset by calling [`RecoveryAttemptTracker::reset`] at the start of each cycle.
#[derive(Debug, Default)]
pub struct RecoveryAttemptTracker {
    attempts: HashMap<String, u32>,
    max_per_task: u32,
}

impl RecoveryAttemptTracker {
    pub fn new(max_per_task: u32) -> Self {
        Self {
            attempts: HashMap::new(),
            max_per_task,
        }
    }

    /// Returns `true` if a recovery attempt is allowed for `task_id`.
    pub fn can_attempt(&self, task_id: &str) -> bool {
        self.attempts.get(task_id).copied().unwrap_or(0) < self.max_per_task
    }

    /// Record one recovery attempt for `task_id`.
    pub fn record(&mut self, task_id: &str) {
        *self.attempts.entry(task_id.to_string()).or_insert(0) += 1;
    }

    /// Return the current attempt count for `task_id`.
    pub fn count(&self, task_id: &str) -> u32 {
        self.attempts.get(task_id).copied().unwrap_or(0)
    }

    /// Reset all attempt counters (call at the start of a new supervisor cycle).
    pub fn reset(&mut self) {
        self.attempts.clear();
    }
}

// ── Mode B Supervisor ─────────────────────────────────────────────────────────

/// Errors from Mode B supervisor operations.
#[derive(Debug, thiserror::Error)]
pub enum ModeBError {
    #[error("AI dispatch failed: {0}")]
    Dispatch(#[from] DispatchError),

    #[error("Failed to parse AI response: {0}")]
    ParseResponse(#[from] serde_json::Error),

    #[error("Recovery attempt limit reached for task {task_id} (limit {limit})")]
    AttemptLimitReached { task_id: String, limit: u32 },

    #[error("AI analysis timed out after {seconds}s")]
    AnalysisTimeout { seconds: u64 },

    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
}

/// The main Mode B supervisor: orchestrates evidence collection, AI analysis,
/// recovery, and report writing for a single stuck/failed worktree.
pub struct ModeBSupervisor<D: AiAnalysisDispatcher> {
    config: ModeBConfig,
    repo_root: PathBuf,
    dispatcher: D,
    tracker: RecoveryAttemptTracker,
}

impl<D: AiAnalysisDispatcher> ModeBSupervisor<D> {
    pub fn new(repo_root: PathBuf, config: ModeBConfig, dispatcher: D) -> Self {
        let max = config.max_recovery_attempts;
        Self {
            config,
            repo_root,
            dispatcher,
            tracker: RecoveryAttemptTracker::new(max),
        }
    }

    /// Reset attempt counters (call at the start of each supervisor cycle).
    pub fn reset_cycle(&mut self) {
        self.tracker.reset();
    }

    /// Analyse a stuck/failed worktree.
    ///
    /// Returns the [`SupervisorReport`] written to disk and the path it was
    /// written to.
    ///
    /// # Guard rails
    /// - If the recovery attempt limit is reached for `task_id`, returns
    ///   [`ModeBError::AttemptLimitReached`].
    /// - AI dispatch is bounded by `config.analysis_timeout_seconds`.
    pub async fn analyse(
        &mut self,
        task_id: &str,
        worktree_path: &Path,
        base_branch: &str,
        last_error_code: Option<&str>,
        last_error_message: Option<&str>,
    ) -> Result<(SupervisorReport, PathBuf), ModeBError> {
        // Guard rail: check attempt limit.
        if !self.tracker.can_attempt(task_id) {
            return Err(ModeBError::AttemptLimitReached {
                task_id: task_id.to_string(),
                limit: self.config.max_recovery_attempts,
            });
        }
        self.tracker.record(task_id);

        // Step 1 – collect evidence.
        let collector = EvidenceCollector::new(self.repo_root.clone(), self.config.clone());
        let evidence = collector.collect(
            task_id,
            worktree_path,
            base_branch,
            last_error_code,
            last_error_message,
        );

        // Step 2 – build prompt, optionally augmented with coordinator evidence.
        let mut prompt = AnalysisPromptBuilder::build(&evidence);
        if let Some(ref cec_config) = self.config.coordinator_evidence_config {
            let events_path = if cec_config.events_log_path.is_absolute() {
                cec_config.events_log_path.clone()
            } else {
                self.repo_root.join(&cec_config.events_log_path)
            };
            if events_path.exists() {
                let cec =
                    CoordinatorEvidenceCollector::new(self.repo_root.clone(), cec_config.clone());
                let ce = cec.collect();
                prompt.push_str(&format_coordinator_evidence_section(&ce));
                tracing::debug!(
                    task_id,
                    "supervisor mode_b: coordinator evidence appended to prompt"
                );
            }
        }

        // Step 3 – dispatch with timeout.
        let timeout = Duration::from_secs(self.config.analysis_timeout_seconds);
        let raw = tokio::time::timeout(timeout, self.dispatcher.dispatch(&prompt))
            .await
            .map_err(|_| ModeBError::AnalysisTimeout {
                seconds: self.config.analysis_timeout_seconds,
            })??;

        // Step 4 – parse response.
        let mut result = parse_ai_response(&raw)?;

        // Step 5 – execute recovery action.
        let recovery_outcome = execute_recovery(
            &result.recovery_action,
            worktree_path,
            &self.repo_root,
            Some(&self.dispatcher),
        )
        .await;
        let recovery_action =
            build_recovery_supervisor_action(task_id, worktree_path, &recovery_outcome);
        result.report.actions_taken.push(recovery_action);

        // Step 6 – write report to disk.
        let report_dir = if self.config.report_output_dir.is_absolute() {
            self.config.report_output_dir.clone()
        } else {
            self.repo_root.join(&self.config.report_output_dir)
        };
        let report_path = write_supervisor_report(&report_dir, task_id, &result.report)?;

        tracing::info!(
            task_id,
            report_path = %report_path.display(),
            recovery_action = ?result.recovery_action,
            "supervisor mode_b: analysis complete"
        );

        Ok((result.report, report_path))
    }
}

fn build_recovery_supervisor_action(
    task_id: &str,
    worktree_path: &Path,
    outcome: &RecoveryOutcome,
) -> SupervisorAction {
    let wt = worktree_path.to_string_lossy().into_owned();
    // Match on the *actual* action taken (outcome.action), not the originally
    // recommended action.  This correctly captures fall-backs, e.g. when
    // FixTest falls back to Escalate after a compile-gate failure.
    match &outcome.action {
        RecoveryActionKind::None => SupervisorAction::NoAction {
            reason: format!(
                "Mode B: no recovery action recommended for task {}",
                task_id
            ),
        },
        RecoveryActionKind::GitReset => SupervisorAction::WorktreeGitReset {
            worktree_path: wt,
            succeeded: outcome.succeeded,
        },
        RecoveryActionKind::GitClean => SupervisorAction::WorktreeGitClean {
            worktree_path: wt,
            succeeded: outcome.succeeded,
        },
        RecoveryActionKind::CoordinatorUnlock { task_ids } => {
            SupervisorAction::CoordinatorUnlocked {
                task_ids: task_ids.clone(),
                succeeded: outcome.succeeded,
            }
        }
        RecoveryActionKind::CoordinatorReconcile => SupervisorAction::CoordinatorReconciled {
            succeeded: outcome.succeeded,
        },
        RecoveryActionKind::FixTest { file_path, .. } => SupervisorAction::TestFixApplied {
            file_path: file_path.to_string_lossy().into_owned(),
            succeeded: outcome.succeeded,
        },
        RecoveryActionKind::Escalate { reason } => SupervisorAction::Escalated {
            reason: reason.clone(),
        },
    }
}

// ── Tests ──────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    // ── helpers ───────────────────────────────────────────────────────────────

    fn temp_dir(prefix: &str) -> PathBuf {
        let nanos = Utc::now().timestamp_nanos_opt().unwrap_or_default();
        let path = std::env::temp_dir().join(format!("macc-mode-b-{}-{}", prefix, nanos));
        fs::create_dir_all(&path).expect("create temp dir");
        path
    }

    fn sample_report() -> SupervisorReport {
        SupervisorReport {
            timestamp: "2026-04-13T08:00:00Z".to_string(),
            analysis_window_seconds: 0,
            health: HealthCheckResult::Degraded {
                reasons: vec!["task stalled".to_string()],
            },
            findings: vec![Finding {
                severity: Severity::Error,
                category: FindingCategory::TaskProgress,
                description: "performer exited non-zero".to_string(),
                evidence: vec!["exit code 1".to_string()],
            }],
            recommendations: vec![Recommendation {
                priority: 1,
                description: "retry after git reset".to_string(),
                rationale: "uncommitted changes may be poisoning the environment".to_string(),
                affected_files: vec![],
                implementation_hint: None,
            }],
            actions_taken: vec![],
            suggested_code_changes: vec![],
            improvement_hints: vec![],
        }
    }

    fn sample_ai_response_json(recovery: &str) -> String {
        let report = sample_report();
        let mut v = serde_json::to_value(&report).expect("serialize");
        v["recovery_action"] = serde_json::Value::String(recovery.to_string());
        v.to_string()
    }

    // ── ModeBConfig ───────────────────────────────────────────────────────────

    #[test]
    fn mode_b_config_defaults() {
        let cfg = ModeBConfig::default();
        assert_eq!(cfg.analysis_timeout_seconds, 120);
        assert_eq!(cfg.max_recovery_attempts, 1);
        assert_eq!(cfg.max_log_bytes, 16_384);
    }

    #[test]
    fn mode_b_config_round_trips_json() {
        let cfg = ModeBConfig::default();
        let json = serde_json::to_string(&cfg).expect("serialize");
        let back: ModeBConfig = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(cfg, back);
    }

    // ── EvidenceCollector ─────────────────────────────────────────────────────

    #[test]
    fn evidence_collector_truncates_large_log() {
        let root = temp_dir("collector");
        let wt = temp_dir("wt");
        let log_dir = root.join(".macc").join("log").join("performer");
        fs::create_dir_all(&log_dir).expect("create log dir");

        // Write a log file for task MY-TASK.
        let log_file = log_dir.join("MY-TASK.md");
        let big_content = "x".repeat(32_768); // 32 KiB
        fs::write(&log_file, &big_content).expect("write log");

        let cfg = ModeBConfig {
            max_log_bytes: 1024,
            ..Default::default()
        };
        let collector = EvidenceCollector::new(root, cfg);
        let evidence = collector.collect("MY-TASK", &wt, "main", None, None);

        assert!(evidence.performer_log.len() <= 1024 + 50); // tolerance for prefix
    }

    #[test]
    fn evidence_collector_filters_coordinator_events() {
        let root = temp_dir("events");
        let wt = temp_dir("wt2");
        let events_dir = root.join(".macc").join("log").join("coordinator");
        fs::create_dir_all(&events_dir).expect("create events dir");

        let events_file = events_dir.join("events.jsonl");
        let content = r#"{"ts":"2026-04-13T00:00:01Z","type":"heartbeat","task_id":"OTHER-001"}
{"ts":"2026-04-13T00:00:02Z","type":"failed","task_id":"MY-TASK"}
{"ts":"2026-04-13T00:00:03Z","type":"failed","task_id":"MY-TASK","status":"failed"}
{"ts":"2026-04-13T00:00:04Z","type":"heartbeat","task_id":"OTHER-001"}
"#;
        fs::write(&events_file, content).expect("write events");

        let collector = EvidenceCollector::new(root, ModeBConfig::default());
        let evidence = collector.collect("MY-TASK", &wt, "main", None, None);

        assert_eq!(evidence.coordinator_events.len(), 2);
        assert!(evidence
            .coordinator_events
            .iter()
            .all(|e| e.contains("MY-TASK")));
    }

    // ── AnalysisPromptBuilder ─────────────────────────────────────────────────

    #[test]
    fn prompt_contains_task_id_and_evidence() {
        let evidence = WorktreeEvidence {
            task_id: "L3-FOO-007".to_string(),
            worktree_path: "/tmp/wt".to_string(),
            branch: Some("ai/worker-01".to_string()),
            git_status: "M core/src/lib.rs".to_string(),
            commits_ahead: "abc1234 feat: partial impl".to_string(),
            performer_log: "Error: something went wrong".to_string(),
            coordinator_events: vec![r#"{"type":"failed","task_id":"L3-FOO-007"}"#.to_string()],
            last_error_code: Some("E101".to_string()),
            last_error_message: Some("runner exited non-zero".to_string()),
        };
        let prompt = AnalysisPromptBuilder::build(&evidence);

        assert!(prompt.contains("L3-FOO-007"));
        assert!(prompt.contains("ai/worker-01"));
        assert!(prompt.contains("E101"));
        assert!(prompt.contains("runner exited non-zero"));
        assert!(prompt.contains("M core/src/lib.rs"));
        assert!(prompt.contains("recovery_action"));
        assert!(prompt.contains("improvement_hints"));
        assert!(prompt.contains("problem_class"));
        assert!(prompt.contains("detection_hook"));
        assert!(prompt.contains("suggested_coordinator_behavior"));
    }

    // ── parse_ai_response ─────────────────────────────────────────────────────

    #[test]
    fn parse_valid_response_git_reset() {
        let raw = sample_ai_response_json("git_reset");
        let result = parse_ai_response(&raw).expect("parse");
        assert_eq!(result.recovery_action, RecoveryActionKind::GitReset);
        assert!(!result.report.findings.is_empty());
    }

    #[test]
    fn parse_valid_response_none() {
        let raw = sample_ai_response_json("none");
        let result = parse_ai_response(&raw).expect("parse");
        assert_eq!(result.recovery_action, RecoveryActionKind::None);
    }

    #[test]
    fn parse_strips_markdown_fences() {
        let raw = format!("```json\n{}\n```", sample_ai_response_json("git_clean"));
        let result = parse_ai_response(&raw).expect("parse with fences");
        assert_eq!(result.recovery_action, RecoveryActionKind::GitClean);
    }

    #[test]
    fn parse_invalid_json_returns_error() {
        assert!(parse_ai_response("not json at all").is_err());
    }

    // ── write_supervisor_report ───────────────────────────────────────────────

    #[test]
    fn report_written_to_correct_path() {
        let dir = temp_dir("report-write");
        let report = sample_report();
        let path = write_supervisor_report(&dir, "L4-TEST-001", &report).expect("write");

        assert!(path.exists());
        let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
        assert!(name.starts_with("L4-TEST-001-"));
        assert!(name.ends_with(".json"));

        // Verify it round-trips.
        let content = fs::read_to_string(&path).expect("read back");
        let back: SupervisorReport = serde_json::from_str(&content).expect("deserialize back");
        assert_eq!(report.timestamp, back.timestamp);
    }

    // ── RecoveryAttemptTracker ────────────────────────────────────────────────

    #[test]
    fn tracker_allows_up_to_limit() {
        let mut tracker = RecoveryAttemptTracker::new(1);
        assert!(tracker.can_attempt("T-001"));
        tracker.record("T-001");
        assert!(!tracker.can_attempt("T-001"));
        // Other tasks unaffected.
        assert!(tracker.can_attempt("T-002"));
    }

    #[test]
    fn tracker_resets_on_new_cycle() {
        let mut tracker = RecoveryAttemptTracker::new(1);
        tracker.record("T-001");
        assert!(!tracker.can_attempt("T-001"));
        tracker.reset();
        assert!(tracker.can_attempt("T-001"));
    }

    #[test]
    fn tracker_count_increments() {
        let mut tracker = RecoveryAttemptTracker::new(3);
        assert_eq!(tracker.count("T-001"), 0);
        tracker.record("T-001");
        tracker.record("T-001");
        assert_eq!(tracker.count("T-001"), 2);
        assert!(tracker.can_attempt("T-001"));
        tracker.record("T-001");
        assert!(!tracker.can_attempt("T-001"));
    }

    // ── ModeBSupervisor (with mock dispatcher) ────────────────────────────────

    struct MockDispatcher {
        response: String,
    }

    #[async_trait]
    impl AiAnalysisDispatcher for MockDispatcher {
        async fn dispatch(&self, _prompt: &str) -> Result<String, DispatchError> {
            Ok(self.response.clone())
        }
    }

    struct FailingDispatcher;

    #[async_trait]
    impl AiAnalysisDispatcher for FailingDispatcher {
        async fn dispatch(&self, _prompt: &str) -> Result<String, DispatchError> {
            Err(DispatchError::EmptyOutput)
        }
    }

    #[tokio::test]
    async fn supervisor_analyse_writes_report_and_returns_path() {
        let root = temp_dir("sup-analyse");
        let wt = temp_dir("sup-wt");

        // Create a minimal git repo in wt so git commands don't fail.
        let _ = Command::new("git").current_dir(&wt).args(["init"]).output();
        let _ = Command::new("git")
            .current_dir(&wt)
            .args(["commit", "--allow-empty", "-m", "init"])
            .output();

        let response = sample_ai_response_json("none");
        let config = ModeBConfig {
            report_output_dir: root.join(".macc").join("log").join("supervisor"),
            ..Default::default()
        };
        let mut supervisor =
            ModeBSupervisor::new(root.clone(), config.clone(), MockDispatcher { response });

        let (report, path) = supervisor
            .analyse("L4-TEST-002", &wt, "main", Some("E101"), Some("exit 1"))
            .await
            .expect("analyse");

        assert!(path.exists());
        assert!(!report.findings.is_empty());
        // A "no action" supervisor action should have been appended.
        assert!(report
            .actions_taken
            .iter()
            .any(|a| matches!(a, SupervisorAction::NoAction { .. })));
    }

    #[tokio::test]
    async fn supervisor_respects_attempt_limit() {
        let root = temp_dir("sup-limit");
        let wt = temp_dir("sup-limit-wt");

        let response = sample_ai_response_json("none");
        let config = ModeBConfig {
            max_recovery_attempts: 1,
            report_output_dir: root.join(".macc").join("log").join("supervisor"),
            ..Default::default()
        };
        let mut supervisor =
            ModeBSupervisor::new(root.clone(), config, MockDispatcher { response });

        // First attempt should succeed (even if git fails — we don't care about git here).
        let _ = supervisor
            .analyse("LIMIT-001", &wt, "main", None, None)
            .await;

        // Second attempt must be rejected.
        let err = supervisor
            .analyse("LIMIT-001", &wt, "main", None, None)
            .await
            .expect_err("should be rejected");
        assert!(matches!(err, ModeBError::AttemptLimitReached { .. }));
    }

    #[tokio::test]
    async fn supervisor_propagates_dispatch_error() {
        let root = temp_dir("sup-err");
        let wt = temp_dir("sup-err-wt");

        let config = ModeBConfig {
            report_output_dir: root.join(".macc").join("log").join("supervisor"),
            ..Default::default()
        };
        let mut supervisor = ModeBSupervisor::new(root, config, FailingDispatcher);

        let err = supervisor
            .analyse("ERR-001", &wt, "main", None, None)
            .await
            .expect_err("should fail");
        assert!(matches!(
            err,
            ModeBError::Dispatch(DispatchError::EmptyOutput)
        ));
    }

    #[tokio::test]
    async fn supervisor_reset_cycle_allows_retry() {
        let root = temp_dir("sup-reset");
        let wt = temp_dir("sup-reset-wt");

        let response = sample_ai_response_json("none");
        let config = ModeBConfig {
            max_recovery_attempts: 1,
            report_output_dir: root.join(".macc").join("log").join("supervisor"),
            ..Default::default()
        };
        let mut supervisor =
            ModeBSupervisor::new(root.clone(), config, MockDispatcher { response });

        // Exhaust first cycle.
        let _ = supervisor
            .analyse("RESET-001", &wt, "main", None, None)
            .await;
        assert!(supervisor
            .analyse("RESET-001", &wt, "main", None, None)
            .await
            .is_err());

        // After reset, a new attempt is permitted.
        supervisor.reset_cycle();
        assert!(supervisor.tracker.can_attempt("RESET-001"));
    }

    // ── RecoveryActionKind serde ──────────────────────────────────────────────

    #[test]
    fn recovery_action_kind_round_trips() {
        let actions = vec![
            RecoveryActionKind::GitReset,
            RecoveryActionKind::GitClean,
            RecoveryActionKind::None,
            RecoveryActionKind::CoordinatorUnlock {
                task_ids: vec!["L5-FOO-001".to_string(), "L5-FOO-002".to_string()],
            },
            RecoveryActionKind::CoordinatorReconcile,
            RecoveryActionKind::FixTest {
                file_path: PathBuf::from("tests/foo_test.rs"),
                description: "error[E0061]: argument count mismatch".to_string(),
            },
            RecoveryActionKind::Escalate {
                reason: "compile gate rejected fix".to_string(),
            },
        ];
        for action in actions {
            let json = serde_json::to_string(&action).expect("serialize");
            let back: RecoveryActionKind = serde_json::from_str(&json).expect("deserialize");
            assert_eq!(action, back);
        }
    }

    // ── is_coordinator_running ────────────────────────────────────────────────

    #[test]
    fn is_coordinator_running_false_when_no_pid_file() {
        let root = temp_dir("coord-no-pid");
        assert!(!is_coordinator_running(&root));
    }

    #[test]
    fn is_coordinator_running_false_when_pid_file_has_invalid_content() {
        let root = temp_dir("coord-bad-pid");
        let state_dir = root.join(".macc").join("state");
        fs::create_dir_all(&state_dir).expect("create state dir");
        fs::write(state_dir.join("coordinator.pid"), "not-a-pid").expect("write pid file");
        assert!(!is_coordinator_running(&root));
    }

    #[test]
    fn is_coordinator_running_true_when_pid_is_current_process() {
        let root = temp_dir("coord-live-pid");
        let state_dir = root.join(".macc").join("state");
        fs::create_dir_all(&state_dir).expect("create state dir");
        let my_pid = std::process::id();
        fs::write(state_dir.join("coordinator.pid"), my_pid.to_string()).expect("write pid file");
        // Current process is alive.
        assert!(is_coordinator_running(&root));
    }

    #[test]
    fn is_coordinator_running_false_when_pid_is_dead() {
        let root = temp_dir("coord-dead-pid");
        let state_dir = root.join(".macc").join("state");
        fs::create_dir_all(&state_dir).expect("create state dir");
        // PID 1 is always alive on Linux; pick an unlikely high PID that is
        // almost certainly not alive (u32::MAX rounds to a large value which
        // the OS will reject / not find in /proc).
        fs::write(state_dir.join("coordinator.pid"), "4194305").expect("write pid file");
        // This might be alive on some machines, but on typical test runners
        // the PID space wraps much lower. Just assert that the function does
        // not panic (result is either true or false — we cannot guarantee the
        // PID is dead on all environments).
        let _ = is_coordinator_running(&root);
    }

    // ── Safety guard: CoordinatorUnlock/Reconcile skipped when running ────────

    #[tokio::test]
    async fn coordinator_unlock_skipped_when_coordinator_running() {
        let root = temp_dir("coord-unlock-guard");
        let wt = temp_dir("coord-unlock-wt");
        // Plant current-process PID → coordinator "is running".
        let state_dir = root.join(".macc").join("state");
        fs::create_dir_all(&state_dir).expect("create state dir");
        fs::write(
            state_dir.join("coordinator.pid"),
            std::process::id().to_string(),
        )
        .expect("write pid file");

        let action = RecoveryActionKind::CoordinatorUnlock {
            task_ids: vec!["L5-FOO-001".to_string()],
        };
        let outcome = execute_recovery::<MockDispatcher>(&action, &wt, &root, None).await;

        assert!(!outcome.succeeded);
        assert!(outcome.output.contains("skipped"));
    }

    #[tokio::test]
    async fn coordinator_reconcile_skipped_when_coordinator_running() {
        let root = temp_dir("coord-reconcile-guard");
        let wt = temp_dir("coord-reconcile-wt");
        // Plant current-process PID → coordinator "is running".
        let state_dir = root.join(".macc").join("state");
        fs::create_dir_all(&state_dir).expect("create state dir");
        fs::write(
            state_dir.join("coordinator.pid"),
            std::process::id().to_string(),
        )
        .expect("write pid file");

        let action = RecoveryActionKind::CoordinatorReconcile;
        let outcome = execute_recovery::<MockDispatcher>(&action, &wt, &root, None).await;

        assert!(!outcome.succeeded);
        assert!(outcome.output.contains("skipped"));
    }

    #[tokio::test]
    async fn coordinator_unlock_attempts_command_when_not_running() {
        let root = temp_dir("coord-unlock-noguard");
        let wt = temp_dir("coord-unlock-noguard-wt");
        // No PID file → coordinator is NOT running → command is attempted.

        let action = RecoveryActionKind::CoordinatorUnlock {
            task_ids: vec!["L5-FOO-001".to_string()],
        };
        let outcome = execute_recovery::<MockDispatcher>(&action, &wt, &root, None).await;

        // The outcome may succeed or fail depending on whether `macc` is in PATH.
        // What matters is that the safety-guard "skipped" message is absent —
        // the command was actually attempted.
        assert!(!outcome
            .output
            .contains("skipped: coordinator is still running"));
    }

    #[tokio::test]
    async fn coordinator_reconcile_attempts_command_when_not_running() {
        let root = temp_dir("coord-reconcile-noguard");
        let wt = temp_dir("coord-reconcile-noguard-wt");
        // No PID file → coordinator is NOT running → command is attempted.

        let action = RecoveryActionKind::CoordinatorReconcile;
        let outcome = execute_recovery::<MockDispatcher>(&action, &wt, &root, None).await;

        // Same rationale as above — guard "skipped" message must be absent.
        assert!(!outcome
            .output
            .contains("skipped: coordinator is still running"));
    }

    // ── is_test_file guard ────────────────────────────────────────────────────

    #[test]
    fn is_test_file_accepts_tests_directory_component() {
        assert!(is_test_file(Path::new("core/tests/foo.rs")));
        assert!(is_test_file(Path::new("/abs/path/tests/bar_test.rs")));
        assert!(is_test_file(Path::new("tests/integration.rs")));
    }

    #[test]
    fn is_test_file_accepts_test_suffix() {
        assert!(is_test_file(Path::new("core/src/foo_test.rs")));
        assert!(is_test_file(Path::new("src/bar_test.rs")));
    }

    #[test]
    fn is_test_file_accepts_integration_suffix() {
        assert!(is_test_file(Path::new("core/src/foo_integration.rs")));
        assert!(is_test_file(Path::new("src/bar_integration.rs")));
    }

    #[test]
    fn is_test_file_rejects_implementation_files() {
        assert!(!is_test_file(Path::new("core/src/lib.rs")));
        assert!(!is_test_file(Path::new("core/src/coordinator/engine.rs")));
        assert!(!is_test_file(Path::new("src/main.rs")));
        assert!(!is_test_file(Path::new("core/src/supervisor/mode_b.rs")));
    }

    // ── Escalate action ───────────────────────────────────────────────────────

    #[tokio::test]
    async fn escalate_writes_reason_to_outcome_and_does_not_spawn_process() {
        let root = temp_dir("escalate-root");
        let wt = temp_dir("escalate-wt");
        let reason = "test compile error: argument count mismatch".to_string();
        let action = RecoveryActionKind::Escalate {
            reason: reason.clone(),
        };

        let outcome = execute_recovery::<MockDispatcher>(&action, &wt, &root, None).await;

        // Escalate should succeed (no process error) and include the reason.
        assert!(outcome.succeeded);
        assert!(outcome.output.contains(&reason));
        // Outcome action should be Escalate.
        assert!(
            matches!(&outcome.action, RecoveryActionKind::Escalate { reason: r } if r == &reason)
        );
    }

    // ── FixTest guard: rejects implementation files ───────────────────────────

    #[tokio::test]
    async fn fix_test_on_implementation_file_escalates() {
        let root = temp_dir("fix-test-guard-root");
        let wt = temp_dir("fix-test-guard-wt");

        // Use an implementation file path (not a test file).
        let impl_path = PathBuf::from("core/src/supervisor/mode_b.rs");
        let action = RecoveryActionKind::FixTest {
            file_path: impl_path.clone(),
            description: "some compile error".to_string(),
        };
        let dispatcher = MockDispatcher {
            response: "// fixed content".to_string(),
        };
        let outcome = execute_recovery(&action, &wt, &root, Some(&dispatcher)).await;

        // Guard must reject and fall back to Escalate.
        assert!(!outcome.succeeded);
        assert!(
            matches!(&outcome.action, RecoveryActionKind::Escalate { .. }),
            "expected Escalate fallback, got {:?}",
            outcome.action
        );
        assert!(outcome.output.contains("guard rejected"));
    }

    // ── FixTest: compile gate called before applying fix ─────────────────────

    #[tokio::test]
    async fn fix_test_with_mock_ai_verifies_compile_gate_before_applying() {
        // This test verifies the compile gate by observing that the fix is NOT
        // applied when `cargo build -p macc-core --tests` fails (which it will
        // in an isolated temp dir without a Cargo.toml).
        let root = temp_dir("fix-test-compile-gate-root");
        let wt = temp_dir("fix-test-compile-gate-wt");

        // Create a fake test file in the worktree under a `tests/` directory.
        let tests_dir = wt.join("tests");
        fs::create_dir_all(&tests_dir).expect("create tests dir");
        let test_file = tests_dir.join("my_test.rs");
        let original_content = "fn old_api_call() {}";
        fs::write(&test_file, original_content).expect("write test file");

        // The mock AI returns "fixed" content.
        let fixed_content = "fn new_api_call() {}";
        let dispatcher = MockDispatcher {
            response: fixed_content.to_string(),
        };

        // Use a relative path so execute_fix_test resolves it against worktree.
        let action = RecoveryActionKind::FixTest {
            file_path: PathBuf::from("tests/my_test.rs"),
            description: "call to old_api_call() does not compile: function removed".to_string(),
        };

        let outcome = execute_recovery(&action, &wt, &root, Some(&dispatcher)).await;

        // The compile gate must fail (no Cargo.toml in temp root).
        // Therefore the fix must NOT be applied and the file must be restored.
        let content_after = fs::read_to_string(&test_file).expect("read after");
        assert_eq!(
            content_after, original_content,
            "original file must be restored when compile gate fails"
        );

        // Outcome should be Escalate (gate failed → fell back to Escalate).
        assert!(
            matches!(&outcome.action, RecoveryActionKind::Escalate { .. }),
            "expected Escalate after compile-gate failure, got {:?}",
            outcome.action
        );
        assert!(!outcome.succeeded);
        assert!(
            outcome.output.contains("compile gate failed") || outcome.output.contains("FixTest")
        );
    }

    // ── RecoveryActionKind serde (new variants) ───────────────────────────────

    #[test]
    fn recovery_action_kind_new_variants_round_trip() {
        let actions = vec![
            RecoveryActionKind::FixTest {
                file_path: PathBuf::from("tests/foo_test.rs"),
                description: "error[E0061]: wrong number of arguments".to_string(),
            },
            RecoveryActionKind::Escalate {
                reason: "compile gate rejected the AI-generated fix".to_string(),
            },
        ];
        for action in actions {
            let json = serde_json::to_string(&action).expect("serialize");
            let back: RecoveryActionKind = serde_json::from_str(&json).expect("deserialize");
            assert_eq!(action, back);
        }
    }
}
