use macc_core::config::WebAssetsMode;
use macc_core::plan::{PlannedOpKind, Scope};
use macc_core::tool::spec::{CheckSeverity, DoctorCheckKind};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::BTreeMap;

/// Shared API DTOs for the local web UI.
///
/// All success payloads in this module use camelCase JSON fields.
/// All failures for the related endpoints must use the existing web error
/// envelope defined in `cli/src/commands/web/errors.rs`.

/// Canonical configuration payload returned by config endpoints.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ApiConfigResponse {
    /// Optional schema or config version from the canonical file.
    pub version: Option<String>,
    /// Enabled tool IDs in canonical order.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub enabled_tools: Vec<String>,
    /// Tool-specific configuration values keyed by tool ID.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub tool_config: BTreeMap<String, Value>,
    /// Legacy flattened tool settings preserved for compatibility.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub tool_settings: BTreeMap<String, Value>,
    /// Optional external standards file path.
    pub standards_path: Option<String>,
    /// Inline standards values keyed by field name.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub standards_inline: BTreeMap<String, String>,
    /// Selected skill IDs enabled for the project.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub selected_skills: Vec<String>,
    /// Selected agent IDs enabled for the project.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub selected_agents: Vec<String>,
    /// Selected MCP IDs enabled for the project.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub selected_mcp: Vec<String>,
    /// Whether normal CLI output should be reduced.
    pub quiet: bool,
    /// Whether network-backed operations should avoid remote access.
    pub offline: bool,
    /// Configured web server port.
    pub web_port: Option<u16>,
    /// Asset serving mode for the local web UI.
    pub web_assets: Option<WebAssetsMode>,
    /// Whether the Ralph automation flow is enabled.
    pub ralph_enabled: Option<bool>,
    /// Default Ralph iteration count.
    pub ralph_iterations_default: Option<usize>,
    /// Default Ralph branch name.
    pub ralph_branch_name: Option<String>,
    /// Whether Ralph stops on the first failure.
    pub ralph_stop_on_failure: Option<bool>,
    /// Coordinator tool override.
    pub coordinator_tool: Option<String>,
    /// Reference branch used by the coordinator.
    pub reference_branch: Option<String>,
    /// PRD source file path.
    pub prd_file: Option<String>,
    /// Legacy registry file path from canonical config.
    pub task_registry_file: Option<String>,
    /// Tool dispatch preference order.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tool_priority: Vec<String>,
    /// Maximum parallelism overrides by tool ID.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub max_parallel_per_tool: BTreeMap<String, usize>,
    /// Task category specializations per tool.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub tool_specializations: BTreeMap<String, Vec<String>>,
    /// Global dispatch cap per coordinator cycle.
    pub max_dispatch: Option<usize>,
    /// Global maximum parallel tasks.
    pub max_parallel: Option<usize>,
    /// Coordinator timeout in seconds.
    pub timeout_seconds: Option<usize>,
    /// Maximum attempts per phase runner.
    pub phase_runner_max_attempts: Option<usize>,
    /// Log flush threshold measured in lines.
    pub log_flush_lines: Option<usize>,
    /// Log flush threshold measured in milliseconds.
    pub log_flush_ms: Option<u64>,
    /// JSON mirror debounce interval in milliseconds.
    pub mirror_json_debounce_ms: Option<u64>,
    /// Claimed-task staleness threshold in seconds.
    pub stale_claimed_seconds: Option<usize>,
    /// In-progress staleness threshold in seconds.
    pub stale_in_progress_seconds: Option<usize>,
    /// Changes-requested staleness threshold in seconds.
    pub stale_changes_requested_seconds: Option<usize>,
    /// Action to take when stale work is detected.
    pub stale_action: Option<String>,
    /// Coordinator storage backend mode.
    pub storage_mode: Option<String>,
    /// Whether AI-assisted merge recovery is enabled.
    pub merge_ai_fix: Option<bool>,
    /// Merge job timeout in seconds.
    pub merge_job_timeout_seconds: Option<usize>,
    /// Merge hook timeout in milliseconds.
    pub merge_hook_timeout_seconds: Option<u64>,
    /// Grace period before ghost heartbeats are considered stale.
    pub ghost_heartbeat_grace_seconds: Option<i64>,
    /// Dispatch cooldown interval in seconds.
    pub dispatch_cooldown_seconds: Option<u64>,
    /// Whether JSON compatibility mode is enabled.
    pub json_compat: Option<bool>,
    /// Whether legacy JSON fallback is enabled.
    pub legacy_json_fallback: Option<bool>,
    /// Retry list for normalized error codes.
    pub error_code_retry_list: Option<String>,
    /// Maximum retries for configured error codes.
    pub error_code_retry_max: Option<usize>,
    /// Number of events in the cutover gate window.
    pub cutover_gate_window_events: Option<usize>,
    /// Maximum blocked ratio allowed during cutover.
    pub cutover_gate_max_blocked_ratio: Option<f64>,
    /// Maximum stale ratio allowed during cutover.
    pub cutover_gate_max_stale_ratio: Option<f64>,
    /// Base rate-limit backoff interval in seconds.
    pub rate_limit_backoff_base_seconds: Option<u64>,
    /// Maximum rate-limit backoff interval in seconds.
    pub rate_limit_backoff_max_seconds: Option<u64>,
    /// Whether fallback behavior is enabled after rate limiting.
    pub rate_limit_fallback_enabled: Option<bool>,
    /// Whether rate limiting can reduce effective parallelism.
    pub rate_limit_throttle_parallel: Option<bool>,
    /// Whether managed environment constraints were detected.
    pub requirements_detected: bool,
    /// Managed-environment warnings surfaced to the UI.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub managed_environment_warnings: Vec<String>,
}

