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
//! Mode B **must not** modify coordinator state (task registry, locks, or IPC
//! state). Only worktree-local git operations are permitted as recovery actions.

use crate::supervisor::{
    Finding, FindingCategory, HealthCheckResult, Recommendation, Severity, SupervisorAction,
    SupervisorReport,
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
/// Only worktree-local git operations are permitted.  Coordinator state is
/// never modified here.
pub fn execute_recovery(action: &RecoveryActionKind, worktree_path: &Path) -> RecoveryOutcome {
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

        // Step 2 – build prompt.
        let prompt = AnalysisPromptBuilder::build(&evidence);

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
        let recovery_outcome = execute_recovery(&result.recovery_action, worktree_path);
        let recovery_action = build_recovery_supervisor_action(
            task_id,
            worktree_path,
            &result.recovery_action,
            &recovery_outcome,
        );
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
    action: &RecoveryActionKind,
    outcome: &RecoveryOutcome,
) -> SupervisorAction {
    let wt = worktree_path.to_string_lossy().into_owned();
    match action {
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
    }
}

// ── Tests ──────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

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
        for action in [
            RecoveryActionKind::GitReset,
            RecoveryActionKind::GitClean,
            RecoveryActionKind::None,
        ] {
            let json = serde_json::to_string(&action).expect("serialize");
            let back: RecoveryActionKind = serde_json::from_str(&json).expect("deserialize");
            assert_eq!(action, back);
        }
    }
}
