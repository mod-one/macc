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
    /// Filter to events newer than this number of minutes.
    pub since_minutes: Option<u64>,
    /// Minimum severity to display (debug, info, notice, warn, error, fatal).
    pub severity: Option<String>,
}

impl ExplainCommand {
    pub fn new(
        app: AppContext,
        task_id: String,
        json: bool,
        since_minutes: Option<u64>,
        severity: Option<String>,
    ) -> Self {
        Self { app, task_id, json, since_minutes, severity }
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

        let task = snapshot.registry.tasks.iter().find(|t| {
            t.id.eq_ignore_ascii_case(&self.task_id)
        });

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
    fn print_human(&self, task: &Task, project_root: &PathBuf) -> Result<()> {
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
        let has_logs = rt.stdout_log.is_some() || rt.stderr_log.is_some() || rt.events_log.is_some();
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
        let events_log_path = rt
            .events_log
            .as_deref()
            .map(|p| project_root.join(p));

        if let Some(events_path) = events_log_path {
            if events_path.exists() {
                self.print_events_timeline(&events_path)?;
            } else {
                println!("Timeline:");
                println!("  No structured events found ({})", events_path.display());
                println!("  Showing available registry state only.");
            }
        } else {
            // Try the global events log
            let global_events = project_root.join(".macc/log/events.jsonl");
            if global_events.exists() {
                println!("Timeline (from global events log):");
                self.print_events_from_file(&global_events, Some(&task.id))?;
            } else {
                println!("Timeline:");
                println!("  No structured events log found.");
                println!("  (Events are written to .macc/log/events.jsonl during coordinator runs.)");
            }
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
            MaccError::Validation(format!("Failed to open events log {}: {}", path.display(), e))
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

        let cutoff_ts: Option<String> = self.since_minutes.map(|mins| {
            let cutoff = chrono::Utc::now() - chrono::Duration::minutes(mins as i64);
            cutoff.to_rfc3339()
        });

        let mut found = false;
        for line in reader.lines() {
            let Ok(line) = line else { continue };
            let Ok(val) = serde_json::from_str::<serde_json::Value>(&line) else { continue };

            // Filter by task_id if specified
            if let Some(tid) = filter_task_id {
                let event_task = val.get("task_id").and_then(|v| v.as_str()).unwrap_or("");
                if !event_task.eq_ignore_ascii_case(tid) {
                    continue;
                }
            }

            // Filter by severity
            let sev = val.get("severity").and_then(|v| v.as_str()).unwrap_or("info");
            if severity_rank(sev) < min_rank {
                continue;
            }

            // Filter by cutoff time
            if let Some(ref cutoff) = cutoff_ts {
                let ts = val.get("timestamp").and_then(|v| v.as_str()).unwrap_or("");
                if ts < cutoff.as_str() {
                    continue;
                }
            }

            let ts = val.get("timestamp").and_then(|v| v.as_str()).unwrap_or("-");
            let phase = val.get("phase").and_then(|v| v.as_str()).unwrap_or("-");
            let event_type = val.get("event_type").and_then(|v| v.as_str()).unwrap_or("");
            let message = val.get("message").and_then(|v| v.as_str()).unwrap_or("");

            // Format: HH:MM:SS  severity  phase  message
            let time_part = if ts.len() >= 19 { &ts[11..19] } else { ts };
            println!("  {}  {:<6} {:<8} {}", time_part, sev, phase, message);
            if !event_type.is_empty() && event_type != "status_message" && event_type != "heartbeat" {
                // Show event type as a sub-detail for important events
            }
            found = true;
        }

        if !found {
            println!("  (no events match the current filters)");
        }
        Ok(())
    }

    fn print_json(&self, task: &Task) -> Result<()> {
        let val = serde_json::to_value(task)
            .map_err(|e| MaccError::Validation(format!("Failed to serialize task: {}", e)))?;
        println!("{}", serde_json::to_string_pretty(&val)
            .map_err(|e| MaccError::Validation(format!("Failed to format JSON: {}", e)))?);
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
}

impl DiffCommand {
    pub fn new(
        app: AppContext,
        task_id: String,
        stat: bool,
        name_only: bool,
        base: Option<String>,
        format: Option<String>,
    ) -> Self {
        Self { app, task_id, stat, name_only, base, format }
    }
}

impl Command for DiffCommand {
    fn run(&self) -> Result<()> {
        let paths = self.app.project_paths()?;
        let storage_paths = CoordinatorStoragePaths::from_project_paths(&paths);
        let snapshot = SqliteStorage::new(storage_paths).load_snapshot().map_err(|e| {
            MaccError::Validation(format!(
                "Failed to load coordinator snapshot: {}",
                e
            ))
        })?;

        let task = snapshot.registry.tasks.iter().find(|t| {
            t.id.eq_ignore_ascii_case(&self.task_id)
        });

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
        let use_stat = self.stat
            || self.format.as_deref() == Some("stat");
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
                let mut args = vec!["diff", &diff_target];
                if use_stat {
                    args.push("--stat");
                } else if use_name_only {
                    args.push("--name-only");
                }
                let output = macc_core::git::run_git_output_mapped(wt, &args, "git diff worktree")?;
                print!("{}", String::from_utf8_lossy(&output.stdout));
                if !output.stderr.is_empty() {
                    eprint!("{}", String::from_utf8_lossy(&output.stderr));
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
            let output = macc_core::git::run_git_output_mapped(&paths.root, &args, "git diff commit")?;
            print!("{}", String::from_utf8_lossy(&output.stdout));
            if !output.stderr.is_empty() {
                eprint!("{}", String::from_utf8_lossy(&output.stderr));
            }
            return Ok(());
        }

        Err(MaccError::Validation(format!(
            "No active worktree or commit found for task '{}'.\n\nThe task may not have started, or its worktree was cleaned up without a recorded commit SHA.",
            self.task_id
        )))
    }
}