/// Partial config update payload accepted by config write endpoints.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ApiConfigUpdateRequest {
    /// Optional schema or config version to persist.
    pub version: Option<String>,
    /// Replacement enabled tool ID list.
    pub enabled_tools: Option<Vec<String>>,
    /// Partial tool-specific configuration keyed by tool ID.
    pub tool_config: Option<BTreeMap<String, Value>>,
    /// Partial legacy flattened tool settings keyed by field name.
    pub tool_settings: Option<BTreeMap<String, Value>>,
    /// New external standards file path.
    pub standards_path: Option<String>,
    /// Replacement inline standards values.
    pub standards_inline: Option<BTreeMap<String, String>>,
    /// Replacement selected skill IDs.
    pub selected_skills: Option<Vec<String>>,
    /// Replacement selected agent IDs.
    pub selected_agents: Option<Vec<String>>,
    /// Replacement selected MCP IDs.
    pub selected_mcp: Option<Vec<String>>,
    /// Updated quiet-mode flag.
    pub quiet: Option<bool>,
    /// Updated offline-mode flag.
    pub offline: Option<bool>,
    /// Updated web server port.
    pub web_port: Option<u16>,
    /// Updated web assets mode.
    pub web_assets: Option<WebAssetsMode>,
    /// Updated Ralph enabled flag.
    pub ralph_enabled: Option<bool>,
    /// Updated Ralph default iteration count.
    pub ralph_iterations_default: Option<usize>,
    /// Updated Ralph branch name.
    pub ralph_branch_name: Option<String>,
    /// Updated Ralph stop-on-failure flag.
    pub ralph_stop_on_failure: Option<bool>,
    /// Updated coordinator tool override.
    pub coordinator_tool: Option<String>,
    /// Updated reference branch.
    pub reference_branch: Option<String>,
    /// Updated PRD file path.
    pub prd_file: Option<String>,
    /// Updated legacy registry file path.
    pub task_registry_file: Option<String>,
    /// Replacement tool priority list.
    pub tool_priority: Option<Vec<String>>,
    /// Replacement max-parallel overrides per tool.
    pub max_parallel_per_tool: Option<BTreeMap<String, usize>>,
    /// Replacement tool specializations.
    pub tool_specializations: Option<BTreeMap<String, Vec<String>>>,
    /// Updated dispatch cap.
    pub max_dispatch: Option<usize>,
    /// Updated max parallel value.
    pub max_parallel: Option<usize>,
    /// Updated timeout in seconds.
    pub timeout_seconds: Option<usize>,
    /// Updated per-phase attempt cap.
    pub phase_runner_max_attempts: Option<usize>,
    /// Updated log flush line threshold.
    pub log_flush_lines: Option<usize>,
    /// Updated log flush millisecond threshold.
    pub log_flush_ms: Option<u64>,
    /// Updated JSON debounce interval.
    pub mirror_json_debounce_ms: Option<u64>,
    /// Updated claimed-task stale threshold.
    pub stale_claimed_seconds: Option<usize>,
    /// Updated in-progress stale threshold.
    pub stale_in_progress_seconds: Option<usize>,
    /// Updated changes-requested stale threshold.
    pub stale_changes_requested_seconds: Option<usize>,
    /// Updated stale-action behavior.
    pub stale_action: Option<String>,
    /// Updated storage backend mode.
    pub storage_mode: Option<String>,
    /// Updated AI merge-fix flag.
    pub merge_ai_fix: Option<bool>,
    /// Updated merge job timeout.
    pub merge_job_timeout_seconds: Option<usize>,
    /// Updated merge hook timeout.
    pub merge_hook_timeout_seconds: Option<u64>,
    /// Updated ghost heartbeat grace period.
    pub ghost_heartbeat_grace_seconds: Option<i64>,
    /// Updated dispatch cooldown interval.
    pub dispatch_cooldown_seconds: Option<u64>,
    /// Updated JSON compatibility flag.
    pub json_compat: Option<bool>,
    /// Updated legacy JSON fallback flag.
    pub legacy_json_fallback: Option<bool>,
    /// Updated retry code allowlist.
    pub error_code_retry_list: Option<String>,
    /// Updated retry count limit.
    pub error_code_retry_max: Option<usize>,
    /// Updated cutover event window size.
    pub cutover_gate_window_events: Option<usize>,
    /// Updated cutover blocked ratio cap.
    pub cutover_gate_max_blocked_ratio: Option<f64>,
    /// Updated cutover stale ratio cap.
    pub cutover_gate_max_stale_ratio: Option<f64>,
    /// Updated rate-limit backoff base interval.
    pub rate_limit_backoff_base_seconds: Option<u64>,
    /// Updated rate-limit backoff max interval.
    pub rate_limit_backoff_max_seconds: Option<u64>,
    /// Updated rate-limit fallback flag.
    pub rate_limit_fallback_enabled: Option<bool>,
    /// Updated rate-limit throttle-parallel flag.
    pub rate_limit_throttle_parallel: Option<bool>,
}

