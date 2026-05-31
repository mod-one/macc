use clap::{Parser, Subcommand};
use commands::Command;
#[cfg(test)]
use macc_core::config::WebAssetsMode;
#[cfg(test)]
use macc_core::coordinator::{RuntimeStatus, WorkflowState};
#[cfg(test)]
use macc_core::coordinator_storage::CoordinatorStorageTransfer;
use macc_core::engine::MaccEngine;
#[cfg(test)]
use macc_core::Engine;
use macc_core::{MaccError, Result};
use std::process::exit;
use tracing::{debug, error, info};

mod commands;
mod coordinator;
mod services;
#[cfg(test)]
mod test_support;

#[cfg(test)]
use macc_core::coordinator::args::{
    RuntimeStatusFromEventArgs, RuntimeTransitionArgs, StorageSyncArgs, WorkflowTransitionArgs,
};
use macc_core::coordinator::types::CoordinatorEnvConfig;

#[derive(Parser)]
#[command(name = "macc")]
#[command(about = "MACC (Multi-Agentic Coding Config)", long_about = None)]
#[command(version = concat!(env!("CARGO_PKG_VERSION"), " (build ", env!("MACC_BUILD_SHA"), ")"))]
struct Cli {
    /// Working directory
    #[arg(short, long, global = true, default_value = ".")]
    cwd: String,

    /// Enable verbose output
    #[arg(short, long, global = true)]
    verbose: bool,

    /// Suppress all non-essential output
    #[arg(short, long, global = true)]
    quiet: bool,

    /// Force offline mode (no remote fetching)
    #[arg(long, global = true)]
    offline: bool,

    /// Port for the MACC web interface
    #[arg(long, global = true)]
    web_port: Option<u16>,

    #[command(subcommand)]
    command: Option<Commands>,
}

#[derive(Subcommand, Clone, Debug)]
enum SaveSubcommands {
    /// List MACC save bundles
    List {
        /// List matching saves only
        #[arg(long)]
        matching: bool,
    },
    /// Show details of a save bundle
    Show {
        name: String,
    },
    /// Delete a MACC save bundle
    Delete {
        name: String,
        /// Skip confirmation prompt
        #[arg(short = 'y', long)]
        yes: bool,
    },
}

#[derive(Subcommand)]
#[allow(clippy::large_enum_variant)]
enum Commands {
    /// Initialize MACC in a project
    Init {
        /// Force initialization even if already initialized
        #[arg(short, long)]
        force: bool,
        /// Run interactive setup wizard (3 questions)
        #[arg(long)]
        wizard: bool,
        /// Restore a saved profile after initialization
        #[arg(long)]
        profile: Option<String>,
        /// Ignore matching saves and create a new baseline
        #[arg(long)]
        fresh: bool,
        /// Restore the best matching save, or specify a save name
        #[arg(long, num_args = 0..=1, default_missing_value = "")]
        restore: Option<String>,
        /// Do not suggest matching saves
        #[arg(long)]
        no_restore_prompt: bool,
        /// Run macc apply after restoration/initialization
        #[arg(long)]
        apply: bool,
    },
    /// Manage saved configuration profiles
    Config {
        #[command(subcommand)]
        config_command: ConfigCommands,
    },
    /// Zero-friction setup: check environment, init, then open TUI or run plan+apply
    Quickstart {
        /// Auto-confirm interactive prompts
        #[arg(short = 'y', long)]
        yes: bool,
        /// Run plan first, then apply after confirmation
        #[arg(long)]
        apply: bool,
        /// Do not open TUI at the end
        #[arg(long)]
        no_tui: bool,
    },
    /// Plan changes to the project
    Plan {
        /// CSV list of tools to use
        #[arg(short, long)]
        tools: Option<String>,
        /// Output machine-readable JSON (for CI/logging)
        #[arg(long)]
        json: bool,
        /// Explain why each file operation exists
        #[arg(long)]
        explain: bool,
    },
    /// Apply configuration to the project
    Apply {
        /// CSV list of tools to use
        #[arg(short, long)]
        tools: Option<String>,

        /// Run in dry-run mode (same as plan)
        #[arg(long)]
        dry_run: bool,

        /// Allow user-scope operations (requires explicit consent)
        #[arg(long)]
        allow_user_scope: bool,
        /// Output machine-readable JSON for dry-run preview
        #[arg(long)]
        json: bool,
        /// Explain why each file operation exists in preview
        #[arg(long)]
        explain: bool,

        /// Fail if resolution differs from the lock file
        #[arg(long)]
        locked: bool,
    },
    /// Catalog management
    Catalog {
        #[command(subcommand)]
        catalog_command: CatalogCommands,
    },
    /// Install items directly from catalog
    Install {
        #[command(subcommand)]
        install_command: InstallCommands,
    },
    /// Open the interactive TUI
    Tui,
    /// Launch the web UI server
    Web {
        /// Bind address for the web server. Use `0.0.0.0` to expose it explicitly.
        #[arg(long, default_value = "127.0.0.1")]
        host: String,
        /// Bind port for the web server. Defaults to the configured web port or `3450`.
        #[arg(long)]
        port: Option<u16>,
        /// Static asset source. `dist` serves from `web/dist`; `embedded` serves rust-embed assets.
        #[arg(long, value_enum)]
        assets: Option<WebAssetsArg>,
    },
    /// Tool management
    Tool {
        #[command(subcommand)]
        tool_command: ToolCommands,
    },
    /// Ask AI tools to update their context files directly in the repo
    Context {
        /// Generate context for a single tool ID
        #[arg(long)]
        tool: Option<String>,
        /// Additional source files to include in the prompt context
        #[arg(long = "from")]
        from_files: Vec<String>,
        /// Preview only; do not run tool commands
        #[arg(long)]
        dry_run: bool,
        /// Print generated prompt(s)
        #[arg(long)]
        print_prompt: bool,
    },
    /// Run diagnostic checks for the environment and supported tools
    Doctor {
        /// Apply safe automatic fixes
        #[arg(long)]
        fix: bool,
    },
    /// Migrate legacy configuration to the new format
    Migrate {
        /// Actually write the migrated config to disk
        #[arg(short, long)]
        apply: bool,
    },
    /// Backup set utilities
    Backups {
        #[command(subcommand)]
        backups_command: BackupsCommands,
    },
    /// Save MACC setup as a bundle
    Save {
        /// Name of the save bundle to create
        name: Option<String>,
        /// Overwrite if save already exists
        #[arg(long)]
        overwrite: bool,
        /// Optional description of the save
        #[arg(long)]
        description: Option<String>,
        /// Comma-separated list of sections to save
        #[arg(long)]
        only: Option<String>,
        /// Do not save coordinator sessions
        #[arg(long)]
        no_sessions: bool,
        /// Include redacted logs in the save
        #[arg(long)]
        include_logs: bool,
        /// Maximum size of log files to include
        #[arg(long, default_value = "50MB")]
        log_max_size: String,
        /// Duration of log files to include
        #[arg(long, default_value = "7d")]
        log_since: String,
        /// Redact secret-like patterns from logs
        #[arg(long, default_value = "true")]
        redact_logs: bool,
        /// Show what would be saved without writing
        #[arg(long)]
        dry_run: bool,

        /// Save bundle management subcommand
        #[command(subcommand)]
        cmd: Option<SaveSubcommands>,
    },
    /// Restore MACC setup from a save bundle or safety backup
    Restore {
        /// Name of the save bundle to restore
        name: Option<String>,
        /// Restore the most recent backup set or save bundle
        #[arg(long)]
        latest: bool,
        /// Use user-level backup root (~/.macc/backups) instead of project backup root
        #[arg(long)]
        user: bool,
        /// Restore from an explicit backup set name (timestamp folder)
        #[arg(long)]
        backup: Option<String>,
        /// Show what would be restored without writing files
        #[arg(long)]
        dry_run: bool,
        /// Skip confirmation prompt
        #[arg(short = 'y', long)]
        yes: bool,
        /// Run macc apply after restore
        #[arg(long)]
        apply: bool,
        /// Restore only the config file
        #[arg(long)]
        config_only: bool,
        /// Force session restore
        #[arg(long)]
        sessions: bool,
        /// Skip session restore
        #[arg(long)]
        no_sessions: bool,
        /// Restore archived logs to .macc/restored-logs/<name>/
        #[arg(long)]
        include_logs: bool,
    },
    /// Remove MACC-managed project files and optionally save state
    Clear {
        /// Save name to backup before clearing
        #[arg(long)]
        save: Option<String>,
        /// Include logs if saving before clearing
        #[arg(long)]
        include_logs: bool,
        /// Do not prompt to save unsaved state
        #[arg(long)]
        no_save_prompt: bool,
        /// Skip confirmation prompts and force clear
        #[arg(short, long)]
        force: bool,
        /// Show what would be cleared without deleting files
        #[arg(long)]
        dry_run: bool,
    },
    /// Worktree utilities
    Worktree {
        #[command(subcommand)]
        worktree_command: WorktreeCommands,
    },
    /// View coordinator/performer logs
    Logs {
        #[command(subcommand)]
        logs_command: LogsCommands,
    },
    /// Supervisor watchdog controls
    Supervisor {
        #[command(subcommand)]
        supervisor_command: SupervisorCommands,
    },
    /// Inspect and recover process ownership state
    Process {
        #[command(subcommand)]
        process_command: commands::process::ProcessCommands,
    },
    /// Run the project coordinator automation script
    #[command(trailing_var_arg = true)]
    Coordinator {
        /// Coordinator command (run, control-plane-run, dispatch, advance, resume, sync, sync-prd, audit-prd, status, reconcile, unlock, cleanup, retry-phase, cutover-gate, stop, sessions, validate-transition, validate-runtime-transition, runtime-status-from-event, storage-import, storage-export, events-export, storage-verify, storage-sync, select-ready-task, state-apply-transition, state-set-runtime, state-task-field, state-task-exists, state-counts, state-locks, state-set-merge-pending, state-set-merge-processed, state-increment-retries, state-upsert-slo-warning, state-slo-metric)
        #[arg(default_value = "run")]
        command_name: String,
        /// Execute coordinator mutations as this client identity
        #[arg(long)]
        as_client: Option<String>,
        /// Disable TUI live view for `macc coordinator run`
        #[arg(long)]
        no_tui: bool,
        /// Start supervisor in daemon+attach mode before running coordinator
        #[arg(long)]
        supervisor: bool,
        /// Drain stop (disable new dispatch, let active tasks finish)
        #[arg(long)]
        drain: bool,
        /// Graceful stop (SIGTERM only, no SIGKILL escalation)
        #[arg(long)]
        graceful: bool,
        /// Force stop (SIGTERM then SIGKILL process groups)
        #[arg(long)]
        force: bool,
        /// When action=stop, remove all project worktrees after coordinator shutdown
        #[arg(long)]
        remove_worktrees: bool,
        /// Force stop and cleanup worktrees
        #[arg(long)]
        cleanup_worktrees: bool,
        /// When action=stop and --remove-worktrees is set, also delete associated branches
        #[arg(long)]
        remove_branches: bool,
        /// Override PRD file path
        #[arg(long)]
        prd: Option<String>,
        /// Fixed tool for coordinator phase hooks (review/fix)
        #[arg(long)]
        coordinator_tool: Option<String>,
        /// Default reference/base branch when task.base_branch is not provided
        #[arg(long)]
        reference_branch: Option<String>,
        /// Tool priority order (comma-separated, e.g. tool-a,tool-b,tool-c)
        #[arg(long)]
        tool_priority: Option<String>,
        /// Per-tool concurrency cap JSON (e.g. {"tool-a":3,"tool-b":2})
        #[arg(long)]
        max_parallel_per_tool_json: Option<String>,
        /// Preset configurations (conservative, balanced, throughput)
        #[arg(long)]
        preset: Option<String>,
        /// Category->tools routing JSON (e.g. {"frontend":["tool-b","tool-c"]})
        #[arg(long)]
        tool_specializations_json: Option<String>,
        /// Override MAX_DISPATCH
        #[arg(long)]
        max_dispatch: Option<usize>,
        /// Override MAX_PARALLEL
        #[arg(long)]
        max_parallel: Option<usize>,
        /// Override TIMEOUT_SECONDS
        #[arg(long)]
        timeout_seconds: Option<usize>,
        /// Override PHASE_RUNNER_MAX_ATTEMPTS
        #[arg(long)]
        phase_runner_max_attempts: Option<usize>,
        /// Flush coordinator log buffer every N lines (default: 32)
        #[arg(long)]
        log_flush_lines: Option<usize>,
        /// Flush coordinator log buffer every N milliseconds (default: 1000)
        #[arg(long)]
        log_flush_ms: Option<u64>,
        /// Debounce SQLite -> JSON compatibility export in milliseconds (0 disables debounce)
        #[arg(long)]
        mirror_json_debounce_ms: Option<u64>,
        /// Override STALE_CLAIMED_SECONDS
        #[arg(long)]
        stale_claimed_seconds: Option<usize>,
        /// Override STALE_IN_PROGRESS_SECONDS
        #[arg(long)]
        stale_in_progress_seconds: Option<usize>,
        /// Override STALE_CHANGES_REQUESTED_SECONDS
        #[arg(long)]
        stale_changes_requested_seconds: Option<usize>,
        /// Override STALE_ACTION (abandon, todo, blocked)
        #[arg(long)]
        stale_action: Option<String>,
        /// Coordinator storage mode (json, dual-write, sqlite)
        #[arg(long)]
        storage_mode: Option<String>,
        /// Enable AI-driven merge conflict resolution
        #[arg(long)]
        merge_ai_fix: Option<bool>,
        /// Timeout for merge operations in seconds
        #[arg(long)]
        merge_job_timeout_seconds: Option<usize>,
        /// Timeout for merge-fix hook in seconds
        #[arg(long)]
        merge_hook_timeout_seconds: Option<u64>,
        /// Grace period for ghost heartbeat in seconds
        #[arg(long)]
        ghost_heartbeat_grace_seconds: Option<i64>,
        /// Cooldown between task dispatch in seconds
        #[arg(long)]
        dispatch_cooldown_seconds: Option<u64>,
        /// Enable JSON compatibility mode for storage
        #[arg(long)]
        json_compat: Option<bool>,
        /// Allow falling back to JSON task registry if SQLite is corrupted or missing
        #[arg(long)]
        legacy_json_fallback: Option<bool>,
        /// Number of recent events to evaluate for cutover gate
        #[arg(long)]
        cutover_gate_window_events: Option<usize>,
        /// Maximum allowed ratio of blocked events for cutover gate
        #[arg(long)]
        cutover_gate_max_blocked_ratio: Option<f64>,
        /// Maximum allowed ratio of stale events for cutover gate
        #[arg(long)]
        cutover_gate_max_stale_ratio: Option<f64>,
        /// Comma-separated list of error codes that trigger auto-retry
        #[arg(long)]
        error_code_retry_list: Option<String>,
        /// Maximum number of auto-retries for a task
        #[arg(long)]
        error_code_retry_max: Option<usize>,
        /// Grace period (seconds) before force-killing a performer that signaled failure via IPC but did not exit
        #[arg(long)]
        force_kill_grace_seconds: Option<u64>,
        /// Max review cycles per task (0=skip review, 1=one review+fix, N=N loops). Default: unlimited.
        #[arg(long)]
        max_review_cycles: Option<usize>,
        /// Extra args passed to coordinator subcommands (positional or after --)
        #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
        extra_args: Vec<String>,
    },
    /// Guided onboarding and task startup entry point
    Start {
        /// Select the startup intent
        #[arg(long)]
        intent: Option<String>,
        /// Run in dry-run mode (no changes written to disk)
        #[arg(long)]
        dry_run: bool,
        /// Launch the web client/server
        #[arg(long)]
        web: bool,
        /// Open the TUI dashboard
        #[arg(long)]
        tui: bool,
        /// Profile to restore or use
        #[arg(long)]
        profile: Option<String>,
        /// Preset configurations (conservative, balanced, throughput)
        #[arg(long)]
        preset: Option<String>,

        /// Fail if resolution differs from the lock file
        #[arg(long)]
        locked: bool,
    },
    /// Audit and display repository Trust and safety parameters
    Trust,
    /// Manage environment reproducibility and macc.lock.yaml
    Lock {
        #[command(subcommand)]
        lock_command: commands::lock::LockCommands,
    },
    /// Diagnostics and recovery for failed tasks
    Failure {
        #[command(subcommand)]
        failure_command: commands::failure::FailureCommands,
    },
    /// Manage progressive settings and presets
    Settings {
        #[command(subcommand)]
        settings_command: commands::settings::SettingsCommands,
    },
    /// Print a chronological task timeline (events, phases, decisions)
    Explain {
        /// Task ID to explain (e.g. NOY-L5-WINWIDGET-001)
        task_id: String,
        /// Output machine-readable JSON
        #[arg(long)]
        json: bool,
        /// Show only events from the last duration (e.g. 30m, 1h, 1d)
        #[arg(long)]
        since: Option<String>,
        /// Minimum severity to display (debug, info, notice, warn, error, fatal)
        #[arg(long)]
        severity: Option<String>,
        /// Print raw task execution logs (stdout/stderr)
        #[arg(long)]
        logs: bool,
        /// Print generated task artifacts
        #[arg(long)]
        artifacts: bool,
        /// Print a condensed timeline hiding verbose ticks
        #[arg(long)]
        compact: bool,
    },
    /// Show the diff for a task's active worktree (no cd required)
    Diff {
        /// Task ID whose diff to show (e.g. NOY-L5-WINWIDGET-001)
        task_id: String,
        /// Show a summary stat instead of full diff
        #[arg(long)]
        stat: bool,
        /// Show only changed file names
        #[arg(long)]
        name_only: bool,
        /// Override base branch (default: uses task's recorded base or main)
        #[arg(long)]
        base: Option<String>,
        /// Output format: patch or stat
        #[arg(long)]
        format: Option<String>,
    },
}


#[derive(clap::ValueEnum, Clone, Copy, Debug, PartialEq, Eq)]
enum WebAssetsArg {
    Dist,
    Embedded,
}

impl From<WebAssetsArg> for macc_core::config::WebAssetsMode {
    fn from(value: WebAssetsArg) -> Self {
        match value {
            WebAssetsArg::Dist => Self::Dist,
            WebAssetsArg::Embedded => Self::Embedded,
        }
    }
}

#[derive(Subcommand)]
pub enum InstallCommands {
    /// Install a skill from the catalog
    Skill {
        /// Tool to install the skill for (e.g. tool-id)
        #[arg(long)]
        tool: String,
        /// Skill ID from catalog
        #[arg(long)]
        id: String,
    },
    /// Install an MCP server from the catalog
    Mcp {
        /// MCP ID from catalog
        #[arg(long)]
        id: String,
    },
}

