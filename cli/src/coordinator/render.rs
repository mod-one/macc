use macc_core::service::coordinator_workflow::CoordinatorStatus;
use std::path::Path;

pub(crate) fn print_status_summary(repo_root: &Path, status: &CoordinatorStatus) {
    let registry_path = repo_root
        .join(".macc")
        .join("automation")
        .join("task")
        .join("task_registry.json");
    println!("Registry: {}", registry_path.display());
    println!("Tasks: {}", status.total);
    println!("  todo: {}", status.todo);
    println!("  active: {}", status.active);
    println!("  blocked: {}", status.blocked);
    println!("  merged: {}", status.merged);
    if status.paused {
        println!("Paused: yes");
        if let Some(task) = &status.pause_task_id {
            println!("  task: {}", task);
        }
        if let Some(phase) = &status.pause_phase {
            println!("  phase: {}", phase);
        }
        if let Some(reason) = &status.pause_reason {
            println!("  reason: {}", reason);
        }
    } else {
        println!("Paused: no");
    }
    if let Some(latest_error) = &status.latest_error {
        println!("Latest error: {}", latest_error);
    }
    if let Some(summary) = &status.last_run_summary {
        if let Some(note) = &summary.supervisor {
            println!("Last result: {}", summary.headline);
            println!("  Cause: {}", summary.cause);
            print_supervisor_note(note);
        } else {
            print_previous_diagnosis(summary);
        }
    }
    if let Some(report) = &status.failure_report {
        println!("Failure report:");
        println!("  kind: {:?}", report.kind);
        println!("  source: {}", report.source);
        println!("  blocking: {}", report.blocking);
        if let Some(task) = &report.task_id {
            println!("  task: {}", task);
        }
        if let Some(phase) = &report.phase {
            println!("  phase: {}", phase);
        }
        if let Some(event_type) = &report.event_type {
            println!("  event: {}", event_type);
        }
        if !report.suggested_fixes.is_empty() {
            println!("  suggested fixes:");
            for fix in &report.suggested_fixes {
                println!("    - {}", fix);
            }
        }
    }
}

pub(crate) fn print_supervisor_note(note: &macc_core::service::run_summary::SupervisorAnnotation) {
    println!("  {}: {}", note.heading(), note.diagnosis);
    if let Some(result) = &note.outcome {
        println!("  Outcome: {result}");
    }
    if let Some(action) = &note.next_action {
        println!("  Supervisor next: {action}");
    }
}

pub(crate) fn print_previous_diagnosis(
    summary: &macc_core::service::run_summary::CoordinatorRunSummary,
) {
    if let Some((run, note)) = summary.previous_diagnosis() {
        println!("Previous incident ({run}):");
        print_supervisor_note(note);
    }
}
