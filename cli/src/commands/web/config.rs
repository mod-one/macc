use super::errors::ApiError;
use super::types::{ApiConfigResponse, ApiConfigUpdateRequest};
use super::WebState;
use axum::body::Bytes;
use axum::extract::State;
use axum::Json;
use macc_core::config::{CanonicalConfig, CoordinatorConfig, RalphConfig};

pub(super) async fn get_config_handler(
    State(state): State<WebState>,
) -> std::result::Result<Json<ApiConfigResponse>, ApiError> {
    let canonical = state
        .engine
        .load_canonical_config(&state.paths)
        .map_err(ApiError::from)?;
    Ok(Json(ApiConfigResponse::from(canonical)))
}

pub(super) async fn update_config_handler(
    State(state): State<WebState>,
    body: Bytes,
) -> std::result::Result<Json<ApiConfigResponse>, ApiError> {
    let request: ApiConfigUpdateRequest = serde_json::from_slice(&body).map_err(|err| {
        ApiError::from(macc_core::MaccError::Validation(format!(
            "Invalid config request body: {}",
            err
        )))
    })?;
    let mut canonical = state
        .engine
        .load_canonical_config(&state.paths)
        .map_err(ApiError::from)?;
    apply_update(&mut canonical, &request);
    state
        .engine
        .save_canonical_config(&state.paths, &canonical)
        .map_err(ApiError::from)?;
    Ok(Json(ApiConfigResponse::from(canonical)))
}