#[derive(Subcommand)]
pub enum ToolCommands {
    /// Install a local AI tool using steps defined in ToolSpec
    Install {
        /// Tool ID from ToolSpec
        tool_id: String,
        /// Skip confirmation prompt
        #[arg(short = 'y', long)]
        yes: bool,
    },
    /// Update a local AI tool using steps defined in ToolSpec
    Update {
        /// Tool ID from ToolSpec
        tool_id: Option<String>,
        /// Update all matching tools
        #[arg(long)]
        all: bool,
        /// Filter when used with --all: enabled or installed
        #[arg(long, value_parser = ["enabled", "installed"])]
        only: Option<String>,
        /// Show what would be updated without running commands
        #[arg(long)]
        check: bool,
        /// Skip confirmation prompt
        #[arg(short = 'y', long)]
        yes: bool,
        /// Force update even when already up-to-date
        #[arg(long)]
        force: bool,
        /// Best-effort rollback to previous version on failure (npm tools only)
        #[arg(long)]
        rollback_on_fail: bool,
    },
    /// Show installed/outdated status for tools
    Outdated {
        /// Filter results: enabled or installed
        #[arg(long, value_parser = ["enabled", "installed"])]
        only: Option<String>,
    },
}

#[derive(Subcommand)]
pub enum CatalogCommands {
    /// Manage skills catalog
    Skills {
        #[command(subcommand)]
        skills_command: CatalogSubCommands,
    },
    /// Manage MCP catalog (Not implemented yet)
    Mcp {
        #[command(subcommand)]
        mcp_command: CatalogSubCommands,
    },
    /// Import an entry from a URL (e.g. GitHub tree)
    ImportUrl {
        /// Kind of entry (skill or mcp)
        #[arg(long, value_parser = ["skill", "mcp"])]
        kind: String,

        /// Entry ID
        #[arg(long)]
        id: String,

        /// URL to import
        #[arg(long)]
        url: String,

        /// Name (optional, defaults to ID)
        #[arg(long)]
        name: Option<String>,

        /// Description (optional)
        #[arg(long, default_value = "")]
        description: String,

        /// Comma-separated tags (optional)
        #[arg(long)]
        tags: Option<String>,
    },
    /// Search remote registry
    SearchRemote {
        /// API URL
        #[arg(long, default_value = "https://registry.macc.dev")]
        api: String,

        /// Kind (skill or mcp)
        #[arg(long, value_parser = ["skill", "mcp"])]
        kind: String,

        /// Search query
        #[arg(long)]
        q: String,

        /// Add all found results to local catalog
        #[arg(long)]
        add: bool,

        /// Add specific IDs from results to local catalog (comma-separated)
        #[arg(long)]
        add_ids: Option<String>,
    },
}

#[derive(Subcommand)]
pub enum CatalogSubCommands {
    /// List entries in the catalog
    List,
    /// Search for entries in the catalog
    Search {
        /// Search query (matches id, name, description, tags)
        query: String,
    },
    /// Add or update an entry in the catalog
    Add {
        /// Entry ID
        #[arg(long)]
        id: String,
        /// Entry Name
        #[arg(long)]
        name: String,
        /// Entry Description
        #[arg(long)]
        description: String,
        /// Comma-separated tags
        #[arg(long)]
        tags: Option<String>,
        /// Subpath within the source
        #[arg(long, default_value = "")]
        subpath: String,
        /// Source kind (git or http)
        #[arg(long)]
        kind: String,
        /// Source URL
        #[arg(long)]
        url: String,
        /// Source reference (e.g. branch, tag, commit)
        #[arg(long, default_value = "main")]
        reference: String,
        /// Source checksum (optional)
        #[arg(long)]
        checksum: Option<String>,
    },
    /// Remove an entry from the catalog
    Remove {
        /// Entry ID
        #[arg(long)]
        id: String,
    },
}

#[derive(Subcommand)]
pub enum SupervisorCommands {
    /// Start the supervisor watchdog
    Start {
        /// Run in background as a daemon process
        #[arg(long)]
        daemon: bool,
        /// Attach to an existing coordinator run and monitor its PID lifecycle
        #[arg(long)]
        attach: bool,
        /// Explicit coordinator PID to write into the coordinator PID file before attach
        #[arg(long)]
        coordinator_pid: Option<u32>,
    },
    /// Stop the supervisor watchdog
    Stop,
    /// Show supervisor and coordinator health
    Status,
    /// Show the latest supervisor report summary
    Report,
}

#[derive(Subcommand)]
pub enum WorktreeCommands {
    /// Create worktrees for parallel runs
    Create {
        /// Slug for worktree IDs (e.g. "feature")
        slug: String,
        /// Tool to apply in each worktree
        #[arg(long)]
        tool: String,
        /// Number of worktrees to create
        #[arg(long, default_value_t = 1)]
        count: usize,
        /// Base branch to create from
        #[arg(long, default_value = "main")]
        base: String,
        /// Optional scope text (written to .macc/scope.md)
        #[arg(long)]
        scope: Option<String>,
        /// Optional feature label (stored in worktree.json)
        #[arg(long)]
        feature: Option<String>,
        /// Skip applying config in the new worktrees
        #[arg(long)]
        skip_apply: bool,
        /// Allow user-scope operations during apply
        #[arg(long)]
        allow_user_scope: bool,
    },
    /// Show status for the current worktree (if any)
    Status,
    /// List git worktrees
    List,
    /// Open a worktree in an editor and/or terminal
    Open {
        /// Worktree id (folder name under .macc/worktree) or path
        id: String,
        /// Editor command (defaults to "code")
        #[arg(long)]
        editor: Option<String>,
        /// Open in a terminal (uses $TERMINAL if set)
        #[arg(long)]
        terminal: bool,
    },
    /// Apply configuration in a worktree
    Apply {
        /// Worktree id (folder name under .macc/worktree) or path
        #[arg(required_unless_present = "all")]
        id: Option<String>,
        /// Apply all worktrees (excluding the main worktree)
        #[arg(long)]
        all: bool,
        /// Allow user-scope operations
        #[arg(long)]
        allow_user_scope: bool,
    },
    /// Run doctor checks in a worktree
    Doctor {
        /// Worktree id (folder name under .macc/worktree) or path
        id: String,
    },
    /// Run performer.sh inside a worktree
    Run {
        /// Worktree id (folder name under .macc/worktree) or path
        id: String,
    },
    /// Execute a command inside a worktree
    Exec {
        /// Worktree id (folder name under .macc/worktree) or path
        id: String,
        /// Command to execute after `--`
        #[arg(last = true, required = true)]
        cmd: Vec<String>,
    },
    /// Remove a worktree by id or path
    Remove {
        /// Worktree id (folder name under .worktree) or path
        #[arg(required_unless_present = "all")]
        id: Option<String>,
        /// Force removal
        #[arg(long)]
        force: bool,
        /// Remove all worktrees (excluding the main worktree)
        #[arg(long)]
        all: bool,
        /// Also delete the git branch for the removed worktree(s)
        #[arg(long)]
        remove_branch: bool,
    },
    /// Prune git worktrees
    Prune,
}

#[derive(Subcommand)]
pub enum LogsCommands {
    /// Tail the latest matching log file
    Tail {
        /// Component filter
        #[arg(long, default_value = "all", value_parser = ["all", "coordinator", "performer"])]
        component: String,
        /// Worktree ID/path filter (performer logs)
        #[arg(long)]
        worktree: Option<String>,
        /// Task ID filter (performer logs filename contains this value)
        #[arg(long)]
        task: Option<String>,
        /// Number of lines to display
        #[arg(short = 'n', long, default_value_t = 120)]
        lines: usize,
        /// Follow log updates
        #[arg(long)]
        follow: bool,
    },
}

#[derive(Subcommand)]
pub enum BackupsCommands {
    /// List available backup sets
    List {
        /// List user-level backup sets (~/.macc/backups)
        #[arg(long)]
        user: bool,
    },
    /// Print or open a backup set path
    Open {
        /// Backup set name (timestamp folder)
        #[arg(required_unless_present = "latest")]
        id: Option<String>,
        /// Open latest backup set
        #[arg(long)]
        latest: bool,
        /// Open from user-level backup root (~/.macc/backups)
        #[arg(long)]
        user: bool,
        /// Open using a specific editor command
        #[arg(long)]
        editor: Option<String>,
    },
}

#[derive(Subcommand)]
pub enum ConfigCommands {
    /// Save current project config as a user-level profile
    Save {
        /// Profile name
        name: String,
        /// Optional description
        #[arg(long)]
        description: Option<String>,
        /// Save only specific sections (comma-separated: tools,standards,selections,automation,settings,mcp_templates)
        #[arg(long)]
        only: Option<String>,
    },
    /// Restore a saved profile into the current project
    Restore {
        /// Profile name
        name: String,
        /// Restore only specific sections (comma-separated)
        #[arg(long)]
        only: Option<String>,
    },
    /// List all saved profiles
    List,
    /// Delete a saved profile
    Delete {
        /// Profile name
        name: String,
    },
}

fn parse_since_to_seconds(s: &str) -> Option<u64> {
    let s = s.trim().to_ascii_lowercase();
    if s.is_empty() {
        return None;
    }
    let (num_part, multiplier) = if s.ends_with('d') {
        (&s[..s.len() - 1], 24 * 3600)
    } else if s.ends_with('h') {
        (&s[..s.len() - 1], 3600)
    } else if s.ends_with('m') {
        (&s[..s.len() - 1], 60)
    } else if s.ends_with('s') {
        (&s[..s.len() - 1], 1)
    } else {
        // Default to minutes if no suffix is specified
        (s.as_str(), 60)
    };
    num_part.trim().parse::<u64>().ok().map(|val| val * multiplier)
}

fn main() {
    let cli = Cli::parse();
    init_tracing(cli.verbose);

    if cli.verbose {
        info!("Verbose mode enabled");
    }

    // Initialize the real engine with default registry
    let engine = MaccEngine::new(macc_registry::default_registry());
    let provider = services::engine_provider::EngineProvider::new(engine);

    if let Err(e) = run_with_engine_provider(cli, provider) {
        error!(error = %e, "Command failed");
        eprintln!("Error: {}", e);
        exit(get_exit_code(&e));
    }
}

fn init_tracing(verbose: bool) {
    let fallback = if verbose { "debug" } else { "info" };
    let filter = tracing_subscriber::EnvFilter::try_from_default_env()
        .or_else(|_| tracing_subscriber::EnvFilter::try_new(fallback))
        .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info"));
    let _ = tracing_subscriber::fmt()
        .with_env_filter(filter)
        .with_target(false)
        .try_init();
}

fn get_exit_code(err: &MaccError) -> i32 {
    match err {
        MaccError::Validation(_) => 1,
        MaccError::NotProcessOwner { .. } => 14,
        MaccError::UserScopeNotAllowed(_) => 2,
        MaccError::Io { .. } => 3,
        MaccError::ProjectRootNotFound { .. } => 4,
        MaccError::Config { .. } => 5,
        MaccError::SecretDetected { .. } => 6,
        MaccError::HomeDirNotFound => 7,
        MaccError::ToolSpec { .. } => 8,
        MaccError::Coordinator { .. } => 9,
        MaccError::Storage { .. } => 10,
        MaccError::Git { .. } => 11,
        MaccError::Fetch { .. } => 12,
        MaccError::Catalog { .. } => 13,
    }
}

#[cfg(test)]
fn run_with_engine<E: Engine + Send + Sync + 'static>(cli: Cli, engine: E) -> Result<()> {
    let provider = services::engine_provider::EngineProvider::new(engine);
    run_with_engine_provider(cli, provider)
}

fn run_with_engine_provider(
    cli: Cli,
    provider: services::engine_provider::EngineProvider,
) -> Result<()> {
    debug!(cwd = %cli.cwd, "Starting CLI command routing");
    let cwd = std::path::PathBuf::from(&cli.cwd);
    let absolute_cwd = if cwd.is_absolute() {
        cwd
    } else {
        std::env::current_dir()
            .map_err(|e| MaccError::Io {
                path: ".".into(),
                action: "get current_dir".into(),
                source: e,
            })?
            .join(cwd)
    };

    // Try to canonicalize to resolve .. and symlinks if it exists
    let absolute_cwd = absolute_cwd.canonicalize().unwrap_or(absolute_cwd);
    let engine = provider.shared();
    let app = commands::AppContext::new(
        absolute_cwd.clone(),
        engine.clone(),
        macc_core::resolve::CliOverrides {
            tools: None,
            quiet: if cli.quiet { Some(true) } else { None },
            offline: if cli.offline { Some(true) } else { None },
        },
    );

    match &cli.command {
        Some(Commands::Init {
            force,
            wizard,
            profile,
            fresh,
            restore,
            no_restore_prompt,
            apply,
        }) => {
            commands::init::InitCommand::new(
                app.clone(),
                *force,
                *wizard,
                *fresh,
                restore.clone(),
                *no_restore_prompt,
                *apply,
            )
            .run()?;
            if let Some(profile_name) = profile {
                let config_cmd = commands::config::ConfigCommand::new(
                    app.clone(),
                    commands::config::ConfigAction::Restore {
                        name: profile_name,
                        only: None,
                    },
                );
                config_cmd.run()?;
            }
            Ok(())
        }
        Some(Commands::Config { config_command }) => match config_command {
            ConfigCommands::Save {
                name,
                description,
                only,
            } => commands::config::ConfigCommand::new(
                app.clone(),
                commands::config::ConfigAction::Save {
                    name,
                    description: description.as_deref(),
                    only: only.as_deref(),
                },
            )
            .run(),
            ConfigCommands::Restore { name, only } => commands::config::ConfigCommand::new(
                app.clone(),
                commands::config::ConfigAction::Restore {
                    name,
                    only: only.as_deref(),
                },
            )
            .run(),
            ConfigCommands::List => commands::config::ConfigCommand::new(
                app.clone(),
                commands::config::ConfigAction::List,
            )
            .run(),
            ConfigCommands::Delete { name } => commands::config::ConfigCommand::new(
                app.clone(),
                commands::config::ConfigAction::Delete { name },
            )
            .run(),
        },
        Some(Commands::Quickstart { yes, apply, no_tui }) => {
            commands::quickstart::QuickstartCommand::new(app.clone(), *yes, *apply, *no_tui).run()
        }
        Some(Commands::Plan {
            tools,
            json,
            explain,
        }) => commands::plan::PlanCommand::new(app.clone(), tools.clone(), *json, *explain).run(),
        Some(Commands::Apply {
            tools,
            dry_run,
            allow_user_scope,
            json,
            explain,
            locked,
        }) => commands::apply::ApplyCommand::new(
            app.clone(),
            tools.clone(),
            *dry_run,
            *allow_user_scope,
            *json,
            *explain,
            *locked,
        )
        .run(),
        Some(Commands::Catalog { catalog_command }) => {
            commands::catalog::CatalogCommand::new(app.clone(), catalog_command).run()
        }
        Some(Commands::Install { install_command }) => {
            commands::install::InstallCommand::new(app.clone(), install_command).run()
        }
        Some(Commands::Tui) => {
            let paths = engine.project_ensure_initialized_paths(&absolute_cwd)?;
            std::env::set_current_dir(&paths.root).map_err(|e| MaccError::Io {
                path: paths.root.to_string_lossy().into(),
                action: "set current_dir for tui".into(),
                source: e,
            })?;
            macc_tui::run_tui().map_err(|e| MaccError::Io {
                path: "tui".into(),
                action: "run_tui".into(),
                source: std::io::Error::other(e.to_string()),
            })
        }
        Some(Commands::Web { host, port, assets }) => commands::web::WebCommand::new(
            app.clone(),
            host.clone(),
            *port,
            assets.map(|value| value.into()),
        )
        .run(),
        Some(Commands::Tool { tool_command }) => {
            commands::tool::ToolCommand::new(app.clone(), tool_command).run()
        }
        Some(Commands::Context {
            tool,
            from_files,
            dry_run,
            print_prompt,
        }) => commands::context::ContextCommand::new(
            app.clone(),
            tool.as_deref(),
            from_files,
            *dry_run,
            *print_prompt,
        )
        .run(),
        Some(Commands::Doctor { fix }) => {
            commands::doctor::DoctorCommand::new(app.clone(), *fix).run()
        }
        Some(Commands::Migrate { apply }) => {
            commands::migrate::MigrateCommand::new(app.clone(), *apply).run()
        }
        Some(Commands::Backups { backups_command }) => {
            commands::backups::BackupsCommand::new(app.clone(), backups_command).run()
        }
        Some(Commands::Save {
            name,
            overwrite,
            description,
            only,
            no_sessions,
            include_logs,
            log_max_size,
            log_since,
            redact_logs,
            dry_run,
            cmd,
        }) => {
            let action = match cmd {
                Some(SaveSubcommands::List { matching }) => {
                    commands::save::SaveAction::List { matching: *matching }
                }
                Some(SaveSubcommands::Show { name }) => {
                    commands::save::SaveAction::Show { name: name.clone() }
                }
                Some(SaveSubcommands::Delete { name, yes }) => {
                    commands::save::SaveAction::Delete { name: name.clone(), yes: *yes }
                }
                None => {
                    let n = name.clone().unwrap_or_default();
                    if n.is_empty() {
                        commands::save::SaveAction::List { matching: false }
                    } else {
                        commands::save::SaveAction::Create {
                            name: n,
                            overwrite: *overwrite,
                            description: description.clone(),
                            only: only.clone(),
                            no_sessions: *no_sessions,
                            include_logs: *include_logs,
                            log_max_size: log_max_size.clone(),
                            log_since: log_since.clone(),
                            redact_logs: *redact_logs,
                            dry_run: *dry_run,
                        }
                    }
                }
            };
            commands::save::SaveCommand::new(app.clone(), action).run()
        }
        Some(Commands::Restore {
            name,
            latest,
            user,
            backup,
            dry_run,
            yes,
            apply,
            config_only,
            sessions,
            no_sessions,
            include_logs,
        }) => commands::restore::RestoreCommand::new(
            app.clone(),
            name.clone(),
            *latest,
            *user,
            backup.clone(),
            *dry_run,
            *yes,
            *apply,
            *config_only,
            *sessions,
            *no_sessions,
            *include_logs,
        )
        .run(),
        Some(Commands::Clear {
            save,
            include_logs,
            no_save_prompt,
            force,
            dry_run,
        }) => commands::clear::ClearCommand::new(
            app.clone(),
            save.clone(),
            *include_logs,
            *no_save_prompt,
            *force,
            *dry_run,
        )
        .run(),
        Some(Commands::Worktree { worktree_command }) => {
            commands::worktree::WorktreeCommand::new(app.clone(), worktree_command).run()
        }
        Some(Commands::Logs { logs_command }) => {
            commands::logs::LogsCommand::new(app.clone(), logs_command).run()
        }
        Some(Commands::Supervisor { supervisor_command }) => {
            commands::supervisor::SupervisorCommand::new(app.clone(), supervisor_command).run()
        }
        Some(Commands::Process { process_command }) => {
            commands::process::ProcessCommand::new(app.clone(), process_command).run()
        }
        Some(Commands::Start {
            intent,
            dry_run,
            web,
            tui,
            profile,
            preset,
            locked,
        }) => commands::start::StartCommand::new(
            app.clone(),
            intent.clone(),
            *dry_run,
            *web,
            *tui,
            profile.clone(),
            preset.clone(),
            *locked,
        )
        .run(),
        Some(Commands::Trust) => commands::trust::TrustCommand::new(app.clone()).run(),
        Some(Commands::Lock { lock_command }) => {
            commands::lock::LockCommand::new(app.clone(), lock_command.clone()).run()
        }
        Some(Commands::Failure { failure_command }) => {
            commands::failure::FailureCommand::new(app.clone(), failure_command.clone()).run()
        }
        Some(Commands::Settings { settings_command }) => {
            commands::settings::SettingsCommand::new(app.clone(), settings_command.clone()).run()
        }

        Some(Commands::Explain { task_id, json, since, severity, logs, artifacts, compact }) => {
            let since_seconds = since.as_deref().and_then(parse_since_to_seconds);
            commands::task::ExplainCommand::new(
                app.clone(),
                task_id.clone(),
                *json,
                since_seconds,
                severity.clone(),
                *logs,
                *artifacts,
                *compact,
            )
            .run()
        }

        Some(Commands::Diff { task_id, stat, name_only, base, format }) => {
            commands::task::DiffCommand::new(
                app.clone(),
                task_id.clone(),
                *stat,
                *name_only,
                base.clone(),
                format.clone(),
            )
            .run()
        }

        Some(Commands::Coordinator {
            command_name,
            as_client,
            no_tui,
            supervisor,
            drain,
            graceful,
            force,
            remove_worktrees,
            cleanup_worktrees,
            remove_branches,
            prd,
            coordinator_tool,
            reference_branch,
            tool_priority,
            max_parallel_per_tool_json,
            preset,
            tool_specializations_json,
            max_dispatch,
            max_parallel,
            timeout_seconds,
            phase_runner_max_attempts,
            log_flush_lines,
            log_flush_ms,
            mirror_json_debounce_ms,
            stale_claimed_seconds,
            stale_in_progress_seconds,
            stale_changes_requested_seconds,
            stale_action,
            storage_mode,
            merge_ai_fix,
            merge_job_timeout_seconds,
            merge_hook_timeout_seconds,
            ghost_heartbeat_grace_seconds,
            dispatch_cooldown_seconds,
            json_compat,
            legacy_json_fallback,
            cutover_gate_window_events,
            cutover_gate_max_blocked_ratio,
            cutover_gate_max_stale_ratio,
            error_code_retry_list,
            error_code_retry_max,
            force_kill_grace_seconds,
            max_review_cycles,
            extra_args,
        }) => {
            let mut env_cfg = CoordinatorEnvConfig {
                prd: prd.clone(),
                coordinator_tool: coordinator_tool.clone(),
                reference_branch: reference_branch.clone(),
                tool_priority: tool_priority.clone(),
                max_parallel_per_tool_json: max_parallel_per_tool_json.clone(),
                tool_specializations_json: tool_specializations_json.clone(),
                max_dispatch: *max_dispatch,
                max_parallel: *max_parallel,
                timeout_seconds: *timeout_seconds,
                phase_runner_max_attempts: *phase_runner_max_attempts,
                log_flush_lines: *log_flush_lines,
                log_flush_ms: *log_flush_ms,
                mirror_json_debounce_ms: *mirror_json_debounce_ms,
                stale_claimed_seconds: *stale_claimed_seconds,
                stale_in_progress_seconds: *stale_in_progress_seconds,
                stale_changes_requested_seconds: *stale_changes_requested_seconds,
                stale_action: stale_action.clone(),
                storage_mode: storage_mode.clone(),
                merge_ai_fix: *merge_ai_fix,
                merge_job_timeout_seconds: *merge_job_timeout_seconds,
                merge_hook_timeout_seconds: *merge_hook_timeout_seconds,
                ghost_heartbeat_grace_seconds: *ghost_heartbeat_grace_seconds,
                dispatch_cooldown_seconds: *dispatch_cooldown_seconds,
                json_compat: *json_compat,
                legacy_json_fallback: *legacy_json_fallback,
                error_code_retry_list: error_code_retry_list.clone(),
                error_code_retry_max: *error_code_retry_max,
                cutover_gate_window_events: *cutover_gate_window_events,
                cutover_gate_max_blocked_ratio: *cutover_gate_max_blocked_ratio,
                cutover_gate_max_stale_ratio: *cutover_gate_max_stale_ratio,
                rate_limit_backoff_base_seconds: None,
                rate_limit_backoff_max_seconds: None,
                rate_limit_fallback_enabled: None,
                rate_limit_throttle_parallel: None,
                force_kill_grace_seconds: *force_kill_grace_seconds,
                max_review_cycles: *max_review_cycles,
                safety_policy: None,
                destructive_actions: None,
            };
            if let Some(ref p) = preset {
                macc_core::ops_motif::apply_preset_to_env_cfg(&mut env_cfg, p)?;
            }
            commands::coordinator::CoordinatorCommand::new(
                app.clone(),
                coordinator::command::CoordinatorCommandInput {
                    command_name: command_name.clone(),
                    client_id: as_client
                        .clone()
                        .unwrap_or_else(|| format!("cli-{}", std::process::id())),
                    no_tui: *no_tui,
                    supervisor: *supervisor,
                    drain: *drain,
                    graceful: *graceful,
                    force: *force,
                    remove_worktrees: *remove_worktrees || *cleanup_worktrees,
                    remove_branches: *remove_branches,
                    env_cfg,
                    extra_args: extra_args.clone(),
                },
            )
            .run()
        }
        None => {
            let paths = engine.project_ensure_initialized_paths(&absolute_cwd)?;
            std::env::set_current_dir(&paths.root).map_err(|e| MaccError::Io {
                path: paths.root.to_string_lossy().into(),
                action: "set current_dir for tui".into(),
                source: e,
            })?;
            macc_tui::run_tui().map_err(|e| MaccError::Io {
                path: "tui".into(),
                action: "run_tui".into(),
                source: std::io::Error::other(e.to_string()),
            })
        }
    }
}

