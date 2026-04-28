//! Supervisor coordinator-level evidence collector and analysis prompt builder.
//!
//! When the coordinator itself exits with `result: failed` (all tasks exhausted,
//! clean exit with a failure result), the per-worktree Mode B analysis is not
//! sufficient — the supervisor needs a coordinator-scoped view of the full failure
//! chain.  This module provides:
//!
//! - [`CoordinatorEvidenceCollector`]: reads the last N coordinator events,
//!   task registry state counts, performer log excerpts for failed tasks, and the
//!   coordinator run log footer.
//! - [`CoordinatorAnalysisPromptBuilder`]: converts [`CoordinatorEvidence`] into a
//!   structured prompt that an AI can use to reason about the full failure chain
//!   and suggest recovery actions or improvement hints.
//!
//! # Design constraints
//! - This module must stay independent of `mode_b.rs` (which is already at the
//!   anti-god-file threshold).
//! - The call path that invokes the collector from Mode B is wired by L5-SUP-005.
//! - Recovery execution is out of scope here (L5-SUP-005).
//! - [`SupervisorReport`] schema is not changed here (L5-SUP-007).

use crate::coordinator::model::{TaskRegistry, TaskRuntime};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

// ── Configuration ─────────────────────────────────────────────────────────────

/// Configuration for the coordinator-level evidence collector.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct CoordinatorEvidenceConfig {
    /// Maximum number of coordinator event lines to read from `events.jsonl`.
    #[serde(default = "default_max_events")]
    pub max_events: usize,

    /// Maximum number of performer log lines to include per failed task.
    #[serde(default = "default_max_performer_log_lines")]
    pub max_performer_log_lines: usize,

    /// Number of lines to read from the coordinator run log footer.
    #[serde(default = "default_run_log_footer_lines")]
    pub run_log_footer_lines: usize,

    /// Path to `events.jsonl` (relative to repo root, or absolute).
    #[serde(default = "default_events_log_path")]
    pub events_log_path: PathBuf,

    /// Path to the task registry JSON file (relative to repo root, or absolute).
    #[serde(default = "default_registry_path")]
    pub registry_path: PathBuf,

    /// Directory containing performer logs (relative to repo root, or absolute).
    #[serde(default = "default_performer_log_dir")]
    pub performer_log_dir: PathBuf,

    /// Path to the coordinator run log (relative to repo root, or absolute).
    #[serde(default = "default_run_log_path")]
    pub run_log_path: PathBuf,
}

fn default_max_events() -> usize {
    200
}
fn default_max_performer_log_lines() -> usize {
    100
}
fn default_run_log_footer_lines() -> usize {
    50
}
fn default_events_log_path() -> PathBuf {
    PathBuf::from(".macc/log/coordinator/events.jsonl")
}
fn default_registry_path() -> PathBuf {
    PathBuf::from(".macc/automation/task/task_registry.json")
}
fn default_performer_log_dir() -> PathBuf {
    PathBuf::from(".macc/log/performer")
}
fn default_run_log_path() -> PathBuf {
    PathBuf::from(".macc/log/coordinator/coordinator.log")
}

impl Default for CoordinatorEvidenceConfig {
    fn default() -> Self {
        Self {
            max_events: default_max_events(),
            max_performer_log_lines: default_max_performer_log_lines(),
            run_log_footer_lines: default_run_log_footer_lines(),
            events_log_path: default_events_log_path(),
            registry_path: default_registry_path(),
            performer_log_dir: default_performer_log_dir(),
            run_log_path: default_run_log_path(),
        }
    }
}

// ── Evidence types ────────────────────────────────────────────────────────────

/// Counts of tasks broken down by workflow state.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct TaskStateCounts {
    /// Map from workflow state name (e.g. `"failed"`, `"todo"`) to task count.
    pub counts: HashMap<String, usize>,
    /// Total tasks across all states.
    pub total: usize,
}

