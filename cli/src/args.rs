//! Keeping option-heavy commands in separate Args derives bounds the stack
//! needed to construct the complete clap command tree in debug builds.
use crate::SaveSubcommands;

#[derive(clap::Args)]
pub(crate) struct CoordinatorArgs {
    /// Coordinator command (run, control-plane-run, dispatch, advance, resume, sync, sync-prd, status, reconcile, block-task, unblock-task, approvals, approve, reject, request-changes, unlock, cleanup, retry-phase, cutover-gate, stop, sessions, validate-transition, validate-runtime-transition, runtime-status-from-event, storage-import, storage-export, events-export, storage-verify, storage-sync, select-ready-task, state-apply-transition, state-set-runtime, state-task-field, state-task-exists, state-counts, state-locks, state-set-merge-pending, state-set-merge-processed, state-increment-retries, state-upsert-slo-warning, state-slo-metric)
    /// Use `prds help` to manage the ordered PRD file queue and visual editor.
    #[arg(default_value = "run")]
    pub(crate) command_name: String,
    /// Start the coordinator after a relative delay, for example 30m or 2h
    #[arg(long = "in", value_name = "DURATION", conflicts_with = "at")]
    pub(crate) run_in: Option<String>,
    /// Start the coordinator at an absolute date-time
    #[arg(long, value_name = "DATETIME", conflicts_with = "run_in")]
    pub(crate) at: Option<String>,
    /// Execute coordinator mutations as this client identity
    #[arg(long)]
    pub(crate) as_client: Option<String>,
    /// Client to open after the coordinator starts: tui (default), web, none
    #[arg(long, value_name = "CLIENT")]
    pub(crate) client: Option<String>,
    /// Run headless without any client (alias for --client none)
    #[arg(long, alias = "no-tui")]
    pub(crate) no_client: bool,
    /// Start supervisor in daemon+attach mode before running coordinator
    #[arg(long)]
    pub(crate) supervisor: bool,
    /// Explicitly disable autonomous supervision (also skips the startup question)
    #[arg(long, conflicts_with = "supervisor")]
    pub(crate) no_supervisor: bool,
    /// Drain stop (disable new dispatch, let active tasks finish)
    #[arg(long)]
    pub(crate) drain: bool,
    /// Graceful stop (SIGTERM only, no SIGKILL escalation)
    #[arg(long)]
    pub(crate) graceful: bool,
    /// Force stop (SIGTERM then SIGKILL process groups)
    #[arg(long)]
    pub(crate) force: bool,
    /// When action=stop, remove all project worktrees after coordinator shutdown
    #[arg(long)]
    pub(crate) remove_worktrees: bool,
    /// Force stop and cleanup worktrees
    #[arg(long)]
    pub(crate) cleanup_worktrees: bool,
    /// When action=stop and --remove-worktrees is set, also delete associated branches
    #[arg(long)]
    pub(crate) remove_branches: bool,
    /// Override PRD file path
    #[arg(long)]
    pub(crate) prd: Option<String>,
    /// Fixed tool for coordinator phase hooks (review/fix)
    #[arg(long)]
    pub(crate) coordinator_tool: Option<String>,
    /// Default reference/base branch when task.base_branch is not provided
    #[arg(long)]
    pub(crate) reference_branch: Option<String>,
    /// Run preflight checks and exit without starting the coordinator
    #[arg(long)]
    pub(crate) preflight_only: bool,
    /// Allow coordinator run even when reference branch worktree is dirty
    #[arg(long)]
    pub(crate) allow_dirty_reference: bool,
    /// Create the reference branch if it is missing
    #[arg(long)]
    pub(crate) create_reference_branch: bool,
    /// Base branch or revision used when creating a missing reference branch
    #[arg(long)]
    pub(crate) reference_branch_base: Option<String>,
    /// Tool priority order (comma-separated, e.g. tool-a,tool-b,tool-c)
    #[arg(long)]
    pub(crate) tool_priority: Option<String>,
    /// Per-tool concurrency cap JSON (e.g. {"tool-a":3,"tool-b":2})
    #[arg(long)]
    pub(crate) max_parallel_per_tool_json: Option<String>,
    /// Preset configurations (conservative, balanced, throughput)
    #[arg(long)]
    pub(crate) preset: Option<String>,
    /// Category->tools routing JSON (e.g. {"frontend":["tool-b","tool-c"]})
    #[arg(long)]
    pub(crate) tool_specializations_json: Option<String>,
    /// Override MAX_DISPATCH
    #[arg(long)]
    pub(crate) max_dispatch: Option<usize>,
    /// Override MAX_PARALLEL
    #[arg(long)]
    pub(crate) max_parallel: Option<usize>,
    /// Override TIMEOUT_SECONDS
    #[arg(long)]
    pub(crate) timeout_seconds: Option<usize>,
    /// Override PHASE_RUNNER_MAX_ATTEMPTS
    #[arg(long)]
    pub(crate) phase_runner_max_attempts: Option<usize>,
    /// Flush coordinator log buffer every N lines (default: 32)
    #[arg(long)]
    pub(crate) log_flush_lines: Option<usize>,
    /// Flush coordinator log buffer every N milliseconds (default: 1000)
    #[arg(long)]
    pub(crate) log_flush_ms: Option<u64>,
    /// Debounce SQLite -> JSON compatibility export in milliseconds (0 disables debounce)
    #[arg(long)]
    pub(crate) mirror_json_debounce_ms: Option<u64>,
    /// Override STALE_CLAIMED_SECONDS
    #[arg(long)]
    pub(crate) stale_claimed_seconds: Option<usize>,
    /// Override STALE_IN_PROGRESS_SECONDS
    #[arg(long)]
    pub(crate) stale_in_progress_seconds: Option<usize>,
    /// Override STALE_CHANGES_REQUESTED_SECONDS
    #[arg(long)]
    pub(crate) stale_changes_requested_seconds: Option<usize>,
    /// Override STALE_ACTION (abandon, todo, blocked)
    #[arg(long)]
    pub(crate) stale_action: Option<String>,
    /// Coordinator storage mode (json, dual-write, sqlite)
    #[arg(long)]
    pub(crate) storage_mode: Option<String>,
    /// Enable AI-driven merge conflict resolution
    #[arg(long)]
    pub(crate) merge_ai_fix: Option<bool>,
    /// Timeout for merge operations in seconds
    #[arg(long)]
    pub(crate) merge_job_timeout_seconds: Option<usize>,
    /// Timeout for merge-fix hook in seconds
    #[arg(long)]
    pub(crate) merge_hook_timeout_seconds: Option<u64>,
    /// Grace period for ghost heartbeat in seconds
    #[arg(long)]
    pub(crate) ghost_heartbeat_grace_seconds: Option<i64>,
    /// Cooldown between task dispatch in seconds
    #[arg(long)]
    pub(crate) dispatch_cooldown_seconds: Option<u64>,
    /// Enable JSON compatibility mode for storage
    #[arg(long)]
    pub(crate) json_compat: Option<bool>,
    /// Allow falling back to JSON task registry if SQLite is corrupted or missing
    #[arg(long)]
    pub(crate) legacy_json_fallback: Option<bool>,
    /// Number of recent events to evaluate for cutover gate
    #[arg(long)]
    pub(crate) cutover_gate_window_events: Option<usize>,
    /// Maximum allowed ratio of blocked events for cutover gate
    #[arg(long)]
    pub(crate) cutover_gate_max_blocked_ratio: Option<f64>,
    /// Maximum allowed ratio of stale events for cutover gate
    #[arg(long)]
    pub(crate) cutover_gate_max_stale_ratio: Option<f64>,
    /// Comma-separated list of error codes that trigger auto-retry
    #[arg(long)]
    pub(crate) error_code_retry_list: Option<String>,
    /// Maximum number of auto-retries for a task
    #[arg(long)]
    pub(crate) error_code_retry_max: Option<usize>,
    /// Grace period (seconds) before force-killing a performer that signaled failure via IPC but did not exit
    #[arg(long)]
    pub(crate) force_kill_grace_seconds: Option<u64>,
    /// Maximum reviewer attempts per task, including retries. Zero disables new reviews. Default: unlimited.
    #[arg(long)]
    pub(crate) max_review_cycles: Option<usize>,
    /// Disable the testing phase entirely
    #[arg(long)]
    pub(crate) disable_testing: bool,
    /// Disable the review phase entirely
    #[arg(long)]
    pub(crate) disable_review: bool,
    /// Override the testing phase mode (disabled, required, risk_based, manual)
    #[arg(long)]
    pub(crate) testing: Option<String>,
    /// Override the review phase mode (disabled, required, risk_based, manual)
    #[arg(long)]
    pub(crate) review: Option<String>,
    /// Force a global model tier for all tasks in this run: mini, standard, heavy.
    /// Overrides per-task routing_hints. Only effective when model-routing mode is auto.
    #[arg(long)]
    pub(crate) model_tier: Option<String>,
    /// Extra args passed to coordinator subcommands (positional or after --)
    #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
    pub(crate) extra_args: Vec<String>,
}