/// PRD payload returned to the web UI.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ApiPrdResponse {
    /// PRD tasks in display order.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tasks: Vec<ApiPrdTask>,
    /// Additional PRD-level metadata preserved from the source document.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub metadata: BTreeMap<String, Value>,
}

/// A single PRD task as exposed by the web API.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ApiPrdTask {
    /// Stable PRD task identifier.
    pub id: String,
    /// Human-readable task title.
    pub title: Option<String>,
    /// Priority label from the PRD.
    pub priority: Option<String>,
    /// Category label from the PRD.
    pub category: Option<String>,
    /// Scope label from the PRD.
    pub scope: Option<String>,
    /// Base branch requested for the task.
    pub base_branch: Option<String>,
    /// Coordinator tool override requested by the PRD.
    pub coordinator_tool: Option<String>,
    /// Task description text.
    pub description: Option<String>,
    /// Objective text for the task.
    pub objective: Option<String>,
    /// Expected result artifact or outcome.
    pub result: Option<String>,
    /// Upstream task IDs that must complete first.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub dependencies: Vec<String>,
    /// Resources that must not overlap with sibling tasks.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub exclusive_resources: Vec<String>,
    /// Ordered implementation steps for the task.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub steps: Vec<String>,
    /// Free-form notes attached to the task.
    pub notes: Option<String>,
    /// Additional task-level metadata preserved from the PRD.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub metadata: BTreeMap<String, Value>,
}

