use crate::commands::{AppContext, Command};
use macc_core::coordinator::model::Task;
use macc_core::coordinator_storage::{CoordinatorStorage, CoordinatorStoragePaths, SqliteStorage};
use macc_core::{MaccError, Result};
use std::path::PathBuf;

/// Options for `macc explain <task-id>`
pub struct ExplainCommand {
    pub app: AppContext,
    pub task_id: String,
    /// Output machine-readable JSON.
    pub json: bool,
    /// Filter to events newer than this number of seconds.
    pub since_seconds: Option<u64>,
    /// Minimum severity to display (debug, info, notice, warn, error, fatal).
    pub severity: Option<String>,
    /// Print raw task execution logs (stdout/stderr)
    pub logs: bool,
    /// Print generated task artifacts
    pub artifacts: bool,
    /// Print a condensed timeline hiding verbose ticks
    pub compact: bool,
}

impl ExplainCommand {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        app: AppContext,
        task_id: String,
        json: bool,
        since_seconds: Option<u64>,
        severity: Option<String>,
        logs: bool,
        artifacts: bool,
        compact: bool,
    ) -> Self {
        Self {
            app,
            task_id,
            json,
            since_seconds,
            severity,
            logs,
            artifacts,
            compact,
        }
    }
}

impl Command for ExplainCommand {
    fn run(&self) -> Result<()> {
        let paths = self.app.project_paths()?;
        let storage_paths = CoordinatorStoragePaths::from_project_paths(&paths);
        let snapshot = SqliteStorage::new(storage_paths).load_snapshot().map_err(|e| {
            MaccError::Validation(format!(
                "Failed to load coordinator snapshot: {}\nMake sure a coordinator has run at least once.",
                e
            ))
        })?;

        let task = snapshot
            .registry
            .tasks
            .iter()
            .find(|t| t.id.eq_ignore_ascii_case(&self.task_id));

        let Some(task) = task else {
            return Err(MaccError::Validation(format!(
                "Task '{}' not found in registry.\n\nTip: run `macc coordinator status` to list active tasks.",
                self.task_id
            )));
        };

        if self.json {
            self.print_json(task)
        } else {
            self.print_human(task, &paths.root)
        }
    }
}

