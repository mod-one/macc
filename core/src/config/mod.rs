pub mod migrate;

use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashSet};
use std::path::Path;

#[derive(Debug, Serialize, Deserialize, PartialEq, Clone)]
#[serde(deny_unknown_fields)]
pub struct CanonicalConfig {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub version: Option<String>,
    pub tools: ToolsConfig,
    #[serde(default)]
    pub standards: StandardsConfig,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub selections: Option<SelectionsConfig>,
    #[serde(default)]
    pub automation: AutomationConfig,
    #[serde(default)]
    pub settings: SettingsConfig,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub process_ownership: Option<ProcessOwnershipConfig>,
    #[serde(default = "default_mcp_templates")]
    pub mcp_templates: Vec<McpTemplateDefinition>,
}

#[derive(Debug, Serialize, Deserialize, PartialEq, Clone, Default)]
#[serde(deny_unknown_fields)]
pub struct SettingsConfig {
    #[serde(default)]
    pub quiet: bool,
    #[serde(default)]
    pub offline: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub web_port: Option<u16>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub web_assets: Option<WebAssetsMode>,
}

#[derive(Debug, Serialize, Deserialize, PartialEq, Eq, Clone, Copy)]
#[serde(rename_all = "lowercase")]
pub enum WebAssetsMode {
    Dist,
    Embedded,
}

#[derive(Debug, Serialize, Deserialize, PartialEq, Clone, Default)]
#[serde(deny_unknown_fields)]
pub struct ProcessOwnershipConfig {
    /// Seconds before a pending project-control takeover request expires.
    /// `0` disables the timeout. Default: 60.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub takeover_timeout_seconds: Option<u64>,
    /// Default response applied when a takeover request times out:
    /// "deny" (default), "auto_accept", or "admin_takeover".
    #[serde(skip_serializing_if = "Option::is_none")]
    pub takeover_default_response: Option<String>,
}

#[derive(Debug, Serialize, Deserialize, PartialEq, Clone, Default)]
pub struct ToolsConfig {
    pub enabled: Vec<String>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub config: BTreeMap<String, serde_json::Value>,
    #[serde(flatten)]
    pub settings: BTreeMap<String, serde_json::Value>,
}

#[derive(Debug, Serialize, Deserialize, PartialEq, Clone, Default)]
pub struct StandardsConfig {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub path: Option<String>,
    #[serde(flatten, skip_serializing_if = "BTreeMap::is_empty")]
    pub inline: BTreeMap<String, String>,
}

#[derive(Debug, Serialize, Deserialize, PartialEq, Clone, Default)]
#[serde(deny_unknown_fields)]
pub struct SelectionsConfig {
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub skills: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub agents: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub mcp: Vec<String>,
}

#[derive(Debug, Serialize, Deserialize, PartialEq, Clone, Default)]
#[serde(deny_unknown_fields)]
pub struct AutomationConfig {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ralph: Option<RalphConfig>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub coordinator: Option<CoordinatorConfig>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub supervisor: Option<crate::supervisor::SupervisorConfig>,
}

#[derive(Debug, Serialize, Deserialize, PartialEq, Clone)]
#[serde(deny_unknown_fields)]
pub struct RalphConfig {
    #[serde(default = "default_true")]
    pub enabled: bool,
    #[serde(default = "default_ralph_iterations")]
    pub iterations_default: usize,
    #[serde(default = "default_ralph_branch")]
    pub branch_name: String,
    #[serde(default = "default_true")]
    pub stop_on_failure: bool,
}

#[derive(Debug, Serialize, Deserialize, PartialEq, Clone)]
#[serde(deny_unknown_fields)]
pub struct CoordinatorConfig {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub coordinator_tool: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reference_branch: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub prd_file: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub task_registry_file: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tool_priority: Vec<String>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub max_parallel_per_tool: BTreeMap<String, usize>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub tool_specializations: BTreeMap<String, Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_dispatch: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_parallel: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub timeout_seconds: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub phase_runner_max_attempts: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub log_flush_lines: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub log_flush_ms: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub mirror_json_debounce_ms: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stale_claimed_seconds: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stale_in_progress_seconds: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stale_changes_requested_seconds: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stale_action: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub storage_mode: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub merge_ai_fix: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub merge_job_timeout_seconds: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub merge_hook_timeout_seconds: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ghost_heartbeat_grace_seconds: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub dispatch_cooldown_seconds: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_dispatch_retries: Option<u32>,
    /// Session cache TTL in seconds used by dispatch when preferring warm
    /// worktree sessions. Default behavior when unset: 300 seconds.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub session_cache_ttl_seconds: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub json_compat: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub legacy_json_fallback: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error_code_retry_list: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error_code_retry_max: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cutover_gate_window_events: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cutover_gate_max_blocked_ratio: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cutover_gate_max_stale_ratio: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rate_limit_backoff_base_seconds: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rate_limit_backoff_max_seconds: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rate_limit_fallback_enabled: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rate_limit_throttle_parallel: Option<bool>,
    /// Grace period in seconds between receiving a terminal failure IPC signal
    /// and force-killing the performer process.  Default: 30.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub force_kill_grace_seconds: Option<u64>,
    /// Maximum number of review cycles allowed per task.
    /// 0 = skip review entirely, 1 = one review + one fix (no loopback),
    /// N = up to N review→fix→review loops.  Default: None (unlimited).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_review_cycles: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub safety_policy: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub destructive_actions: Option<String>,

    // ── Reliability feature toggles ──────────────────────────────────────────
    /// Attempt to salvage partial work before retrying a failed task.
    /// Default: true.
    #[serde(default = "default_true", skip_serializing_if = "is_true")]
    pub salvage_before_retry: bool,

    /// Retry a failed task on the same worktree slot when possible.
    /// Default: true.
    #[serde(default = "default_true", skip_serializing_if = "is_true")]
    pub retry_on_same_worktree: bool,

    /// Gate dispatch behind a merge-health check to prevent cascading failures.
    /// Default: true.
    #[serde(default = "default_true", skip_serializing_if = "is_true")]
    pub merge_gate_on_dispatch: bool,

    /// Remove newly-created worktrees when sanitization fails during dispatch.
    /// Default: true.
    #[serde(default = "default_true", skip_serializing_if = "is_true")]
    pub remove_worktree_on_sanitize_failure: bool,

    /// Tag branches for abandoned tasks so they are discoverable after cleanup.
    /// Default: true.
    #[serde(default = "default_true", skip_serializing_if = "is_true")]
    pub tag_abandoned_branches: bool,

    /// Scan unmerged branches during sync to recover partially-complete work.
    /// Default: true.
    #[serde(default = "default_true", skip_serializing_if = "is_true")]
    pub sync_unmerged_branches: bool,

    /// Timeout in seconds for the salvage-merge operation.
    /// Default: 120.
    #[serde(
        default = "default_salvage_merge_timeout",
        skip_serializing_if = "is_default_salvage_merge_timeout"
    )]
    pub salvage_merge_timeout_seconds: u64,

    /// Maximum number of salvage attempts allowed per task before giving up.
    /// Default: 1.
    #[serde(
        default = "default_max_salvage_attempts",
        skip_serializing_if = "is_default_max_salvage_attempts"
    )]
    pub max_salvage_attempts_per_task: u32,

    /// Seconds before a pending process-ownership takeover request expires.
    /// `0` disables the timeout (request stays pending forever).  Default: 60.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub takeover_timeout_seconds: Option<u64>,

    /// Default response applied when a takeover request times out:
    /// "deny" (default), "auto_accept", or "admin_takeover".
    #[serde(skip_serializing_if = "Option::is_none")]
    pub takeover_default_response: Option<String>,
}