impl TaskStateCounts {
    fn from_registry(registry: &TaskRegistry) -> Self {
        let mut counts: HashMap<String, usize> = HashMap::new();
        for task in &registry.tasks {
            *counts.entry(task.state.clone()).or_insert(0) += 1;
        }
        let total = registry.tasks.len();
        Self { counts, total }
    }
}

/// A summary of a single failed task, extracted from the task registry.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct FailedTaskSummary {
    /// Task identifier.
    pub task_id: String,
    /// Human-readable title, if available.
    pub title: Option<String>,
    /// Workflow state at the time of collection.
    pub state: String,
    /// Last error recorded in `task_runtime.last_error`.
    pub last_error: Option<String>,
    /// Last error code recorded in `task_runtime.last_error_code`.
    pub last_error_code: Option<String>,
    /// Number of retry attempts.
    pub retries: Option<i64>,
    /// Recent performer log lines (up to `max_performer_log_lines`).
    pub performer_log_tail: Vec<String>,
}

/// All evidence gathered at the coordinator level for a single run.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct CoordinatorEvidence {
    /// RFC-3339 timestamp when evidence was collected.
    pub collected_at: String,

    /// Last N lines from `events.jsonl`.
    pub recent_events: Vec<String>,

    /// Task counts broken down by workflow state.
    pub task_state_counts: TaskStateCounts,

    /// Summaries of failed/blocked tasks including last errors and log tails.
    pub failed_tasks: Vec<FailedTaskSummary>,

    /// Tail of the coordinator run log (footer containing exit reason/result).
    pub run_log_footer: Vec<String>,

    /// Coordinator exit result parsed from the run log footer, if found.
    /// E.g. `"failed"`, `"success"`, or `None` if not determinable.
    pub coordinator_result: Option<String>,

    /// Coordinator exit reason parsed from the run log footer, if found.
    pub coordinator_exit_reason: Option<String>,
}

// ── Evidence collector ────────────────────────────────────────────────────────

/// Collects coordinator-level evidence for AI analysis.
///
/// Reads from:
/// - `events.jsonl` — coordinator event stream
/// - `task_registry.json` — task workflow states and runtime metadata
/// - performer log files — per-task log tails for failed tasks
/// - coordinator run log — exit result/reason footer
pub struct CoordinatorEvidenceCollector {
    repo_root: PathBuf,
    config: CoordinatorEvidenceConfig,
}

impl CoordinatorEvidenceCollector {
    pub fn new(repo_root: PathBuf, config: CoordinatorEvidenceConfig) -> Self {
        Self { repo_root, config }
    }

    /// Collect all coordinator-level evidence.
    ///
    /// Missing files are silently skipped; the corresponding evidence fields will
    /// be empty.  This is intentional: evidence collection must not fail the
    /// supervisor cycle because a log is absent.
    pub fn collect(&self) -> CoordinatorEvidence {
        let collected_at = chrono::Utc::now().to_rfc3339();

        let recent_events = self.read_recent_events();
        let (task_state_counts, failed_tasks) = self.read_registry_state();
        let run_log_footer = self.read_run_log_footer();
        let (coordinator_result, coordinator_exit_reason) =
            parse_coordinator_result(&run_log_footer);

        CoordinatorEvidence {
            collected_at,
            recent_events,
            task_state_counts,
            failed_tasks,
            run_log_footer,
            coordinator_result,
            coordinator_exit_reason,
        }
    }

    fn resolve(&self, relative: &Path) -> PathBuf {
        if relative.is_absolute() {
            relative.to_owned()
        } else {
            self.repo_root.join(relative)
        }
    }

    fn read_recent_events(&self) -> Vec<String> {
        let path = self.resolve(&self.config.events_log_path);
        let Ok(content) = fs::read_to_string(&path) else {
            return Vec::new();
        };
        let all: Vec<String> = content
            .lines()
            .filter(|l| !l.trim().is_empty())
            .map(str::to_owned)
            .collect();

        let n = self.config.max_events;
        if all.len() > n {
            all[all.len() - n..].to_vec()
        } else {
            all
        }
    }