impl ExplainCommand {
    fn print_human(&self, task: &Task, project_root: &std::path::Path) -> Result<()> {
        let rt = &task.task_runtime;
        let state = &task.state;
        let title = task.title.as_deref().unwrap_or("(no title)");

        println!("{} — {}", task.id, title);
        println!();

        // Header fields
        println!("State:     {}", state);
        if let Some(status) = &rt.status {
            println!("Runtime:   {}", status);
        }
        if let Some(phase) = &rt.current_phase {
            if !phase.is_empty() {
                println!("Phase:     {}", phase);
            }
        }
        if let Some(tool) = &task.tool {
            println!("Tool:      {}", tool);
        }
        if let Some(worker) = &rt.worker_id {
            if !worker.is_empty() {
                println!("Worker:    {}", worker);
            }
        }
        if let Some(worktree) = &rt.worktree {
            if !worktree.is_empty() {
                println!("Worktree:  {}", worktree);
            }
        }
        if let Some(branch) = &rt.branch {
            if !branch.is_empty() {
                println!("Branch:    {}", branch);
            }
        }
        if let Some(started) = &rt.started_at {
            if !started.is_empty() {
                println!("Started:   {}", started);
            }
        }
        if let Some(hb) = &rt.last_heartbeat {
            if !hb.is_empty() {
                println!("Heartbeat: {}", hb);
            }
        }
        if let Some(msg) = &rt.message {
            if !msg.is_empty() {
                println!("Message:   {}", msg);
            }
        }
        if let Some(err) = &rt.last_error {
            if !err.is_empty() {
                println!("Error:     {}", err);
            }
        }

        // Log file pointers
        println!();
        let has_logs =
            rt.stdout_log.is_some() || rt.stderr_log.is_some() || rt.events_log.is_some();
        if has_logs {
            println!("Log files:");
            if let Some(p) = &rt.stdout_log {
                println!("  stdout: {}", p);
            }
            if let Some(p) = &rt.stderr_log {
                println!("  stderr: {}", p);
            }
            if let Some(p) = &rt.events_log {
                println!("  events: {}", p);
            }
            println!();
        }

        // Timeline from structured events log
        let events_log_path = rt.events_log.as_deref().map(|p| project_root.join(p));

        let events_resolved_path = if let Some(ref path) = events_log_path {
            if path.exists() {
                Some(path.clone())
            } else {
                None
            }
        } else {
            let global_events = project_root.join(".macc/log/events.jsonl");
            if global_events.exists() {
                Some(global_events)
            } else {
                None
            }
        };

        if let Some(ref events_path) = events_resolved_path {
            if events_log_path.is_some() && events_log_path.as_ref().unwrap().exists() {
                self.print_events_timeline(events_path)?;
            } else {
                println!("Timeline (from global events log):");
                self.print_events_from_file(events_path, Some(&task.id))?;
            }
        } else {
            println!("Timeline:");
            if let Some(ref path) = events_log_path {
                println!("  No structured events found ({})", path.display());
            } else {
                println!("  No structured events log found.");
                println!(
                    "  (Events are written to .macc/log/events.jsonl during coordinator runs.)"
                );
            }
            println!("  Showing available registry state only.");
        }

        if self.artifacts {
            println!();
            if let Some(ref events_path) = events_resolved_path {
                let filter_id = if events_log_path.is_none() {
                    Some(task.id.as_str())
                } else {
                    None
                };
                self.print_artifacts(events_path, filter_id)?;
            } else {
                println!("Artifacts:");
                println!("  No events log found to scan for artifacts.");
            }
        }

        if self.logs {
            println!();
            self.print_raw_logs(rt, project_root)?;
        }

        Ok(())
    }

    fn print_events_timeline(&self, path: &PathBuf) -> Result<()> {
        println!("Timeline:");
        self.print_events_from_file(path, None)
    }

    fn print_events_from_file(&self, path: &PathBuf, filter_task_id: Option<&str>) -> Result<()> {
        use std::io::{BufRead, BufReader};
        let file = std::fs::File::open(path).map_err(|e| {
            MaccError::Validation(format!(
                "Failed to open events log {}: {}",
                path.display(),
                e
            ))
        })?;
        let reader = BufReader::new(file);

        let min_severity = self.severity.as_deref().unwrap_or("info");
        let severity_rank = |s: &str| match s {
            "debug" => 0,
            "info" => 1,
            "notice" => 2,
            "warn" => 3,
            "error" => 4,
            "fatal" => 5,
            _ => 1,
        };
        let min_rank = severity_rank(min_severity);

        let cutoff_ts: Option<String> = self.since_seconds.map(|secs| {
            let cutoff = chrono::Utc::now() - chrono::Duration::seconds(secs as i64);
            cutoff.to_rfc3339()
        });

        let mut found = false;
        for line in reader.lines() {
            let Ok(line) = line else { continue };
            let Ok(val) = serde_json::from_str::<serde_json::Value>(&line) else {
                continue;
            };

            // Filter by task_id if specified
            if let Some(tid) = filter_task_id {
                let event_task = val.get("task_id").and_then(|v| v.as_str()).unwrap_or("");
                if !event_task.eq_ignore_ascii_case(tid) {
                    continue;
                }
            }

            // Filter by severity
            let sev = val
                .get("severity")
                .and_then(|v| v.as_str())
                .unwrap_or("info");
            if severity_rank(sev) < min_rank {
                continue;
            }

            // Filter by cutoff time
            if let Some(ref cutoff) = cutoff_ts {
                let ts_val = val
                    .get("timestamp")
                    .and_then(|v| v.as_str())
                    .or_else(|| val.get("ts").and_then(|v| v.as_str()))
                    .unwrap_or("");
                if ts_val < cutoff.as_str() {
                    continue;
                }
            }

            let ts = val
                .get("timestamp")
                .and_then(|v| v.as_str())
                .or_else(|| val.get("ts").and_then(|v| v.as_str()))
                .unwrap_or("-");
            let phase = val.get("phase").and_then(|v| v.as_str()).unwrap_or("-");
            let event_type = val
                .get("event_type")
                .and_then(|v| v.as_str())
                .or_else(|| val.get("type").and_then(|v| v.as_str()))
                .unwrap_or("");
            let message = val
                .get("message")
                .and_then(|v| v.as_str())
                .or_else(|| val.get("msg").and_then(|v| v.as_str()))
                .or_else(|| {
                    val.get("payload")
                        .and_then(|p| p.get("message"))
                        .and_then(|v| v.as_str())
                })
                .unwrap_or("");

            if self.compact
                && (event_type.eq_ignore_ascii_case("heartbeat")
                    || event_type.eq_ignore_ascii_case("status_message")
                    || event_type.eq_ignore_ascii_case("progress"))
            {
                continue;
            }

            // Format: HH:MM:SS  severity  phase  message
            let time_part = if ts.len() >= 19 { &ts[11..19] } else { ts };
            println!("  {}  {:<6} {:<8} {}", time_part, sev, phase, message);
            found = true;
        }

        if !found {
            println!("  (no events match the current filters)");
        }
        Ok(())
    }