impl From<CanonicalConfig> for ApiConfigResponse {
    fn from(config: CanonicalConfig) -> Self {
        let selections = config.selections.unwrap_or_default();
        let ralph = config.automation.ralph;
        let coordinator = config.automation.coordinator;
        Self {
            version: config.version,
            enabled_tools: config.tools.enabled,
            tool_config: config.tools.config,
            tool_settings: config.tools.settings,
            standards_path: config.standards.path,
            standards_inline: config.standards.inline,
            selected_skills: selections.skills,
            selected_agents: selections.agents,
            selected_mcp: selections.mcp,
            quiet: config.settings.quiet,
            offline: config.settings.offline,
            web_port: config.settings.web_port,
            web_assets: config.settings.web_assets,
            ralph_enabled: ralph.as_ref().map(|cfg| cfg.enabled),
            ralph_iterations_default: ralph.as_ref().map(|cfg| cfg.iterations_default),
            ralph_branch_name: ralph.as_ref().map(|cfg| cfg.branch_name.clone()),
            ralph_stop_on_failure: ralph.as_ref().map(|cfg| cfg.stop_on_failure),
            coordinator_tool: coordinator
                .as_ref()
                .and_then(|cfg| cfg.coordinator_tool.clone()),
            reference_branch: coordinator
                .as_ref()
                .and_then(|cfg| cfg.reference_branch.clone()),
            prd_file: coordinator.as_ref().and_then(|cfg| cfg.prd_file.clone()),
            task_registry_file: coordinator
                .as_ref()
                .and_then(|cfg| cfg.task_registry_file.clone()),
            tool_priority: coordinator
                .as_ref()
                .map(|cfg| cfg.tool_priority.clone())
                .unwrap_or_default(),
            max_parallel_per_tool: coordinator
                .as_ref()
                .map(|cfg| cfg.max_parallel_per_tool.clone())
                .unwrap_or_default(),
            tool_specializations: coordinator
                .as_ref()
                .map(|cfg| cfg.tool_specializations.clone())
                .unwrap_or_default(),
            max_dispatch: coordinator.as_ref().and_then(|cfg| cfg.max_dispatch),
            max_parallel: coordinator.as_ref().and_then(|cfg| cfg.max_parallel),
            timeout_seconds: coordinator.as_ref().and_then(|cfg| cfg.timeout_seconds),
            phase_runner_max_attempts: coordinator
                .as_ref()
                .and_then(|cfg| cfg.phase_runner_max_attempts),
            log_flush_lines: coordinator.as_ref().and_then(|cfg| cfg.log_flush_lines),
            log_flush_ms: coordinator.as_ref().and_then(|cfg| cfg.log_flush_ms),
            mirror_json_debounce_ms: coordinator
                .as_ref()
                .and_then(|cfg| cfg.mirror_json_debounce_ms),
            stale_claimed_seconds: coordinator
                .as_ref()
                .and_then(|cfg| cfg.stale_claimed_seconds),
            stale_in_progress_seconds: coordinator
                .as_ref()
                .and_then(|cfg| cfg.stale_in_progress_seconds),
            stale_changes_requested_seconds: coordinator
                .as_ref()
                .and_then(|cfg| cfg.stale_changes_requested_seconds),
            stale_action: coordinator
                .as_ref()
                .and_then(|cfg| cfg.stale_action.clone()),
            storage_mode: coordinator
                .as_ref()
                .and_then(|cfg| cfg.storage_mode.clone()),
            merge_ai_fix: coordinator.as_ref().and_then(|cfg| cfg.merge_ai_fix),
            merge_job_timeout_seconds: coordinator
                .as_ref()
                .and_then(|cfg| cfg.merge_job_timeout_seconds),
            merge_hook_timeout_seconds: coordinator
                .as_ref()
                .and_then(|cfg| cfg.merge_hook_timeout_seconds),
            ghost_heartbeat_grace_seconds: coordinator
                .as_ref()
                .and_then(|cfg| cfg.ghost_heartbeat_grace_seconds),
            dispatch_cooldown_seconds: coordinator
                .as_ref()
                .and_then(|cfg| cfg.dispatch_cooldown_seconds),
            json_compat: coordinator.as_ref().and_then(|cfg| cfg.json_compat),
            legacy_json_fallback: coordinator
                .as_ref()
                .and_then(|cfg| cfg.legacy_json_fallback),
            error_code_retry_list: coordinator
                .as_ref()
                .and_then(|cfg| cfg.error_code_retry_list.clone()),
            error_code_retry_max: coordinator
                .as_ref()
                .and_then(|cfg| cfg.error_code_retry_max),
            cutover_gate_window_events: coordinator
                .as_ref()
                .and_then(|cfg| cfg.cutover_gate_window_events),
            cutover_gate_max_blocked_ratio: coordinator
                .as_ref()
                .and_then(|cfg| cfg.cutover_gate_max_blocked_ratio),
            cutover_gate_max_stale_ratio: coordinator
                .as_ref()
                .and_then(|cfg| cfg.cutover_gate_max_stale_ratio),
            rate_limit_backoff_base_seconds: coordinator
                .as_ref()
                .and_then(|cfg| cfg.rate_limit_backoff_base_seconds),
            rate_limit_backoff_max_seconds: coordinator
                .as_ref()
                .and_then(|cfg| cfg.rate_limit_backoff_max_seconds),
            rate_limit_fallback_enabled: coordinator
                .as_ref()
                .and_then(|cfg| cfg.rate_limit_fallback_enabled),
            rate_limit_throttle_parallel: coordinator
                .as_ref()
                .and_then(|cfg| cfg.rate_limit_throttle_parallel),
            requirements_detected: false,
            managed_environment_warnings: Vec::new(),
        }
    }
}

fn apply_update(config: &mut CanonicalConfig, request: &ApiConfigUpdateRequest) {
    if let Some(version) = &request.version {
        config.version = Some(version.clone());
    }
    if let Some(enabled_tools) = &request.enabled_tools {
        config.tools.enabled = enabled_tools.clone();
    }
    if let Some(tool_config) = &request.tool_config {
        config.tools.config = tool_config.clone();
    }
    if let Some(tool_settings) = &request.tool_settings {
        config.tools.settings = tool_settings.clone();
    }
    if let Some(standards_path) = &request.standards_path {
        config.standards.path = Some(standards_path.clone());
    }
    if let Some(standards_inline) = &request.standards_inline {
        config.standards.inline = standards_inline.clone();
    }
    apply_selection_update(config, &request);
    apply_settings_update(config, &request);
    apply_ralph_update(config, &request);
    apply_coordinator_update(config, &request);
}

