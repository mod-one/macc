use super::{AppContext, Command};
use macc_core::runtime::{CoordinatorStatus, RuntimeSnapshot};
use macc_core::Result;

pub struct StatusCommand {
    pub app: AppContext,
    pub json: bool,
    pub watch: bool,
    pub control: bool,
    pub logs_only: bool,
    pub events_only: bool,
    pub events_count: usize,
    pub verbose: bool,
}

impl Command for StatusCommand {
    fn run(&self) -> Result<()> {
        let paths = self.app.project_paths()?;

        if self.watch {
            return launch_watch_tui(self.control, self.logs_only, self.events_only);
        }

        let snapshot = self.app.engine.runtime_snapshot(&paths)?;

        if self.json {
            let json = serde_json::to_string_pretty(&snapshot).map_err(|e| {
                macc_core::MaccError::Validation(format!("Failed to serialize snapshot: {}", e))
            })?;
            println!("{}", json);
            return Ok(());
        }

        print_status_human(&snapshot, self.events_count, self.verbose);
        Ok(())
    }
}

fn print_status_human(snapshot: &RuntimeSnapshot, events_count: usize, verbose: bool) {
    println!("MACC Status");
    println!("  Project: {}", snapshot.project.root.display());
    if let Some(branch) = &snapshot.git.current_branch {
        println!("  Branch:  {}", branch);
    }
    if let Some(ver) = &snapshot.project.config_version {
        println!("  Config:  .macc/macc.yaml (v{})", ver);
    } else {
        println!("  Config:  .macc/macc.yaml");
    }
    println!();

    // Coordinator section
    print_coordinator_section(&snapshot.coordinator);
    println!();

    // Tasks section
    let q = &snapshot.queue;
    println!("Tasks");
    println!("  todo:              {}", q.todo);
    println!("  ready:             {}", q.ready);
    println!("  in_progress:       {}", q.in_progress);
    println!("  reviewing:         {}", q.reviewing);
    println!("  changes_requested: {}", q.changes_requested);
    println!("  blocked:           {}", q.blocked);
    println!("  merged:            {}", q.merged);
    println!("  failed:            {}", q.failed);
    println!();

    // Workers section
    if !snapshot.workers.is_empty() || verbose {
        println!(
            "Workers\n  active: {} / (max: configured in macc.yaml)",
            snapshot.workers.len()
        );
        for w in &snapshot.workers {
            let tool = &w.tool;
            let phase = w.phase.as_deref().unwrap_or("-");
            let hb_age = heartbeat_age_label(w.last_heartbeat.as_deref());
            let stale = if is_stale(w.last_heartbeat.as_deref()) {
                " ▲ stale"
            } else {
                ""
            };
            println!("  {}  {} [{}] hb={}{}", w.id, tool, phase, hb_age, stale);
        }
        println!();
    }

    // Worktrees section
    let total_wt = snapshot.git.worktrees_count;
    let active_wt = snapshot.workers.len();
    if total_wt > 0 || verbose {
        println!("Worktrees");
        println!("  total:  {}", total_wt);
        println!("  active: {}", active_wt);
        println!();
    }

    // Health section (lightweight checks from diagnostics)
    let diag = &snapshot.diagnostics;
    if diag.issues_count > 0 || diag.warnings_count > 0 || verbose {
        println!("Health");
        if diag.critical_count > 0 {
            println!("  ❌ {} critical issue(s)", diag.critical_count);
        }
        if diag.warnings_count > 0 {
            println!("  ⚠️  {} warning(s)", diag.warnings_count);
        }
        if diag.issues_count == 0 && diag.warnings_count == 0 {
            println!("  ✅ All checks passed");
        }
        for t in &snapshot.throttled_tools {
            println!(
                "  ⚠️  {} rate-limited for {}s until {}",
                t.tool,
                t.backoff_seconds,
                t.delayed_until.as_deref().unwrap_or("unknown")
            );
        }
        println!();
    }

    // Recent events section
    if !snapshot.recent_events.is_empty() {
        println!("Recent events");
        for ev in snapshot
            .recent_events
            .iter()
            .rev()
            .take(events_count)
            .collect::<Vec<_>>()
            .into_iter()
            .rev()
        {
            let ts = ev.ts.as_deref().unwrap_or("-");
            let task = ev.task_id.as_deref().unwrap_or("");
            let sep = if task.is_empty() { "" } else { "  " };
            println!("  {}  {}{}{}", ts, ev.event_type, sep, task);
        }
    } else if verbose {
        println!("Recent events\n  No recent coordinator events found.");
    }
}

fn print_coordinator_section(coord: &CoordinatorStatus) {
    println!("Coordinator");
    if coord.running {
        println!("  State: running");
        if let Some(run_id) = &coord.run_id {
            println!("  Run:   {}", run_id);
        }
        if let Some(epoch) = coord.epoch {
            println!("  Epoch: {}", epoch);
        }
    } else if coord.paused {
        println!("  State: paused");
        if let Some(reason) = &coord.pause_reason {
            println!("  Reason: {}", reason);
        }
        println!();
        println!("  Next action:");
        println!("    macc coordinator run");
    } else {
        println!("  State: not running");
        println!();
        println!("  Next action:");
        println!("    macc coordinator run");
    }
}

fn heartbeat_age_label(hb: Option<&str>) -> String {
    let Some(ts_str) = hb else {
        return "—".to_string();
    };
    match chrono::DateTime::parse_from_rfc3339(ts_str) {
        Ok(ts) => {
            let age = chrono::Utc::now().signed_duration_since(ts.with_timezone(&chrono::Utc));
            let secs = age.num_seconds();
            if secs < 60 {
                format!("{}s ago", secs)
            } else if secs < 3600 {
                format!("{}m ago", secs / 60)
            } else {
                format!("{}h ago", secs / 3600)
            }
        }
        Err(_) => ts_str.to_string(),
    }
}

fn is_stale(hb: Option<&str>) -> bool {
    let Some(ts_str) = hb else { return false };
    match chrono::DateTime::parse_from_rfc3339(ts_str) {
        Ok(ts) => {
            let age = chrono::Utc::now().signed_duration_since(ts.with_timezone(&chrono::Utc));
            age.num_seconds() > 180
        }
        Err(_) => false,
    }
}

fn launch_watch_tui(control: bool, logs_only: bool, events_only: bool) -> Result<()> {
    macc_tui::run_tui_with_launch(macc_tui::LaunchMode::Watch {
        control,
        logs_only,
        events_only,
    })
    .map_err(|e| macc_core::MaccError::Validation(format!("TUI error: {}", e)))
}