fn default_true() -> bool {
    true
}

fn is_true(v: &bool) -> bool {
    *v
}

fn default_salvage_merge_timeout() -> u64 {
    120
}

fn is_default_salvage_merge_timeout(v: &u64) -> bool {
    *v == 120
}

fn default_max_salvage_attempts() -> u32 {
    1
}

fn is_default_max_salvage_attempts(v: &u32) -> bool {
    *v == 1
}

fn default_ralph_iterations() -> usize {
    5
}

fn default_ralph_branch() -> String {
    "ralph".to_string()
}

#[derive(Debug, Serialize, Deserialize, PartialEq, Clone)]
#[serde(deny_unknown_fields)]
pub struct McpTemplateDefinition {
    pub id: String,
    pub title: String,
    pub description: String,
    pub command: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub args: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub env_placeholders: Vec<McpEnvPlaceholder>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub auth_notes: Option<String>,
}

#[derive(Debug, Serialize, Deserialize, PartialEq, Clone)]
#[serde(deny_unknown_fields)]
pub struct McpEnvPlaceholder {
    pub name: String,
    pub placeholder: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
}

pub fn load_canonical_config<P: AsRef<Path>>(path: P) -> crate::Result<CanonicalConfig> {
    let path = path.as_ref();
    let content = std::fs::read_to_string(path).map_err(|e| crate::MaccError::Io {
        path: path.to_string_lossy().into(),
        action: "read config".into(),
        source: e,
    })?;

    let config = CanonicalConfig::from_yaml(&content).map_err(|e| crate::MaccError::Config {
        path: path.to_string_lossy().into(),
        source: e,
    })?;

    config.validate()?;
    Ok(config)
}

impl Default for CoordinatorConfig {
    fn default() -> Self {
        Self {
            coordinator_tool: None,
            reference_branch: None,
            prd_file: None,
            task_registry_file: None,
            tool_priority: Vec::new(),
            max_parallel_per_tool: BTreeMap::new(),
            tool_specializations: BTreeMap::new(),
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
            max_dispatch_retries: None,
            session_cache_ttl_seconds: None,
            json_compat: None,
            legacy_json_fallback: None,
            error_code_retry_list: None,
            error_code_retry_max: None,
            cutover_gate_window_events: None,
            cutover_gate_max_blocked_ratio: None,
            cutover_gate_max_stale_ratio: None,
            rate_limit_backoff_base_seconds: None,
            rate_limit_backoff_max_seconds: None,
            rate_limit_fallback_enabled: None,
            rate_limit_throttle_parallel: None,
            force_kill_grace_seconds: None,
            max_review_cycles: None,
            salvage_before_retry: true,
            retry_on_same_worktree: true,
            merge_gate_on_dispatch: true,
            remove_worktree_on_sanitize_failure: true,
            tag_abandoned_branches: true,
            sync_unmerged_branches: true,
            salvage_merge_timeout_seconds: 120,
            max_salvage_attempts_per_task: 1,
            takeover_timeout_seconds: None,
            takeover_default_response: None,
            safety_policy: None,
            destructive_actions: None,
        }
    }
}

/// All coordinator configuration fields with canonical defaults baked in.
///
/// Construct via [`CoordinatorConfigResolved::resolve`] to guarantee that every
/// field has a single, well-documented value regardless of whether the user
/// supplied a `CoordinatorConfig` in `macc.yaml`.
///
/// Call sites that previously called `config.field.unwrap_or(N)` should be
/// updated to read `resolved.field` directly (tracked in L5-CFG-002).
#[derive(Debug, Clone, PartialEq)]
pub struct CoordinatorConfigResolved {
    // ── Identity / tool selection ────────────────────────────────────────────
    /// Override which tool executes coordinator phases (review, fix).
    /// `None` means auto-select: first enabled tool from `tool_priority`, or
    /// the first enabled tool overall.
    pub coordinator_tool: Option<String>,

    // ── Task source ──────────────────────────────────────────────────────────
    /// Path to the PRD JSON file.
    /// `None` means use the project-default path (`prd.json` in project root).
    pub prd_file: Option<String>,

    /// Path to the task registry JSON file.
    /// `None` means use the fixed default: `.macc/automation/task/task_registry.json`.
    pub task_registry_file: Option<String>,

    // ── Dispatch / parallelism ───────────────────────────────────────────────
    /// Git branch to rebase/merge finished task branches onto.
    /// Default: `"master"` — matches git's historical default.
    pub reference_branch: String,

    /// Ordered list of tools to prefer when dispatching new tasks.
    /// Default: empty — all enabled tools are eligible equally.
    pub tool_priority: Vec<String>,

    /// Per-tool cap on concurrent performers.
    /// Default: empty — no per-tool cap; `max_parallel` is the only bound.
    pub max_parallel_per_tool: BTreeMap<String, usize>,

    /// Restrict specific task categories to specific tools.
    /// Default: empty — all tools handle all categories.
    pub tool_specializations: BTreeMap<String, Vec<String>>,

    /// Maximum number of tasks dispatched per coordinator cycle.
    /// Default: `10` — limits blast radius on first run.
    pub max_dispatch: usize,

    /// Maximum number of tasks executing in parallel across all tools.
    /// Default: `3` — conservative; raise for machines with more cores/quota.
    pub max_parallel: usize,

    /// Wall-clock timeout for the entire coordinator run in seconds.
    /// `0` means unlimited.  Default: `0`.
    pub timeout_seconds: usize,

    /// Number of times a phase runner (performer/reviewer) is retried on
    /// non-fatal failures before the task is marked failed.
    /// Default: `1` (one attempt, no automatic retry at the runner level).
    pub phase_runner_max_attempts: usize,
    /// Maximum number of dispatch cooldown retries before the task is blocked.
    /// Default: `5`.
    pub max_dispatch_retries: u32,

    // ── Logging ──────────────────────────────────────────────────────────────
    /// Flush the coordinator log file after this many buffered lines.
    /// Default: `500` — balances I/O cost vs. data-loss risk.
    pub log_flush_lines: usize,

    /// Flush the coordinator log file after this many milliseconds even if
    /// the line threshold has not been reached.
    /// Default: `60_000` (1 minute).
    pub log_flush_ms: u64,

    /// Debounce interval in milliseconds for the JSON mirror writer.
    /// `None` means the mirror writer uses its own internal default.
    pub mirror_json_debounce_ms: Option<u64>,

    // ── Staleness / health ───────────────────────────────────────────────────
    /// Mark a task as stale (and apply `stale_action`) if it has been in the
    /// `claimed` state for more than this many seconds.
    /// `0` disables stale-claimed detection.  Default: `0`.
    pub stale_claimed_seconds: usize,

    /// Mark a task as stale if it has been `in_progress` for more than this
    /// many seconds.  `0` disables.  Also used as the phase execution timeout.
    /// Default: `0`.
    pub stale_in_progress_seconds: usize,

    /// Mark a task as stale if it has been waiting for `changes_requested`
    /// review for more than this many seconds.  `0` disables.  Default: `0`.
    pub stale_changes_requested_seconds: usize,