fn apply_selection_update(config: &mut CanonicalConfig, request: &ApiConfigUpdateRequest) {
    if request.selected_skills.is_none()
        && request.selected_agents.is_none()
        && request.selected_mcp.is_none()
    {
        return;
    }

    let selections = config.selections.get_or_insert_with(Default::default);
    if let Some(selected_skills) = &request.selected_skills {
        selections.skills = selected_skills.clone();
    }
    if let Some(selected_agents) = &request.selected_agents {
        selections.agents = selected_agents.clone();
    }
    if let Some(selected_mcp) = &request.selected_mcp {
        selections.mcp = selected_mcp.clone();
    }
}

fn apply_settings_update(config: &mut CanonicalConfig, request: &ApiConfigUpdateRequest) {
    if let Some(quiet) = request.quiet {
        config.settings.quiet = quiet;
    }
    if let Some(offline) = request.offline {
        config.settings.offline = offline;
    }
    if let Some(web_port) = request.web_port {
        config.settings.web_port = Some(web_port);
    }
    if let Some(web_assets) = request.web_assets {
        config.settings.web_assets = Some(web_assets);
    }
}

fn apply_ralph_update(config: &mut CanonicalConfig, request: &ApiConfigUpdateRequest) {
    if request.ralph_enabled.is_none()
        && request.ralph_iterations_default.is_none()
        && request.ralph_branch_name.is_none()
        && request.ralph_stop_on_failure.is_none()
    {
        return;
    }

    let ralph = config
        .automation
        .ralph
        .get_or_insert_with(default_ralph_config);
    if let Some(enabled) = request.ralph_enabled {
        ralph.enabled = enabled;
    }
    if let Some(iterations_default) = request.ralph_iterations_default {
        ralph.iterations_default = iterations_default;
    }
    if let Some(branch_name) = &request.ralph_branch_name {
        ralph.branch_name = branch_name.clone();
    }
    if let Some(stop_on_failure) = request.ralph_stop_on_failure {
        ralph.stop_on_failure = stop_on_failure;
    }
}

fn apply_coordinator_update(config: &mut CanonicalConfig, request: &ApiConfigUpdateRequest) {
    if !has_coordinator_updates(request) {
        return;
    }

    let coordinator = config
        .automation
        .coordinator
        .get_or_insert_with(CoordinatorConfig::default);
    if let Some(value) = &request.coordinator_tool {
        coordinator.coordinator_tool = Some(value.clone());
    }
    if let Some(value) = &request.reference_branch {
        coordinator.reference_branch = Some(value.clone());
    }
    if let Some(value) = &request.prd_file {
        coordinator.prd_file = Some(value.clone());
    }
    if let Some(value) = &request.task_registry_file {
        coordinator.task_registry_file = Some(value.clone());
    }
    if let Some(value) = &request.tool_priority {
        coordinator.tool_priority = value.clone();
    }
    if let Some(value) = &request.max_parallel_per_tool {
        coordinator.max_parallel_per_tool = value.clone();
    }
    if let Some(value) = &request.tool_specializations {
        coordinator.tool_specializations = value.clone();
    }
    if let Some(value) = request.max_dispatch {
        coordinator.max_dispatch = Some(value);
    }
    if let Some(value) = request.max_parallel {
        coordinator.max_parallel = Some(value);
    }
    if let Some(value) = request.timeout_seconds {
        coordinator.timeout_seconds = Some(value);
    }
    if let Some(value) = request.phase_runner_max_attempts {
        coordinator.phase_runner_max_attempts = Some(value);
    }
    if let Some(value) = request.log_flush_lines {
        coordinator.log_flush_lines = Some(value);
    }
    if let Some(value) = request.log_flush_ms {
        coordinator.log_flush_ms = Some(value);
    }
    if let Some(value) = request.mirror_json_debounce_ms {
        coordinator.mirror_json_debounce_ms = Some(value);
    }
    if let Some(value) = request.stale_claimed_seconds {
        coordinator.stale_claimed_seconds = Some(value);
    }
    if let Some(value) = request.stale_in_progress_seconds {
        coordinator.stale_in_progress_seconds = Some(value);
    }
    if let Some(value) = request.stale_changes_requested_seconds {
        coordinator.stale_changes_requested_seconds = Some(value);
    }
    if let Some(value) = &request.stale_action {
        coordinator.stale_action = Some(value.clone());
    }
    if let Some(value) = &request.storage_mode {
        coordinator.storage_mode = Some(value.clone());
    }
    if let Some(value) = request.merge_ai_fix {
        coordinator.merge_ai_fix = Some(value);
    }
    if let Some(value) = request.merge_job_timeout_seconds {
        coordinator.merge_job_timeout_seconds = Some(value);
    }
    if let Some(value) = request.merge_hook_timeout_seconds {
        coordinator.merge_hook_timeout_seconds = Some(value);
    }
    if let Some(value) = request.ghost_heartbeat_grace_seconds {
        coordinator.ghost_heartbeat_grace_seconds = Some(value);
    }
    if let Some(value) = request.dispatch_cooldown_seconds {
        coordinator.dispatch_cooldown_seconds = Some(value);
    }
    if let Some(value) = request.json_compat {
        coordinator.json_compat = Some(value);
    }
    if let Some(value) = request.legacy_json_fallback {
        coordinator.legacy_json_fallback = Some(value);
    }
    if let Some(value) = &request.error_code_retry_list {
        coordinator.error_code_retry_list = Some(value.clone());
    }
    if let Some(value) = request.error_code_retry_max {
        coordinator.error_code_retry_max = Some(value);
    }
    if let Some(value) = request.cutover_gate_window_events {
        coordinator.cutover_gate_window_events = Some(value);
    }
    if let Some(value) = request.cutover_gate_max_blocked_ratio {
        coordinator.cutover_gate_max_blocked_ratio = Some(value);
    }
    if let Some(value) = request.cutover_gate_max_stale_ratio {
        coordinator.cutover_gate_max_stale_ratio = Some(value);
    }
    if let Some(value) = request.rate_limit_backoff_base_seconds {
        coordinator.rate_limit_backoff_base_seconds = Some(value);
    }
    if let Some(value) = request.rate_limit_backoff_max_seconds {
        coordinator.rate_limit_backoff_max_seconds = Some(value);
    }
    if let Some(value) = request.rate_limit_fallback_enabled {
        coordinator.rate_limit_fallback_enabled = Some(value);
    }
    if let Some(value) = request.rate_limit_throttle_parallel {
        coordinator.rate_limit_throttle_parallel = Some(value);
    }
}