    /// Read the task registry and return state counts + failed task summaries.
    fn read_registry_state(&self) -> (TaskStateCounts, Vec<FailedTaskSummary>) {
        let path = self.resolve(&self.config.registry_path);
        let Ok(content) = fs::read_to_string(&path) else {
            return (TaskStateCounts::default(), Vec::new());
        };
        let Ok(value) = serde_json::from_str::<serde_json::Value>(&content) else {
            return (TaskStateCounts::default(), Vec::new());
        };
        let Ok(registry) = TaskRegistry::from_value(&value) else {
            return (TaskStateCounts::default(), Vec::new());
        };

        let counts = TaskStateCounts::from_registry(&registry);

        // Collect summaries for failed/blocked tasks.
        let failed_states = ["failed", "abandoned", "blocked"];
        let mut failed_tasks: Vec<FailedTaskSummary> = Vec::new();

        for task in &registry.tasks {
            if !failed_states.contains(&task.state.as_str()) {
                continue;
            }
            let rt = &task.task_runtime;
            let performer_log_tail =
                self.read_performer_log_tail(&task.id, self.config.max_performer_log_lines);

            failed_tasks.push(FailedTaskSummary {
                task_id: task.id.clone(),
                title: task.title.clone(),
                state: task.state.clone(),
                last_error: rt.last_error.clone(),
                last_error_code: rt.last_error_code.clone(),
                retries: retries_from_runtime(rt),
                performer_log_tail,
            });
        }

        (counts, failed_tasks)
    }

    fn read_performer_log_tail(&self, task_id: &str, max_lines: usize) -> Vec<String> {
        let log_dir = self.resolve(&self.config.performer_log_dir);
        let Ok(entries) = fs::read_dir(&log_dir) else {
            return Vec::new();
        };
        for entry in entries.flatten() {
            let path = entry.path();
            let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
            if name.contains(task_id) && path.is_file() {
                if let Ok(content) = fs::read_to_string(&path) {
                    let lines: Vec<String> = content
                        .lines()
                        .map(str::to_owned)
                        .collect();
                    let tail = if lines.len() > max_lines {
                        lines[lines.len() - max_lines..].to_vec()
                    } else {
                        lines
                    };
                    return tail;
                }
            }
        }
        Vec::new()
    }

    fn read_run_log_footer(&self) -> Vec<String> {
        let path = self.resolve(&self.config.run_log_path);
        let Ok(content) = fs::read_to_string(&path) else {
            return Vec::new();
        };
        let lines: Vec<String> = content
            .lines()
            .map(str::to_owned)
            .collect();
        let n = self.config.run_log_footer_lines;
        if lines.len() > n {
            lines[lines.len() - n..].to_vec()
        } else {
            lines
        }
    }
}

/// Extract the coordinator result and exit reason from run log footer lines.
///
/// Looks for patterns like `result: failed`, `result: success`, and
/// `exit_reason: <reason>` that coordinators conventionally emit as their last
/// structured output.
fn parse_coordinator_result(footer: &[String]) -> (Option<String>, Option<String>) {
    let mut result: Option<String> = None;
    let mut exit_reason: Option<String> = None;

    for line in footer.iter().rev() {
        let l = line.trim().to_lowercase();
        if result.is_none() {
            if l.contains("result: failed") || l.contains("\"result\":\"failed\"") {
                result = Some("failed".to_string());
            } else if l.contains("result: success") || l.contains("\"result\":\"success\"") {
                result = Some("success".to_string());
            }
        }
        if exit_reason.is_none() {
            // Try `exit_reason: <value>` pattern.
            if let Some(pos) = l.find("exit_reason:") {
                let raw = line[pos + "exit_reason:".len()..].trim();
                let cleaned = raw.trim_matches(|c| c == '"' || c == ',' || c == '}');
                if !cleaned.is_empty() {
                    exit_reason = Some(cleaned.to_owned());
                }
            }
            // Also try JSON `"exit_reason": "..."` pattern.
            if exit_reason.is_none() {
                if let Some(pos) = l.find("\"exit_reason\"") {
                    let after = &line[pos + "\"exit_reason\"".len()..];
                    if let Some(val_start) = after.find('"') {
                        let rest = &after[val_start + 1..];
                        if let Some(val_end) = rest.find('"') {
                            let val = &rest[..val_end];
                            if !val.is_empty() {
                                exit_reason = Some(val.to_owned());
                            }
                        }
                    }
                }
            }
        }
        if result.is_some() && exit_reason.is_some() {
            break;
        }
    }

    (result, exit_reason)
}