    /// Action taken when a task is detected as stale.
    /// Valid values: `"block"`, `"retry"`, `"requeue"`.
    /// Default: `"block"` — safe; requires operator to investigate.
    pub stale_action: String,

    /// Grace period in seconds before a task whose heartbeat has stopped is
    /// considered a ghost and cleaned up.
    /// Default: `30` — matches the default performer heartbeat interval.
    pub ghost_heartbeat_grace_seconds: i64,

    // ── Storage ──────────────────────────────────────────────────────────────
    /// Backend used for coordinator state persistence.
    /// Valid values: `"sqlite"`, `"json"`, `"dual-write"`.
    /// Default: `"sqlite"` — recommended for production; atomic and fast.
    pub storage_mode: String,

    /// When `true`, write the coordinator snapshot to the legacy JSON mirror
    /// in addition to SQLite.  Useful during migration.
    /// Default: `false`.
    pub json_compat: bool,

    /// When `true`, fall back to the JSON store if the SQLite store is empty
    /// or missing.  Useful during migration.
    /// Default: `false`.
    pub legacy_json_fallback: bool,

    // ── Merging ───────────────────────────────────────────────────────────────
    /// When `true`, invoke an AI tool to auto-resolve merge conflicts.
    /// Default: `false` — AI merge is opt-in; manual review is safer by
    /// default.
    pub merge_ai_fix: bool,

    /// Timeout in seconds for a merge job.
    /// `0` means unlimited.  Default: `0`.
    pub merge_job_timeout_seconds: usize,

    /// Timeout in seconds for the post-merge hook.
    /// Default: `90` — enough for most CI-style hooks.
    pub merge_hook_timeout_seconds: u64,

    // ── Dispatch scheduling ───────────────────────────────────────────────────
    /// Minimum seconds between successive dispatch cycles.
    /// Default: `2` — prevents tight spin-loop when all slots are busy.
    pub dispatch_cooldown_seconds: u64,

    /// How long (seconds) a worktree session is considered "warm" for
    /// dispatch preference.  Coordinator prefers re-using warm sessions to
    /// avoid cold-start latency in the AI tool.
    /// Default: `300` (5 minutes) — matches typical prompt-cache TTL.
    pub session_cache_ttl_seconds: u64,

    // ── Error-code retry ──────────────────────────────────────────────────────
    /// Comma-separated list of error codes eligible for automatic retry.
    /// Default: `"E101,E102,E103,E301,E302,E303,E601,E603"` — covers
    /// transient runner, filesystem, and rate-limit failures.
    pub error_code_retry_list: String,

    /// Maximum number of automatic retries per task before the task is
    /// permanently marked failed.
    /// Default: `2`.
    pub error_code_retry_max: usize,

    // ── Rate-limit handling ───────────────────────────────────────────────────
    /// Base backoff in seconds for the first rate-limit retry (E601).
    /// Subsequent retries use exponential backoff capped at
    /// `rate_limit_backoff_max_seconds`.
    /// Default: `30`.
    pub rate_limit_backoff_base_seconds: u64,

    /// Maximum backoff in seconds for rate-limit retries.
    /// Default: `300` (5 minutes).
    pub rate_limit_backoff_max_seconds: u64,

    /// When `true`, fall back to the next tool in `tool_priority` when the
    /// primary tool is rate-limited.
    /// Default: `true`.
    pub rate_limit_fallback_enabled: bool,

    /// When `true`, reduce `effective_max_parallel` on each rate-limit event
    /// and restore it on recovery.
    /// Default: `true`.
    pub rate_limit_throttle_parallel: bool,

    // ── Cutover gate ─────────────────────────────────────────────────────────
    /// Number of recent coordinator events inspected by the SQLite→primary
    /// cutover gate to assess storage health.
    /// Default: `2000`.
    pub cutover_gate_window_events: usize,

    /// Maximum fraction of inspected events that may be `blocked` before the
    /// cutover gate refuses to proceed.
    /// Default: `0.25` (25 %).
    pub cutover_gate_max_blocked_ratio: f64,

    /// Maximum fraction of inspected events that may be `stale` before the
    /// cutover gate refuses to proceed.
    /// Default: `0.25` (25 %).
    pub cutover_gate_max_stale_ratio: f64,

    // ── Process lifecycle ─────────────────────────────────────────────────────
    /// Seconds between receiving a terminal failure signal and force-killing
    /// the performer process.
    /// Default: `30` — gives performers time to flush logs before SIGKILL.
    pub force_kill_grace_seconds: u64,

    /// Maximum review→fix→review cycles allowed per task.
    /// `None` means unlimited.  `0` skips review entirely.
    pub max_review_cycles: Option<usize>,

    // ── Reliability feature toggles ───────────────────────────────────────────
    /// Attempt to salvage partial work from the last worktree before retrying.
    /// Default: `true`.
    pub salvage_before_retry: bool,

    /// Retry a failed task on the same worktree slot when available.
    /// Preserves local context and warm session state.
    /// Default: `true`.
    pub retry_on_same_worktree: bool,

    /// Run a merge-health check before dispatching a new task.
    /// Prevents cascade failures when the reference branch is broken.
    /// Default: `true`.
    pub merge_gate_on_dispatch: bool,

    /// Remove newly-created worktrees when sanitization fails during dispatch.
    /// Default: `true`.
    pub remove_worktree_on_sanitize_failure: bool,

    /// Tag branches of abandoned tasks before cleanup so they are
    /// discoverable via `git tag`.
    /// Default: `true`.
    pub tag_abandoned_branches: bool,

    /// Scan unmerged branches during sync to recover partially-complete work.
    /// Default: `true`.
    pub sync_unmerged_branches: bool,

    /// Timeout in seconds for the salvage-merge operation.
    /// Default: `120`.
    pub salvage_merge_timeout_seconds: u64,

    /// Maximum salvage attempts per task before giving up and hard-failing.
    /// Default: `1`.
    pub max_salvage_attempts_per_task: u32,

    /// Permitted tool write scopes and validations.
    /// Default: `"standard"`.
    pub safety_policy: String,

    /// Risk policy for destructive actions.
    /// Default: `"double_confirm"`.
    pub destructive_actions: String,
}

