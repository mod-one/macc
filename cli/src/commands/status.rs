use super::{AppContext, Command};
use macc_core::engine::Engine;
use macc_core::Result;

pub struct StatusCommand {
    pub app: AppContext,
    pub json: bool,
    pub watch: bool,
    pub control: bool,
    pub task: Option<String>,
    pub tool: Option<String>,
    pub failed: bool,
    pub rate_limited: bool,
    pub logs_only: bool,
    pub events_only: bool,
}

impl Command for StatusCommand {
    fn run(&self) -> Result<()> {
        let paths = self.app.project_paths()?;

        if self.watch {
            return launch_watch_tui(self.control);
        }

        let snapshot = self.app.engine.runtime_snapshot(&paths)?;

        if self.json {
            let json = serde_json::to_string_pretty(&snapshot).map_err(|e| {
                macc_core::MaccError::Validation(format!("Failed to serialize snapshot: {}", e))
            })?;
            println!("{}", json);
            return Ok(());
        }

        print_snapshot_summary(&snapshot);
        Ok(())
    }
}

fn print_snapshot_summary(snapshot: &macc_core::runtime::RuntimeSnapshot) {
    println!("MACC Status — {}", snapshot.project.name);
    println!("  Root:    {}", snapshot.project.root.display());
    if let Some(branch) = &snapshot.git.current_branch {
        println!("  Branch:  {}", branch);
    }
    println!();

    println!("Queue:");
    let q = &snapshot.queue;
    println!(
        "  todo={} in_progress={} reviewing={} blocked={} merged={} total={}",
        q.todo, q.in_progress, q.reviewing, q.blocked, q.merged, q.total
    );
    println!();

    if !snapshot.workers.is_empty() {
        println!("Active Workers:");
        for w in &snapshot.workers {
            let phase = w.phase.as_deref().unwrap_or("-");
            let hb = w.last_heartbeat.as_deref().unwrap_or("-");
            println!(
                "  {} [{}] {} {} hb={}",
                w.id,
                w.runtime_status,
                w.tool,
                phase,
                hb
            );
        }
        println!();
    }

    if !snapshot.throttled_tools.is_empty() {
        println!("Throttled Tools:");
        for t in &snapshot.throttled_tools {
            println!(
                "  {} — backoff {}s until {}",
                t.tool,
                t.backoff_seconds,
                t.delayed_until.as_deref().unwrap_or("unknown")
            );
        }
        println!();
    }

    if !snapshot.recent_events.is_empty() {
        println!("Recent Events:");
        for ev in snapshot.recent_events.iter().rev().take(5).rev() {
            let ts = ev.ts.as_deref().unwrap_or("-");
            let task = ev.task_id.as_deref().unwrap_or("");
            println!("  {} {} {}", ts, ev.event_type, task);
        }
    }
}

fn launch_watch_tui(control: bool) -> Result<()> {
    macc_tui::run_tui_with_launch(macc_tui::LaunchMode::Watch { control })
        .map_err(|e| macc_core::MaccError::Validation(format!("TUI error: {}", e)))
}