/// PRD update payload accepted by write endpoints.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ApiPrdUpdateRequest {
    /// Replacement task list.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tasks: Vec<ApiPrdTask>,
    /// PRD-level metadata to persist alongside the tasks.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub metadata: BTreeMap<String, Value>,
}

/// Plan request accepted by preview endpoints.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ApiPlanRequest {
    /// Target scope for planned operations.
    pub scope: Option<Scope>,
    /// Tool IDs to include in planning.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tools: Vec<String>,
    /// Whether user-scope operations are allowed into the plan.
    pub allow_user_scope: Option<bool>,
    /// Whether file diffs should be included in the response.
    pub include_diff: Option<bool>,
    /// Whether human-readable explanations should be generated.
    pub explain: Option<bool>,
}

/// Plan preview returned by the web API.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ApiPlanResponse {
    /// Aggregate statistics about the proposed plan.
    pub summary: ApiPlanSummary,
    /// File-level operations derived from the action plan.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub files: Vec<ApiPlanFile>,
    /// Diff payloads for previewable operations.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub diffs: Vec<ApiPlanDiff>,
    /// Risks called out to the UI before apply.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub risks: Vec<String>,
    /// Explicit consent prompts required by the plan.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub consents: Vec<ApiPlanConsent>,
}

/// Aggregate metadata for a plan preview.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ApiPlanSummary {
    /// Total number of low-level actions in the plan.
    pub total_actions: usize,
    /// Number of write operations in the preview.
    pub files_write: usize,
    /// Number of merge operations in the preview.
    pub files_merge: usize,
    /// Number of operations gated by user consent.
    pub consent_required: usize,
    /// Number of operations that trigger backups.
    pub backup_required: usize,
    /// Backup root that would be used on apply.
    pub backup_path: String,
}

/// A single file operation shown in the plan preview.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ApiPlanFile {
    /// Relative path targeted by the operation.
    pub path: String,
    /// Operation kind for the file.
    pub kind: PlannedOpKind,
    /// Scope of the operation.
    pub scope: Scope,
    /// Whether user approval is required for the operation.
    pub consent_required: bool,
    /// Whether a backup should be created before mutation.
    pub backup_required: bool,
    /// Whether executable bits should be set.
    pub set_executable: bool,
    /// Optional human-readable explanation for the operation.
    pub explain: Option<String>,
}

/// A single diff payload attached to a plan preview.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ApiPlanDiff {
    /// Relative path for the diff.
    pub path: String,
    /// Rendering mode for the diff payload.
    pub diff_kind: String,
    /// Unified or structured diff text when available.
    pub diff: Option<String>,
    /// Whether the diff was truncated before serialization.
    pub diff_truncated: bool,
}

/// Consent prompt generated by a plan preview.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ApiPlanConsent {
    /// Stable consent ID for UI tracking.
    pub id: String,
    /// Scope affected by the consent.
    pub scope: Scope,
    /// Human-readable consent message.
    pub message: String,
    /// Paths covered by the consent request.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub paths: Vec<String>,
}

/// Apply request accepted by execution endpoints.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ApiApplyRequest {
    /// Target scope for apply execution.
    pub scope: Option<Scope>,
    /// Tool IDs to include in apply execution.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tools: Vec<String>,
    /// Whether user-scope operations are allowed.
    pub allow_user_scope: Option<bool>,
    /// Whether the endpoint should only simulate writes.
    pub dry_run: bool,
    /// Whether prompts should be auto-confirmed.
    pub yes: Option<bool>,
}