fn retries_from_runtime(rt: &TaskRuntime) -> Option<i64> {
    rt.retries.or(rt.attempt)
}

// ── Prompt builder ────────────────────────────────────────────────────────────

/// Builds a coordinator-level analysis prompt from [`CoordinatorEvidence`].
///
/// The resulting prompt is distinct from the per-worktree Mode B prompt: it covers
/// the full coordinator failure chain, which tasks failed and why, the coordinator
/// exit reason, and asks for recovery actions and improvement hints.
pub struct CoordinatorAnalysisPromptBuilder;

impl CoordinatorAnalysisPromptBuilder {
    /// Build the coordinator-context analysis prompt.
    pub fn build(evidence: &CoordinatorEvidence) -> String {
        let state_summary = format_state_summary(&evidence.task_state_counts);
        let failed_tasks_text = format_failed_tasks(&evidence.failed_tasks);
        let recent_events_text = if evidence.recent_events.is_empty() {
            "(none)".to_string()
        } else {
            evidence.recent_events.join("\n")
        };
        let run_log_footer_text = if evidence.run_log_footer.is_empty() {
            "(no coordinator run log found)".to_string()
        } else {
            evidence.run_log_footer.join("\n")
        };
        let coordinator_result = evidence
            .coordinator_result
            .as_deref()
            .unwrap_or("(unknown)");
        let coordinator_exit_reason = evidence
            .coordinator_exit_reason
            .as_deref()
            .unwrap_or("(not recorded)");

        format!(
            r#"You are the MACC supervisor. The coordinator has exited with a failure result.
Analyze the evidence below and produce a structured JSON report describing the full failure chain,
which tasks failed and why, and concrete recovery actions and improvement hints.

## Coordinator run summary
Result:      {coordinator_result}
Exit reason: {coordinator_exit_reason}
Collected:   {collected_at}

## Task registry state
{state_summary}

## Failed / blocked task details
{failed_tasks_text}

## Recent coordinator events (last {event_count} lines from events.jsonl)
```
{recent_events_text}
```

## Coordinator run log footer
```
{run_log_footer_text}
```

## Known MACC error codes
- E101: Runner exited non-zero (retryable)
- E102: Tool runner not found
- E201: Requested unavailable tool
- E301: Worktree missing
- E302: PRD missing
- E401: Task registry read/write failure
- E402: Task state transition invalid
- E501: Merge conflict
- E503: Merge blocked by policy (not retryable, requires operator action)
- E601: Rate-limited / transient 429 or 529 (retryable with backoff)
- E602: Quota exhausted (requires operator action)

## Required output format
Respond with ONLY valid JSON matching this schema (no markdown fences, no extra text):
{{
  "timestamp": "<RFC-3339 UTC>",
  "analysis_window_seconds": 0,
  "health": {{"status": "crashed", "exit_code": null}},
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
      "description": "<recovery action or improvement>",
      "rationale": "<why — cite specific failed tasks or error codes>",
      "affected_files": [],
      "implementation_hint": "<optional MACC command or code pointer>"
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
  ]
}}
"#,
            coordinator_result = coordinator_result,
            coordinator_exit_reason = coordinator_exit_reason,
            collected_at = evidence.collected_at,
            state_summary = state_summary,
            failed_tasks_text = failed_tasks_text,
            event_count = evidence.recent_events.len(),
            recent_events_text = recent_events_text,
            run_log_footer_text = run_log_footer_text,
        )
    }
}