pub(crate) fn confirm_yes_no(prompt: &str) -> Result<bool> {
    use std::io::{self, Write, IsTerminal};

    if std::env::var("CARGO_MANIFEST_DIR").is_ok() || !io::stdin().is_terminal() {
        println!("{} [y/N]: (auto-confirmed yes in test/non-interactive environment)", prompt);
        return Ok(true);
    }

    print!("{}", prompt);
    io::stdout().flush().map_err(|e| MaccError::Io {
        path: "stdout".into(),
        action: "flush prompt".into(),
        source: e,
    })?;

    let mut input = String::new();
    io::stdin()
        .read_line(&mut input)
        .map_err(|e| MaccError::Io {
            path: "stdin".into(),
            action: "read confirmation".into(),
            source: e,
        })?;
    let value = input.trim().to_ascii_lowercase();
    Ok(value == "y" || value == "yes")
}

pub(crate) fn print_checks(checks: &[macc_core::doctor::ToolCheck]) {
    println!("{:<20} {:<10} {:<30}", "CHECK", "STATUS", "TARGET");
    println!("{:-<20} {:-<10} {:-<30}", "", "", "");

    for check in checks {
        let status_str = match &check.status {
            macc_core::doctor::ToolStatus::Installed => "OK".to_string(),
            macc_core::doctor::ToolStatus::Missing => "MISSING".to_string(),
            macc_core::doctor::ToolStatus::Error(e) => format!("ERROR: {}", e),
        };
        println!(
            "{:<20} {:<10} {:<30}",
            check.name, status_str, check.check_target
        );
    }
}

#[derive(Debug, serde::Serialize)]
struct PlanPreviewSummary {
    total_actions: usize,
    files_write: usize,
    files_merge: usize,
    consent_required: usize,
    backup_required: usize,
    backup_path: String,
}

#[derive(Debug, serde::Serialize)]
struct PlanPreviewOp {
    path: String,
    kind: String,
    scope: String,
    consent_required: bool,
    backup_required: bool,
    set_executable: bool,
    explain: String,
    diff_kind: String,
    diff: Option<String>,
    diff_truncated: bool,
}

#[derive(Debug, serde::Serialize)]
struct PlanPreviewOutput {
    summary: PlanPreviewSummary,
    operations: Vec<PlanPreviewOp>,
}

fn render_plan_preview(
    paths: &macc_core::ProjectPaths,
    plan: &macc_core::plan::ActionPlan,
    ops: &[macc_core::plan::PlannedOp],
    json_output: bool,
    explain: bool,
) -> Result<()> {
    // Keep core validation behavior from legacy preview.
    macc_core::validate_plan(plan, true)?;
    let summary = build_plan_preview_summary(paths, plan, ops);

    if json_output {
        let payload = PlanPreviewOutput {
            summary,
            operations: build_plan_preview_ops(ops, true),
        };
        let rendered = serde_json::to_string_pretty(&payload).map_err(|e| {
            MaccError::Validation(format!("Failed to serialize plan JSON output: {}", e))
        })?;
        println!("{}", rendered);
        return Ok(());
    }

    print_plan_preview_summary(&summary);
    print_plan_preview_ops(ops, explain);
    println!("Core: Total actions planned: {}", plan.actions.len());
    Ok(())
}

fn build_plan_preview_summary(
    paths: &macc_core::ProjectPaths,
    plan: &macc_core::plan::ActionPlan,
    ops: &[macc_core::plan::PlannedOp],
) -> PlanPreviewSummary {
    let files_write = ops
        .iter()
        .filter(|op| op.kind == macc_core::plan::PlannedOpKind::Write)
        .count();
    let files_merge = ops
        .iter()
        .filter(|op| op.kind == macc_core::plan::PlannedOpKind::Merge)
        .count();
    let consent_required = ops.iter().filter(|op| op.consent_required).count();
    let backup_required = ops.iter().filter(|op| op.metadata.backup_required).count();
    PlanPreviewSummary {
        total_actions: plan.actions.len(),
        files_write,
        files_merge,
        consent_required,
        backup_required,
        backup_path: paths.backups_dir.display().to_string(),
    }
}

fn print_plan_preview_summary(summary: &PlanPreviewSummary) {
    println!("Plan summary:");
    println!(
        "  - files write: {} | merges: {} | user-level changes: {}{}",
        summary.files_write,
        summary.files_merge,
        summary.consent_required,
        if summary.consent_required > 0 {
            " (consent required)"
        } else {
            ""
        }
    );
    println!(
        "  - backup-required ops: {} | backup path: {}",
        summary.backup_required, summary.backup_path
    );
}

fn build_plan_preview_ops(
    ops: &[macc_core::plan::PlannedOp],
    include_diff: bool,
) -> Vec<PlanPreviewOp> {
    ops.iter()
        .map(|op| {
            let mut diff_kind = "unsupported".to_string();
            let mut diff = None;
            let mut truncated = false;
            if include_diff {
                let view = macc_core::plan::render_diff(op);
                diff_kind = match view.kind {
                    macc_core::plan::DiffViewKind::Text => "text".to_string(),
                    macc_core::plan::DiffViewKind::Json => "json".to_string(),
                    macc_core::plan::DiffViewKind::Unsupported => "unsupported".to_string(),
                };
                truncated = view.truncated;
                if !view.diff.is_empty() {
                    diff = Some(view.diff);
                }
            }
            PlanPreviewOp {
                path: op.path.clone(),
                kind: format!("{:?}", op.kind).to_ascii_lowercase(),
                scope: match op.scope {
                    macc_core::plan::Scope::Project => "project".into(),
                    macc_core::plan::Scope::User => "user".into(),
                },
                consent_required: op.consent_required,
                backup_required: op.metadata.backup_required,
                set_executable: op.metadata.set_executable,
                explain: explain_operation(op),
                diff_kind,
                diff,
                diff_truncated: truncated,
            }
        })
        .collect()
}

fn print_plan_preview_ops(ops: &[macc_core::plan::PlannedOp], explain: bool) {
    for op in ops {
        let scope = match op.scope {
            macc_core::plan::Scope::Project => "project",
            macc_core::plan::Scope::User => "user",
        };
        println!(
            "\n[{}] {} ({})",
            format!("{:?}", op.kind).to_ascii_uppercase(),
            op.path,
            scope
        );
        if explain {
            println!("  why: {}", explain_operation(op));
        }
        let diff_view = macc_core::plan::render_diff(op);
        if !diff_view.diff.is_empty() {
            let indented = diff_view
                .diff
                .lines()
                .map(|line| format!("    {}", line))
                .collect::<Vec<_>>()
                .join("\n");
            println!("{}", indented);
            if diff_view.truncated {
                println!("  warning: diff truncated for readability.");
            }
        } else {
            println!("  (no textual diff available)");
        }
    }
}

fn explain_operation(op: &macc_core::plan::PlannedOp) -> String {
    match op.kind {
        macc_core::plan::PlannedOpKind::Write => {
            if op.path == ".gitignore" {
                "ensures required ignore patterns are present".into()
            } else {
                "writes generated configuration/content".into()
            }
        }
        macc_core::plan::PlannedOpKind::Merge => {
            "merges generated JSON fragment into existing file".into()
        }
        macc_core::plan::PlannedOpKind::Mkdir => "creates required directory structure".into(),
        macc_core::plan::PlannedOpKind::Delete => "deletes stale managed artifact".into(),
        macc_core::plan::PlannedOpKind::Other => "normalization/supplementary operation".into(),
    }
}

fn print_pre_apply_summary(
    paths: &macc_core::ProjectPaths,
    plan: &macc_core::plan::ActionPlan,
    ops: &[macc_core::plan::PlannedOp],
) {
    let summary = build_plan_preview_summary(paths, plan, ops);
    println!("Pre-apply summary:");
    println!(
        "  - {} writes, {} merges, {} user-level changes{}",
        summary.files_write,
        summary.files_merge,
        summary.consent_required,
        if summary.consent_required > 0 {
            " (consent required)"
        } else {
            ""
        }
    );
    println!("  - backups may be created under {}", summary.backup_path);
}

fn print_pre_apply_explanations(ops: &[macc_core::plan::PlannedOp]) {
    println!("Pre-apply explain:");
    for op in ops {
        println!("  - {}: {}", op.path, explain_operation(op));
    }
}

fn confirm_user_scope_apply(
    paths: &macc_core::ProjectPaths,
    ops: &[macc_core::plan::PlannedOp],
) -> Result<()> {
    let user_ops: Vec<&macc_core::plan::PlannedOp> = ops
        .iter()
        .filter(|op| op.scope == macc_core::plan::Scope::User)
        .collect();
    if user_ops.is_empty() {
        return Ok(());
    }

    println!("\nUser-level merge confirmation required");
    println!(
        "  - {} user-scoped file(s) will be touched.",
        user_ops.len()
    );
    let preview_limit = 12usize;
    for op in user_ops.iter().take(preview_limit) {
        println!("    - {}", op.path);
    }
    if user_ops.len() > preview_limit {
        println!("    ... and {} more", user_ops.len() - preview_limit);
    }

    let user_backup_root = macc_core::domain::backups::user_backup_root()?;
    println!(
        "  - Backups will be written under: {}",
        user_backup_root.display()
    );
    println!("  - To inspect backups: macc backups list --user");
    println!("  - To restore latest user backup set: macc restore --latest --user");
    println!(
        "  - Project backups (if any) are under: {}",
        paths.backups_dir.display()
    );

    if !confirm_yes_no("Proceed with user-level changes [y/N]? ")? {
        return Err(MaccError::Validation(
            "Apply cancelled by user at user-level merge confirmation.".into(),
        ));
    }

    Ok(())
}