/// Apply result returned by execution endpoints.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ApiApplyResponse {
    /// Whether the request completed as a dry run.
    pub dry_run: bool,
    /// Number of actions applied or simulated.
    pub applied_actions: usize,
    /// Number of files written or merged.
    pub changed_files: usize,
    /// Backups created or referenced by the apply workflow.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub backup_locations: Vec<String>,
    /// Per-file execution results.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub results: Vec<ApiApplyResult>,
    /// Non-fatal warnings surfaced during apply.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub warnings: Vec<String>,
}

/// Per-file result item from an apply operation.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ApiApplyResult {
    /// Relative path affected by the operation.
    pub path: String,
    /// Operation kind performed for the path.
    pub kind: PlannedOpKind,
    /// Whether the operation completed successfully.
    pub success: bool,
    /// Human-readable status message for the operation.
    pub message: Option<String>,
    /// Backup location created for the operation, when any.
    pub backup_location: Option<String>,
}

/// Worktree summary returned by worktree endpoints.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ApiWorktree {
    /// Stable worktree identifier.
    pub id: String,
    /// User-facing slug for the worktree family.
    pub slug: Option<String>,
    /// Branch checked out in the worktree.
    pub branch: Option<String>,
    /// Tool assigned to the worktree.
    pub tool: Option<String>,
    /// Derived worktree status shown in the UI.
    pub status: Option<String>,
    /// Absolute worktree path.
    pub path: String,
    /// Base branch used to create the worktree.
    pub base_branch: Option<String>,
    /// Current HEAD commit if known.
    pub head: Option<String>,
    /// Requested scope recorded in metadata.
    pub scope: Option<String>,
    /// Optional feature label recorded in metadata.
    pub feature: Option<String>,
    /// Whether Git marks the worktree as locked.
    pub locked: bool,
    /// Whether Git marks the worktree as prunable.
    pub prunable: bool,
    /// Optional session label associated with the worktree.
    pub session_label: Option<String>,
}

/// Worktree creation payload accepted by the web API.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ApiWorktreeCreateRequest {
    /// Slug used to derive worktree IDs.
    pub slug: String,
    /// Tool assigned to created worktrees.
    pub tool: String,
    /// Number of worktrees to create.
    pub count: usize,
    /// Base branch used for new worktrees.
    pub base: String,
    /// Optional scope written into worktree metadata.
    pub scope: Option<String>,
    /// Optional feature label stored in metadata.
    pub feature: Option<String>,
    /// Whether initial apply should be skipped.
    pub skip_apply: Option<bool>,
    /// Whether user-scope apply operations are allowed.
    pub allow_user_scope: Option<bool>,
}

/// Registry task payload returned by registry endpoints.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ApiRegistryTask {
    /// Stable task identifier.
    pub id: String,
    /// Human-readable task title.
    pub title: Option<String>,
    /// Priority preserved from the task registry.
    pub priority: Option<String>,
    /// Current task state from the registry.
    pub state: String,
    /// Assigned tool ID.
    pub tool: Option<String>,
    /// Current attempt count.
    pub attempts: Option<i64>,
    /// Most recent heartbeat timestamp.
    pub heartbeat: Option<String>,
    /// Backoff timestamp before redispatch is allowed.
    pub delayed_until: Option<String>,
    /// Current workflow phase, if known.
    pub current_phase: Option<String>,
    /// Latest task error summary.
    pub last_error: Option<String>,
    /// Latest normalized task error code.
    pub last_error_code: Option<String>,
    /// Assignee payload preserved from the registry.
    pub assignee: Option<Value>,
    /// Worktree metadata attached to the task.
    pub worktree: Option<ApiRegistryTaskWorktree>,
    /// Related coordinator events for UI timelines.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub events: Vec<ApiRegistryEvent>,
    /// Task update timestamp from the registry.
    pub updated_at: Option<String>,
}