impl CoordinatorConfigResolved {
    /// Apply canonical defaults for every field in `CoordinatorConfig`.
    ///
    /// Pass `None` to get a fully-default resolved config (useful for tests
    /// and callers that have no `macc.yaml`).  Pass `Some(cfg)` to layer the
    /// user's overrides on top of the defaults.
    ///
    /// Every `Option<T>` field in `CoordinatorConfig` maps to a concrete
    /// value here so call sites never need scattered `unwrap_or(N)` calls.
    pub fn resolve(config: Option<&CoordinatorConfig>) -> Self {
        Self {
            coordinator_tool: config.and_then(|c| c.coordinator_tool.clone()),
            prd_file: config.and_then(|c| c.prd_file.clone()),
            task_registry_file: config.and_then(|c| c.task_registry_file.clone()),
            reference_branch: config
                .and_then(|c| c.reference_branch.clone())
                .unwrap_or_else(|| "master".to_string()),
            tool_priority: config.map(|c| c.tool_priority.clone()).unwrap_or_default(),
            max_parallel_per_tool: config
                .map(|c| c.max_parallel_per_tool.clone())
                .unwrap_or_default(),
            tool_specializations: config
                .map(|c| c.tool_specializations.clone())
                .unwrap_or_default(),
            max_dispatch: config.and_then(|c| c.max_dispatch).unwrap_or(10),
            max_parallel: config.and_then(|c| c.max_parallel).unwrap_or(3),
            timeout_seconds: config.and_then(|c| c.timeout_seconds).unwrap_or(0),
            phase_runner_max_attempts: config
                .and_then(|c| c.phase_runner_max_attempts)
                .unwrap_or(1)
                .max(1),
            max_dispatch_retries: config.and_then(|c| c.max_dispatch_retries).unwrap_or(5),
            log_flush_lines: config
                .and_then(|c| c.log_flush_lines)
                .filter(|v| *v > 0)
                .unwrap_or(500),
            log_flush_ms: config
                .and_then(|c| c.log_flush_ms)
                .filter(|v| *v > 0)
                .unwrap_or(60_000),
            mirror_json_debounce_ms: config.and_then(|c| c.mirror_json_debounce_ms),
            stale_claimed_seconds: config.and_then(|c| c.stale_claimed_seconds).unwrap_or(0),
            stale_in_progress_seconds: config
                .and_then(|c| c.stale_in_progress_seconds)
                .unwrap_or(0),
            stale_changes_requested_seconds: config
                .and_then(|c| c.stale_changes_requested_seconds)
                .unwrap_or(0),
            stale_action: config
                .and_then(|c| c.stale_action.clone())
                .unwrap_or_else(|| "block".to_string()),
            ghost_heartbeat_grace_seconds: config
                .and_then(|c| c.ghost_heartbeat_grace_seconds)
                .unwrap_or(30),
            storage_mode: config
                .and_then(|c| c.storage_mode.clone())
                .unwrap_or_else(|| "sqlite".to_string()),
            json_compat: config.and_then(|c| c.json_compat).unwrap_or(false),
            legacy_json_fallback: config.and_then(|c| c.legacy_json_fallback).unwrap_or(false),
            merge_ai_fix: config.and_then(|c| c.merge_ai_fix).unwrap_or(false),
            merge_job_timeout_seconds: config
                .and_then(|c| c.merge_job_timeout_seconds)
                .unwrap_or(0),
            merge_hook_timeout_seconds: config
                .and_then(|c| c.merge_hook_timeout_seconds)
                .unwrap_or(90),
            dispatch_cooldown_seconds: config
                .and_then(|c| c.dispatch_cooldown_seconds)
                .unwrap_or(2),
            session_cache_ttl_seconds: config
                .and_then(|c| c.session_cache_ttl_seconds)
                .unwrap_or(300),
            error_code_retry_list: config
                .and_then(|c| c.error_code_retry_list.clone())
                .unwrap_or_else(|| "E101,E102,E103,E301,E302,E303,E601,E603".to_string()),
            error_code_retry_max: config.and_then(|c| c.error_code_retry_max).unwrap_or(2),
            rate_limit_backoff_base_seconds: config
                .and_then(|c| c.rate_limit_backoff_base_seconds)
                .unwrap_or(30),
            rate_limit_backoff_max_seconds: config
                .and_then(|c| c.rate_limit_backoff_max_seconds)
                .unwrap_or(300),
            rate_limit_fallback_enabled: config
                .and_then(|c| c.rate_limit_fallback_enabled)
                .unwrap_or(true),
            rate_limit_throttle_parallel: config
                .and_then(|c| c.rate_limit_throttle_parallel)
                .unwrap_or(true),
            cutover_gate_window_events: config
                .and_then(|c| c.cutover_gate_window_events)
                .unwrap_or(2000),
            cutover_gate_max_blocked_ratio: config
                .and_then(|c| c.cutover_gate_max_blocked_ratio)
                .unwrap_or(0.25),
            cutover_gate_max_stale_ratio: config
                .and_then(|c| c.cutover_gate_max_stale_ratio)
                .unwrap_or(0.25),
            force_kill_grace_seconds: config
                .and_then(|c| c.force_kill_grace_seconds)
                .unwrap_or(30),
            max_review_cycles: config.and_then(|c| c.max_review_cycles),
            salvage_before_retry: config.map(|c| c.salvage_before_retry).unwrap_or(true),
            retry_on_same_worktree: config.map(|c| c.retry_on_same_worktree).unwrap_or(true),
            merge_gate_on_dispatch: config.map(|c| c.merge_gate_on_dispatch).unwrap_or(true),
            remove_worktree_on_sanitize_failure: config
                .map(|c| c.remove_worktree_on_sanitize_failure)
                .unwrap_or(true),
            tag_abandoned_branches: config.map(|c| c.tag_abandoned_branches).unwrap_or(true),
            sync_unmerged_branches: config.map(|c| c.sync_unmerged_branches).unwrap_or(true),
            salvage_merge_timeout_seconds: config
                .map(|c| c.salvage_merge_timeout_seconds)
                .unwrap_or(120),
            max_salvage_attempts_per_task: config
                .map(|c| c.max_salvage_attempts_per_task)
                .unwrap_or(1),
            safety_policy: config
                .and_then(|c| c.safety_policy.clone())
                .unwrap_or_else(|| "standard".to_string()),
            destructive_actions: config
                .and_then(|c| c.destructive_actions.clone())
                .unwrap_or_else(|| "double_confirm".to_string()),
        }
    }
}

impl CanonicalConfig {
    pub fn from_yaml(yaml: &str) -> Result<Self, serde_yaml::Error> {
        serde_yaml::from_str(yaml)
    }

    pub fn to_yaml(&self) -> Result<String, serde_yaml::Error> {
        serde_yaml::to_string(self)
    }

    pub fn validate(&self) -> crate::Result<()> {
        let mut seen_ids = HashSet::new();

        for template in &self.mcp_templates {
            let normalized_id = template.id.trim();
            if normalized_id.is_empty() {
                return Err(crate::MaccError::Validation(
                    "MCP template ID cannot be empty".into(),
                ));
            }

            if !seen_ids.insert(normalized_id.to_string()) {
                return Err(crate::MaccError::Validation(format!(
                    "Duplicate MCP template ID: {}",
                    normalized_id
                )));
            }

            if template.command.trim().is_empty() {
                return Err(crate::MaccError::Validation(format!(
                    "MCP template '{}' must include a command",
                    template.id
                )));
            }

            for placeholder in &template.env_placeholders {
                if placeholder.name.trim().is_empty() {
                    return Err(crate::MaccError::Validation(format!(
                        "MCP template '{}' contains an env placeholder without a name",
                        template.id
                    )));
                }

                if placeholder.placeholder.trim().is_empty() {
                    return Err(crate::MaccError::Validation(format!(
                        "MCP template '{}' contains an env placeholder '{}' without a placeholder value",
                        template.id, placeholder.name
                    )));
                }
            }
        }

        Ok(())
    }
}

impl Default for CanonicalConfig {
    fn default() -> Self {
        Self {
            version: None,
            tools: ToolsConfig::default(),
            standards: StandardsConfig::default(),
            selections: None,
            automation: AutomationConfig::default(),
            settings: SettingsConfig::default(),
            process_ownership: None,
            mcp_templates: default_mcp_templates(),
        }
    }
}