    fn print_artifacts(&self, path: &PathBuf, filter_task_id: Option<&str>) -> Result<()> {
        use std::io::{BufRead, BufReader};
        let file = std::fs::File::open(path).map_err(|e| {
            MaccError::Validation(format!(
                "Failed to open events log {}: {}",
                path.display(),
                e
            ))
        })?;
        let reader = BufReader::new(file);

        let mut artifacts = Vec::new();

        for line in reader.lines() {
            let Ok(line) = line else { continue };
            let Ok(val) = serde_json::from_str::<serde_json::Value>(&line) else {
                continue;
            };

            // Filter by task_id if specified
            if let Some(tid) = filter_task_id {
                let event_task = val.get("task_id").and_then(|v| v.as_str()).unwrap_or("");
                if !event_task.eq_ignore_ascii_case(tid) {
                    continue;
                }
            }

            let event_type = val
                .get("event_type")
                .and_then(|v| v.as_str())
                .or_else(|| val.get("type").and_then(|v| v.as_str()))
                .unwrap_or("");

            if event_type.eq_ignore_ascii_case("artifact_created")
                || event_type.eq_ignore_ascii_case("artifact")
            {
                let ts = val
                    .get("timestamp")
                    .and_then(|v| v.as_str())
                    .or_else(|| val.get("ts").and_then(|v| v.as_str()))
                    .unwrap_or("-");
                let time_part = if ts.len() >= 19 { &ts[11..19] } else { ts };

                // Get artifact path/name from various potential keys
                let artifact_path = val
                    .get("path")
                    .and_then(|v| v.as_str())
                    .or_else(|| val.get("artifact").and_then(|v| v.as_str()))
                    .or_else(|| {
                        val.get("payload")
                            .and_then(|p| p.get("path"))
                            .and_then(|v| v.as_str())
                    })
                    .or_else(|| {
                        val.get("payload")
                            .and_then(|p| p.get("artifact"))
                            .and_then(|v| v.as_str())
                    })
                    .or_else(|| val.get("message").and_then(|v| v.as_str()))
                    .or_else(|| {
                        val.get("payload")
                            .and_then(|p| p.get("message"))
                            .and_then(|v| v.as_str())
                    })
                    .unwrap_or("unknown artifact");

                artifacts.push((time_part.to_string(), artifact_path.to_string()));
            }
        }

        println!("Artifacts:");
        if artifacts.is_empty() {
            println!("  No registered artifacts found for this task.");
        } else {
            for (time, path) in artifacts {
                println!("  {}  {}", time, path);
            }
        }
        Ok(())
    }