#[derive(clap::Args)]
pub(crate) struct SaveArgs {
    /// Name of the save bundle to create
    pub(crate) name: Option<String>,
    /// Overwrite if save already exists
    #[arg(long)]
    pub(crate) overwrite: bool,
    /// Optional description of the save
    #[arg(long)]
    pub(crate) description: Option<String>,
    /// Comma-separated list of sections to save
    #[arg(long)]
    pub(crate) only: Option<String>,
    /// Do not save coordinator sessions
    #[arg(long)]
    pub(crate) no_sessions: bool,
    /// Include redacted logs in the save
    #[arg(long)]
    pub(crate) include_logs: bool,
    /// Maximum size of log files to include
    #[arg(long, default_value = "50MB")]
    pub(crate) log_max_size: String,
    /// Duration of log files to include
    #[arg(long, default_value = "7d")]
    pub(crate) log_since: String,
    /// Redact secret-like patterns from logs
    #[arg(long, default_value = "true")]
    pub(crate) redact_logs: bool,
    /// Show what would be saved without writing
    #[arg(long)]
    pub(crate) dry_run: bool,

    /// Save bundle management subcommand
    #[command(subcommand)]
    pub(crate) cmd: Option<SaveSubcommands>,
}

#[derive(clap::Args)]
pub(crate) struct RestoreArgs {
    /// Name of the save bundle to restore
    pub(crate) name: Option<String>,
    /// Restore the most recent backup set or save bundle
    #[arg(long)]
    pub(crate) latest: bool,
    /// Use user-level backup root (~/.macc/backups) instead of project backup root
    #[arg(long)]
    pub(crate) user: bool,
    /// Restore from an explicit backup set name (timestamp folder)
    #[arg(long)]
    pub(crate) backup: Option<String>,
    /// Show what would be restored without writing files
    #[arg(long)]
    pub(crate) dry_run: bool,
    /// Skip confirmation prompt
    #[arg(short = 'y', long)]
    pub(crate) yes: bool,
    /// Run macc apply after restoring (incompatible with --dry-run)
    #[arg(long, conflicts_with = "dry_run")]
    pub(crate) apply: bool,
    /// Restore only the config file
    #[arg(long)]
    pub(crate) config_only: bool,
    /// Force session restore
    #[arg(long)]
    pub(crate) sessions: bool,
    /// Skip session restore
    #[arg(long)]
    pub(crate) no_sessions: bool,
    /// Restore archived logs to .macc/restored-logs/<name>/
    #[arg(long)]
    pub(crate) include_logs: bool,
}