fn default_mcp_templates() -> Vec<McpTemplateDefinition> {
    vec![
        McpTemplateDefinition {
            id: "brave-search".to_string(),
            title: "Brave Search".to_string(),
            description: "Search the web via the Brave Search API (placeholder only).".to_string(),
            command: "node".to_string(),
            args: vec!["scripts/brave-search-mcp.js".to_string()],
            env_placeholders: vec![McpEnvPlaceholder {
                name: "BRAVE_API_KEY".to_string(),
                placeholder: "${BRAVE_API_KEY}".to_string(),
                description: Some(
                    "Brave Search API key placeholder; set this locally before running."
                        .to_string(),
                ),
            }],
            auth_notes: Some(
                "Provide ${BRAVE_API_KEY} via your environment; MACC only writes the placeholder."
                    .to_string(),
            ),
        },
        McpTemplateDefinition {
            id: "github-issues".to_string(),
            title: "GitHub Issues".to_string(),
            description: "Manage GitHub issues for the current repository (placeholder auth)."
                .to_string(),
            command: "python".to_string(),
            args: vec!["scripts/github-issues-mcp.py".to_string()],
            env_placeholders: vec![McpEnvPlaceholder {
                name: "GITHUB_TOKEN".to_string(),
                placeholder: "${GITHUB_TOKEN}".to_string(),
                description: Some(
                    "Personal access token with repo scope; MACC keeps only the placeholder."
                        .to_string(),
                ),
            }],
            auth_notes: Some(
                "Set ${GITHUB_TOKEN} locally and keep the real token out of version control."
                    .to_string(),
            ),
        },
        McpTemplateDefinition {
            id: "local-notes".to_string(),
            title: "Local Notes".to_string(),
            description:
                "Expose project notes stored in the repository without additional authentication."
                    .to_string(),
            command: "bash".to_string(),
            args: vec![
                "scripts/local-notes.sh".to_string(),
                "--dir".to_string(),
                "./notes".to_string(),
            ],
            env_placeholders: vec![],
            auth_notes: Some(
                "No secrets required; reads from the checked-in notes directory.".to_string(),
            ),
        },
    ]
}