    fn print_raw_logs(
        &self,
        rt: &macc_core::coordinator::model::TaskRuntime,
        project_root: &std::path::Path,
    ) -> Result<()> {
        println!("Raw Logs:");
        let mut printed = false;

        if let Some(stdout_rel) = &rt.stdout_log {
            let path = project_root.join(stdout_rel);
            println!("--- stdout: {} ---", stdout_rel);
            if path.exists() {
                match std::fs::read_to_string(&path) {
                    Ok(content) => {
                        print!("{}", content);
                        if !content.ends_with('\n') {
                            println!();
                        }
                    }
                    Err(e) => println!("(error reading stdout log: {})", e),
                }
            } else {
                println!("(stdout log file does not exist)");
            }
            printed = true;
        }

        if let Some(stderr_rel) = &rt.stderr_log {
            if printed {
                println!();
            }
            let path = project_root.join(stderr_rel);
            println!("--- stderr: {} ---", stderr_rel);
            if path.exists() {
                match std::fs::read_to_string(&path) {
                    Ok(content) => {
                        print!("{}", content);
                        if !content.ends_with('\n') {
                            println!();
                        }
                    }
                    Err(e) => println!("(error reading stderr log: {})", e),
                }
            } else {
                println!("(stderr log file does not exist)");
            }
            printed = true;
        }

        if !printed {
            println!("  No log file paths are registered for this task.");
        }

        Ok(())
    }

    fn print_json(&self, task: &Task) -> Result<()> {
        let val = serde_json::to_value(task)
            .map_err(|e| MaccError::Validation(format!("Failed to serialize task: {}", e)))?;
        println!(
            "{}",
            serde_json::to_string_pretty(&val)
                .map_err(|e| MaccError::Validation(format!("Failed to format JSON: {}", e)))?
        );
        Ok(())
    }
}

// ---------------------------------------------------------------------------

/// Options for `macc diff <task-id>`
pub struct DiffCommand {
    pub app: AppContext,
    pub task_id: String,
    /// Show only stat summary.
    pub stat: bool,
    /// Show only changed file names.
    pub name_only: bool,
    /// Use this branch as the base instead of the recorded base branch.
    pub base: Option<String>,
    /// Output format: "patch" or "stat".
    pub format: Option<String>,
    /// Show staged changes in the active worktree.
    pub cached: bool,
    /// Open the diff output in an editor or viewer.
    pub open: bool,
}

impl DiffCommand {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        app: AppContext,
        task_id: String,
        stat: bool,
        name_only: bool,
        base: Option<String>,
        format: Option<String>,
        cached: bool,
        open: bool,
    ) -> Self {
        Self {
            app,
            task_id,
            stat,
            name_only,
            base,
            format,
            cached,
            open,
        }
    }
}