// ... existing catalog functions (run_remote_search, list_skills, etc) ...

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_since_to_seconds() {
        assert_eq!(parse_since_to_seconds(""), None);
        assert_eq!(parse_since_to_seconds("   "), None);
        assert_eq!(parse_since_to_seconds("30m"), Some(1800));
        assert_eq!(parse_since_to_seconds("1h"), Some(3600));
        assert_eq!(parse_since_to_seconds("2d"), Some(2 * 24 * 3600));
        assert_eq!(parse_since_to_seconds("10s"), Some(10));
        // Raw integer defaults to minutes for backward compatibility
        assert_eq!(parse_since_to_seconds("30"), Some(1800));
    }

    use crate::coordinator::legacy_helpers::{
        read_registry_counts, run_coordinator_command,
        validate_coordinator_runtime_transition_action, validate_coordinator_transition_action,
        COORDINATOR_TASK_REGISTRY_REL_PATH,
    };
    use crate::test_support::run_git_ok;
    use macc_core::service::tooling::{extract_version_token, run_version_command};
    use macc_core::TestEngine;
    use macc_core::{MaccError, McpCatalog, SkillsCatalog};
    use std::fs;
    use std::io;
    use std::net::TcpListener;
    use std::path::{Path, PathBuf};
    use std::sync::{Mutex, MutexGuard, OnceLock};

    fn env_test_lock() -> MutexGuard<'static, ()> {
        static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
        LOCK.get_or_init(|| Mutex::new(())).lock().unwrap()
    }

    fn bind_loopback() -> Option<(TcpListener, u16)> {
        match TcpListener::bind("127.0.0.1:0") {
            Ok(listener) => {
                let port = listener.local_addr().ok()?.port();
                Some((listener, port))
            }
            Err(e) if e.kind() == std::io::ErrorKind::PermissionDenied => {
                tracing::warn!("Skipping test: cannot bind loopback socket ({})", e);
                None
            }
            Err(e) => panic!("Failed to bind loopback socket: {}", e),
        }
    }

    fn fixture_ids() -> Vec<String> {
        TestEngine::generate_fixture_ids(2)
    }

    fn fixture_engine(ids: &[String]) -> TestEngine {
        TestEngine::with_fixtures_for_ids(ids)
    }

    fn write_executable_script(path: &std::path::Path, content: &str) {
        std::fs::write(path, content).unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mut perms = std::fs::metadata(path).unwrap().permissions();
            perms.set_mode(0o755);
            std::fs::set_permissions(path, perms).unwrap();
        }
    }

    fn run_scripted_full_cycle(
        repo_root: &Path,
        coordinator_path: &Path,
        canonical: &macc_core::config::CanonicalConfig,
        coordinator: Option<&macc_core::config::CoordinatorConfig>,
        env_cfg: &CoordinatorEnvConfig,
    ) -> macc_core::Result<()> {
        let registry_path = repo_root.join(COORDINATOR_TASK_REGISTRY_REL_PATH);
        let timeout_seconds = env_cfg
            .timeout_seconds
            .or_else(|| coordinator.and_then(|cfg| cfg.timeout_seconds))
            .unwrap_or(30) as u64;
        let max_cycles = 32usize;
        let mut no_progress_cycles = 0usize;
        let started = std::time::Instant::now();

        for _cycle in 1..=max_cycles {
            let before = read_registry_counts(&registry_path)?;

            for command_name in ["sync", "dispatch", "advance", "reconcile", "cleanup"] {
                run_coordinator_command(
                    repo_root,
                    coordinator_path,
                    command_name,
                    &[],
                    canonical,
                    coordinator,
                    env_cfg,
                )?;
            }

            let after = read_registry_counts(&registry_path)?;
            if after.todo == 0 && after.active == 0 && after.blocked == 0 {
                return Ok(());
            }

            if after == before {
                no_progress_cycles += 1;
            } else {
                no_progress_cycles = 0;
            }

            if no_progress_cycles >= 2 {
                return Err(MaccError::Validation(format!(
                    "Coordinator made no progress for {} cycles (todo={}, active={}, blocked={}).",
                    no_progress_cycles, after.todo, after.active, after.blocked
                )));
            }

            if started.elapsed() > std::time::Duration::from_secs(timeout_seconds) {
                return Err(MaccError::Validation(format!(
                    "Coordinator run timed out after {} seconds.",
                    timeout_seconds
                )));
            }
        }

        Err(MaccError::Validation(format!(
            "Coordinator run reached max cycles ({}) without converging.",
            max_cycles
        )))
    }

    fn collect_rs_files(root: &Path, out: &mut Vec<PathBuf>) {
        let Ok(entries) = std::fs::read_dir(root) else {
            return;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                collect_rs_files(&path, out);
            } else if path.extension().and_then(|s| s.to_str()) == Some("rs") {
                out.push(path);
            }
        }
    }

    #[test]
    fn test_exit_code_mapping() {
        assert_eq!(get_exit_code(&MaccError::Validation("test".into())), 1);
        assert_eq!(
            get_exit_code(&MaccError::UserScopeNotAllowed("test".into())),
            2
        );
        assert_eq!(
            get_exit_code(&MaccError::Io {
                path: "test".into(),
                action: "test".into(),
                source: io::Error::new(io::ErrorKind::Other, "test")
            }),
            3
        );
        assert_eq!(
            get_exit_code(&MaccError::ProjectRootNotFound {
                start_dir: "test".into()
            }),
            4
        );
        let yaml_err = serde_yaml::from_str::<serde_yaml::Value>("[").unwrap_err();
        assert_eq!(
            get_exit_code(&MaccError::Config {
                path: "test.yaml".into(),
                source: yaml_err
            }),
            5
        );
        assert_eq!(
            get_exit_code(&MaccError::SecretDetected {
                path: "test.txt".into(),
                details: "test".into()
            }),
            6
        );
    }

    #[test]
    fn test_no_direct_git_process_invocations_in_cli_tui() {
        let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        let cli_src = manifest_dir.join("src");
        let tui_src = manifest_dir.join("../tui/src");
        let patterns = [
            "Command::new(\"git\")",
            "std::process::Command::new(\"git\")",
        ];

        let mut files = Vec::new();
        collect_rs_files(&cli_src, &mut files);
        collect_rs_files(&tui_src, &mut files);

        let mut violations = Vec::new();
        for file in files {
            let Ok(content) = std::fs::read_to_string(&file) else {
                continue;
            };
            if patterns.iter().any(|p| content.contains(p)) {
                violations.push(file.display().to_string());
            }
        }

        assert!(
            violations.is_empty(),
            "Direct git process invocation detected; use macc_core::git facade instead: {:?}",
            violations
        );
    }

    #[test]
    fn test_tui_has_no_direct_process_management() {
        let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        let tui_src = manifest_dir.join("../tui/src");
        let patterns = [
            "std::process::Child",
            "std::process::Command",
            "tokio::process::Child",
            "tokio::process::Command",
        ];

        let mut files = Vec::new();
        collect_rs_files(&tui_src, &mut files);

        let mut violations = Vec::new();
        for file in files {
            let Ok(content) = std::fs::read_to_string(&file) else {
                continue;
            };
            if patterns.iter().any(|p| content.contains(p)) {
                violations.push(file.display().to_string());
            }
        }

        assert!(
            violations.is_empty(),
            "Direct process management in TUI detected; route via Engine coordinator/task-runner APIs instead: {:?}",
            violations
        );
    }

    #[test]
    fn test_parse_coordinator_validate_transition_args() {
        let args = vec![
            "--from".to_string(),
            "todo".to_string(),
            "--to".to_string(),
            "claimed".to_string(),
        ];
        let parsed = WorkflowTransitionArgs::try_from(args.as_slice()).unwrap();
        let from = parsed.from;
        let to = parsed.to;
        assert_eq!(from, WorkflowState::Todo);
        assert_eq!(to, WorkflowState::Claimed);
    }

    #[test]
    fn test_parse_coordinator_run_supervisor_flag() {
        let cli = Cli::try_parse_from(["macc", "coordinator", "run", "--supervisor"])
            .expect("parse coordinator run --supervisor");
        match cli.command {
            Some(Commands::Coordinator {
                command_name,
                supervisor,
                ..
            }) => {
                assert_eq!(command_name, "run");
                assert!(supervisor);
            }
            _ => panic!("unexpected command"),
        }
    }

    #[test]
    fn test_parse_settings_and_preset_args() {
        let cli = Cli::try_parse_from(["macc", "settings", "show", "--advanced", "--admin"])
            .expect("parse settings show");
        match cli.command {
            Some(Commands::Settings { settings_command }) => match settings_command {
                commands::settings::SettingsCommands::Show { advanced, admin, .. } => {
                    assert!(advanced);
                    assert!(admin);
                }
                _ => panic!("expected Show"),
            },
            _ => panic!("unexpected command"),
        }

        let cli2 = Cli::try_parse_from(["macc", "settings", "preset", "balanced"])
            .expect("parse settings preset");
        match cli2.command {
            Some(Commands::Settings { settings_command }) => match settings_command {
                commands::settings::SettingsCommands::Preset { name } => {
                    assert_eq!(name, "balanced");
                }
                _ => panic!("expected Preset"),
            },
            _ => panic!("unexpected command"),
        }

        let cli3 = Cli::try_parse_from(["macc", "coordinator", "run", "--preset", "throughput"])
            .expect("parse coordinator run --preset");
        match cli3.command {
            Some(Commands::Coordinator { preset, .. }) => {
                assert_eq!(preset, Some("throughput".to_string()));
            }
            _ => panic!("unexpected command"),
        }
    }

    #[test]
    fn test_parse_process_claim_command() {
        let cli = Cli::try_parse_from([
            "macc",
            "process",
            "claim",
            "--kind",
            "coordinator",
            "--pid",
            "1234",
        ])
        .expect("parse process claim command");
        match cli.command {
            Some(Commands::Process { process_command }) => match process_command {
                commands::process::ProcessCommands::Claim { kind, pid } => {
                    assert_eq!(kind, commands::process::ProcessKindArg::Coordinator);
                    assert_eq!(pid, 1234);
                }
                _ => panic!("unexpected process command"),
            },
            _ => panic!("unexpected command"),
        }
    }

    #[test]
    fn test_help_includes_process_subcommand() {
        use clap::CommandFactory;

        let mut command = Cli::command();
        let help = command.render_long_help().to_string();
        assert!(help.contains("process"));
    }

    #[test]
    fn test_validate_coordinator_transition_action_rejects_invalid() {
        let args = vec![
            "--from".to_string(),
            "todo".to_string(),
            "--to".to_string(),
            "merged".to_string(),
        ];
        let err = validate_coordinator_transition_action(&args).unwrap_err();
        assert!(err.to_string().contains("invalid transition"));
    }

    #[test]
    fn test_parse_coordinator_validate_runtime_transition_args() {
        let args = vec![
            "--from".to_string(),
            "running".to_string(),
            "--to".to_string(),
            "phase_done".to_string(),
        ];
        let parsed = RuntimeTransitionArgs::try_from(args.as_slice()).unwrap();
        let from = parsed.from;
        let to = parsed.to;
        assert_eq!(from, RuntimeStatus::Running);
        assert_eq!(to, RuntimeStatus::PhaseDone);
    }

    #[test]
    fn test_validate_coordinator_runtime_transition_action_rejects_invalid() {
        let args = vec![
            "--from".to_string(),
            "idle".to_string(),
            "--to".to_string(),
            "phase_done".to_string(),
        ];
        let err = validate_coordinator_runtime_transition_action(&args).unwrap_err();
        assert!(err.to_string().contains("invalid runtime transition"));
    }

    #[test]
    fn test_parse_coordinator_runtime_status_from_event_args() {
        let args = vec![
            "--type".to_string(),
            "heartbeat".to_string(),
            "--status".to_string(),
            "running".to_string(),
        ];
        let parsed = RuntimeStatusFromEventArgs::try_from(args.as_slice()).unwrap();
        let event_type = parsed.event_type;
        let status = parsed.status;
        assert_eq!(event_type, "heartbeat");
        assert_eq!(status, "running");
    }

    #[test]
    fn test_parse_coordinator_storage_sync_args() {
        let args = vec!["--direction".to_string(), "import".to_string()];
        let direction = StorageSyncArgs::try_from(args.as_slice())
            .unwrap()
            .direction;
        assert_eq!(direction, CoordinatorStorageTransfer::ImportJsonToSqlite);
    }

    #[test]
    fn test_read_coordinator_counts() {
        let root = std::env::temp_dir().join(format!("macc_counts_test_{}", uuid_v4_like()));
        let registry = root
            .join(".macc")
            .join("automation")
            .join("task")
            .join("task_registry.json");
        std::fs::create_dir_all(registry.parent().unwrap()).unwrap();
        std::fs::write(
            &registry,
            r#"{
  "tasks": [
    {"id":"A","state":"todo"},
    {"id":"B","state":"in_progress"},
    {"id":"C","state":"blocked"},
    {"id":"D","state":"merged"},
    {"id":"E","state":"queued"}
  ]
}"#,
        )
        .unwrap();
        let counts = read_registry_counts(&registry).unwrap();
        assert_eq!(counts.total, 5);
        assert_eq!(counts.todo, 1);
        assert_eq!(counts.active, 2);
        assert_eq!(counts.blocked, 1);
        assert_eq!(counts.merged, 1);
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn test_cwd_support() -> macc_core::Result<()> {
        let temp_base = std::env::temp_dir().join(format!("macc_cli_test_{}", uuid_v4_like()));
        let project_dir = temp_base.join("nested/project");
        // Do not create project_dir, let 'init' handle it (or create its parent)
        std::fs::create_dir_all(&temp_base).unwrap();

        // Mock Cli for 'init'
        let cli = Cli {
            cwd: project_dir.to_string_lossy().into(),
            verbose: true,
            quiet: false,
            offline: false,
            web_port: None,
            command: Some(Commands::Init {
                force: false,
                wizard: false,
                profile: None,
                fresh: false,
                restore: None,
                no_restore_prompt: true,
                apply: false,
}),
        };

        run_with_engine(cli, TestEngine::with_fixtures())?;

        // Verify files created
        assert!(project_dir.exists());
        assert!(project_dir.join(".macc/macc.yaml").exists());

        // Cleanup
        std::fs::remove_dir_all(&temp_base).ok();

        Ok(())
    }

    #[test]
    fn test_init_idempotence_and_force() -> macc_core::Result<()> {
        let temp_base = std::env::temp_dir().join(format!("macc_init_test_{}", uuid_v4_like()));
        std::fs::create_dir_all(&temp_base).unwrap();

        // 1. Initial init
        let cli = Cli {
            cwd: temp_base.to_string_lossy().into(),
            verbose: false,
            quiet: false,
            offline: false,
            web_port: None,
            command: Some(Commands::Init {
                force: false,
                wizard: false,
                profile: None,
                fresh: false,
                restore: None,
                no_restore_prompt: true,
                apply: false,
}),
        };
        run_with_engine(cli, TestEngine::with_fixtures())?;

        assert!(temp_base.join(".macc/macc.yaml").exists());
        assert!(temp_base.join(".macc/backups").is_dir());
        assert!(temp_base.join(".macc/tmp").is_dir());

        // Modify config to check if it's preserved
        let config_path = temp_base.join(".macc/macc.yaml");
        let original_content = "modified: true";
        std::fs::write(&config_path, original_content).unwrap();

        // 2. Second init without force (idempotence)
        let cli_idempotent = Cli {
            cwd: temp_base.to_string_lossy().into(),
            verbose: false,
            quiet: false,
            offline: false,
            web_port: None,
            command: Some(Commands::Init {
                force: false,
                wizard: false,
                profile: None,
                fresh: false,
                restore: None,
                no_restore_prompt: true,
                apply: false,
}),
        };
        run_with_engine(cli_idempotent, TestEngine::with_fixtures())?;

        let content = std::fs::read_to_string(&config_path).unwrap();
        assert_eq!(
            content, original_content,
            "Config should not be overwritten without --force"
        );

        // 3. Third init with force
        let cli_force = Cli {
            cwd: temp_base.to_string_lossy().into(),
            verbose: false,
            quiet: false,
            offline: false,
            web_port: None,
            command: Some(Commands::Init {
                force: true,
                wizard: false,
                profile: None,
                fresh: false,
                restore: None,
                no_restore_prompt: true,
                apply: false,
}),
        };
        run_with_engine(cli_force, TestEngine::with_fixtures())?;

        let content = std::fs::read_to_string(&config_path).unwrap();
        assert_ne!(
            content, original_content,
            "Config should be overwritten with --force"
        );
        assert!(
            content.contains("version: v1"),
            "Should contain default config"
        );

        // Cleanup
        std::fs::remove_dir_all(&temp_base).ok();

        Ok(())
    }

    #[test]
    fn test_plan_with_tools_override() -> macc_core::Result<()> {
        let _guard = env_test_lock();
        let temp_base = std::env::temp_dir().join(format!("macc_tools_test_{}", uuid_v4_like()));
        std::fs::create_dir_all(&temp_base).unwrap();
        let temp_home = temp_base.join("home");
        std::fs::create_dir_all(&temp_home).unwrap();
        let old_home = std::env::var("HOME").ok();
        std::env::set_var("HOME", &temp_home);
        let ids = fixture_ids();
        let tool_one = ids[0].clone();
        let tool_two = ids[1].clone();

        // 1. Init
        run_with_engine(
            Cli {
                cwd: temp_base.to_string_lossy().into(),
                verbose: false,
                quiet: false,
                offline: false,
                web_port: None,
                command: Some(Commands::Init {
                    force: false,
                    wizard: false,
                    profile: None,
                    fresh: false,
                    restore: None,
                    no_restore_prompt: true,
                    apply: false,
}),
            },
            fixture_engine(&ids),
        )?;

        // 2. Plan with valid tool override (using fixtures)
        run_with_engine(
            Cli {
                cwd: temp_base.to_string_lossy().into(),
                verbose: false,
                quiet: false,
                offline: false,
                web_port: None,
                command: Some(Commands::Plan {
                    tools: Some(format!("{},{}", tool_one, tool_two)),
                    json: false,
                    explain: false,
                }),
            },
            fixture_engine(&ids),
        )?;

        // 3. Plan with unknown tool (should NOT error, just skip/warn)
        run_with_engine(
            Cli {
                cwd: temp_base.to_string_lossy().into(),
                verbose: false,
                quiet: false,
                offline: false,
                web_port: None,
                command: Some(Commands::Plan {
                    tools: Some(format!("{},unknown", tool_one)),
                    json: false,
                    explain: false,
                }),
            },
            fixture_engine(&ids),
        )?;

        // Cleanup
        if let Some(old) = old_home {
            std::env::set_var("HOME", old);
        } else {
            std::env::remove_var("HOME");
        }
        std::fs::remove_dir_all(&temp_base).ok();
        Ok(())
    }

    #[test]
    fn test_coordinator_run_full_cycle_converges() -> macc_core::Result<()> {
        let root = std::env::temp_dir().join(format!("macc_cli_coord_run_{}", uuid_v4_like()));
        std::fs::create_dir_all(&root).unwrap();
        let registry = root.join(COORDINATOR_TASK_REGISTRY_REL_PATH);
        std::fs::create_dir_all(registry.parent().expect("registry parent")).unwrap();
        fs::write(
            &registry,
            r#"{
  "schema_version": 1,
  "tasks": [
    {
      "id": "TASK-1",
      "state": "todo",
      "dependencies": [],
      "exclusive_resources": []
    }
  ],
  "resource_locks": {},
  "state_mapping": {}
}"#,
        )
        .unwrap();
        let prd_path = root.join("prd.json");
        fs::write(
            &prd_path,
            r#"{
  "lot": "Test",
  "tasks": [
    {
      "id": "TASK-1",
      "title": "Test task",
      "dependencies": [],
      "exclusive_resources": []
    }
  ]
}"#,
        )
        .unwrap();

        let script = root.join("fake-full-cycle.sh");
        write_executable_script(
            &script,
            r#"#!/usr/bin/env bash
set -euo pipefail
action="${1:-dispatch}"
case "$action" in
  sync|reconcile|cleanup)
    ;;
  dispatch)
    tmp="$(mktemp)"
    jq '
      .tasks |= map(
        if .state == "todo" then .state = "in_progress" else . end
      )
    ' "$TASK_REGISTRY_FILE" >"$tmp"
    mv "$tmp" "$TASK_REGISTRY_FILE"
    ;;
  advance)
    tmp="$(mktemp)"
    jq '
      .tasks |= map(
        if .state == "in_progress" then .state = "merged" else . end
      )
    ' "$TASK_REGISTRY_FILE" >"$tmp"
    mv "$tmp" "$TASK_REGISTRY_FILE"
    ;;