/// Worktree details embedded in registry tasks.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ApiRegistryTaskWorktree {
    /// Absolute or project-relative worktree path.
    pub worktree_path: Option<String>,
    /// Active branch name.
    pub branch: Option<String>,
    /// Base branch name.
    pub base_branch: Option<String>,
    /// Last recorded commit SHA.
    pub last_commit: Option<String>,
    /// Session ID for the performer, when present.
    pub session_id: Option<String>,
}

/// Coordinator event excerpt attached to a registry task.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ApiRegistryEvent {
    /// Stable event identifier.
    pub event_id: Option<String>,
    /// Event type name.
    pub event_type: String,
    /// Event timestamp.
    pub ts: Option<String>,
    /// Event status field.
    pub status: Option<String>,
    /// Event severity field.
    pub severity: Option<String>,
    /// Human-readable event message.
    pub message: Option<String>,
}

/// Task action request envelope accepted by registry mutation endpoints.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ApiRegistryTaskActionRequest {
    /// Task identifier to mutate.
    pub task_id: String,
    /// Requested registry action.
    pub action: ApiRegistryTaskAction,
}

/// Supported registry task actions exposed by the web API.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase", tag = "kind")]
pub(crate) enum ApiRegistryTaskAction {
    /// Requeue a task back to the dispatchable state.
    Requeue {
        /// Human-readable justification recorded for the action.
        justification: Option<String>,
    },
    /// Reassign a task to a different tool.
    Reassign {
        /// Target tool ID for reassignment.
        tool: String,
        /// Human-readable justification recorded for the action.
        justification: String,
    },
}

/// Log file metadata returned by log listing endpoints.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ApiLogFile {
    /// Log path relative to the logical log root.
    pub path: String,
    /// File size in bytes.
    pub size: u64,
    /// Last modified timestamp in RFC 3339 format when available.
    pub modified: Option<String>,
}

/// Log content payload returned by log read endpoints.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ApiLogContent {
    /// Log path that was read.
    pub path: String,
    /// Selected log lines in display order.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub lines: Vec<String>,
    /// Total number of lines available in the source file.
    pub total: usize,
}

/// Doctor report returned by health endpoints.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ApiDoctorReport {
    /// Aggregate project health score from 0 to 100.
    pub health_score: u8,
    /// Counts of issues grouped by severity.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub issues_by_severity: BTreeMap<String, usize>,
    /// Individual issues returned by doctor checks.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub issues: Vec<ApiDoctorIssue>,
}

/// Single doctor issue item surfaced to the web UI.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ApiDoctorIssue {
    /// Human-readable check name.
    pub name: String,
    /// Tool ID associated with the check when available.
    pub tool_id: Option<String>,
    /// Check target such as a binary name or path.
    pub target: String,
    /// Severity assigned to the issue.
    pub severity: CheckSeverity,
    /// Doctor check kind that produced the issue.
    pub kind: DoctorCheckKind,
    /// Result status rendered for the UI.
    pub status: String,
    /// Optional detailed message for errors.
    pub message: Option<String>,
}

/// Backup set metadata returned by backup endpoints.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ApiBackup {
    /// Backup set identifier, typically the directory name.
    pub id: String,
    /// Timestamp string used for display and sorting.
    pub timestamp: String,
    /// Number of files contained in the backup set.
    pub files: usize,
    /// Absolute path to the backup set directory.
    pub path: String,
    /// Whether the backup lives in the user-scoped backup root.
    pub user_scope: bool,
}

/// Restore request accepted by backup restore endpoints.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ApiRestoreRequest {
    /// Explicit backup set identifier to restore.
    pub backup_id: Option<String>,
    /// Whether the latest backup set should be selected automatically.
    pub latest: bool,
    /// Whether the user-scoped backup root should be used.
    pub user: bool,
    /// Whether the restore should be simulated without writes.
    pub dry_run: bool,
    /// Whether restore confirmation prompts should be bypassed.
    pub yes: Option<bool>,
}
