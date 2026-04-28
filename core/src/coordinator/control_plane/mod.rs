mod base;
mod dispatch;
mod merge_gate;
mod phase_runner;
mod sanitize;

pub use base::{
    advance_tasks_native, apply_stale_heartbeat_policy, consume_heartbeat_events,
    consume_runtime_events, dispatch_ready_tasks_native, monitor_active_jobs_native,
    monitor_merge_jobs_native, sync_registry_from_prd_native, CoordinatorLog,
};
pub use phase_runner::{run_phase_for_task_native, run_review_phase_for_task_native};