esac
"#,
        );

        let canonical = macc_core::config::CanonicalConfig::default();
        let coordinator_cfg = macc_core::config::CoordinatorConfig {
            timeout_seconds: Some(10),
            ..Default::default()
        };
        let env_cfg = CoordinatorEnvConfig {
            prd: Some(prd_path.to_string_lossy().into_owned()),
            timeout_seconds: Some(10),
            ..Default::default()
        };

        run_scripted_full_cycle(&root, &script, &canonical, Some(&coordinator_cfg), &env_cfg)?;

        let final_state: serde_json::Value =
            serde_json::from_str(&fs::read_to_string(&registry).unwrap()).unwrap();
        assert_eq!(
            final_state["tasks"][0]["state"].as_str(),
            Some("merged"),
            "coordinator run should converge to merged"
        );
        std::fs::remove_dir_all(&root).ok();
        Ok(())
    }

    #[test]
    fn test_coordinator_run_detects_no_progress() -> macc_core::Result<()> {
        let root = std::env::temp_dir().join(format!("macc_cli_coord_stall_{}", uuid_v4_like()));
        std::fs::create_dir_all(&root).unwrap();
        let registry = root.join(COORDINATOR_TASK_REGISTRY_REL_PATH);
        std::fs::create_dir_all(registry.parent().expect("registry parent")).unwrap();
        fs::write(
            &registry,
            r#"{
  "schema_version": 1,
  "tasks": [
    {
      "id": "TASK-STALL",
      "state": "todo",
      "dependencies": [],
      "exclusive_resources": []
    }
  ],
  "resource_locks": {},
  "state_mapping": {}
}"#,
        )
        .unwrap();
        let prd_path = root.join("prd.json");
        fs::write(
            &prd_path,
            r#"{
  "lot": "Test",
  "tasks": [
    {
      "id": "TASK-STALL",
      "title": "Stall task",
      "dependencies": [],
      "exclusive_resources": []
    }
  ]
}"#,
        )
        .unwrap();

        let script = root.join("fake-no-progress.sh");
        write_executable_script(
            &script,
            r#"#!/usr/bin/env bash
set -euo pipefail
exit 0
"#,
        );

        let canonical = macc_core::config::CanonicalConfig::default();
        let coordinator_cfg = macc_core::config::CoordinatorConfig {
            timeout_seconds: Some(10),
            ..Default::default()
        };
        let env_cfg = CoordinatorEnvConfig {
            prd: Some(prd_path.to_string_lossy().into_owned()),
            timeout_seconds: Some(10),
            ..Default::default()
        };

        let err =
            run_scripted_full_cycle(&root, &script, &canonical, Some(&coordinator_cfg), &env_cfg)
                .expect_err("stalling coordinator should fail");
        let msg = err.to_string();
        assert!(
            msg.contains("no progress"),
            "expected no-progress error, got: {}",
            msg
        );
        std::fs::remove_dir_all(&root).ok();
        Ok(())
    }

    #[test]
    fn test_coordinator_control_plane_same_input_same_final_state() -> macc_core::Result<()> {
        fn run_once(
            root: &std::path::Path,
            script: &std::path::Path,
        ) -> macc_core::Result<serde_json::Value> {
            let registry = root.join(COORDINATOR_TASK_REGISTRY_REL_PATH);
            std::fs::create_dir_all(registry.parent().expect("registry parent")).unwrap();
            fs::write(
                &registry,
                r#"{
  "schema_version": 1,
  "tasks": [
    {"id":"T1","state":"todo","dependencies":[],"exclusive_resources":[]},
    {"id":"T2","state":"todo","dependencies":[],"exclusive_resources":[]}
  ],
  "resource_locks": {},
  "state_mapping": {}
}"#,
            )
            .unwrap();
            let prd_path = root.join("prd.json");
            fs::write(
                &prd_path,
                r#"{
  "lot": "Deterministic",
  "tasks": [
    {"id":"T1","title":"Task 1","dependencies":[],"exclusive_resources":[]},
    {"id":"T2","title":"Task 2","dependencies":[],"exclusive_resources":[]}
  ]
}"#,
            )
            .unwrap();

            let canonical = macc_core::config::CanonicalConfig::default();
            let coordinator_cfg = macc_core::config::CoordinatorConfig {
                timeout_seconds: Some(10),
                ..Default::default()
            };
            let env_cfg = CoordinatorEnvConfig {
                prd: Some(prd_path.to_string_lossy().into_owned()),
                timeout_seconds: Some(10),
                ..Default::default()
            };

            run_scripted_full_cycle(root, script, &canonical, Some(&coordinator_cfg), &env_cfg)?;

            let final_state: serde_json::Value =
                serde_json::from_str(&fs::read_to_string(&registry).unwrap()).unwrap();
            Ok(final_state)
        }

        let root =
            std::env::temp_dir().join(format!("macc_cli_cp_deterministic_{}", uuid_v4_like()));
        std::fs::create_dir_all(&root).unwrap();
        let script = root.join("fake-cp-deterministic.sh");
        write_executable_script(
            &script,
            r#"#!/usr/bin/env bash
set -euo pipefail
action="${1:-dispatch}"
case "$action" in
  dispatch)
    tmp="$(mktemp)"
    jq '
      .tasks |= map(
        if .state == "todo" then .state = "in_progress" else . end
      )
    ' "$TASK_REGISTRY_FILE" >"$tmp"
    mv "$tmp" "$TASK_REGISTRY_FILE"
    ;;
  advance)
    tmp="$(mktemp)"
    jq '
      .tasks |= map(
        if .state == "in_progress" then .state = "merged" else . end
      )
    ' "$TASK_REGISTRY_FILE" >"$tmp"
    mv "$tmp" "$TASK_REGISTRY_FILE"
    ;;
  sync|reconcile|cleanup) ;;
  *) ;;
esac
"#,
        );

        let first = run_once(&root, &script)?;
        let second = run_once(&root, &script)?;
        assert_eq!(first, second, "same inputs must yield same final state");

        std::fs::remove_dir_all(&root).ok();
        Ok(())
    }

    #[test]
    fn test_coordinator_parallel_dispatch_behavior() -> macc_core::Result<()> {
        let root =
            std::env::temp_dir().join(format!("macc_cli_parallel_dispatch_{}", uuid_v4_like()));
        std::fs::create_dir_all(&root).unwrap();
        let registry = root.join(COORDINATOR_TASK_REGISTRY_REL_PATH);
        std::fs::create_dir_all(registry.parent().expect("registry parent")).unwrap();
        fs::write(
            &registry,
            r#"{
  "schema_version": 1,
  "tasks": [
    {"id":"T1","state":"todo","dependencies":[],"exclusive_resources":[]},
    {"id":"T2","state":"todo","dependencies":[],"exclusive_resources":[]},
    {"id":"T3","state":"todo","dependencies":[],"exclusive_resources":[]}
  ],
  "resource_locks": {},
  "state_mapping": {}
}"#,
        )
        .unwrap();

        let script = root.join("fake-parallel-dispatch.sh");
        write_executable_script(
            &script,
            r#"#!/usr/bin/env bash
set -euo pipefail
action="${1:-dispatch}"
if [[ "$action" == "dispatch" ]]; then
  tmp="$(mktemp)"
  jq '
    .tasks |= (
      reduce .[] as $task ({count: 0, out: []};
        if ($task.state == "todo" and .count < 2) then
          {count: (.count + 1), out: (.out + [($task + {state: "in_progress"})])}
        else
          {count: .count, out: (.out + [$task])}
        end
      ) | .out
    )
  ' "$TASK_REGISTRY_FILE" >"$tmp"
  mv "$tmp" "$TASK_REGISTRY_FILE"
fi
"#,
        );

        let canonical = macc_core::config::CanonicalConfig::default();
        let env_cfg = CoordinatorEnvConfig {
            ..Default::default()
        };

        run_coordinator_command(&root, &script, "dispatch", &[], &canonical, None, &env_cfg)?;

        let value: serde_json::Value =
            serde_json::from_str(&fs::read_to_string(&registry).unwrap()).unwrap();
        let active = value["tasks"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|t| t["state"].as_str() == Some("in_progress"))
            .count();
        let todo = value["tasks"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|t| t["state"].as_str() == Some("todo"))
            .count();
        assert_eq!(active, 2, "dispatch should activate two tasks in parallel");
        assert_eq!(todo, 1, "one task should remain todo");

        std::fs::remove_dir_all(&root).ok();
        Ok(())
    }

    #[test]
    fn test_coordinator_retry_phase_behavior() -> macc_core::Result<()> {
        let root = std::env::temp_dir().join(format!("macc_cli_retry_phase_{}", uuid_v4_like()));
        std::fs::create_dir_all(&root).unwrap();
        let registry = root.join(COORDINATOR_TASK_REGISTRY_REL_PATH);
        std::fs::create_dir_all(registry.parent().expect("registry parent")).unwrap();
        fs::write(
            &registry,
            r#"{
  "schema_version": 1,
  "tasks": [
    {"id":"TASK-R","state":"blocked","dependencies":[],"exclusive_resources":[]}
  ],
  "resource_locks": {},
  "state_mapping": {}
}"#,
        )
        .unwrap();

        let script = root.join("fake-retry-phase.sh");
        write_executable_script(
            &script,
            r#"#!/usr/bin/env bash
set -euo pipefail
action="${1:-dispatch}"
if [[ "$action" == "retry-phase" ]]; then
  shift
  task=""
  phase=""
  while [[ $# -gt 0 ]]; do
    case "$1" in
      --retry-task) task="$2"; shift 2 ;;
      --retry-phase) phase="$2"; shift 2 ;;
      *) shift ;;
    esac
  done
  [[ "$task" == "TASK-R" ]] || exit 2
  [[ "$phase" == "fix" ]] || exit 3
  tmp="$(mktemp)"
  jq '.tasks |= map(if .id=="TASK-R" then .state="queued" else . end)' "$TASK_REGISTRY_FILE" >"$tmp"
  mv "$tmp" "$TASK_REGISTRY_FILE"