fn format_state_summary(counts: &TaskStateCounts) -> String {
    if counts.total == 0 {
        return "No tasks found in registry.".to_string();
    }
    let mut lines: Vec<String> = vec![format!("Total tasks: {}", counts.total)];
    // Sort for deterministic output.
    let mut sorted: Vec<(&String, &usize)> = counts.counts.iter().collect();
    sorted.sort_by_key(|(k, _)| k.as_str());
    for (state, count) in sorted {
        lines.push(format!("  {}: {}", state, count));
    }
    lines.join("\n")
}

fn format_failed_tasks(tasks: &[FailedTaskSummary]) -> String {
    if tasks.is_empty() {
        return "(no failed or blocked tasks found in registry)".to_string();
    }
    let mut parts: Vec<String> = Vec::new();
    for t in tasks {
        let title = t.title.as_deref().unwrap_or("(no title)");
        let last_error = t.last_error.as_deref().unwrap_or("(none)");
        let last_error_code = t.last_error_code.as_deref().unwrap_or("(none)");
        let retries = t
            .retries
            .map(|r| r.to_string())
            .unwrap_or_else(|| "(none)".to_string());

        let log_tail = if t.performer_log_tail.is_empty() {
            "(no performer log found)".to_string()
        } else {
            t.performer_log_tail.join("\n")
        };

        parts.push(format!(
            "### Task: {} — {} [state: {}]\nError code:  {}\nLast error:  {}\nRetries:     {}\nPerformer log tail:\n```\n{}\n```",
            t.task_id, title, t.state,
            last_error_code, last_error, retries,
            log_tail,
        ));
    }
    parts.join("\n\n")
}