impl Command for DiffCommand {
    fn run(&self) -> Result<()> {
        let paths = self.app.project_paths()?;
        let storage_paths = CoordinatorStoragePaths::from_project_paths(&paths);
        let snapshot = SqliteStorage::new(storage_paths)
            .load_snapshot()
            .map_err(|e| {
                MaccError::Validation(format!("Failed to load coordinator snapshot: {}", e))
            })?;

        let task = snapshot
            .registry
            .tasks
            .iter()
            .find(|t| t.id.eq_ignore_ascii_case(&self.task_id));

        let Some(task) = task else {
            return Err(MaccError::Validation(format!(
                "Task '{}' not found in registry.",
                self.task_id
            )));
        };

        let title = task.title.as_deref().unwrap_or("(no title)");
        println!("{} — {}", task.id, title);
        println!();

        // Resolve worktree path from task_runtime or task.worktree
        let worktree_path = task
            .task_runtime
            .worktree
            .as_deref()
            .filter(|s| !s.is_empty())
            .or_else(|| {
                task.worktree
                    .as_ref()
                    .and_then(|w| w.worktree_path.as_deref())
                    .filter(|s| !s.is_empty())
            })
            .map(|p| paths.root.join(p));

        // Resolve base branch
        let base_branch = self
            .base
            .clone()
            .or_else(|| {
                task.worktree
                    .as_ref()
                    .and_then(|w| w.base_branch.clone())
                    .filter(|s| !s.is_empty())
            })
            .or_else(|| task.base_branch.clone().filter(|s| !s.is_empty()))
            .unwrap_or_else(|| "main".to_string());

        let branch = task
            .task_runtime
            .branch
            .as_deref()
            .filter(|s| !s.is_empty())
            .or_else(|| {
                task.worktree
                    .as_ref()
                    .and_then(|w| w.branch.as_deref())
                    .filter(|s| !s.is_empty())
            });

        // Determine effective format flags
        let use_stat = self.stat || self.format.as_deref() == Some("stat");
        let use_name_only = self.name_only;

        if let Some(ref wt) = worktree_path {
            if wt.exists() {
                println!("Worktree:  {}", wt.display());
                if let Some(br) = branch {
                    println!("Branch:    {}", br);
                }
                println!("Base:      {}", base_branch);
                println!();
                println!("Diff stat");
                // Run: git diff <base>...HEAD [--stat|--name-only] via facade
                let diff_target = format!("{}...HEAD", base_branch);
                let mut args = vec!["diff"];
                if self.cached {
                    args.push("--cached");
                } else {
                    args.push(&diff_target);
                }
                if use_stat {
                    args.push("--stat");
                } else if use_name_only {
                    args.push("--name-only");
                }
                let output = macc_core::git::run_git_output_mapped(wt, &args, "git diff worktree")?;
                let diff_str = String::from_utf8_lossy(&output.stdout);
                let stderr_str = String::from_utf8_lossy(&output.stderr);

                if self.open {
                    use std::io::Write;
                    let mut temp = tempfile::Builder::new()
                        .prefix("macc-diff-")
                        .suffix(".patch")
                        .tempfile()
                        .map_err(|e| {
                            MaccError::Validation(format!(
                                "Failed to create temporary file for diff: {}",
                                e
                            ))
                        })?;
                    temp.write_all(diff_str.as_bytes()).map_err(|e| {
                        MaccError::Validation(format!(
                            "Failed to write diff to temporary file: {}",
                            e
                        ))
                    })?;
                    temp.flush().map_err(|e| {
                        MaccError::Validation(format!("Failed to flush temporary file: {}", e))
                    })?;

                    let editor = std::env::var("EDITOR")
                        .or_else(|_| std::env::var("VISUAL"))
                        .unwrap_or_else(|_| "less".to_string());

                    println!("Opening diff in editor/viewer: {} ...", editor);
                    macc_core::service::task_runner::open_in_editor(temp.path(), &editor)?;
                } else {
                    print!("{}", diff_str);
                    if !stderr_str.is_empty() {
                        eprint!("{}", stderr_str);
                    }
                }
                return Ok(());
            }
        }

        // Fallback: try commit-based diff if worktree is gone
        let last_commit = task.worktree.as_ref().and_then(|w| w.last_commit.clone());
        if let Some(commit) = last_commit {
            println!("Worktree no longer exists. Using commit-based diff.");
            println!("Commit:    {}", commit);
            println!("Base:      {}", base_branch);
            println!();
            // Run: git diff <base>...<commit> [--stat|--name-only] via facade
            let diff_target = format!("{}...{}", base_branch, commit);
            let mut args = vec!["diff", &diff_target];
            if use_stat {
                args.push("--stat");
            } else if use_name_only {
                args.push("--name-only");
            }
            let output =
                macc_core::git::run_git_output_mapped(&paths.root, &args, "git diff commit")?;
            let diff_str = String::from_utf8_lossy(&output.stdout);
            let stderr_str = String::from_utf8_lossy(&output.stderr);

            if self.open {
                use std::io::Write;
                let mut temp = tempfile::Builder::new()
                    .prefix("macc-diff-")
                    .suffix(".patch")
                    .tempfile()
                    .map_err(|e| {
                        MaccError::Validation(format!(
                            "Failed to create temporary file for diff: {}",
                            e
                        ))
                    })?;
                temp.write_all(diff_str.as_bytes()).map_err(|e| {
                    MaccError::Validation(format!("Failed to write diff to temporary file: {}", e))
                })?;
                temp.flush().map_err(|e| {
                    MaccError::Validation(format!("Failed to flush temporary file: {}", e))
                })?;

                let editor = std::env::var("EDITOR")
                    .or_else(|_| std::env::var("VISUAL"))
                    .unwrap_or_else(|_| "less".to_string());

                println!("Opening diff in editor/viewer: {} ...", editor);
                macc_core::service::task_runner::open_in_editor(temp.path(), &editor)?;
            } else {
                print!("{}", diff_str);
                if !stderr_str.is_empty() {
                    eprint!("{}", stderr_str);
                }
            }
            return Ok(());
        }

        Err(MaccError::Validation(format!(
            "No active worktree or commit found for task '{}'.\n\nThe task may not have started, or its worktree was cleaned up without a recorded commit SHA.",
            self.task_id
        )))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use macc_core::coordinator::model::TaskRuntime;
    use macc_core::{resolve::CliOverrides, TestEngine};
    use std::fs;
    use std::sync::Arc;
    use tempfile::tempdir;

    fn test_app_context(cwd: PathBuf) -> AppContext {
        AppContext::new(
            cwd,
            Arc::new(TestEngine::with_fixtures()),
            CliOverrides::default(),
        )
    }

    #[test]
    fn test_explain_compact_filtering() {
        let dir = tempdir().unwrap();
        let app = test_app_context(dir.path().to_path_buf());
        let explain = ExplainCommand::new(
            app,
            "T-1".to_string(),
            false,
            None,
            None,
            false,
            false,
            true, // compact
        );

        let log_path = dir.path().join("events.jsonl");
        let events = r#"{"ts":"2026-05-30T23:59:00Z","type":"heartbeat","message":"ping"}
{"ts":"2026-05-30T23:59:01Z","type":"status_message","message":"working"}
{"ts":"2026-05-30T23:59:02Z","type":"phase_result","message":"success"}
"#;
        fs::write(&log_path, events).unwrap();

        let res = explain.print_events_from_file(&log_path, None);
        assert!(res.is_ok());
    }

    #[test]
    fn test_explain_artifacts_listing() {
        let dir = tempdir().unwrap();
        let app = test_app_context(dir.path().to_path_buf());
        let explain = ExplainCommand::new(
            app,
            "T-1".to_string(),
            false,
            None,
            None,
            false,
            true, // artifacts
            false,
        );

        let log_path = dir.path().join("events.jsonl");
        let events = r#"{"ts":"2026-05-30T23:59:00Z","type":"artifact_created","path":"docs/report.pdf"}
{"ts":"2026-05-30T23:59:01Z","type":"artifact","artifact":"src/lib.rs"}
"#;
        fs::write(&log_path, events).unwrap();

        let res = explain.print_artifacts(&log_path, None);
        assert!(res.is_ok());
    }

    #[test]
    fn test_explain_raw_logs() {
        let dir = tempdir().unwrap();
        let app = test_app_context(dir.path().to_path_buf());
        let explain = ExplainCommand::new(
            app,
            "T-1".to_string(),
            false,
            None,
            None,
            true, // logs
            false,
            false,
        );

        let stdout_path = dir.path().join("stdout.log");
        let stderr_path = dir.path().join("stderr.log");
        fs::write(&stdout_path, "stdout content\n").unwrap();
        fs::write(&stderr_path, "stderr content\n").unwrap();

        let mut rt = TaskRuntime::default();
        rt.stdout_log = Some("stdout.log".to_string());
        rt.stderr_log = Some("stderr.log".to_string());

        let res = explain.print_raw_logs(&rt, &dir.path().to_path_buf());
        assert!(res.is_ok());
    }

    #[test]
    fn test_diff_command_fields() {
        let dir = tempdir().unwrap();
        let app = test_app_context(dir.path().to_path_buf());
        let diff = DiffCommand::new(
            app,
            "T-1".to_string(),
            false,
            false,
            None,
            None,
            true, // cached
            true, // open
        );
        assert_eq!(diff.task_id, "T-1");
        assert!(diff.cached);
        assert!(diff.open);
    }
}