fi
"#,
        );

        let canonical = macc_core::config::CanonicalConfig::default();
        let env_cfg = CoordinatorEnvConfig {
            ..Default::default()
        };

        run_coordinator_command(
            &root,
            &script,
            "retry-phase",
            &[
                "--retry-task".to_string(),
                "TASK-R".to_string(),
                "--retry-phase".to_string(),
                "fix".to_string(),
            ],
            &canonical,
            None,
            &env_cfg,
        )?;

        let value: serde_json::Value =
            serde_json::from_str(&fs::read_to_string(&registry).unwrap()).unwrap();
        assert_eq!(
            value["tasks"][0]["state"].as_str(),
            Some("queued"),
            "retry-phase fix should update blocked task to queued in this test harness"
        );

        std::fs::remove_dir_all(&root).ok();
        Ok(())
    }

    #[test]
    fn test_coordinator_stop_removes_worktrees_and_branches() -> macc_core::Result<()> {
        let root = std::env::temp_dir().join(format!("macc_cli_coord_stop_{}", uuid_v4_like()));
        std::fs::create_dir_all(&root).unwrap();
        let ids = fixture_ids();
        std::fs::write(root.join("README.md"), "seed\n").unwrap();
        run_git_ok(&root, &["init"]);
        run_git_ok(&root, &["config", "user.email", "macc-tests@example.com"]);
        run_git_ok(&root, &["config", "user.name", "macc-tests"]);
        run_git_ok(&root, &["add", "README.md"]);
        run_git_ok(&root, &["commit", "-m", "seed"]);

        run_with_engine(
            Cli {
                cwd: root.to_string_lossy().into(),
                verbose: false,
                quiet: false,
                offline: false,
                web_port: None,
                command: Some(Commands::Init {
                    force: false,
                    wizard: false,
                    profile: None,
                    fresh: false,
                    restore: None,
                    no_restore_prompt: true,
                    apply: false,
}),
            },
            fixture_engine(&ids),
        )?;

        // Prepare required coordinator inputs for reconcile/cleanup during stop.
        std::fs::write(
            root.join("prd.json"),
            r#"{
  "lot":"Test",
  "version":"1.0",
  "generated_at":"2026-01-01",
  "timezone":"UTC",
  "priority_mapping":{},
  "tasks":[]
}"#,
        )
        .unwrap();
        let stop_registry = root.join(COORDINATOR_TASK_REGISTRY_REL_PATH);
        std::fs::create_dir_all(stop_registry.parent().expect("registry parent")).unwrap();
        std::fs::write(
            stop_registry,
            r#"{
  "schema_version":1,
  "tasks":[],
  "resource_locks":{},
  "state_mapping":{}
}"#,
        )
        .unwrap();

        let wt_path = root.join(".macc/worktree/stop-test");
        std::fs::create_dir_all(root.join(".macc/worktree")).unwrap();
        run_git_ok(
            &root,
            &[
                "worktree",
                "add",
                "-b",
                "ai/stop-test",
                wt_path.to_string_lossy().as_ref(),
                "HEAD",
            ],
        );

        run_with_engine(
            Cli {
                cwd: root.to_string_lossy().into(),
                verbose: false,
                quiet: false,
                offline: false,
                web_port: None,
                command: Some(Commands::Coordinator {
                    command_name: "stop".to_string(),
                    as_client: None,
                    no_tui: true,
                    supervisor: false,
                    drain: false,
                    graceful: true,
                    force: false,
                    remove_worktrees: true,
                    cleanup_worktrees: false,
                    remove_branches: true,
                    prd: None,
                    coordinator_tool: None,
                    reference_branch: None,
                    tool_priority: None,
                    max_parallel_per_tool_json: None,
                    preset: None,
                    tool_specializations_json: None,
                    max_dispatch: None,
                    max_parallel: None,
                    timeout_seconds: None,
                    phase_runner_max_attempts: None,
                    log_flush_lines: None,
                    log_flush_ms: None,
                    mirror_json_debounce_ms: None,
                    stale_claimed_seconds: None,
                    stale_in_progress_seconds: None,
                    stale_changes_requested_seconds: None,
                    stale_action: None,
                    storage_mode: None,
                    merge_ai_fix: None,
                    merge_job_timeout_seconds: None,
                    merge_hook_timeout_seconds: None,
                    ghost_heartbeat_grace_seconds: None,
                    dispatch_cooldown_seconds: None,
                    json_compat: None,
                    legacy_json_fallback: None,
                    cutover_gate_window_events: None,
                    cutover_gate_max_blocked_ratio: None,
                    cutover_gate_max_stale_ratio: None,
                    error_code_retry_list: None,
                    error_code_retry_max: None,
                    force_kill_grace_seconds: None,
                    max_review_cycles: None,
                    extra_args: Vec::new(),
                }),
            },
            fixture_engine(&ids),
        )?;

        assert!(
            !wt_path.exists(),
            "worktree should be removed by coordinator stop"
        );

        assert!(
            !macc_core::git::rev_parse_verify(&root, "ai/stop-test").unwrap_or(false),
            "branch should be deleted by coordinator stop --remove-branches"
        );

        std::fs::remove_dir_all(&root).ok();
        Ok(())
    }

    #[test]
    fn test_apply_with_test_adapter() -> macc_core::Result<()> {
        let temp_base = std::env::temp_dir().join(format!("macc_apply_test_{}", uuid_v4_like()));
        std::fs::create_dir_all(&temp_base).unwrap();
        let ids = fixture_ids();
        let tool_one = ids[0].clone();

        // 1. Init
        run_with_engine(
            Cli {
                cwd: temp_base.to_string_lossy().into(),
                verbose: false,
                quiet: false,
                offline: false,
                web_port: None,
                command: Some(Commands::Init {
                    force: false,
                    wizard: false,
                    profile: None,
                    fresh: false,
                    restore: None,
                    no_restore_prompt: true,
                    apply: false,
}),
            },
            fixture_engine(&ids),
        )?;

        // 2. Apply with first tool
        run_with_engine(
            Cli {
                cwd: temp_base.to_string_lossy().into(),
                verbose: false,
                quiet: false,
                offline: false,
                web_port: None,
                command: Some(Commands::Apply {
                    tools: Some(tool_one.clone()),
                    dry_run: false,
                    allow_user_scope: false,
                    json: false,
                    explain: false,
                    locked: false,
                }),
            },
            fixture_engine(&ids),
        )?;

        // 3. Verify files created
        let generated_txt = temp_base.join(format!("{}-output.txt", tool_one));

        assert!(generated_txt.exists(), "expected output.txt should exist");

        let txt_content = std::fs::read_to_string(generated_txt).unwrap();
        assert!(txt_content.contains("fixture content for"));

        // Cleanup
        std::fs::remove_dir_all(&temp_base).ok();
        Ok(())
    }

    #[test]
    fn test_context_requires_prior_apply() -> macc_core::Result<()> {
        let temp_base =
            std::env::temp_dir().join(format!("macc_context_gate_test_{}", uuid_v4_like()));
        std::fs::create_dir_all(&temp_base).unwrap();

        run_with_engine(
            Cli {
                cwd: temp_base.to_string_lossy().into(),
                verbose: false,
                quiet: false,
                offline: false,
                web_port: None,
                command: Some(Commands::Init {
                    force: false,
                    wizard: false,
                    profile: None,
                    fresh: false,
                    restore: None,
                    no_restore_prompt: true,
                    apply: false,
}),
            },
            TestEngine::with_fixtures(),
        )?;

        let err = run_with_engine(
            Cli {
                cwd: temp_base.to_string_lossy().into(),
                verbose: false,
                quiet: false,
                offline: false,
                web_port: None,
                command: Some(Commands::Context {
                    tool: None,
                    from_files: Vec::new(),
                    dry_run: true,
                    print_prompt: false,
                }),
            },
            TestEngine::with_fixtures(),
        )
        .expect_err("context should require at least one successful apply");

        let msg = err.to_string();
        assert!(
            msg.contains("at least one successful 'macc apply'"),
            "unexpected error message: {}",
            msg
        );

        std::fs::remove_dir_all(&temp_base).ok();
        Ok(())
    }

    #[test]
    fn test_catalog_skills_workflow() -> macc_core::Result<()> {
        let temp_base =
            std::env::temp_dir().join(format!("macc_catalog_cli_test_{}", uuid_v4_like()));
        std::fs::create_dir_all(&temp_base).unwrap();

        // 1. Init
        run_with_engine(
            Cli {
                cwd: temp_base.to_string_lossy().into(),
                verbose: false,
                quiet: false,
                offline: false,
                web_port: None,
                command: Some(Commands::Init {
                    force: false,
                    wizard: false,
                    profile: None,
                    fresh: false,
                    restore: None,
                    no_restore_prompt: true,
                    apply: false,
}),
            },
            TestEngine::with_fixtures(),
        )?;

        // 2. Add skill
        run_with_engine(
            Cli {
                cwd: temp_base.to_string_lossy().into(),
                verbose: false,
                quiet: false,
                offline: false,
                web_port: None,
                command: Some(Commands::Catalog {
                    catalog_command: CatalogCommands::Skills {
                        skills_command: CatalogSubCommands::Add {
                            id: "test-skill".into(),
                            name: "Test Skill".into(),
                            description: "A test skill".into(),
                            tags: Some("tag1,tag2".into()),
                            subpath: "path".into(),
                            kind: "git".into(),
                            url: "https://github.com/test/test.git".into(),
                            reference: "main".into(),
                            checksum: None,
                        },
                    },
                }),
            },
            TestEngine::with_fixtures(),
        )?;

        let catalog_path = macc_core::ProjectPaths::from_root(&temp_base).skills_catalog_path();
        assert!(catalog_path.exists());

        // 3. List skills (mostly for coverage and ensuring no crash)
        run_with_engine(
            Cli {
                cwd: temp_base.to_string_lossy().into(),
                verbose: false,
                quiet: false,
                offline: false,
                web_port: None,
                command: Some(Commands::Catalog {
                    catalog_command: CatalogCommands::Skills {
                        skills_command: CatalogSubCommands::List,
                    },
                }),
            },
            TestEngine::with_fixtures(),
        )?;

        // 4. Search skill
        run_with_engine(
            Cli {
                cwd: temp_base.to_string_lossy().into(),
                verbose: false,
                quiet: false,
                offline: false,
                web_port: None,
                command: Some(Commands::Catalog {
                    catalog_command: CatalogCommands::Skills {
                        skills_command: CatalogSubCommands::Search {
                            query: "test".into(),
                        },
                    },
                }),
            },
            TestEngine::with_fixtures(),
        )?;

        // 5. Remove skill
        run_with_engine(
            Cli {
                cwd: temp_base.to_string_lossy().into(),
                verbose: false,
                quiet: false,
                offline: false,
                web_port: None,
                command: Some(Commands::Catalog {
                    catalog_command: CatalogCommands::Skills {
                        skills_command: CatalogSubCommands::Remove {
                            id: "test-skill".into(),
                        },
                    },
                }),
            },
            TestEngine::with_fixtures(),
        )?;

        let catalog = SkillsCatalog::load(&catalog_path)?;
        assert_eq!(catalog.entries.len(), 0);

        // Cleanup
        fs::remove_dir_all(&temp_base).ok();
        Ok(())
    }

    #[test]
    fn test_catalog_mcp_workflow() -> macc_core::Result<()> {
        let temp_base = std::env::temp_dir().join(format!("macc_mcp_cli_test_{}", uuid_v4_like()));
        std::fs::create_dir_all(&temp_base).unwrap();

        // 1. Init
        run_with_engine(
            Cli {
                cwd: temp_base.to_string_lossy().into(),
                verbose: false,
                quiet: false,
                offline: false,
                web_port: None,
                command: Some(Commands::Init {
                    force: false,
                    wizard: false,
                    profile: None,
                    fresh: false,
                    restore: None,
                    no_restore_prompt: true,
                    apply: false,
}),
            },
            TestEngine::with_fixtures(),
        )?;

        // 2. Add MCP
        run_with_engine(
            Cli {
                cwd: temp_base.to_string_lossy().into(),
                verbose: false,
                quiet: false,
                offline: false,
                web_port: None,
                command: Some(Commands::Catalog {
                    catalog_command: CatalogCommands::Mcp {
                        mcp_command: CatalogSubCommands::Add {
                            id: "test-mcp".into(),
                            name: "Test MCP".into(),
                            description: "A test MCP".into(),
                            tags: Some("mcp".into()),
                            subpath: "".into(),
                            kind: "http".into(),
                            url: "https://example.com/mcp.zip".into(),
                            reference: "".into(),
                            checksum: Some("sha256:123".into()),
                        },
                    },
                }),
            },
            TestEngine::with_fixtures(),
        )?;

        let catalog_path = macc_core::ProjectPaths::from_root(&temp_base).mcp_catalog_path();
        assert!(catalog_path.exists());

        // 3. List MCP
        run_with_engine(
            Cli {
                cwd: temp_base.to_string_lossy().into(),
                verbose: false,
                quiet: false,
                offline: false,
                web_port: None,
                command: Some(Commands::Catalog {
                    catalog_command: CatalogCommands::Mcp {
                        mcp_command: CatalogSubCommands::List,
                    },
                }),
            },
            TestEngine::with_fixtures(),
        )?;

        // 4. Search MCP
        run_with_engine(
            Cli {
                cwd: temp_base.to_string_lossy().into(),
                verbose: false,
                quiet: false,
                offline: false,
                web_port: None,
                command: Some(Commands::Catalog {
                    catalog_command: CatalogCommands::Mcp {
                        mcp_command: CatalogSubCommands::Search {
                            query: "mcp".into(),
                        },
                    },
                }),
            },
            TestEngine::with_fixtures(),
        )?;

        // 5. Remove MCP
        run_with_engine(
            Cli {
                cwd: temp_base.to_string_lossy().into(),
                verbose: false,
                quiet: false,
                offline: false,
                web_port: None,
                command: Some(Commands::Catalog {
                    catalog_command: CatalogCommands::Mcp {
                        mcp_command: CatalogSubCommands::Remove {
                            id: "test-mcp".into(),
                        },
                    },
                }),
            },
            TestEngine::with_fixtures(),
        )?;

        let catalog = McpCatalog::load(&catalog_path)?;
        assert_eq!(catalog.entries.len(), 0);

        // Cleanup
        std::fs::remove_dir_all(&temp_base).ok();
        Ok(())
    }

    #[test]
    fn test_install_skill_cli() -> macc_core::Result<()> {
        let temp_base =
            std::env::temp_dir().join(format!("macc_install_skill_test_{}", uuid_v4_like()));
        std::fs::create_dir_all(&temp_base).unwrap();
        let ids = fixture_ids();
        let tool_one = ids[0].clone();

        // 1. Init
        run_with_engine(
            Cli {
                cwd: temp_base.to_string_lossy().into(),
                verbose: false,
                quiet: false,
                offline: false,
                web_port: None,
                command: Some(Commands::Init {
                    force: false,
                    wizard: false,
                    profile: None,
                    fresh: false,
                    restore: None,
                    no_restore_prompt: true,
                    apply: false,
}),
            },
            fixture_engine(&ids),
        )?;

        // 2. Add skill to catalog
        let skill_source_dir = temp_base.join("remote_skill");
        std::fs::create_dir_all(&skill_source_dir).unwrap();
        let manifest = format!(
            r#"{{
  "type": "skill",
  "id": "remote-skill",
  "version": "0.1.0",
  "targets": {{
    "{tool_one}": [
      {{ "src": "SKILL.md", "dest": ".{tool_one}/skills/remote-skill/SKILL.md" }}
    ]
  }}
}}
"#
        );
        std::fs::write(skill_source_dir.join("macc.package.json"), manifest).unwrap();
        std::fs::write(skill_source_dir.join("SKILL.md"), "remote content").unwrap();

        run_git_ok(&skill_source_dir, &["init", "-b", "main"]);
        run_git_ok(
            &skill_source_dir,
            &["config", "user.email", "test@example.com"],
        );
        run_git_ok(&skill_source_dir, &["config", "user.name", "Test"]);
        run_git_ok(&skill_source_dir, &["add", "."]);
        run_git_ok(&skill_source_dir, &["commit", "-m", "initial"]);

        run_with_engine(
            Cli {
                cwd: temp_base.to_string_lossy().into(),
                verbose: false,
                quiet: false,
                offline: false,
                web_port: None,
                command: Some(Commands::Catalog {
                    catalog_command: CatalogCommands::Skills {
                        skills_command: CatalogSubCommands::Add {
                            id: "remote-skill".into(),
                            name: "Remote Skill".into(),
                            description: "desc".into(),
                            tags: None,
                            subpath: "".into(),
                            kind: "git".into(),
                            url: skill_source_dir.to_string_lossy().into(),
                            reference: "main".into(),
                            checksum: None,
                        },
                    },
                }),
            },
            fixture_engine(&ids),
        )?;

        // 3. Install skill
        run_with_engine(
            Cli {
                cwd: temp_base.to_string_lossy().into(),
                verbose: false,
                quiet: false,
                offline: false,
                web_port: None,
                command: Some(Commands::Install {
                    install_command: InstallCommands::Skill {
                        tool: tool_one.clone(),
                        id: "remote-skill".into(),
                    },
                }),
            },
            fixture_engine(&ids),
        )?;

        // 4. Verify installation
        let installed_file = temp_base.join(format!(".{}/skills/remote-skill/SKILL.md", tool_one));
        assert!(installed_file.exists());
        assert_eq!(
            std::fs::read_to_string(installed_file).unwrap(),
            "remote content"
        );

        // Cleanup
        std::fs::remove_dir_all(&temp_base).ok();
        Ok(())
    }

    #[test]
    fn test_install_mcp_cli() -> macc_core::Result<()> {
        let temp_base =
            std::env::temp_dir().join(format!("macc_install_mcp_test_{}", uuid_v4_like()));
        std::fs::create_dir_all(&temp_base).unwrap();

        // 1. Init
        run_with_engine(
            Cli {
                cwd: temp_base.to_string_lossy().into(),
                verbose: false,
                quiet: false,
                offline: false,
                web_port: None,
                command: Some(Commands::Init {
                    force: false,
                    wizard: false,
                    profile: None,
                    fresh: false,
                    restore: None,
                    no_restore_prompt: true,
                    apply: false,
}),
            },
            TestEngine::with_fixtures(),
        )?;

        // 2. Prepare MCP source (git repo)
        let mcp_source_dir = temp_base.join("remote_mcp");
        std::fs::create_dir_all(&mcp_source_dir).unwrap();
        let manifest = serde_json::json!({
            "type": "mcp",
            "id": "remote-mcp",
            "version": "1.0.0",
            "mcp": {
                "server": {
                    "command": "node",
                    "args": ["index.js"]
                }
            },
            "merge_target": "mcpServers.remote-mcp"
        });
        std::fs::write(
            mcp_source_dir.join("macc.package.json"),
            serde_json::to_string(&manifest).unwrap(),
        )
        .unwrap();

        run_git_ok(&mcp_source_dir, &["init", "-b", "main"]);
        run_git_ok(
            &mcp_source_dir,
            &["config", "user.email", "test@example.com"],
        );
        run_git_ok(&mcp_source_dir, &["config", "user.name", "Test"]);
        run_git_ok(&mcp_source_dir, &["add", "."]);
        run_git_ok(&mcp_source_dir, &["commit", "-m", "initial"]);

        // 3. Add to catalog
        run_with_engine(
            Cli {
                cwd: temp_base.to_string_lossy().into(),
                verbose: false,
                quiet: false,
                offline: false,
                web_port: None,
                command: Some(Commands::Catalog {
                    catalog_command: CatalogCommands::Mcp {
                        mcp_command: CatalogSubCommands::Add {
                            id: "remote-mcp".into(),
                            name: "Remote MCP".into(),
                            description: "desc".into(),
                            tags: None,
                            subpath: "".into(),
                            kind: "git".into(),
                            url: mcp_source_dir.to_string_lossy().into(),
                            reference: "main".into(),
                            checksum: None,
                        },
                    },
                }),
            },
            TestEngine::with_fixtures(),
        )?;

        // 4. Install MCP
        run_with_engine(
            Cli {
                cwd: temp_base.to_string_lossy().into(),
                verbose: false,
                quiet: false,
                offline: false,
                web_port: None,
                command: Some(Commands::Install {
                    install_command: InstallCommands::Mcp {
                        id: "remote-mcp".into(),
                    },
                }),
            },
            TestEngine::with_fixtures(),
        )?;

        // 5. Verify .mcp.json update
        let mcp_json = temp_base.join(".mcp.json");
        assert!(mcp_json.exists());
        let content = std::fs::read_to_string(mcp_json).unwrap();
        let json: serde_json::Value = serde_json::from_str(&content).unwrap();
        assert_eq!(
            json["mcpServers"]["remote-mcp"]["command"],
            serde_json::Value::String("node".into())
        );

        // Cleanup
        std::fs::remove_dir_all(&temp_base).ok();
        Ok(())
    }

    #[test]
    fn test_catalog_import_url() -> macc_core::Result<()> {
        let temp_base =
            std::env::temp_dir().join(format!("macc_catalog_import_test_{}", uuid_v4_like()));
        std::fs::create_dir_all(&temp_base).unwrap();

        // 1. Init
        run_with_engine(
            Cli {
                cwd: temp_base.to_string_lossy().into(),
                verbose: false,
                quiet: false,
                offline: false,
                web_port: None,
                command: Some(Commands::Init {
                    force: false,
                    wizard: false,
                    profile: None,
                    fresh: false,
                    restore: None,
                    no_restore_prompt: true,
                    apply: false,
}),
            },
            TestEngine::with_fixtures(),
        )?;

        // 2. Import Skill from GitHub tree URL
        run_with_engine(
            Cli {
                cwd: temp_base.to_string_lossy().into(),
                verbose: false,
                quiet: false,
                offline: false,
                web_port: None,
                command: Some(Commands::Catalog {
                    catalog_command: CatalogCommands::ImportUrl {
                        kind: "skill".into(),
                        id: "imported-skill".into(),
                        url: "https://github.com/org/repo/tree/v1.0/path/to/skill".into(),
                        name: Some("Imported Skill".into()),
                        description: "Imported from URL".into(),
                        tags: Some("import".into()),
                    },
                }),
            },
            TestEngine::with_fixtures(),
        )?;

        // Verify Skill Catalog
        let skills_catalog_path =
            macc_core::ProjectPaths::from_root(&temp_base).skills_catalog_path();
        let skills_catalog = SkillsCatalog::load(&skills_catalog_path)?;
        assert_eq!(skills_catalog.entries.len(), 1);
        let entry = &skills_catalog.entries[0];
        assert_eq!(entry.id, "imported-skill");
        assert_eq!(entry.name, "Imported Skill");
        assert_eq!(entry.selector.subpath, "path/to/skill");
        assert_eq!(entry.source.url, "https://github.com/org/repo.git");
        assert_eq!(entry.source.reference, "v1.0");

        // 3. Import MCP from GitHub root URL (implicit main/empty subpath)
        run_with_engine(
            Cli {
                cwd: temp_base.to_string_lossy().into(),
                verbose: false,
                quiet: false,
                offline: false,
                web_port: None,
                command: Some(Commands::Catalog {
                    catalog_command: CatalogCommands::ImportUrl {
                        kind: "mcp".into(),
                        id: "imported-mcp".into(),
                        url: "https://github.com/org/mcp-repo".into(),
                        name: None,
                        description: "Imported MCP".into(),
                        tags: None,
                    },
                }),
            },
            TestEngine::with_fixtures(),
        )?;

        // Verify MCP Catalog
        let mcp_catalog_path = macc_core::ProjectPaths::from_root(&temp_base).mcp_catalog_path();
        let mcp_catalog = McpCatalog::load(&mcp_catalog_path)?;
        assert_eq!(mcp_catalog.entries.len(), 1);
        let entry = &mcp_catalog.entries[0];
        assert_eq!(entry.id, "imported-mcp");
        assert_eq!(entry.name, "imported-mcp"); // Default to ID
        assert_eq!(entry.selector.subpath, "");
        assert_eq!(entry.source.url, "https://github.com/org/mcp-repo.git");
        assert_eq!(entry.source.reference, "");

        // Cleanup
        std::fs::remove_dir_all(&temp_base).ok();
        Ok(())
    }

    #[test]
    fn test_search_remote_cli() -> macc_core::Result<()> {
        use std::io::{BufRead, BufReader, Write};
        use std::thread;

        // Mock server
        let (listener, port) = match bind_loopback() {
            Some(v) => v,
            None => return Ok(()),
        };
        let server_url = format!("http://127.0.0.1:{}", port);

        thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let mut reader = BufReader::new(&mut stream);
            let mut line = String::new();
            reader.read_line(&mut line).unwrap();
            // Consume headers
            while line.trim() != "" {
                line.clear();
                reader.read_line(&mut line).unwrap();
            }

            // Return mock response
            let response_body = r#"{
                "items": [
                    {
                        "id": "remote-skill-1",
                        "name": "Remote Skill 1",
                        "description": "Desc",
                        "tags": ["remote"],
                        "selector": {"subpath": ""},
                        "source": {
                            "kind": "git",
                            "url": "https://example.com/repo.git",
                            "ref": "main",
                            "checksum": null
                        }
                    }
                ]
            }"#;
            let response = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n{}",
                response_body.len(),
                response_body
            );
            stream.write_all(response.as_bytes()).unwrap();
        });

        let temp_base =
            std::env::temp_dir().join(format!("macc_search_remote_test_{}", uuid_v4_like()));
        std::fs::create_dir_all(&temp_base).unwrap();

        // 1. Init
        run_with_engine(
            Cli {
                cwd: temp_base.to_string_lossy().into(),
                verbose: false,
                quiet: false,
                offline: false,
                web_port: None,
                command: Some(Commands::Init {
                    force: false,
                    wizard: false,
                    profile: None,
                    fresh: false,
                    restore: None,
                    no_restore_prompt: true,
                    apply: false,
}),
            },
            TestEngine::with_fixtures(),
        )?;

        // 2. Search remote and add
        run_with_engine(
            Cli {
                cwd: temp_base.to_string_lossy().into(),
                verbose: false,
                quiet: false,
                offline: false,
                web_port: None,
                command: Some(Commands::Catalog {
                    catalog_command: CatalogCommands::SearchRemote {
                        api: server_url,
                        kind: "skill".into(),
                        q: "test".into(),
                        add: true,
                        add_ids: None,
                    },
                }),
            },
            TestEngine::with_fixtures(),
        )?;

        // 3. Verify it was added to catalog
        let catalog_path = macc_core::ProjectPaths::from_root(&temp_base).skills_catalog_path();
        let catalog = SkillsCatalog::load(&catalog_path)?;
        assert_eq!(catalog.entries.len(), 1);
        assert_eq!(catalog.entries[0].id, "remote-skill-1");

        std::fs::remove_dir_all(&temp_base).ok();
        Ok(())
    }

    #[test]
    fn test_install_skill_multi_zip_cli() -> macc_core::Result<()> {
        use std::io::{BufRead, BufReader, Write};
        use std::thread;

        let ids = fixture_ids();
        let tool_one = ids[0].clone();

        // 1. Prepare a zip file containing two skills
        let archive_bytes = {
            let mut buf = Vec::new();
            {
                let mut zip = zip::ZipWriter::new(std::io::Cursor::new(&mut buf));
                let options = zip::write::SimpleFileOptions::default();

                let manifest_a = format!(
                    r#"{{
  "type": "skill",
  "id": "skill-a",
  "version": "0.1.0",
  "targets": {{
    "{tool_one}": [
      {{ "src": "SKILL.md", "dest": ".{tool_one}/skills/skill-a/SKILL.md" }}
    ]
  }}
}}
"#
                );
                zip.start_file("skills/a/macc.package.json", options)
                    .unwrap();
                zip.write_all(manifest_a.as_bytes()).unwrap();
                zip.start_file("skills/a/SKILL.md", options).unwrap();
                zip.write_all(b"content a").unwrap();

                let manifest_b = format!(
                    r#"{{
  "type": "skill",
  "id": "skill-b",
  "version": "0.1.0",
  "targets": {{
    "{tool_one}": [
      {{ "src": "SKILL.md", "dest": ".{tool_one}/skills/skill-b/SKILL.md" }}
    ]
  }}
}}
"#
                );
                zip.start_file("skills/b/macc.package.json", options)
                    .unwrap();
                zip.write_all(manifest_b.as_bytes()).unwrap();
                zip.start_file("skills/b/SKILL.md", options).unwrap();
                zip.write_all(b"content b").unwrap();

                zip.finish().unwrap();
            }
            buf
        };

        // 2. Mock server to serve this zip
        let (listener, port) = match bind_loopback() {
            Some(v) => v,
            None => return Ok(()),
        };
        let server_url = format!("http://127.0.0.1:{}/skills.zip", port);

        thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let mut reader = BufReader::new(&mut stream);
            let mut line = String::new();
            reader.read_line(&mut line).unwrap();
            // Consume headers
            while line.trim() != "" {
                line.clear();
                reader.read_line(&mut line).unwrap();
            }

            let response = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: application/zip\r\nContent-Length: {}\r\n\r\n",
                archive_bytes.len()
            );
            stream.write_all(response.as_bytes()).unwrap();
            stream.write_all(&archive_bytes).unwrap();
        });

        let temp_base =
            std::env::temp_dir().join(format!("macc_install_multi_zip_test_{}", uuid_v4_like()));
        std::fs::create_dir_all(&temp_base).unwrap();

        // 3. Init
        run_with_engine(
            Cli {
                cwd: temp_base.to_string_lossy().into(),
                verbose: false,
                quiet: false,
                offline: false,
                web_port: None,
                command: Some(Commands::Init {
                    force: false,
                    wizard: false,
                    profile: None,
                    fresh: false,
                    restore: None,
                    no_restore_prompt: true,
                    apply: false,
}),
            },
            fixture_engine(&ids),
        )?;

        // 4. Add skill 'a' to catalog pointing to the zip with subpath 'skills/a'
        run_with_engine(
            Cli {
                cwd: temp_base.to_string_lossy().into(),
                verbose: false,
                quiet: false,
                offline: false,
                web_port: None,
                command: Some(Commands::Catalog {
                    catalog_command: CatalogCommands::Skills {
                        skills_command: CatalogSubCommands::Add {
                            id: "skill-a".into(),
                            name: "Skill A".into(),
                            description: "desc a".into(),
                            tags: None,
                            subpath: "skills/a".into(),
                            kind: "http".into(),
                            url: server_url,
                            reference: "".into(),
                            checksum: None,
                        },
                    },
                }),
            },
            fixture_engine(&ids),
        )?;

        // 5. Install skill 'a'
        run_with_engine(
            Cli {
                cwd: temp_base.to_string_lossy().into(),
                verbose: false,
                quiet: false,
                offline: false,
                web_port: None,
                command: Some(Commands::Install {
                    install_command: InstallCommands::Skill {
                        tool: tool_one.clone(),
                        id: "skill-a".into(),
                    },
                }),
            },
            fixture_engine(&ids),
        )?;

        // 6. Verify skill 'a' exists and 'b' does not
        let skill_a_file = temp_base.join(format!(".{}/skills/skill-a/SKILL.md", tool_one));
        assert!(skill_a_file.exists(), "Skill A should be installed");
        assert_eq!(std::fs::read_to_string(skill_a_file).unwrap(), "content a");

        let skill_b_dir = temp_base.join(format!(".{}/skills/skill-b", tool_one));
        assert!(!skill_b_dir.exists(), "Skill B should NOT be installed");

        // Also ensure that the parent 'skills/a' subpath didn't leak into the destination path
        assert!(!temp_base
            .join(format!(".{}/skills/skill-a/skills", tool_one))
            .exists());

        std::fs::remove_dir_all(&temp_base).ok();
        Ok(())
    }

    #[test]
    fn test_install_skill_multi_git_cli() -> macc_core::Result<()> {
        let _guard = env_test_lock();
        let temp_base =
            std::env::temp_dir().join(format!("macc_install_multi_git_test_{}", uuid_v4_like()));
        std::fs::create_dir_all(&temp_base).unwrap();
        let temp_home = temp_base.join("home");
        std::fs::create_dir_all(&temp_home).unwrap();
        let old_home = std::env::var("HOME").ok();
        std::env::set_var("HOME", &temp_home);
        let ids = fixture_ids();
        let tool_one = ids[0].clone();

        let repo_path = temp_base.join("remote_repo");
        std::fs::create_dir_all(&repo_path).unwrap();

        // 1. Initialize a local git repo
        let run_git = |args: &[&str], dir: &std::path::Path| run_git_ok(dir, args);

        run_git(&["init"], &repo_path);
        // Set user info for commits
        run_git(&["config", "user.email", "test@example.com"], &repo_path);
        run_git(&["config", "user.name", "Test User"], &repo_path);
        run_git(&["config", "commit.gpgsign", "false"], &repo_path);
        run_git(&["checkout", "-b", "main"], &repo_path);

        let skill_a_dir = repo_path.join("skills/a");
        std::fs::create_dir_all(&skill_a_dir).unwrap();
        let manifest_a = format!(
            r#"{{
  "type": "skill",
  "id": "skill-a",
  "version": "0.1.0",
  "targets": {{
    "{tool_one}": [
      {{ "src": "SKILL.md", "dest": ".{tool_one}/skills/skill-a/SKILL.md" }}
    ]
  }}
}}
"#
        );
        std::fs::write(skill_a_dir.join("macc.package.json"), manifest_a).unwrap();
        std::fs::write(skill_a_dir.join("SKILL.md"), "content a").unwrap();

        let skill_b_dir = repo_path.join("skills/b");
        std::fs::create_dir_all(&skill_b_dir).unwrap();
        let manifest_b = format!(
            r#"{{
  "type": "skill",
  "id": "skill-b",
  "version": "0.1.0",
  "targets": {{
    "{tool_one}": [
      {{ "src": "SKILL.md", "dest": ".{tool_one}/skills/skill-b/SKILL.md" }}
    ]
  }}
}}
"#
        );
        std::fs::write(skill_b_dir.join("macc.package.json"), manifest_b).unwrap();
        std::fs::write(skill_b_dir.join("SKILL.md"), "content b").unwrap();

        run_git(&["add", "."], &repo_path);
        run_git(&["commit", "-m", "initial commit"], &repo_path);

        let repo_url = format!("file://{}", repo_path.to_string_lossy());

        let project_path = temp_base.join("project");
        std::fs::create_dir_all(&project_path).unwrap();

        // 2. Init MACC project
        run_with_engine(
            Cli {
                cwd: project_path.to_string_lossy().into(),
                verbose: false,
                quiet: false,
                offline: false,
                web_port: None,
                command: Some(Commands::Init {
                    force: false,
                    wizard: false,
                    profile: None,
                    fresh: false,
                    restore: None,
                    no_restore_prompt: true,
                    apply: false,
}),
            },
            fixture_engine(&ids),
        )?;

        // 3. Add skills 'a' and 'b' to catalog pointing to the same git repo
        run_with_engine(
            Cli {
                cwd: project_path.to_string_lossy().into(),
                verbose: false,
                quiet: false,
                offline: false,
                web_port: None,
                command: Some(Commands::Catalog {
                    catalog_command: CatalogCommands::Skills {
                        skills_command: CatalogSubCommands::Add {
                            id: "skill-a".into(),
                            name: "Skill A".into(),
                            description: "desc a".into(),
                            tags: None,
                            subpath: "skills/a".into(),
                            kind: "git".into(),
                            url: repo_url.clone(),
                            reference: "main".into(),
                            checksum: None,
                        },
                    },
                }),
            },
            fixture_engine(&ids),
        )?;

        run_with_engine(
            Cli {
                cwd: project_path.to_string_lossy().into(),
                verbose: false,
                quiet: false,
                offline: false,
                web_port: None,
                command: Some(Commands::Catalog {
                    catalog_command: CatalogCommands::Skills {
                        skills_command: CatalogSubCommands::Add {
                            id: "skill-b".into(),
                            name: "Skill B".into(),
                            description: "desc b".into(),
                            tags: None,
                            subpath: "skills/b".into(),
                            kind: "git".into(),
                            url: repo_url.clone(),
                            reference: "main".into(),
                            checksum: None,
                        },
                    },
                }),
            },
            fixture_engine(&ids),
        )?;

        // 4. Install skill 'a'
        run_with_engine(
            Cli {
                cwd: project_path.to_string_lossy().into(),
                verbose: false,
                quiet: false,
                offline: false,
                web_port: None,
                command: Some(Commands::Install {
                    install_command: InstallCommands::Skill {
                        tool: tool_one.clone(),
                        id: "skill-a".into(),
                    },
                }),
            },
            fixture_engine(&ids),
        )?;

        // 5. Verify skill 'a' exists and 'b' does not in the project
        let skill_a_file = project_path.join(format!(".{}/skills/skill-a/SKILL.md", tool_one));
        assert!(skill_a_file.exists(), "Skill A should be installed");
        assert_eq!(std::fs::read_to_string(skill_a_file).unwrap(), "content a");

        let skill_b_dir = project_path.join(format!(".{}/skills/skill-b", tool_one));
        assert!(!skill_b_dir.exists(), "Skill B should NOT be installed");

        // 6. Verify sparse checkout in cache (project cache or shared user cache).
        // Only count cache entries whose git remote URL matches this test's repo_url, so that
        // unrelated entries (e.g. from a concurrent test or a previous run that wasn't fully
        // cleaned up) don't trigger a spurious assertion failure.
        let mut found_cache = false;
        let mut found_sparse_match = false;
        let mut cache_roots = vec![project_path.join(".macc/cache")];
        if let Some(home) = std::env::var_os("HOME") {
            cache_roots.push(std::path::PathBuf::from(home).join(".macc/cache"));
        }
        for cache_dir in cache_roots {
            if let Ok(entries) = std::fs::read_dir(cache_dir) {
                for entry in entries.flatten() {
                    let repo_dir = entry.path().join("repo");
                    if !repo_dir.exists() {
                        continue;
                    }
                    // Verify this entry belongs to our test's remote repo, not some other
                    // concurrent or stale cache entry that happens to have a 'repo' subdir.
                    // Route through the macc_core::git facade to avoid direct git invocation.
                    let remote_url = macc_core::git::run_git_output_mapped(
                        &repo_dir,
                        &["remote", "get-url", "origin"],
                        "get git remote url",
                    )
                    .ok()
                    .filter(|o| o.status.success())
                    .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string());
                    if remote_url.as_deref() != Some(&repo_url) {
                        continue;
                    }
                    found_cache = true;
                    // Look for the cache entry matching this test's sparse checkout.
                    if repo_dir.join("skills/a").exists() {
                        assert!(
                            !repo_dir.join("skills/b").exists(),
                            "skills/b should NOT be materialized in sparse checkout"
                        );
                        found_sparse_match = true;
                    }
                }
            }
        }
        if found_cache {
            assert!(
                found_sparse_match,
                "Expected at least one sparse cache entry with skills/a"
            );
        }

        if let Some(old) = old_home {
            std::env::set_var("HOME", old);
        } else {
            std::env::remove_var("HOME");
        }
        std::fs::remove_dir_all(&temp_base).ok();
        Ok(())
    }

    #[test]
    #[cfg(unix)]
    fn test_install_skill_rejects_symlink_cli() -> macc_core::Result<()> {
        use std::io::{BufRead, BufReader, Write};
        use std::thread;

        let ids = fixture_ids();
        let tool_one = ids[0].clone();

        // 1. Prepare a zip file containing a symlink
        let archive_bytes = {
            let mut buf = Vec::new();
            {
                let mut zip = zip::ZipWriter::new(std::io::Cursor::new(&mut buf));
                let options = zip::write::SimpleFileOptions::default();

                let manifest = format!(
                    r#"{{
  "type": "skill",
  "id": "symlink-skill",
  "version": "0.1.0",
  "targets": {{
    "{tool_one}": [
      {{ "src": "SKILL.md", "dest": ".{tool_one}/skills/symlink-skill/SKILL.md" }}
    ]
  }}
}}
"#
                );
                zip.start_file("macc.package.json", options).unwrap();
                zip.write_all(manifest.as_bytes()).unwrap();
                zip.start_file("SKILL.md", options).unwrap();
                zip.write_all(b"real content").unwrap();

                zip.add_symlink("link.txt", "SKILL.md", options).unwrap();

                zip.finish().unwrap();
            }
            buf
        };

        // 2. Mock server
        let (listener, port) = match bind_loopback() {
            Some(v) => v,
            None => return Ok(()),
        };
        let server_url = format!("http://127.0.0.1:{}/malicious.zip", port);

        thread::spawn(move || {
            if let Ok((mut stream, _)) = listener.accept() {
                let mut reader = BufReader::new(&mut stream);
                let mut line = String::new();
                let _ = reader.read_line(&mut line);
                while line.trim() != "" {
                    line.clear();
                    let _ = reader.read_line(&mut line);
                }

                let response = format!(
                    "HTTP/1.1 200 OK\r\nContent-Type: application/zip\r\nContent-Length: {}\r\n\r\n",
                    archive_bytes.len()
                );
                let _ = stream.write_all(response.as_bytes());
                let _ = stream.write_all(&archive_bytes);
            }
        });

        let temp_base =
            std::env::temp_dir().join(format!("macc_install_symlink_test_{}", uuid_v4_like()));
        std::fs::create_dir_all(&temp_base).unwrap();

        run_with_engine(
            Cli {
                cwd: temp_base.to_string_lossy().into(),
                verbose: false,
                quiet: false,
                offline: false,
                web_port: None,
                command: Some(Commands::Init {
                    force: false,
                    wizard: false,
                    profile: None,
                    fresh: false,
                    restore: None,
                    no_restore_prompt: true,
                    apply: false,
}),
            },
            fixture_engine(&ids),
        )?;

        // Add to catalog
        run_with_engine(
            Cli {
                cwd: temp_base.to_string_lossy().into(),
                verbose: false,
                quiet: false,
                offline: false,
                web_port: None,
                command: Some(Commands::Catalog {
                    catalog_command: CatalogCommands::Skills {
                        skills_command: CatalogSubCommands::Add {
                            id: "malicious".into(),
                            name: "Malicious".into(),
                            description: "desc".into(),
                            tags: None,
                            subpath: "".into(),
                            kind: "http".into(),
                            url: server_url,
                            reference: "".into(),
                            checksum: None,
                        },
                    },
                }),
            },
            fixture_engine(&ids),
        )?;

        // Try install
        let result = run_with_engine(
            Cli {
                cwd: temp_base.to_string_lossy().into(),
                verbose: false,
                quiet: false,
                offline: false,
                web_port: None,
                command: Some(Commands::Install {
                    install_command: InstallCommands::Skill {
                        tool: tool_one,
                        id: "malicious".into(),
                    },
                }),
            },
            fixture_engine(&ids),
        );

        assert!(result.is_err());
        let err_msg = result.unwrap_err().to_string();
        assert!(
            err_msg.contains("Symlinks are not supported"),
            "Error message should mention symlinks: {}",
            err_msg
        );

        std::fs::remove_dir_all(&temp_base).ok();
        Ok(())
    }

    fn uuid_v4_like() -> String {
        use std::time::{SystemTime, UNIX_EPOCH};
        let start = SystemTime::now();
        let since_the_epoch = start
            .duration_since(UNIX_EPOCH)
            .expect("Time went backwards");
        format!("{:?}", since_the_epoch.as_nanos())
    }

    #[test]
    fn test_run_version_command_generic() {
        let cmd = macc_core::tool::ToolInstallCommand {
            command: "bash".to_string(),
            args: vec!["-lc".to_string(), "echo v1.2.3".to_string()],
        };
        assert_eq!(run_version_command(&cmd), Some("1.2.3".to_string()));
    }

    #[test]
    fn test_extract_version_token() {
        assert_eq!(
            extract_version_token("tool version v0.101.0"),
            Some("0.101.0".to_string())
        );
        assert_eq!(
            extract_version_token("my-cli 1.2.3-beta"),
            Some("1.2.3-beta".to_string())
        );
        assert_eq!(extract_version_token("no version here"), None);
    }

    #[test]
    fn test_web_command_parses_host_and_port_flags() {
        let cli = Cli::try_parse_from([
            "macc", "web", "--host", "0.0.0.0", "--port", "8080", "--assets", "embedded",
        ])
        .expect("parse web command");

        match cli.command {
            Some(Commands::Web { host, port, assets }) => {
                assert_eq!(host, "0.0.0.0");
                assert_eq!(port, Some(8080));
                assert_eq!(
                    assets.map(WebAssetsMode::from),
                    Some(WebAssetsMode::Embedded)
                );
            }
            other => panic!("unexpected command: {:?}", other.map(|_| "non-web")),
        }
    }

    #[test]
    fn test_web_command_defaults_host_and_leaves_port_unset() {
        let cli = Cli::try_parse_from(["macc", "web"]).expect("parse web command");

        match cli.command {
            Some(Commands::Web { host, port, assets }) => {
                assert_eq!(host, "127.0.0.1");
                assert_eq!(port, None);
                assert_eq!(assets, None);
            }
            other => panic!("unexpected command: {:?}", other.map(|_| "non-web")),
        }
    }

    #[test]
    fn test_supervisor_start_daemon_command_parsing() {
        let cli = Cli::try_parse_from([
            "macc",
            "supervisor",
            "start",
            "--daemon",
            "--attach",
            "--coordinator-pid",
            "1234",
        ])
        .expect("parse supervisor start command");

        match cli.command {
            Some(Commands::Supervisor { supervisor_command }) => match supervisor_command {
                SupervisorCommands::Start {
                    daemon,
                    attach,
                    coordinator_pid,
                } => {
                    assert!(daemon);
                    assert!(attach);
                    assert_eq!(coordinator_pid, Some(1234));
                }
                _ => panic!("unexpected supervisor subcommand"),
            },
            _ => panic!("unexpected command"),
        }
    }

    #[test]
    fn test_supervisor_start_attach_without_daemon_parsing() {
        let cli = Cli::try_parse_from(["macc", "supervisor", "start", "--attach"])
            .expect("parse supervisor start attach command");

        match cli.command {
            Some(Commands::Supervisor { supervisor_command }) => match supervisor_command {
                SupervisorCommands::Start {
                    daemon,
                    attach,
                    coordinator_pid,
                } => {
                    assert!(!daemon);
                    assert!(attach);
                    assert_eq!(coordinator_pid, None);
                }
                _ => panic!("unexpected supervisor subcommand"),
            },
            _ => panic!("unexpected command"),
        }
    }

    #[test]
    fn test_failure_recovery_commands() -> macc_core::Result<()> {
        let temp_base = std::env::temp_dir().join(format!("macc_failure_test_{}", uuid_v4_like()));
        std::fs::create_dir_all(&temp_base).unwrap();

        // 1. Initialize MACC
        run_with_engine(
            Cli {
                cwd: temp_base.to_string_lossy().into(),
                verbose: false,
                quiet: false,
                offline: false,
                web_port: None,
                command: Some(Commands::Init {
                    force: false,
                    wizard: false,
                    profile: None,
                    fresh: false,
                    restore: None,
                    no_restore_prompt: true,
                    apply: false,
}),
            },
            TestEngine::with_fixtures(),
        )?;

        // Set storage_mode to json so JSON registry path is used
        let config_path = temp_base.join(".macc/macc.yaml");
        let config_str = std::fs::read_to_string(&config_path).unwrap();
        let mut config_val: serde_yaml::Value = serde_yaml::from_str(&config_str)
            .unwrap_or(serde_yaml::Value::Mapping(serde_yaml::Mapping::new()));
        if !config_val.is_mapping() {
            config_val = serde_yaml::Value::Mapping(serde_yaml::Mapping::new());
        }
        let automation = config_val.as_mapping_mut()
            .unwrap()
            .entry(serde_yaml::Value::String("automation".to_string()))
            .or_insert_with(|| serde_yaml::Value::Mapping(serde_yaml::Mapping::new()))
            .as_mapping_mut()
            .unwrap();
        let coordinator = automation
            .entry(serde_yaml::Value::String("coordinator".to_string()))
            .or_insert_with(|| serde_yaml::Value::Mapping(serde_yaml::Mapping::new()))
            .as_mapping_mut()
            .unwrap();
        coordinator.insert(
            serde_yaml::Value::String("storage_mode".to_string()),
            serde_yaml::Value::String("json".to_string())
        );
        let new_config_str = serde_yaml::to_string(&config_val).unwrap();
        std::fs::write(&config_path, new_config_str).unwrap();

        // 2. Write a mock task registry with a failed task
        let registry_dir = temp_base.join(".macc/automation/task");
        std::fs::create_dir_all(&registry_dir).unwrap();
        let registry_path = registry_dir.join("task_registry.json");

        use macc_core::coordinator::model::{TaskRegistry, Task, TaskRuntime};
        let mut task = Task::default();
        task.id = "AUTH-014".to_string();
        task.title = Some("Add OAuth callback validation".to_string());
        task.state = "blocked".to_string();
        
        let mut runtime = TaskRuntime::default();
        runtime.status = Some("failed".to_string());
        runtime.last_error_code = Some("E601".to_string());
        runtime.last_error_message = Some("Rate limit exceeded".to_string());
        task.task_runtime = runtime;

        let registry = TaskRegistry {
            tasks: vec![task],
            ..Default::default()
        };

        std::fs::write(&registry_path, serde_json::to_string(&registry).unwrap()).unwrap();

        // 3. Test `macc failure list`
        run_with_engine(
            Cli {
                cwd: temp_base.to_string_lossy().into(),
                verbose: false,
                quiet: false,
                offline: false,
                web_port: None,
                command: Some(Commands::Failure {
                    failure_command: commands::failure::FailureCommands::List,
                }),
            },
            TestEngine::with_fixtures(),
        )?;

        // 4. Test `macc failure show AUTH-014`
        run_with_engine(
            Cli {
                cwd: temp_base.to_string_lossy().into(),
                verbose: false,
                quiet: false,
                offline: false,
                web_port: None,
                command: Some(Commands::Failure {
                    failure_command: commands::failure::FailureCommands::Show {
                        task_id: "AUTH-014".to_string(),
                    },
                }),
            },
            TestEngine::with_fixtures(),
        )?;

        // 5. Test `macc failure retry AUTH-014`
        run_with_engine(
            Cli {
                cwd: temp_base.to_string_lossy().into(),
                verbose: false,
                quiet: false,
                offline: false,
                web_port: None,
                command: Some(Commands::Failure {
                    failure_command: commands::failure::FailureCommands::Retry {
                        task_id: "AUTH-014".to_string(),
                        tool: Some("codex".to_string()),
                    },
                }),
            },
            TestEngine::with_fixtures(),
        )?;

        // Verify task state updated to 'todo'
        let registry_content = std::fs::read_to_string(&registry_path).unwrap();
        let registry_after_retry: TaskRegistry = serde_json::from_str(&registry_content).unwrap();
        assert_eq!(registry_after_retry.tasks[0].state, "todo");
        assert_eq!(registry_after_retry.tasks[0].tool, Some("codex".to_string()));

        // 6. Test `macc failure salvage AUTH-014`
        run_with_engine(
            Cli {
                cwd: temp_base.to_string_lossy().into(),
                verbose: false,
                quiet: false,
                offline: false,
                web_port: None,
                command: Some(Commands::Failure {
                    failure_command: commands::failure::FailureCommands::Salvage {
                        task_id: "AUTH-014".to_string(),
                    },
                }),
            },
            TestEngine::with_fixtures(),
        )?;

        let registry_content = std::fs::read_to_string(&registry_path).unwrap();
        let registry_after_salvage: TaskRegistry = serde_json::from_str(&registry_content).unwrap();
        assert_eq!(registry_after_salvage.tasks[0].state, "changes_requested");

        // 7. Test `macc failure abandon AUTH-014`
        run_with_engine(
            Cli {
                cwd: temp_base.to_string_lossy().into(),
                verbose: false,
                quiet: false,
                offline: false,
                web_port: None,
                command: Some(Commands::Failure {
                    failure_command: commands::failure::FailureCommands::Abandon {
                        task_id: "AUTH-014".to_string(),
                    },
                }),
            },
            TestEngine::with_fixtures(),
        )?;

        let registry_content = std::fs::read_to_string(&registry_path).unwrap();
        let registry_after_abandon: TaskRegistry = serde_json::from_str(&registry_content).unwrap();
        assert_eq!(registry_after_abandon.tasks[0].state, "abandoned");

        // Verify audit log entries were recorded
        let ops_log_path = temp_base.join(".macc/log/ops.jsonl");
        assert!(ops_log_path.exists());
        let ops_log_content = std::fs::read_to_string(ops_log_path).unwrap();
        assert!(ops_log_content.contains("cli failure retry"));
        assert!(ops_log_content.contains("cli failure salvage"));
        assert!(ops_log_content.contains("cli failure abandon"));

        // Cleanup
        std::fs::remove_dir_all(&temp_base).ok();
        Ok(())
    }

    #[test]
    fn test_save_restore_clear_init_workflow() -> macc_core::Result<()> {
        let _guard = env_test_lock();
        let temp_base = std::env::temp_dir().join(format!("macc_save_restore_test_{}", uuid_v4_like()));
        std::fs::create_dir_all(&temp_base).unwrap();

        run_git_ok(&temp_base, &["init"]);
        run_git_ok(&temp_base, &["config", "user.email", "test@example.com"]);
        run_git_ok(&temp_base, &["config", "user.name", "Test User"]);
        std::fs::write(temp_base.join("dummy.txt"), "dummy").unwrap();
        run_git_ok(&temp_base, &["add", "dummy.txt"]);
        run_git_ok(&temp_base, &["commit", "-m", "initial commit"]);

        let temp_home = temp_base.join("home");
        std::fs::create_dir_all(&temp_home).unwrap();
        let old_home = std::env::var("HOME").ok();
        std::env::set_var("HOME", &temp_home);

        // 1. Initialize MACC
        run_with_engine(
            Cli {
                cwd: temp_base.to_string_lossy().into(),
                verbose: false,
                quiet: false,
                offline: false,
                web_port: None,
                command: Some(Commands::Init {
                    force: false,
                    wizard: false,
                    profile: None,
                    fresh: true,
                    restore: None,
                    no_restore_prompt: true,
                    apply: false,
                }),
            },
            TestEngine::with_fixtures(),
        )?;

        // Verify .macc/macc.yaml exists
        let config_path = temp_base.join(".macc/macc.yaml");
        assert!(config_path.exists());

        // Modify config slightly so we have non-default content
        let original_config = "version: v1\nsettings:\n  test_key: val1";
        std::fs::write(&config_path, original_config).unwrap();

        // Create a dummy sessions file and a dummy log file
        let sessions_dir = temp_base.join(".macc/state");
        std::fs::create_dir_all(&sessions_dir).unwrap();
        let sessions_path = sessions_dir.join("tool-sessions.json");
        std::fs::write(&sessions_path, r#"{"session_id": "test-uuid-1234", "pid": 999}"#).unwrap();

        let log_dir = temp_base.join(".macc/log/coordinator");
        std::fs::create_dir_all(&log_dir).unwrap();
        let log_path = log_dir.join("test.log");
        std::fs::write(&log_path, "session key is ghp_123456789012345678901234567890123456").unwrap();

        // Create some directories that should be excluded
        let cache_dir = temp_base.join(".macc/cache");
        std::fs::create_dir_all(&cache_dir).unwrap();
        std::fs::write(cache_dir.join("should-exclude.txt"), "cached data").unwrap();

        let worktree_dir = temp_base.join(".macc/worktree");
        std::fs::create_dir_all(&worktree_dir).unwrap();
        std::fs::write(worktree_dir.join("should-exclude.txt"), "worktree data").unwrap();

        // 2. Save setup to a bundle
        run_with_engine(
            Cli {
                cwd: temp_base.to_string_lossy().into(),
                verbose: false,
                quiet: false,
                offline: false,
                web_port: None,
                command: Some(Commands::Save {
                    name: Some("test-save-1".to_string()),
                    overwrite: true,
                    description: Some("Test Description".to_string()),
                    only: None,
                    no_sessions: false,
                    include_logs: true,
                    log_max_size: "50MB".to_string(),
                    log_since: "7d".to_string(),
                    redact_logs: true,
                    dry_run: false,
                    cmd: None,
                }),
            },
            TestEngine::with_fixtures(),
        )?;

        // Verify save files created
        let save_dir = temp_home.join(".macc/saves/test-save-1");
        assert!(save_dir.exists());
        assert!(save_dir.join("manifest.yaml").exists());
        assert!(save_dir.join("config/macc.yaml").exists());
        assert!(save_dir.join("state/tool-sessions.json").exists());
        assert!(save_dir.join("logs/coordinator/test.log").exists());
        assert!(save_dir.join("checksums/sha256sums.txt").exists());

        // Verify checksums content
        let checksums_content = std::fs::read_to_string(save_dir.join("checksums/sha256sums.txt")).unwrap();
        assert!(checksums_content.contains("manifest.yaml"));
        assert!(checksums_content.contains("config/macc.yaml"));
        assert!(checksums_content.contains("state/tool-sessions.json"));
        assert!(checksums_content.contains("logs/coordinator/test.log"));
        assert!(!checksums_content.contains("checksums/sha256sums.txt"));

        // Verify corrupted file validation failure on restore
        {
            let config_save_path = save_dir.join("config/macc.yaml");
            let original_config_save_content = std::fs::read_to_string(&config_save_path).unwrap();
            std::fs::write(&config_save_path, "corrupted content").unwrap();

            let restore_result = run_with_engine(
                Cli {
                    cwd: temp_base.to_string_lossy().into(),
                    verbose: false,
                    quiet: false,
                    offline: false,
                    web_port: None,
                    command: Some(Commands::Restore {
                        name: Some("test-save-1".to_string()),
                        latest: false,
                        user: false,
                        backup: None,
                        dry_run: false,
                        yes: true,
                        apply: false,
                        config_only: false,
                        sessions: false,
                        no_sessions: false,
                        include_logs: false,
                    }),
                },
                TestEngine::with_fixtures(),
            );
            assert!(restore_result.is_err());
            let err_msg = format!("{:?}", restore_result.err().unwrap());
            assert!(err_msg.contains("MACC-RESTORE-2003")); // Checksum mismatch
            
            // Restore original content
            std::fs::write(&config_save_path, original_config_save_content).unwrap();
        }

        // Verify exclusions
        assert!(!save_dir.join("cache").exists());
        assert!(!save_dir.join("worktree").exists());

        // Verify log redaction
        let redacted_log_content = std::fs::read_to_string(save_dir.join("logs/coordinator/test.log")).unwrap();
        assert!(redacted_log_content.contains("session key is [REDACTED]"));
        assert!(!redacted_log_content.contains("ghp_"));

        // Verify state is currently saved (not unsaved)
        let paths = macc_core::ProjectPaths::from_root(temp_base.clone());
        assert!(!macc_core::save::is_macc_state_unsaved(&paths)?);

        // Modify config so it is now unsaved
        std::fs::write(&config_path, "version: v1\nsettings:\n  test_key: val2").unwrap();
        assert!(macc_core::save::is_macc_state_unsaved(&paths)?);

        // 3. Clear project with save gate (using --save flag)
        run_with_engine(
            Cli {
                cwd: temp_base.to_string_lossy().into(),
                verbose: false,
                quiet: false,
                offline: false,
                web_port: None,
                command: Some(Commands::Clear {
                    save: Some("test-save-2".to_string()),
                    include_logs: false,
                    no_save_prompt: false,
                    force: true,
                    dry_run: false,
                }),
            },
            TestEngine::with_fixtures(),
        )?;

        // Verify we can delete .macc to test restoration on fresh checkout
        std::fs::remove_dir_all(temp_base.join(".macc")).unwrap();
        assert!(!temp_base.join(".macc").exists());

        // Verify test-save-2 was created before clear
        let save2_dir = temp_home.join(".macc/saves/test-save-2");
        assert!(save2_dir.exists());
        assert!(save2_dir.join("config/macc.yaml").exists());

        // 4. Restore test-save-1 during init
        run_with_engine(
            Cli {
                cwd: temp_base.to_string_lossy().into(),
                verbose: false,
                quiet: false,
                offline: false,
                web_port: None,
                command: Some(Commands::Init {
                    force: false,
                    wizard: false,
                    profile: None,
                    fresh: false,
                    restore: Some("test-save-1".to_string()),
                    no_restore_prompt: true,
                    apply: false,
                }),
            },
            TestEngine::with_fixtures(),
        )?;

        // Verify restored files
        assert!(config_path.exists());
        let restored_config = std::fs::read_to_string(&config_path).unwrap();
        assert_eq!(restored_config, original_config);

        // Verify restored session PID was dropped (normalized)
        let restored_sessions = std::fs::read_to_string(sessions_path).unwrap();
        assert!(restored_sessions.contains("test-uuid-1234"));
        assert!(!restored_sessions.contains("pid"));

        // 5. Test Save list
        run_with_engine(
            Cli {
                cwd: temp_base.to_string_lossy().into(),
                verbose: false,
                quiet: false,
                offline: false,
                web_port: None,
                command: Some(Commands::Save {
                    name: None,
                    overwrite: false,
                    description: None,
                    only: None,
                    no_sessions: false,
                    include_logs: false,
                    log_max_size: "50MB".to_string(),
                    log_since: "7d".to_string(),
                    redact_logs: true,
                    dry_run: false,
                    cmd: Some(SaveSubcommands::List { matching: false }),
                }),
            },
            TestEngine::with_fixtures(),
        )?;

        // 6. Test Save show
        run_with_engine(
            Cli {
                cwd: temp_base.to_string_lossy().into(),
                verbose: false,
                quiet: false,
                offline: false,
                web_port: None,
                command: Some(Commands::Save {
                    name: None,
                    overwrite: false,
                    description: None,
                    only: None,
                    no_sessions: false,
                    include_logs: false,
                    log_max_size: "50MB".to_string(),
                    log_since: "7d".to_string(),
                    redact_logs: true,
                    dry_run: false,
                    cmd: Some(SaveSubcommands::Show { name: "test-save-1".to_string() }),
                }),
            },
            TestEngine::with_fixtures(),
        )?;

        // 7. Test Save delete
        run_with_engine(
            Cli {
                cwd: temp_base.to_string_lossy().into(),
                verbose: false,
                quiet: false,
                offline: false,
                web_port: None,
                command: Some(Commands::Save {
                    name: None,
                    overwrite: false,
                    description: None,
                    only: None,
                    no_sessions: false,
                    include_logs: false,
                    log_max_size: "50MB".to_string(),
                    log_since: "7d".to_string(),
                    redact_logs: true,
                    dry_run: false,
                    cmd: Some(SaveSubcommands::Delete { name: "test-save-1".to_string(), yes: true }),
                }),
            },
            TestEngine::with_fixtures(),
        )?;

        // Verify test-save-1 deleted
        assert!(!temp_home.join(".macc/saves/test-save-1").exists());

        // Cleanup HOME env var
        if let Some(h) = old_home {
            std::env::set_var("HOME", h);
        } else {
            std::env::remove_var("HOME");
        }

        std::fs::remove_dir_all(&temp_base).ok();
        Ok(())
    }

    #[cfg(unix)]
    struct StdinRedirector {
        orig_stdin: std::os::raw::c_int,
    }

    #[cfg(unix)]
    impl StdinRedirector {
        fn redirect(input: &str) -> Self {
            use std::io::Write;
            use std::os::unix::io::FromRawFd;

            unsafe {
                let orig_stdin = libc::dup(0);
                let mut fds = [0; 2];
                libc::pipe(fds.as_mut_ptr());
                libc::dup2(fds[0], 0);
                libc::close(fds[0]);

                let mut file = std::fs::File::from_raw_fd(fds[1]);
                file.write_all(input.as_bytes()).unwrap();
                file.flush().unwrap();

                Self { orig_stdin }
            }
        }
    }

    #[cfg(unix)]
    impl Drop for StdinRedirector {
        fn drop(&mut self) {
            unsafe {
                libc::dup2(self.orig_stdin, 0);
                libc::close(self.orig_stdin);
            }
        }
    }

    #[test]
    fn test_save_handles_secrets_interactively() -> macc_core::Result<()> {
        let _guard = env_test_lock();
        let temp_base = std::env::temp_dir().join(format!("macc_secrets_interactive_test_{}", uuid_v4_like()));
        std::fs::create_dir_all(&temp_base).unwrap();

        run_git_ok(&temp_base, &["init"]);
        run_git_ok(&temp_base, &["config", "user.email", "test@example.com"]);
        run_git_ok(&temp_base, &["config", "user.name", "Test User"]);
        std::fs::write(temp_base.join("dummy.txt"), "dummy").unwrap();
        run_git_ok(&temp_base, &["add", "dummy.txt"]);
        run_git_ok(&temp_base, &["commit", "-m", "initial commit"]);

        let temp_home = temp_base.join("home");
        std::fs::create_dir_all(&temp_home).unwrap();
        let old_home = std::env::var("HOME").ok();
        std::env::set_var("HOME", &temp_home);

        // Set env variable to force interactive mode in tests
        std::env::set_var("MACC_FORCE_INTERACTIVE", "true");

        // 1. Initialize MACC
        run_with_engine(
            Cli {
                cwd: temp_base.to_string_lossy().into(),
                verbose: false,
                quiet: false,
                offline: false,
                web_port: None,
                command: Some(Commands::Init {
                    force: false,
                    wizard: false,
                    profile: None,
                    fresh: true,
                    restore: None,
                    no_restore_prompt: true,
                    apply: false,
                }),
            },
            TestEngine::with_fixtures(),
        )?;

        let config_path = temp_base.join(".macc/macc.yaml");
        assert!(config_path.exists());

        // 2. Put a secret in config file
        let config_with_secret = "version: v1\nsettings:\n  my_secret: ghp_123456789012345678901234567890123456";
        std::fs::write(&config_path, config_with_secret).unwrap();

        // Test Case A: User inputs "R" (Redact)
        {
            let _redir = StdinRedirector::redirect("R\n");

            run_with_engine(
                Cli {
                    cwd: temp_base.to_string_lossy().into(),
                    verbose: false,
                    quiet: false,
                    offline: false,
                    web_port: None,
                    command: Some(Commands::Save {
                        name: Some("test-save-redact".to_string()),
                        overwrite: true,
                        description: Some("Test Redaction".to_string()),
                        only: None,
                        no_sessions: false,
                        include_logs: false,
                        log_max_size: "50MB".to_string(),
                        log_since: "7d".to_string(),
                        redact_logs: true,
                        dry_run: false,
                        cmd: None,
                    }),
                },
                TestEngine::with_fixtures(),
            )?;

            // Verify save created and redacted
            let save_dir = temp_home.join(".macc/saves/test-save-redact");
            assert!(save_dir.exists());
            let saved_config = std::fs::read_to_string(save_dir.join("config/macc.yaml")).unwrap();
            assert!(saved_config.contains("my_secret: [REDACTED]"));
            assert!(!saved_config.contains("ghp_"));
        }

        // Test Case B: Exclude
        {
            let _redir = StdinRedirector::redirect("E\n");

            run_with_engine(
                Cli {
                    cwd: temp_base.to_string_lossy().into(),
                    verbose: false,
                    quiet: false,
                    offline: false,
                    web_port: None,
                    command: Some(Commands::Save {
                        name: Some("test-save-exclude".to_string()),
                        overwrite: true,
                        description: Some("Test Exclusion".to_string()),
                        only: None,
                        no_sessions: false,
                        include_logs: false,
                        log_max_size: "50MB".to_string(),
                        log_since: "7d".to_string(),
                        redact_logs: true,
                        dry_run: false,
                        cmd: None,
                    }),
                },
                TestEngine::with_fixtures(),
            )?;

            // Verify save created and config excluded
            let save_dir = temp_home.join(".macc/saves/test-save-exclude");
            assert!(save_dir.exists());
            let manifest_content = std::fs::read_to_string(save_dir.join("manifest.yaml")).unwrap();
            assert!(manifest_content.contains("config: false")); // config should be false in includes
            assert!(!save_dir.join("config").exists()); // config dir should be removed
        }

        // Test Case C: Abort
        {
            let _redir = StdinRedirector::redirect("A\n");

            let result = run_with_engine(
                Cli {
                    cwd: temp_base.to_string_lossy().into(),
                    verbose: false,
                    quiet: false,
                    offline: false,
                    web_port: None,
                    command: Some(Commands::Save {
                        name: Some("test-save-abort".to_string()),
                        overwrite: true,
                        description: Some("Test Abort".to_string()),
                        only: None,
                        no_sessions: false,
                        include_logs: false,
                        log_max_size: "50MB".to_string(),
                        log_since: "7d".to_string(),
                        redact_logs: true,
                        dry_run: false,
                        cmd: None,
                    }),
                },
                TestEngine::with_fixtures(),
            );

            assert!(result.is_err());
            let err_msg = format!("{:?}", result.err().unwrap());
            assert!(err_msg.contains("MACC-SAVE-1003"));
            assert!(!temp_home.join(".macc/saves/test-save-abort").exists());
        }

        // Cleanup
        std::env::remove_var("MACC_FORCE_INTERACTIVE");
        if let Some(h) = old_home {
            std::env::set_var("HOME", h);
        } else {
            std::env::remove_var("HOME");
        }
        std::fs::remove_dir_all(&temp_base).ok();
        Ok(())
    }
}