// ── Tests ──────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn temp_dir(prefix: &str) -> PathBuf {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.subsec_nanos())
            .unwrap_or(0);
        let path = std::env::temp_dir().join(format!("macc-coord-sup-{}-{}", prefix, nanos));
        fs::create_dir_all(&path).expect("create temp dir");
        path
    }

    /// Build a minimal fixture repo root with events, registry, and run log.
    fn fixture_repo(
        events: &[&str],
        registry_json: &str,
        run_log: &[&str],
    ) -> PathBuf {
        let root = temp_dir("fixture");

        // events.jsonl
        let coordinator_log_dir = root.join(".macc").join("log").join("coordinator");
        fs::create_dir_all(&coordinator_log_dir).expect("create coordinator log dir");
        let events_path = coordinator_log_dir.join("events.jsonl");
        let mut f = fs::File::create(&events_path).expect("create events.jsonl");
        for line in events {
            writeln!(f, "{}", line).expect("write event line");
        }

        // run log
        let run_log_path = coordinator_log_dir.join("coordinator.log");
        let mut f = fs::File::create(&run_log_path).expect("create coordinator.log");
        for line in run_log {
            writeln!(f, "{}", line).expect("write run log line");
        }

        // task registry
        let registry_dir = root
            .join(".macc")
            .join("automation")
            .join("task");
        fs::create_dir_all(&registry_dir).expect("create registry dir");
        fs::write(registry_dir.join("task_registry.json"), registry_json)
            .expect("write registry");

        // performer log dir (empty for most tests)
        let performer_log_dir = root.join(".macc").join("log").join("performer");
        fs::create_dir_all(&performer_log_dir).expect("create performer log dir");

        root
    }

    fn default_config() -> CoordinatorEvidenceConfig {
        CoordinatorEvidenceConfig::default()
    }

    // ── CoordinatorEvidenceConfig ─────────────────────────────────────────────

    #[test]
    fn config_defaults_are_sane() {
        let cfg = CoordinatorEvidenceConfig::default();
        assert_eq!(cfg.max_events, 200);
        assert_eq!(cfg.max_performer_log_lines, 100);
        assert_eq!(cfg.run_log_footer_lines, 50);
    }

    #[test]
    fn config_round_trips_json() {
        let cfg = CoordinatorEvidenceConfig::default();
        let json = serde_json::to_string(&cfg).expect("serialize");
        let back: CoordinatorEvidenceConfig = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(cfg, back);
    }

    // ── Evidence collection: events ───────────────────────────────────────────

    #[test]
    fn collects_recent_events_up_to_max() {
        let events: Vec<String> = (0..300)
            .map(|i| format!(r#"{{"event_type":"heartbeat","seq":{}}}"#, i))
            .collect();
        let event_refs: Vec<&str> = events.iter().map(String::as_str).collect();

        let root = fixture_repo(&event_refs, r#"{"tasks":[]}"#, &[]);
        let cfg = CoordinatorEvidenceConfig {
            max_events: 200,
            ..default_config()
        };
        let collector = CoordinatorEvidenceCollector::new(root, cfg);
        let evidence = collector.collect();

        assert_eq!(evidence.recent_events.len(), 200);
        // Should be the last 200 events.
        assert!(evidence.recent_events[0].contains("\"seq\":100"));
        assert!(evidence.recent_events[199].contains("\"seq\":299"));
    }

    #[test]
    fn handles_missing_events_log_gracefully() {
        let root = temp_dir("no-events");
        let collector =
            CoordinatorEvidenceCollector::new(root, CoordinatorEvidenceConfig::default());
        let evidence = collector.collect();
        assert!(evidence.recent_events.is_empty());
    }

    // ── Evidence collection: registry ─────────────────────────────────────────

    #[test]
    fn counts_tasks_by_state() {
        let registry_json = r#"{
            "tasks": [
                {"id": "T1", "state": "merged"},
                {"id": "T2", "state": "merged"},
                {"id": "T3", "state": "failed"},
                {"id": "T4", "state": "todo"}
            ]
        }"#;
        let root = fixture_repo(&[], registry_json, &[]);
        let collector = CoordinatorEvidenceCollector::new(root, default_config());
        let evidence = collector.collect();

        assert_eq!(evidence.task_state_counts.total, 4);
        assert_eq!(evidence.task_state_counts.counts["merged"], 2);
        assert_eq!(evidence.task_state_counts.counts["failed"], 1);
        assert_eq!(evidence.task_state_counts.counts["todo"], 1);
    }

    #[test]
    fn failed_tasks_include_error_metadata() {
        let registry_json = r#"{
            "tasks": [
                {
                    "id": "FAIL-001",
                    "title": "Do something",
                    "state": "failed",
                    "task_runtime": {
                        "last_error": "runner exited non-zero",
                        "last_error_code": "E101",
                        "retries": 2
                    }
                }
            ]
        }"#;
        let root = fixture_repo(&[], registry_json, &[]);
        let collector = CoordinatorEvidenceCollector::new(root, default_config());
        let evidence = collector.collect();

        assert_eq!(evidence.failed_tasks.len(), 1);
        let ft = &evidence.failed_tasks[0];
        assert_eq!(ft.task_id, "FAIL-001");
        assert_eq!(ft.title.as_deref(), Some("Do something"));
        assert_eq!(ft.state, "failed");
        assert_eq!(ft.last_error.as_deref(), Some("runner exited non-zero"));
        assert_eq!(ft.last_error_code.as_deref(), Some("E101"));
        assert_eq!(ft.retries, Some(2));
    }

    #[test]
    fn non_failed_tasks_are_excluded_from_failed_summaries() {
        let registry_json = r#"{
            "tasks": [
                {"id": "T1", "state": "merged"},
                {"id": "T2", "state": "in_progress"},
                {"id": "T3", "state": "todo"}
            ]
        }"#;
        let root = fixture_repo(&[], registry_json, &[]);
        let collector = CoordinatorEvidenceCollector::new(root, default_config());
        let evidence = collector.collect();
        assert!(evidence.failed_tasks.is_empty());
    }

    #[test]
    fn handles_missing_registry_gracefully() {
        let root = temp_dir("no-registry");
        let collector = CoordinatorEvidenceCollector::new(root, default_config());
        let evidence = collector.collect();
        assert_eq!(evidence.task_state_counts.total, 0);
        assert!(evidence.failed_tasks.is_empty());
    }

    // ── Evidence collection: performer log tail ───────────────────────────────

    #[test]
    fn reads_performer_log_tail_for_failed_task() {
        let registry_json = r#"{
            "tasks": [{"id": "MY-TASK", "state": "failed"}]
        }"#;
        let root = fixture_repo(&[], registry_json, &[]);

        // Write a performer log with 200 lines.
        let performer_dir = root.join(".macc").join("log").join("performer");
        let log_path = performer_dir.join("MY-TASK.log");
        let content: String = (0..200)
            .map(|i| format!("line {}", i))
            .collect::<Vec<_>>()
            .join("\n");
        fs::write(&log_path, content).expect("write performer log");

        let cfg = CoordinatorEvidenceConfig {
            max_performer_log_lines: 50,
            ..default_config()
        };
        let collector = CoordinatorEvidenceCollector::new(root, cfg);
        let evidence = collector.collect();

        assert_eq!(evidence.failed_tasks.len(), 1);
        let tail = &evidence.failed_tasks[0].performer_log_tail;
        assert_eq!(tail.len(), 50);
        // Should be the last 50 lines (line 150 .. line 199).
        assert_eq!(tail[0], "line 150");
        assert_eq!(tail[49], "line 199");
    }

    // ── Evidence collection: run log footer ───────────────────────────────────

    #[test]
    fn parses_coordinator_result_failed_from_run_log() {
        let run_log = &[
            "coordinator started",
            "dispatching tasks",
            "all tasks exhausted",
            "result: failed",
            "exit_reason: all_tasks_exhausted_with_failures",
        ];
        let root = fixture_repo(&[], r#"{"tasks":[]}"#, run_log);
        let collector = CoordinatorEvidenceCollector::new(root, default_config());
        let evidence = collector.collect();

        assert_eq!(evidence.coordinator_result.as_deref(), Some("failed"));
        assert_eq!(
            evidence.coordinator_exit_reason.as_deref(),
            Some("all_tasks_exhausted_with_failures")
        );
    }

    #[test]
    fn parses_coordinator_result_success_from_run_log() {
        let run_log = &["coordinator finished", "result: success"];
        let root = fixture_repo(&[], r#"{"tasks":[]}"#, run_log);
        let collector = CoordinatorEvidenceCollector::new(root, default_config());
        let evidence = collector.collect();
        assert_eq!(evidence.coordinator_result.as_deref(), Some("success"));
    }

    #[test]
    fn handles_missing_run_log_gracefully() {
        let root = temp_dir("no-log");
        let collector = CoordinatorEvidenceCollector::new(root, default_config());
        let evidence = collector.collect();
        assert!(evidence.run_log_footer.is_empty());
        assert!(evidence.coordinator_result.is_none());
        assert!(evidence.coordinator_exit_reason.is_none());
    }

    // ── CoordinatorAnalysisPromptBuilder ──────────────────────────────────────

    #[test]
    fn prompt_includes_coordinator_result_and_exit_reason() {
        let evidence = CoordinatorEvidence {
            collected_at: "2026-04-15T12:00:00Z".to_string(),
            coordinator_result: Some("failed".to_string()),
            coordinator_exit_reason: Some("quota_exhausted".to_string()),
            ..Default::default()
        };
        let prompt = CoordinatorAnalysisPromptBuilder::build(&evidence);
        assert!(prompt.contains("Result:      failed"));
        assert!(prompt.contains("Exit reason: quota_exhausted"));
    }

    #[test]
    fn prompt_includes_task_state_summary() {
        let mut counts = HashMap::new();
        counts.insert("merged".to_string(), 5);
        counts.insert("failed".to_string(), 2);
        let evidence = CoordinatorEvidence {
            collected_at: "2026-04-15T12:00:00Z".to_string(),
            task_state_counts: TaskStateCounts { counts, total: 7 },
            ..Default::default()
        };
        let prompt = CoordinatorAnalysisPromptBuilder::build(&evidence);
        assert!(prompt.contains("Total tasks: 7"));
        assert!(prompt.contains("failed: 2"));
        assert!(prompt.contains("merged: 5"));
    }

    #[test]
    fn prompt_includes_failed_task_details() {
        let evidence = CoordinatorEvidence {
            collected_at: "2026-04-15T12:00:00Z".to_string(),
            failed_tasks: vec![FailedTaskSummary {
                task_id: "FAIL-001".to_string(),
                title: Some("Implement X".to_string()),
                state: "failed".to_string(),
                last_error: Some("runner exited non-zero".to_string()),
                last_error_code: Some("E101".to_string()),
                retries: Some(3),
                performer_log_tail: vec!["error: process exit 1".to_string()],
            }],
            ..Default::default()
        };
        let prompt = CoordinatorAnalysisPromptBuilder::build(&evidence);
        assert!(prompt.contains("FAIL-001"));
        assert!(prompt.contains("Implement X"));
        assert!(prompt.contains("E101"));
        assert!(prompt.contains("runner exited non-zero"));
        assert!(prompt.contains("error: process exit 1"));
    }

    #[test]
    fn prompt_includes_recent_events() {
        let evidence = CoordinatorEvidence {
            collected_at: "2026-04-15T12:00:00Z".to_string(),
            recent_events: vec![
                r#"{"event_type":"heartbeat"}"#.to_string(),
                r#"{"event_type":"task_failed","task_id":"T1"}"#.to_string(),
            ],
            ..Default::default()
        };
        let prompt = CoordinatorAnalysisPromptBuilder::build(&evidence);
        assert!(prompt.contains("heartbeat"));
        assert!(prompt.contains("task_failed"));
    }

    #[test]
    fn prompt_requests_json_output() {
        let evidence = CoordinatorEvidence {
            collected_at: "2026-04-15T12:00:00Z".to_string(),
            ..Default::default()
        };
        let prompt = CoordinatorAnalysisPromptBuilder::build(&evidence);
        assert!(prompt.contains("Respond with ONLY valid JSON"));
        assert!(prompt.contains("\"findings\""));
        assert!(prompt.contains("\"recommendations\""));
        assert!(prompt.contains("\"improvement_hints\""));
        assert!(prompt.contains("problem_class"));
        assert!(prompt.contains("detection_hook"));
        assert!(prompt.contains("suggested_coordinator_behavior"));
    }

    // ── parse_coordinator_result ──────────────────────────────────────────────

    #[test]
    fn parse_result_handles_json_style() {
        let footer = vec![
            r#"{"result":"failed","exit_reason":"all_exhausted"}"#.to_string(),
        ];
        let (result, reason) = parse_coordinator_result(&footer);
        assert_eq!(result.as_deref(), Some("failed"));
        assert_eq!(reason.as_deref(), Some("all_exhausted"));
    }

    #[test]
    fn parse_result_handles_plain_text_style() {
        let footer = vec![
            "coordinator loop complete".to_string(),
            "result: success".to_string(),
        ];
        let (result, _) = parse_coordinator_result(&footer);
        assert_eq!(result.as_deref(), Some("success"));
    }

    #[test]
    fn parse_result_returns_none_when_absent() {
        let footer = vec!["some unrelated line".to_string()];
        let (result, reason) = parse_coordinator_result(&footer);
        assert!(result.is_none());
        assert!(reason.is_none());
    }
}