#[derive(clap::Args)]
pub(crate) struct StatusArgs {
    /// Output JSON snapshot
    #[arg(long)]
    pub(crate) json: bool,
    /// Open live TUI observer (read-only)
    #[arg(short, long)]
    pub(crate) watch: bool,
    /// Enable operator actions in watch mode
    #[arg(long)]
    pub(crate) control: bool,
    /// Focus on a specific task ID
    #[arg(long)]
    pub(crate) task: Option<String>,
    /// Filter by tool
    #[arg(long)]
    pub(crate) tool: Option<String>,
    /// Show only failed tasks
    #[arg(long)]
    pub(crate) failed: bool,
    /// Show only rate-limited tools
    #[arg(long)]
    pub(crate) rate_limited: bool,
    /// Show log pane only
    #[arg(long)]
    pub(crate) logs_only: bool,
    /// Show events pane only
    #[arg(long)]
    pub(crate) events_only: bool,
    /// Number of recent events to show (default: 5)
    #[arg(long, default_value = "5")]
    pub(crate) events: usize,
    /// Show verbose output including per-worker details
    #[arg(long)]
    pub(crate) verbose: bool,
}

#[derive(clap::Args)]
pub(crate) struct RunArgs {
    /// Skill ID (e.g. validate, implement, security-check)
    pub(crate) skill: String,
    /// Tool to route through (overrides skill default)
    #[arg(long)]
    pub(crate) tool: Option<String>,
    /// Agent persona to use
    #[arg(long)]
    pub(crate) agent: Option<String>,
    /// Task ID context for the skill run
    #[arg(long)]
    pub(crate) task_id: Option<String>,
    /// Glob scope for the skill
    #[arg(long)]
    pub(crate) scope: Option<String>,
    /// Feature label for the skill
    #[arg(long)]
    pub(crate) feature: Option<String>,
    /// Show what would run without executing
    #[arg(long)]
    pub(crate) dry_run: bool,
    /// Watch the run output
    #[arg(long)]
    pub(crate) watch: bool,
    /// Output JSON result
    #[arg(long)]
    pub(crate) json: bool,
    /// Skip confirmation prompts
    #[arg(short = 'y', long)]
    pub(crate) yes: bool,
}
