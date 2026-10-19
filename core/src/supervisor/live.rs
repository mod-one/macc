//! Live view of the supervisor's current intervention, shared by all clients.
//!
//! The supervisor is not a coordinator task: it never enters the task registry
//! or its counts. Clients show it as its own row beside the live tasks.
use crate::ProjectPaths;
use serde::{Deserialize, Serialize};
use serde_json::Value;

/// Phases during which the supervisor is working on an incident.
const ACTIVE: &[&str] = &[
    "detected",
    "diagnosing",
    "repairing",
    "validating",
    "verifying",
    "integrating",
    "requeuing",
    "restarting",
    "restart_requested",
];

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SupervisorLive {
    /// The supervisor process is alive.
    pub running: bool,
    /// An intervention is in progress right now.
    pub active: bool,
    pub status: String,
    pub attempt: u64,
    pub incident_id: String,
    /// Diagnosed root tasks, or the incident's candidates before diagnosis.
    pub task_ids: Vec<String>,
    pub tool: Option<String>,
    /// The step in progress, in operator terms.
    pub activity: Option<String>,
    pub started_at: Option<String>,
    pub phase_started_at: Option<String>,
    pub diagnosis: Option<String>,
    /// Why an intervention that is no longer active did not recover.
    pub reason: Option<String>,
    pub next_action: Option<String>,
    pub target_branch: Option<String>,
    pub worktree: Option<String>,
    pub log_dir: Option<String>,
    /// MACC improvements recommended by the diagnosis.
    pub findings: Vec<String>,
}

impl SupervisorLive {
    /// An unrecovered intervention stays visible until a coordinator runs
    /// again; an active one is always shown.
    pub fn visible(&self, coordinator_running: bool) -> bool {
        self.active || !coordinator_running
    }

    /// Short label for the row's task column.
    pub fn subject(&self) -> String {
        if self.task_ids.is_empty() {
            format!("incident {}", self.incident_id)
        } else {
            format!("repair {}", self.task_ids.join(", "))
        }
    }
}

/// The intervention a client should show, if any. Recovered interventions and
/// projects the supervisor never acted on yield `None`.
pub fn load(paths: &ProjectPaths) -> Option<SupervisorLive> {
    let raw = std::fs::read_to_string(paths.root.join(".macc/log/supervisor/latest.json")).ok()?;
    let report: Value = serde_json::from_str(&raw).ok()?;
    let running = std::fs::read_to_string(paths.root.join(".macc/state/supervisor.pid"))
        .ok()
        .and_then(|pid| pid.trim().parse::<i64>().ok())
        .is_some_and(crate::coordinator::helpers::is_pid_running);
    from_report(&report, running)
}

fn from_report(report: &Value, running: bool) -> Option<SupervisorLive> {
    let intervention = report.get("intervention")?;
    let text = |pointer: &str| {
        intervention
            .pointer(pointer)
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(str::to_owned)
    };
    let list = |pointer: &str| -> Vec<String> {
        intervention
            .pointer(pointer)
            .and_then(Value::as_array)
            .map(|items| {
                items
                    .iter()
                    .filter_map(Value::as_str)
                    .map(str::to_owned)
                    .collect()
            })
            .unwrap_or_default()
    };
    let recorded = text("/status")?;
    if recorded == "recovered" {
        return None;
    }
    let in_progress = ACTIVE.contains(&recorded.as_str());
    let mut task_ids = list("/diagnosis/task_ids");
    if task_ids.is_empty() {
        task_ids = intervention
            .pointer("/incident/tasks")
            .and_then(Value::as_array)
            .map(|tasks| {
                tasks
                    .iter()
                    .filter_map(|task| task.get("id").and_then(Value::as_str))
                    .map(str::to_owned)
                    .collect()
            })
            .unwrap_or_default();
    }
    // A phase recorded as in progress by a dead process is not progressing.
    let orphaned = in_progress && !running;
    Some(SupervisorLive {
        running,
        active: in_progress && running,
        status: if orphaned {
            "interrupted".into()
        } else {
            recorded
        },
        attempt: intervention
            .get("attempt")
            .and_then(Value::as_u64)
            .unwrap_or(0),
        incident_id: text("/incident/id").unwrap_or_default(),
        task_ids,
        tool: text("/tool"),
        activity: text("/activity").filter(|_| in_progress && running),
        started_at: text("/started_at"),
        phase_started_at: text("/phase_started_at"),
        diagnosis: text("/diagnosis/summary"),
        reason: if orphaned {
            Some("The supervisor process stopped during this intervention".into())
        } else if in_progress {
            None
        } else {
            text("/detail")
        },
        next_action: if orphaned {
            Some("Run `macc supervisor start --daemon` to resume; work is kept in the supervisor worktree.".into())
        } else {
            text("/next_action")
        },
        target_branch: text("/target_branch"),
        worktree: text("/worktree"),
        log_dir: text("/log_dir"),
        findings: list("/diagnosis/macc_findings"),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn report(status: &str) -> Value {
        json!({"intervention":{
            "status":status,"attempt":2,"tool":"claude",
            "incident":{"id":"run-1-3","tasks":[{"id":"A"},{"id":"B"}]},
            "activity":"Validation 3/5: pnpm run test",
            "detail":"Validation failed: pnpm run test",
            "next_action":"Fix the failing command",
            "diagnosis":{"summary":"scope gap","task_ids":["A"],"macc_findings":["Improve X"]}
        }})
    }

    #[test]
    fn active_intervention_reports_its_step_and_diagnosed_roots() {
        let live = from_report(&report("validating"), true).unwrap();
        assert!(live.active);
        assert_eq!(live.subject(), "repair A");
        assert_eq!(
            live.activity.as_deref(),
            Some("Validation 3/5: pnpm run test")
        );
        assert_eq!(live.reason, None);
        assert!(live.visible(true));
    }

    #[test]
    fn escalation_keeps_reason_and_next_step_until_a_coordinator_runs() {
        let live = from_report(&report("escalated"), true).unwrap();
        assert!(!live.active);
        assert_eq!(live.activity, None);
        assert_eq!(
            live.reason.as_deref(),
            Some("Validation failed: pnpm run test")
        );
        assert_eq!(live.next_action.as_deref(), Some("Fix the failing command"));
        assert!(live.visible(false));
        assert!(!live.visible(true));
    }

    #[test]
    fn a_phase_left_by_a_dead_supervisor_is_shown_as_interrupted() {
        let live = from_report(&report("repairing"), false).unwrap();
        assert!(!live.active);
        assert_eq!(live.status, "interrupted");
        assert!(live.next_action.unwrap().contains("macc supervisor start"));
    }

    #[test]
    fn recovered_or_absent_interventions_have_no_row() {
        assert_eq!(from_report(&report("recovered"), true), None);
        assert_eq!(from_report(&json!({"runtime_error":"x"}), true), None);
        let mut undiagnosed = report("diagnosing");
        undiagnosed["intervention"]["diagnosis"] = Value::Null;
        assert_eq!(
            from_report(&undiagnosed, true).unwrap().subject(),
            "repair A, B"
        );
    }
}