fn has_coordinator_updates(request: &ApiConfigUpdateRequest) -> bool {
    request.coordinator_tool.is_some()
        || request.reference_branch.is_some()
        || request.prd_file.is_some()
        || request.task_registry_file.is_some()
        || request.tool_priority.is_some()
        || request.max_parallel_per_tool.is_some()
        || request.tool_specializations.is_some()
        || request.max_dispatch.is_some()
        || request.max_parallel.is_some()
        || request.timeout_seconds.is_some()
        || request.phase_runner_max_attempts.is_some()
        || request.log_flush_lines.is_some()
        || request.log_flush_ms.is_some()
        || request.mirror_json_debounce_ms.is_some()
        || request.stale_claimed_seconds.is_some()
        || request.stale_in_progress_seconds.is_some()
        || request.stale_changes_requested_seconds.is_some()
        || request.stale_action.is_some()
        || request.storage_mode.is_some()
        || request.merge_ai_fix.is_some()
        || request.merge_job_timeout_seconds.is_some()
        || request.merge_hook_timeout_seconds.is_some()
        || request.ghost_heartbeat_grace_seconds.is_some()
        || request.dispatch_cooldown_seconds.is_some()
        || request.json_compat.is_some()
        || request.legacy_json_fallback.is_some()
        || request.error_code_retry_list.is_some()
        || request.error_code_retry_max.is_some()
        || request.cutover_gate_window_events.is_some()
        || request.cutover_gate_max_blocked_ratio.is_some()
        || request.cutover_gate_max_stale_ratio.is_some()
        || request.rate_limit_backoff_base_seconds.is_some()
        || request.rate_limit_backoff_max_seconds.is_some()
        || request.rate_limit_fallback_enabled.is_some()
        || request.rate_limit_throttle_parallel.is_some()
}

fn default_ralph_config() -> RalphConfig {
    RalphConfig {
        enabled: true,
        iterations_default: 5,
        branch_name: "ralph".to_string(),
        stop_on_failure: true,
    }
}
