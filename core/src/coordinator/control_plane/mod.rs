mod base;
mod checkpoint_preflight;
mod compliance_diagnostic;
mod decision_preflight;
mod diagnostic_delivery;
mod dispatch;
mod merge_gate;
mod phase_handoff;
pub(crate) mod phase_job;
mod phase_runner;
#[cfg(test)]
mod quota_tests;
#[cfg(test)]
mod review_limit_tests;
mod sanitize;

pub use base::{
    advance_tasks_native, apply_runtime_event_bus_updates, apply_stale_heartbeat_policy,
    consume_heartbeat_events, consume_runtime_events, dispatch_ready_tasks_native,
    monitor_active_jobs_native, monitor_merge_jobs_native, sync_registry_from_prd_native,
    CoordinatorLog,
};
pub use dispatch::{diagnose_stall_native, waiting_for_tools_native};
pub use phase_runner::{run_phase_for_task_native, run_review_phase_for_task_native};

/// Serialize the registry read, review reservation and native phase handoff
/// across coordinator processes. Process exit releases the lock.
pub(crate) fn try_phase_driver(
    root: &std::path::Path,
) -> crate::Result<Option<crate::fs_lock::AdvisoryLock>> {
    let dir = root.join(".macc/state");
    std::fs::create_dir_all(&dir).map_err(|source| crate::MaccError::Io {
        path: dir.display().to_string(),
        action: "create phase driver state directory".into(),
        source,
    })?;
    crate::fs_lock::AdvisoryLock::try_acquire(
        &dir.join("phase-driver.lock"),
        "coordinator phase driver",
    )
}

/// Acquire exclusive phase ownership for a one-shot operator action.
pub fn acquire_phase_driver(root: &std::path::Path) -> crate::Result<crate::fs_lock::AdvisoryLock> {
    try_phase_driver(root)?.ok_or_else(|| {
        crate::MaccError::Validation(
            "Another coordinator phase is running. Retry after it finishes.".into(),
        )
    })
}
