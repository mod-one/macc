//! Read-only supervisor status strip for the coordinator client.
use macc_core::supervisor::live::SupervisorLive;
use macc_core::ProjectPaths;
use serde_json::Value;
pub fn status_strip(paths: &ProjectPaths) -> Option<String> {
    let raw =
        std::fs::read_to_string(paths.root.join(".macc/state/supervisor-health.json")).ok()?;
    let value: Value = serde_json::from_str(&raw).ok()?;
    let pid = std::fs::read_to_string(paths.root.join(".macc/state/supervisor.pid"))
        .ok()
        .and_then(|p| p.trim().parse::<i64>().ok());
    let alive = pid.is_some_and(macc_core::coordinator::helpers::is_pid_running);
    let status = if alive {
        value
            .pointer("/health/status")
            .and_then(Value::as_str)
            .unwrap_or("running")
    } else {
        "stopped"
    };
    let last = std::fs::read_to_string(paths.root.join(".macc/log/supervisor/latest.json"))
        .ok()
        .and_then(|s| serde_json::from_str::<Value>(&s).ok());
    let phase = last
        .as_ref()
        .and_then(|v| v.pointer("/intervention/status"))
        .and_then(Value::as_str)
        .unwrap_or("none");
    Some(format!(
        "Supervisor: {status} | intervention: {phase} | macc supervisor report"
    ))
}

/// The supervisor's intervention to show beside the live tasks, if any.
pub fn live(paths: &ProjectPaths, coordinator_running: bool) -> Option<SupervisorLive> {
    macc_core::supervisor::live::load(paths).filter(|live| live.visible(coordinator_running))
}

/// Worker-column label that sets the row apart from coordinator workers.
pub const WORKER_LABEL: &str = "SUPERVISOR";

fn elapsed_label(since: Option<&str>, now: chrono::DateTime<chrono::Utc>) -> String {
    let Some(start) = since.and_then(|time| chrono::DateTime::parse_from_rfc3339(time).ok()) else {
        return "-".into();
    };
    let seconds = now.signed_duration_since(start).num_seconds().max(0);
    match seconds {
        0..=59 => format!("{seconds}s"),
        60..=3599 => format!("{}m", seconds / 60),
        _ => format!("{}h{:02}m", seconds / 3600, seconds % 3600 / 60),
    }
}

/// Cells of the supervisor row, in the LIVE TASKS column order:
/// health, worker, task, status, tool, model, effort, age, phase age.
pub fn row_cells(live: &SupervisorLive, now: chrono::DateTime<chrono::Utc>) -> [String; 9] {
    let marker = if live.active { "◆ SUP" } else { "! SUP" };
    [
        marker.into(),
        WORKER_LABEL.into(),
        live.subject(),
        live.status.clone(),
        live.tool.clone().unwrap_or_else(|| "-".into()),
        "-".into(),
        "-".into(),
        elapsed_label(live.started_at.as_deref(), now),
        elapsed_label(live.phase_started_at.as_deref(), now),
    ]
}

/// Whether a detail line is ordinary, a problem, or the step to take.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DetailKind {
    Info,
    Problem,
    Action,
}