pub fn builtin_mcp_templates() -> Vec<McpTemplateDefinition> {
    default_mcp_templates()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tool_ids() -> (String, String) {
        let suffix = uuid_v4_like();
        (format!("tool-a-{}", suffix), format!("tool-b-{}", suffix))
    }

    #[test]
    fn test_minimal_roundtrip() {
        let (tool_one, tool_two) = tool_ids();
        let yaml = format!("tools:\n  enabled:\n  - {}\n  - {}\n", tool_one, tool_two);
        let config = CanonicalConfig::from_yaml(&yaml).expect("Should parse minimal yaml");
        assert_eq!(config.tools.enabled, vec![tool_one, tool_two]);

        let reserialized = config.to_yaml().expect("Should serialize back to yaml");
        let config2 =
            CanonicalConfig::from_yaml(&reserialized).expect("Should parse reserialized yaml");
        assert_eq!(config, config2);
    }

    #[test]
    fn test_full_roundtrip() {
        let (tool_one, _) = tool_ids();
        let yaml = format!(
            "version: v1\ntools:\n  enabled:\n  - {}\nstandards:\n  path: config/standards.md\nselections:\n  skills:\n  - implement\n  agents:\n  - architect\n",
            tool_one
        );
        let config = CanonicalConfig::from_yaml(&yaml).expect("Should parse full yaml");
        assert_eq!(config.version, Some("v1".to_string()));
        assert_eq!(
            config.standards.path,
            Some("config/standards.md".to_string())
        );
        assert_eq!(
            config.selections.as_ref().unwrap().skills,
            vec!["implement"]
        );

        let reserialized = config.to_yaml().expect("Should serialize back to yaml");
        let config2 =
            CanonicalConfig::from_yaml(&reserialized).expect("Should parse reserialized yaml");
        assert_eq!(config, config2);
    }

    #[test]
    fn test_tool_config_agents_roundtrip() {
        let (tool_one, _) = tool_ids();
        let yaml = format!(
            "tools:\n  enabled:\n  - {}\n  {}:\n    agents:\n    - architect\n    - reviewer\n",
            tool_one, tool_one
        );
        let config = CanonicalConfig::from_yaml(&yaml).expect("Should parse tool agents");
        let tool_val = config
            .tools
            .settings
            .get(&tool_one)
            .expect("tool config present");
        let agents = tool_val
            .get("agents")
            .expect("agents present")
            .as_array()
            .expect("is array");
        assert_eq!(agents.len(), 2);
        assert_eq!(agents[0].as_str().unwrap(), "architect");
        assert_eq!(agents[1].as_str().unwrap(), "reviewer");

        let reserialized = config.to_yaml().expect("Should serialize back to yaml");
        let config2 =
            CanonicalConfig::from_yaml(&reserialized).expect("Should parse reserialized yaml");
        assert_eq!(config, config2);
    }

    #[test]
    fn test_tool_user_mcp_merge_roundtrip() {
        let (_, tool_two) = tool_ids();
        let yaml = format!(
            "tools:\n  enabled:\n  - {}\n  {}:\n    user_mcp_merge: true\n",
            tool_two, tool_two
        );
        let config = CanonicalConfig::from_yaml(&yaml).expect("Should parse tool config");
        let tool_val = config
            .tools
            .settings
            .get(&tool_two)
            .expect("tool config present");
        assert_eq!(
            tool_val
                .get("user_mcp_merge")
                .expect("field present")
                .as_bool()
                .unwrap(),
            true
        );

        let reserialized = config.to_yaml().expect("Should serialize back to yaml");
        let config2 =
            CanonicalConfig::from_yaml(&reserialized).expect("Should parse reserialized yaml");
        assert_eq!(config, config2);
    }

    #[test]
    fn test_tools_config_map_roundtrip() {
        let (tool_one, _) = tool_ids();
        let yaml = format!(
            "tools:\n  enabled:\n  - {}\n  config:\n    {}:\n      agents:\n      - architect\n",
            tool_one, tool_one
        );
        let config = CanonicalConfig::from_yaml(&yaml).expect("Should parse tools.config map");
        let tool_config = config
            .tools
            .config
            .get(&tool_one)
            .expect("tool config present in map");
        let agents = tool_config
            .get("agents")
            .expect("agents present")
            .as_array()
            .expect("is array");
        assert_eq!(agents[0].as_str().unwrap(), "architect");

        let reserialized = config.to_yaml().expect("Should serialize back to yaml");
        assert!(reserialized.contains("config:"));
        assert!(reserialized.contains(&format!("{}:", tool_one)));

        let config2 =
            CanonicalConfig::from_yaml(&reserialized).expect("Should parse reserialized yaml");
        assert_eq!(config, config2);
    }

    #[test]
    fn test_ralph_automation_roundtrip() {
        let yaml = r#"tools:
  enabled: []
automation:
  ralph:
    enabled: true
    iterations_default: 10
    branch_name: custom-ralph
    stop_on_failure: false
"#;
        let config = CanonicalConfig::from_yaml(yaml).expect("Should parse ralph config");
        let ralph = config
            .automation
            .ralph
            .as_ref()
            .expect("ralph config present");
        assert!(ralph.enabled);
        assert_eq!(ralph.iterations_default, 10);
        assert_eq!(ralph.branch_name, "custom-ralph");
        assert!(!ralph.stop_on_failure);

        let reserialized = config.to_yaml().expect("Should serialize back to yaml");
        let config2 =
            CanonicalConfig::from_yaml(&reserialized).expect("Should parse reserialized yaml");
        assert_eq!(config, config2);
    }

    #[test]
    fn test_coordinator_automation_roundtrip() {
        let yaml = r#"tools:
  enabled: []
automation:
  coordinator:
    coordinator_tool: tool-alpha
    reference_branch: develop
    prd_file: prd.json
    task_registry_file: task_registry.json
    tool_priority:
      - tool-alpha
      - tool-beta
    max_parallel_per_tool:
      tool-alpha: 3
      tool-beta: 2
    tool_specializations:
      frontend:
        - tool-beta
        - tool-gamma
    max_dispatch: 5
    max_parallel: 2
    timeout_seconds: 30
    phase_runner_max_attempts: 2
    stale_claimed_seconds: 600
    stale_in_progress_seconds: 1200
    stale_changes_requested_seconds: 1800
    stale_action: blocked
    storage_mode: dual-write
"#;
        let config = CanonicalConfig::from_yaml(yaml).expect("Should parse coordinator config");
        let coordinator = config
            .automation
            .coordinator
            .as_ref()
            .expect("coordinator config present");
        assert_eq!(coordinator.coordinator_tool.as_deref(), Some("tool-alpha"));
        assert_eq!(coordinator.reference_branch.as_deref(), Some("develop"));
        assert_eq!(coordinator.prd_file.as_deref(), Some("prd.json"));
        assert_eq!(
            coordinator.task_registry_file.as_deref(),
            Some("task_registry.json")
        );
        assert_eq!(coordinator.tool_priority, vec!["tool-alpha", "tool-beta"]);
        assert_eq!(
            coordinator.max_parallel_per_tool.get("tool-alpha"),
            Some(&3)
        );
        assert_eq!(
            coordinator.tool_specializations.get("frontend"),
            Some(&vec!["tool-beta".to_string(), "tool-gamma".to_string()])
        );
        assert_eq!(coordinator.max_dispatch, Some(5));
        assert_eq!(coordinator.max_parallel, Some(2));
        assert_eq!(coordinator.timeout_seconds, Some(30));
        assert_eq!(coordinator.phase_runner_max_attempts, Some(2));
        assert_eq!(coordinator.stale_claimed_seconds, Some(600));
        assert_eq!(coordinator.stale_in_progress_seconds, Some(1200));
        assert_eq!(coordinator.stale_changes_requested_seconds, Some(1800));
        assert_eq!(coordinator.stale_action.as_deref(), Some("blocked"));
        assert_eq!(coordinator.storage_mode.as_deref(), Some("dual-write"));

        let reserialized = config.to_yaml().expect("Should serialize back to yaml");
        let config2 =
            CanonicalConfig::from_yaml(&reserialized).expect("Should parse reserialized yaml");
        assert_eq!(config, config2);
    }

    #[test]
    fn test_coordinator_config_reliability_defaults() {
        let config = CoordinatorConfig::default();
        assert!(
            config.salvage_before_retry,
            "salvage_before_retry should default to true"
        );
        assert!(
            config.retry_on_same_worktree,
            "retry_on_same_worktree should default to true"
        );
        assert!(
            config.merge_gate_on_dispatch,
            "merge_gate_on_dispatch should default to true"
        );
        assert!(
            config.remove_worktree_on_sanitize_failure,
            "remove_worktree_on_sanitize_failure should default to true"
        );
        assert!(
            config.tag_abandoned_branches,
            "tag_abandoned_branches should default to true"
        );
        assert!(
            config.sync_unmerged_branches,
            "sync_unmerged_branches should default to true"
        );
        assert_eq!(
            config.salvage_merge_timeout_seconds, 120,
            "salvage_merge_timeout_seconds should default to 120"
        );
        assert_eq!(
            config.max_salvage_attempts_per_task, 1,
            "max_salvage_attempts_per_task should default to 1"
        );
    }

    #[test]
    fn test_coordinator_config_reliability_yaml_overrides() {
        let yaml = r#"tools:
  enabled: []
automation:
  coordinator:
    salvage_before_retry: false
    retry_on_same_worktree: false
    merge_gate_on_dispatch: false
    remove_worktree_on_sanitize_failure: false
    tag_abandoned_branches: false
    sync_unmerged_branches: false
    salvage_merge_timeout_seconds: 60
    max_salvage_attempts_per_task: 3
"#;
        let config = CanonicalConfig::from_yaml(yaml).expect("Should parse reliability config");
        let coordinator = config
            .automation
            .coordinator
            .as_ref()
            .expect("coordinator config present");
        assert!(!coordinator.salvage_before_retry);
        assert!(!coordinator.retry_on_same_worktree);
        assert!(!coordinator.merge_gate_on_dispatch);
        assert!(!coordinator.remove_worktree_on_sanitize_failure);
        assert!(!coordinator.tag_abandoned_branches);
        assert!(!coordinator.sync_unmerged_branches);
        assert_eq!(coordinator.salvage_merge_timeout_seconds, 60);
        assert_eq!(coordinator.max_salvage_attempts_per_task, 3);

        let reserialized = config.to_yaml().expect("Should serialize back to yaml");
        let config2 =
            CanonicalConfig::from_yaml(&reserialized).expect("Should parse reserialized yaml");
        assert_eq!(config, config2);
    }

    #[test]
    fn test_supervisor_automation_roundtrip() {
        let yaml = r#"tools:
  enabled: []
automation:
  supervisor:
    watchdog_interval_seconds: 15
    max_restart_attempts: 4
    log_analysis_window_seconds: 120
    report_output_path: .macc/log/supervisor/custom-report.json
    events_log_path: .macc/log/coordinator/events.jsonl
"#;

        let config = CanonicalConfig::from_yaml(yaml).expect("Should parse supervisor config");
        let supervisor = config
            .automation
            .supervisor
            .as_ref()
            .expect("supervisor config present");
        assert_eq!(supervisor.watchdog_interval_seconds, 15);
        assert_eq!(supervisor.max_restart_attempts, 4);
        assert_eq!(supervisor.log_analysis_window_seconds, 120);
        assert_eq!(
            supervisor.report_output_path,
            std::path::PathBuf::from(".macc/log/supervisor/custom-report.json")
        );
        assert_eq!(
            supervisor.events_log_path,
            std::path::PathBuf::from(".macc/log/coordinator/events.jsonl")
        );

        let reserialized = config.to_yaml().expect("Should serialize back to yaml");
        let config2 =
            CanonicalConfig::from_yaml(&reserialized).expect("Should parse reserialized yaml");
        assert_eq!(config, config2);
    }

    #[test]
    fn test_inline_standards() {
        let yaml = r#"tools:
  enabled: []
standards:
  language: English
  package_manager: pnpm
"#;
        let config = CanonicalConfig::from_yaml(yaml).expect("Should parse inline standards");
        assert_eq!(config.standards.inline.get("language").unwrap(), "English");
        assert_eq!(
            config.standards.inline.get("package_manager").unwrap(),
            "pnpm"
        );

        let reserialized = config.to_yaml().expect("Should serialize back to yaml");
        assert!(reserialized.contains("language: English"));
        assert!(reserialized.contains("package_manager: pnpm"));
    }

    #[test]
    fn test_settings_web_assets_roundtrip() {
        let yaml = r#"tools:
  enabled: []
settings:
  web_port: 3450
  web_assets: embedded
"#;
        let config = CanonicalConfig::from_yaml(yaml).expect("Should parse settings");

        assert_eq!(config.settings.web_port, Some(3450));
        assert_eq!(config.settings.web_assets, Some(WebAssetsMode::Embedded));

        let reserialized = config.to_yaml().expect("Should serialize back to yaml");
        assert!(reserialized.contains("web_assets: embedded"));

        let config2 =
            CanonicalConfig::from_yaml(&reserialized).expect("Should parse reserialized yaml");
        assert_eq!(config, config2);
    }

    #[test]
    fn test_deterministic_standards_serialization() {
        let mut inline = BTreeMap::new();
        inline.insert("z".to_string(), "last".to_string());
        inline.insert("a".to_string(), "first".to_string());
        inline.insert("m".to_string(), "middle".to_string());

        let config = CanonicalConfig {
            version: None,
            tools: ToolsConfig {
                enabled: vec!["test".to_string()],
                ..Default::default()
            },
            standards: StandardsConfig { path: None, inline },
            selections: None,
            automation: AutomationConfig::default(),
            settings: SettingsConfig::default(),
            process_ownership: None,
            mcp_templates: Vec::new(),
        };

        let yaml1 = config.to_yaml().expect("Should serialize");
        let yaml2 = config.to_yaml().expect("Should serialize");

        assert_eq!(yaml1, yaml2);

        // Check that keys are in alphabetical order in YAML
        let a_pos = yaml1.find("a: first").unwrap();
        let m_pos = yaml1.find("m: middle").unwrap();
        let z_pos = yaml1.find("z: last").unwrap();

        assert!(a_pos < m_pos);
        assert!(m_pos < z_pos);
    }

    #[test]
    fn test_deny_unknown_fields() {
        let yaml = r#"tools:
  enabled: []
unknown_field: true
"#;
        let err = CanonicalConfig::from_yaml(yaml).unwrap_err();
        assert!(err.to_string().contains("unknown field `unknown_field`"));
    }

    #[test]
    fn test_invalid_yaml_syntax() {
        let yaml = r#"tools:
  enabled: [
"#;
        let err = CanonicalConfig::from_yaml(yaml).unwrap_err();
        assert!(err
            .to_string()
            .contains("did not find expected node content"));
    }

    #[test]
    fn test_missing_required_field() {
        let yaml = r#"version: v1
"#;
        let err = CanonicalConfig::from_yaml(yaml).unwrap_err();
        assert!(err.to_string().contains("missing field `tools`"));
    }

    #[test]
    fn test_default_mcp_templates_ids_stable() {
        let ids: Vec<_> = default_mcp_templates()
            .iter()
            .map(|template| template.id.clone())
            .collect();

        assert_eq!(
            ids,
            vec![
                "brave-search".to_string(),
                "github-issues".to_string(),
                "local-notes".to_string()
            ]
        );
    }

    #[test]
    fn test_duplicate_mcp_template_ids_rejected() {
        let mut config = CanonicalConfig::default();
        let duplicate_id = config.mcp_templates[0].id.clone();
        config.mcp_templates.push(McpTemplateDefinition {
            id: duplicate_id.clone(),
            title: "Duplicate Entry".to_string(),
            description: "Another template using the same ID for testing.".to_string(),
            command: "echo".to_string(),
            args: vec!["test".to_string()],
            env_placeholders: vec![],
            auth_notes: None,
        });

        let err = config.validate().unwrap_err();
        assert!(err
            .to_string()
            .contains(&format!("Duplicate MCP template ID: {}", duplicate_id)));
    }

    #[test]
    fn test_load_config_errors() {
        use std::fs;

        let temp_dir = std::env::temp_dir().join(format!("macc_config_test_{}", uuid_v4_like()));
        fs::create_dir_all(&temp_dir).unwrap();

        // 1. Invalid YAML syntax
        let path = temp_dir.join("invalid_syntax.yaml");
        fs::write(&path, "tools:\n  enabled: [").unwrap();
        let err = load_canonical_config(&path).unwrap_err();
        let msg = err.to_string();
        assert!(msg.contains("Configuration error in"));
        assert!(msg.contains("invalid_syntax.yaml"));
        assert!(msg.contains("did not find expected node content"));

        // 2. Unknown field
        let path = temp_dir.join("unknown_field.yaml");
        fs::write(&path, "tools:\n  enabled: []\nextra: true").unwrap();
        let err = load_canonical_config(&path).unwrap_err();
        let msg = err.to_string();
        assert!(msg.contains("unknown_field.yaml"));
        assert!(msg.contains("unknown field `extra`"));

        // 3. Missing required field
        let path = temp_dir.join("missing_field.yaml");
        fs::write(&path, "version: v1").unwrap();
        let err = load_canonical_config(&path).unwrap_err();
        let msg = err.to_string();
        assert!(msg.contains("missing_field.yaml"));
        assert!(msg.contains("missing field `tools`"));

        // 4. Missing sub-field
        let path = temp_dir.join("missing_subfield.yaml");
        fs::write(&path, "tools: {}").unwrap();
        let err = load_canonical_config(&path).unwrap_err();
        let msg = err.to_string();
        assert!(msg.contains("missing_subfield.yaml"));
        assert!(msg.contains("missing field `enabled`"));

        fs::remove_dir_all(&temp_dir).ok();
    }

    fn uuid_v4_like() -> String {
        use std::time::{SystemTime, UNIX_EPOCH};
        let start = SystemTime::now();
        let since_the_epoch = start
            .duration_since(UNIX_EPOCH)
            .expect("Time went backwards");
        format!("{:?}", since_the_epoch.as_nanos())
    }

    // ── CoordinatorConfigResolved tests ──────────────────────────────────────

    #[test]
    fn test_coordinator_config_resolved_none_produces_defaults() {
        let r = CoordinatorConfigResolved::resolve(None);

        // Identity
        assert!(r.coordinator_tool.is_none());
        assert!(r.prd_file.is_none());
        assert!(r.task_registry_file.is_none());

        // Dispatch / parallelism
        assert_eq!(r.reference_branch, "master");
        assert!(r.tool_priority.is_empty());
        assert!(r.max_parallel_per_tool.is_empty());
        assert!(r.tool_specializations.is_empty());
        assert_eq!(r.max_dispatch, 10);
        assert_eq!(r.max_parallel, 3);
        assert_eq!(r.timeout_seconds, 0);
        assert_eq!(r.phase_runner_max_attempts, 1);
        assert_eq!(r.max_dispatch_retries, 5);

        // Logging
        assert_eq!(r.log_flush_lines, 500);
        assert_eq!(r.log_flush_ms, 60_000);
        assert!(r.mirror_json_debounce_ms.is_none());

        // Staleness / health
        assert_eq!(r.stale_claimed_seconds, 0);
        assert_eq!(r.stale_in_progress_seconds, 0);
        assert_eq!(r.stale_changes_requested_seconds, 0);
        assert_eq!(r.stale_action, "block");
        assert_eq!(r.ghost_heartbeat_grace_seconds, 30);

        // Storage
        assert_eq!(r.storage_mode, "sqlite");
        assert!(!r.json_compat);
        assert!(!r.legacy_json_fallback);

        // Merging
        assert!(!r.merge_ai_fix);
        assert_eq!(r.merge_job_timeout_seconds, 0);
        assert_eq!(r.merge_hook_timeout_seconds, 90);

        // Scheduling
        assert_eq!(r.dispatch_cooldown_seconds, 2);
        assert_eq!(r.session_cache_ttl_seconds, 300);

        // Error-code retry
        assert_eq!(
            r.error_code_retry_list,
            "E101,E102,E103,E301,E302,E303,E601,E603"
        );
        assert_eq!(r.error_code_retry_max, 2);

        // Rate-limit
        assert_eq!(r.rate_limit_backoff_base_seconds, 30);
        assert_eq!(r.rate_limit_backoff_max_seconds, 300);
        assert!(r.rate_limit_fallback_enabled);
        assert!(r.rate_limit_throttle_parallel);

        // Cutover gate
        assert_eq!(r.cutover_gate_window_events, 2000);
        assert!((r.cutover_gate_max_blocked_ratio - 0.25).abs() < f64::EPSILON);
        assert!((r.cutover_gate_max_stale_ratio - 0.25).abs() < f64::EPSILON);

        // Process lifecycle
        assert_eq!(r.force_kill_grace_seconds, 30);
        assert!(r.max_review_cycles.is_none());

        // Reliability toggles
        assert!(r.salvage_before_retry);
        assert!(r.retry_on_same_worktree);
        assert!(r.merge_gate_on_dispatch);
        assert!(r.remove_worktree_on_sanitize_failure);
        assert!(r.tag_abandoned_branches);
        assert!(r.sync_unmerged_branches);
        assert_eq!(r.salvage_merge_timeout_seconds, 120);
        assert_eq!(r.max_salvage_attempts_per_task, 1);
    }

    #[test]
    fn test_coordinator_config_resolved_overrides_provided_fields() {
        let mut cfg = CoordinatorConfig::default();
        cfg.coordinator_tool = Some("my-tool".to_string());
        cfg.reference_branch = Some("develop".to_string());
        cfg.max_dispatch = Some(5);
        cfg.max_parallel = Some(2);
        cfg.phase_runner_max_attempts = Some(3);
        cfg.log_flush_lines = Some(100);
        cfg.log_flush_ms = Some(5_000);
        cfg.mirror_json_debounce_ms = Some(250);
        cfg.stale_claimed_seconds = Some(600);
        cfg.stale_in_progress_seconds = Some(1200);
        cfg.stale_changes_requested_seconds = Some(1800);
        cfg.stale_action = Some("retry".to_string());
        cfg.ghost_heartbeat_grace_seconds = Some(60);
        cfg.storage_mode = Some("json".to_string());
        cfg.json_compat = Some(true);
        cfg.legacy_json_fallback = Some(true);
        cfg.merge_ai_fix = Some(true);
        cfg.merge_job_timeout_seconds = Some(120);
        cfg.merge_hook_timeout_seconds = Some(45);
        cfg.dispatch_cooldown_seconds = Some(5);
        cfg.max_dispatch_retries = Some(7);
        cfg.session_cache_ttl_seconds = Some(600);
        cfg.error_code_retry_list = Some("E601".to_string());
        cfg.error_code_retry_max = Some(5);
        cfg.rate_limit_backoff_base_seconds = Some(60);
        cfg.rate_limit_backoff_max_seconds = Some(3600);
        cfg.rate_limit_fallback_enabled = Some(false);
        cfg.rate_limit_throttle_parallel = Some(false);
        cfg.cutover_gate_window_events = Some(500);
        cfg.cutover_gate_max_blocked_ratio = Some(0.5);
        cfg.cutover_gate_max_stale_ratio = Some(0.1);
        cfg.force_kill_grace_seconds = Some(10);
        cfg.max_review_cycles = Some(2);
        cfg.salvage_before_retry = false;
        cfg.retry_on_same_worktree = false;
        cfg.merge_gate_on_dispatch = false;
        cfg.remove_worktree_on_sanitize_failure = false;
        cfg.tag_abandoned_branches = false;
        cfg.sync_unmerged_branches = false;
        cfg.salvage_merge_timeout_seconds = 60;
        cfg.max_salvage_attempts_per_task = 3;

        let r = CoordinatorConfigResolved::resolve(Some(&cfg));

        assert_eq!(r.coordinator_tool.as_deref(), Some("my-tool"));
        assert_eq!(r.reference_branch, "develop");
        assert_eq!(r.max_dispatch, 5);
        assert_eq!(r.max_parallel, 2);
        assert_eq!(r.phase_runner_max_attempts, 3);
        assert_eq!(r.log_flush_lines, 100);
        assert_eq!(r.log_flush_ms, 5_000);
        assert_eq!(r.mirror_json_debounce_ms, Some(250));
        assert_eq!(r.stale_claimed_seconds, 600);
        assert_eq!(r.stale_in_progress_seconds, 1200);
        assert_eq!(r.stale_changes_requested_seconds, 1800);
        assert_eq!(r.stale_action, "retry");
        assert_eq!(r.ghost_heartbeat_grace_seconds, 60);
        assert_eq!(r.storage_mode, "json");
        assert!(r.json_compat);
        assert!(r.legacy_json_fallback);
        assert!(r.merge_ai_fix);
        assert_eq!(r.merge_job_timeout_seconds, 120);
        assert_eq!(r.merge_hook_timeout_seconds, 45);
        assert_eq!(r.dispatch_cooldown_seconds, 5);
        assert_eq!(r.max_dispatch_retries, 7);
        assert_eq!(r.session_cache_ttl_seconds, 600);
        assert_eq!(r.error_code_retry_list, "E601");
        assert_eq!(r.error_code_retry_max, 5);
        assert_eq!(r.rate_limit_backoff_base_seconds, 60);
        assert_eq!(r.rate_limit_backoff_max_seconds, 3600);
        assert!(!r.rate_limit_fallback_enabled);
        assert!(!r.rate_limit_throttle_parallel);
        assert_eq!(r.cutover_gate_window_events, 500);
        assert!((r.cutover_gate_max_blocked_ratio - 0.5).abs() < f64::EPSILON);
        assert!((r.cutover_gate_max_stale_ratio - 0.1).abs() < f64::EPSILON);
        assert_eq!(r.force_kill_grace_seconds, 10);
        assert_eq!(r.max_review_cycles, Some(2));
        assert!(!r.salvage_before_retry);
        assert!(!r.retry_on_same_worktree);
        assert!(!r.merge_gate_on_dispatch);
        assert!(!r.remove_worktree_on_sanitize_failure);
        assert!(!r.tag_abandoned_branches);
        assert!(!r.sync_unmerged_branches);
        assert_eq!(r.salvage_merge_timeout_seconds, 60);
        assert_eq!(r.max_salvage_attempts_per_task, 3);
    }

    #[test]
    fn test_coordinator_config_resolved_partial_override_uses_default_for_rest() {
        // Only override a few fields; the rest must remain at their defaults.
        let mut cfg = CoordinatorConfig::default();
        cfg.max_dispatch = Some(99);
        cfg.stale_action = Some("requeue".to_string());

        let r = CoordinatorConfigResolved::resolve(Some(&cfg));

        assert_eq!(r.max_dispatch, 99);
        assert_eq!(r.stale_action, "requeue");
        // Unset fields fall back to defaults.
        assert_eq!(r.max_parallel, 3);
        assert_eq!(r.reference_branch, "master");
        assert_eq!(r.dispatch_cooldown_seconds, 2);
        assert_eq!(r.error_code_retry_max, 2);
    }

    #[test]
    fn test_coordinator_config_resolved_phase_runner_min_one() {
        // phase_runner_max_attempts is clamped to at least 1.
        let mut cfg = CoordinatorConfig::default();
        cfg.phase_runner_max_attempts = Some(0);
        let r = CoordinatorConfigResolved::resolve(Some(&cfg));
        assert_eq!(r.phase_runner_max_attempts, 1);
    }
}