/// Labelled lines for the detail pane while the supervisor row is shown.
pub fn detail(
    live: &SupervisorLive,
    now: chrono::DateTime<chrono::Utc>,
) -> Vec<(&'static str, String, DetailKind)> {
    let mut lines = vec![(
        "Supervisor: ",
        format!(
            "{} — attempt {} on incident {} (not a coordinator task)",
            if live.active {
                "intervention in progress"
            } else {
                "intervention needs attention"
            },
            live.attempt,
            live.incident_id
        ),
        DetailKind::Info,
    )];
    let mut scope = if live.task_ids.is_empty() {
        "root task not identified yet".to_string()
    } else {
        live.task_ids.join(", ")
    };
    if let Some(tool) = &live.tool {
        scope.push_str(&format!("   Tool: {tool}"));
    }
    if let Some(branch) = &live.target_branch {
        scope.push_str(&format!("   Publishes to: {branch}"));
    }
    lines.push(("Repairing:  ", scope, DetailKind::Info));
    lines.push((
        "Phase:      ",
        format!(
            "{} for {}   Total: {}",
            live.status,
            elapsed_label(live.phase_started_at.as_deref(), now),
            elapsed_label(live.started_at.as_deref(), now)
        ),
        DetailKind::Info,
    ));
    if let Some(activity) = &live.activity {
        lines.push(("Doing:      ", activity.clone(), DetailKind::Info));
    }
    if let Some(reason) = &live.reason {
        lines.push(("Reason:     ", reason.clone(), DetailKind::Problem));
    }
    if let Some(next) = &live.next_action {
        lines.push(("Next:       ", next.clone(), DetailKind::Action));
    }
    if let Some(diagnosis) = &live.diagnosis {
        lines.push(("Diagnosis:  ", diagnosis.clone(), DetailKind::Info));
    }
    if let Some(first) = live.findings.first() {
        lines.push((
            "MACC:       ",
            format!(
                "{} finding(s), all in `macc supervisor report`. First: {first}",
                live.findings.len()
            ),
            DetailKind::Info,
        ));
    }
    if let Some(logs) = &live.log_dir {
        lines.push(("Logs:       ", logs.clone(), DetailKind::Info));
    }
    lines
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample(active: bool) -> SupervisorLive {
        SupervisorLive {
            running: true,
            active,
            status: if active { "validating" } else { "escalated" }.into(),
            attempt: 1,
            incident_id: "run-1-2".into(),
            task_ids: vec!["WEB-BFF-002".into()],
            tool: Some("claude".into()),
            activity: active.then(|| "Validation 3/5: pnpm run test".into()),
            started_at: Some("2026-10-12T02:00:00Z".into()),
            phase_started_at: Some("2026-10-12T02:17:00Z".into()),
            diagnosis: Some("scope gap".into()),
            reason: (!active).then(|| "Validation failed: pnpm run test".into()),
            next_action: (!active).then(|| "Fix the failing command".into()),
            target_branch: Some("feat/p0p1".into()),
            worktree: None,
            log_dir: None,
            findings: vec!["Improve X".into(), "Improve Y".into()],
        }
    }
    fn now() -> chrono::DateTime<chrono::Utc> {
        "2026-10-12T02:21:30Z".parse().unwrap()
    }

    #[test]
    fn row_is_labelled_as_supervisor_work_with_phase_and_elapsed_times() {
        let cells = row_cells(&sample(true), now());
        assert_eq!(cells[0], "◆ SUP");
        assert_eq!(cells[1], "SUPERVISOR");
        assert_eq!(cells[2], "repair WEB-BFF-002");
        assert_eq!(cells[3], "validating");
        assert_eq!(cells[4], "claude");
        assert_eq!(cells[7], "21m");
        assert_eq!(cells[8], "4m");
        assert_eq!(row_cells(&sample(false), now())[0], "! SUP");
    }

    #[test]
    fn detail_shows_the_current_step_while_active() {
        let lines = detail(&sample(true), now());
        assert!(lines[0].1.contains("not a coordinator task"));
        assert!(lines[1]
            .1
            .contains("WEB-BFF-002   Tool: claude   Publishes to: feat/p0p1"));
        assert!(lines
            .iter()
            .any(|(label, text, _)| *label == "Doing:      "
                && text == "Validation 3/5: pnpm run test"));
        assert!(!lines
            .iter()
            .any(|(_, _, kind)| *kind == DetailKind::Problem));
    }

    #[test]
    fn detail_shows_reason_next_step_and_findings_when_waiting() {
        let lines = detail(&sample(false), now());
        assert!(lines.contains(&(
            "Reason:     ",
            "Validation failed: pnpm run test".to_string(),
            DetailKind::Problem
        )));
        assert!(lines.contains(&(
            "Next:       ",
            "Fix the failing command".to_string(),
            DetailKind::Action
        )));
        assert!(lines
            .iter()
            .any(|(_, text, _)| text.starts_with("2 finding(s)") && text.ends_with("Improve X")));
    }

    #[test]
    fn elapsed_label_scales_and_tolerates_missing_times() {
        assert_eq!(elapsed_label(Some("2026-10-12T02:21:00Z"), now()), "30s");
        assert_eq!(elapsed_label(Some("2026-10-12T01:16:30Z"), now()), "1h05m");
        assert_eq!(elapsed_label(None, now()), "-");
    }
}
