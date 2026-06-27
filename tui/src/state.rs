use crate::screen::Screen;
use chrono::{DateTime, Utc};
use macc_adapter_shared::fetch::materialize_fetch_units;
use macc_core::catalog::{Agent, McpEntry, Skill};
use macc_core::config::{CanonicalConfig, CoordinatorConfig};
use macc_core::coordinator::types::CoordinatorEnvConfig;
use macc_core::coordinator_storage::{
    CoordinatorSnapshot, CoordinatorStorage, CoordinatorStoragePaths, JsonStorage, SqliteStorage,
};
use macc_core::doctor::ToolCheck;
use macc_core::engine::CoordinatorEvent;
use macc_core::plan::{render_diff, ActionPlan, DiffView, PlannedOp, Scope};
use macc_core::process_ownership::{
    ClientIdentity, ClientKind, OwnershipStatus, ProcessHandle, ProcessKind, TakeoverRequest,
};
use macc_core::resolve::{resolve, resolve_fetch_units, CliOverrides};
use macc_core::runtime::RuntimeSnapshot;
use macc_core::service::coordinator::CoordinatorManagedCommandState;
use macc_core::service::coordinator_workflow::{
    coordinator_command_display_name, CoordinatorCommand, CoordinatorCommandRequest,
};
use macc_core::service::process_ownership::{ProcessOwnershipGuard, ProcessViewerGuard};
use macc_core::service::process_ownership_gate::{gate_owner_action, ClientContext};
use macc_core::tool::{ActionKind, FieldDefault, FieldKind, ToolDescriptor, ToolField};
use macc_core::{find_project_root, Engine, MaccError, ProjectPaths};
use serde_json::{Map, Value};
use std::collections::{BTreeMap, HashMap};
use std::path::PathBuf;
use std::str::FromStr;
use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum UiStatusLevel {
    Info,
    Success,
    Warning,
    Error,
}

pub struct UiStatus {
    pub level: UiStatusLevel,
    pub message: String,
    pub expires_at: Option<Instant>,
}

pub struct ApplyContext {
    pub plan: ActionPlan,
    pub operations: Vec<PlannedOp>,
    pub project_ops: usize,
    pub user_ops: usize,
    pub backup_preview: String,
}

impl ApplyContext {
    pub fn needs_user_consent(&self) -> bool {
        self.user_ops > 0
    }
}

pub struct ApplyProgress {
    pub current: usize,
    pub total: usize,
    pub path: Option<String>,
}

pub struct WorktreeStatus {
    pub current: Option<macc_core::WorktreeEntry>,
    pub total: usize,
    pub error: Option<String>,
}

pub struct LogEntry {
    pub path: PathBuf,
    pub relative: String,
}

fn format_hms(total_secs: u64) -> String {
    let hours = total_secs / 3600;
    let minutes = (total_secs % 3600) / 60;
    let seconds = total_secs % 60;
    format!("{}:{:02}:{:02}", hours, minutes, seconds)
}

/// Format an ISO 8601 timestamp as "HH:MM:SS UTC" for throttle-until display.
fn throttle_until_hms(iso: &str) -> String {
    DateTime::parse_from_rfc3339(iso)
        .ok()
        .map(|dt| dt.with_timezone(&Utc).format("%H:%M:%S UTC").to_string())
        .unwrap_or_else(|| iso.to_string())
}

pub struct CoordinatorTaskSnapshot {
    pub total: usize,
    pub todo: usize,
    pub active: usize,
    pub blocked: usize,
    pub merged: usize,
    pub active_tasks: Vec<macc_core::coordinator::view_model::LiveTaskRow>,
    /// RL-TUI-007: tools currently throttled due to rate-limiting.
    pub throttled_tools: Vec<ThrottledToolInfo>,
}

/// RL-TUI-007: per-tool throttle state for TUI display.
#[derive(Clone)]
pub struct ThrottledToolInfo {
    pub tool_id: String,
    /// ISO 8601 timestamp when the throttle expires (raw, for sorting).
    pub throttled_until: String,
    /// Human-readable "HH:MM:SS UTC" form of `throttled_until`.
    pub display_until: String,
    pub backoff_seconds: u64,
    pub consecutive_count: u32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum CoordinatorPauseNextAction {
    RetryPhaseAndRun,
    ResumeRun,
}

fn requires_owner_gate(command: &CoordinatorCommand) -> bool {
    matches!(
        command,
        CoordinatorCommand::Run
            | CoordinatorCommand::Stop { .. }
            | CoordinatorCommand::ResumePausedRun
            | CoordinatorCommand::Unlock { .. }
            | CoordinatorCommand::DispatchReadyTasks
            | CoordinatorCommand::AdvanceTasks
            | CoordinatorCommand::CleanupMaintenance
    )
}

pub struct AppState {
    pub engine: Arc<dyn Engine>,
    pub project_paths: Option<ProjectPaths>,
    pub config: Option<CanonicalConfig>,
    pub working_copy: Option<CanonicalConfig>,
    pub errors: Vec<String>,
    pub notices: Vec<String>,
    pub should_quit: bool,
    /// When true, the TUI exits automatically when the coordinator run succeeds.
    /// Set for `LaunchMode::CoordinatorRun` so `macc coordinator` is self-terminating.
    pub coordinator_run_auto_quit: bool,
    pub screen_stack: Vec<Screen>,
    pub selected_tool_index: usize,
    pub tool_field_index: usize,
    pub current_tool_id: Option<String>,
    pub tool_descriptors: Vec<ToolDescriptor>,
    pub tool_field_editing: bool,
    pub tool_field_input: String,
    pub tool_install_confirm_id: Option<String>,
    pub automation_field_index: usize,
    pub automation_field_editing: bool,
    pub automation_field_input: String,
    // ── Tool-aware special editors (no manual typing of tool names) ──
    /// Field 0: index into ["" (auto), ...enabled_tools] for coordinator tool cycling.
    pub coordinator_tool_cycle_idx: usize,
    /// Field 3: active reorder mode for tool priority list.
    pub tool_priority_editor_active: bool,
    /// Field 3: index of the currently-highlighted (and moving) tool in the priority list.
    pub tool_priority_editor_index: usize,
    /// Field 4: active per-tool parallel count editor.
    pub tool_parallel_editor_active: bool,
    /// Field 4: index of the currently-selected tool in the parallel editor.
    pub tool_parallel_editor_index: usize,
    /// Field 3: whether the currently-selected tool is "grabbed" (ready to be moved).
    /// When true, ↑/↓ moves the tool; when false, ↑/↓ only navigates the cursor.
    pub tool_priority_editor_grabbed: bool,
    pub settings_field_index: usize,
    pub settings_field_editing: bool,
    pub settings_field_input: String,
    /// Unified config screen: active tab (0=General 1=Coordinator 2=Tools 3=Phases 4=Reliability 5=Admin)
    pub config_tab_index: usize,
    /// Unified config screen: selected row within the current tab
    pub config_view_index: usize,
    pub skills: Vec<Skill>,
    pub agents: Vec<Agent>,
    pub skill_selection_index: usize,
    pub agent_selection_index: usize,
    pub skill_target_path: Option<String>,
    pub agent_target_path: Option<String>,
    pub mcp_selection_index: usize,
    pub mcp_entries: Vec<McpEntry>,
    pub log_selection_index: usize,
    pub log_content_scroll: usize,
    pub log_entries: Vec<LogEntry>,
    pub log_view_content: String,
    pub preview_ops: Vec<PlannedOp>,
    pub preview_selection_index: usize,
    pub preview_error: Option<String>,
    pub preview_diff_cache: HashMap<String, DiffView>,
    pub preview_diff_scroll: HashMap<String, usize>,
    pub apply_context: Option<ApplyContext>,
    pub apply_consent_input: String,
    pub apply_user_consent_granted: bool,
    pub apply_feedback: Option<String>,
    pub apply_error: Option<String>,
    pub apply_progress: Option<ApplyProgress>,
    pub help_open: bool,
    pub tool_checks: Vec<ToolCheck>,
    pub last_screen: Option<Screen>,
    pub worktree_status: Option<WorktreeStatus>,
    pub ui_status: Option<UiStatus>,
    pub coordinator_snapshot: Option<CoordinatorTaskSnapshot>,
    pub coordinator_last_refresh: Option<Instant>,
    pub coordinator_running_command: Option<String>,
    pub coordinator_last_result: Option<String>,
    pub coordinator_pause_error: Option<String>,
    pub coordinator_pause_command: Option<String>,
    pub coordinator_pause_task_id: Option<String>,
    pub coordinator_pause_phase: Option<String>,
    pub coordinator_spinner_tick: u64,
    pub coordinator_events: Vec<String>,
    pub coordinator_events_last_refresh: Option<Instant>,
    pub coordinator_events_per_sec: Option<f64>,
    pub coordinator_last_event_age: Option<Duration>,
    pub coordinator_paused: bool,
    pub coordinator_current_run_id: Option<String>,
    coordinator_events_last_seen_count: usize,
    pub search_query: String,
    pub search_editing: bool,
    pub undo_stack: Vec<CanonicalConfig>,
    pub redo_stack: Vec<CanonicalConfig>,
    coordinator_client_id: String,
    coordinator_running_elapsed_secs: Option<u64>,
    coordinator_pause_next_action: Option<CoordinatorPauseNextAction>,
    /// RL-TUI-007: tools currently throttled due to rate-limiting.
    pub coordinator_throttled_tools: Vec<ThrottledToolInfo>,
    /// RL-TUI-007: (effective, original) max_parallel from concurrency_adjusted events.
    pub coordinator_effective_max_parallel: Option<(usize, usize)>,
    pub client_identity: ClientIdentity,
    pub ownership_state: crate::ownership::TuiOwnershipState,
    ownership_guard: Option<ProcessOwnershipGuard>,
    viewer_guards: Vec<ProcessViewerGuard>,
    pub client_context: ClientContext,
    last_ownership_refresh: Option<Instant>,
    /// L6-TUI-002: ownership state for Coordinator process (banner/modal/gate input).
    pub coordinator_ownership: crate::ownership::TuiOwnershipState,
    coordinator_ownership_last_heartbeat: Option<Instant>,
    coordinator_ownership_last_refresh: Option<Instant>,
    pub coordinator_stop_dialog_open: bool,
    pub coordinator_stop_dialog_selection: usize,
    pub coordinator_recover_dialog_open: bool,
    pub coordinator_recover_dialog_selection: usize,
    /// §18: Human-readable summary of active runtime phase overrides, e.g. "[testing:off] [review:required]".
    /// Set at launch time; None when no overrides are active.
    pub coordinator_phase_overrides: Option<String>,
    pub coordinator_selected_task_index: usize,
    pub coordinator_log_pane_visible: bool,
    pub coordinator_task_diff_popup: Option<String>,
    pub coordinator_task_explain_popup: Option<String>,
    pub watch_control_enabled: bool,
    pub watch_logs_only: bool,
    pub watch_events_only: bool,
    pub watch_selected_worker: usize,
    pub watch_snapshot: Option<RuntimeSnapshot>,
    pub watch_log_tail: Vec<String>,
    pub watch_last_refresh: Option<Instant>,
    /// Last doctor check result displayed in the Home readiness panel.
    pub home_doctor_summary: Option<String>,
}

impl AppState {
    const AUTOMATION_FIELD_COUNT: usize = 40;
    const COORDINATOR_EVENTS_EWMA_ALPHA: f64 = 0.30;
    const COORDINATOR_PAUSE_REL_PATH: &'static str = ".macc/automation/task/coordinator.pause.json";

    pub fn automation_field_count(&self) -> usize {
        Self::AUTOMATION_FIELD_COUNT
    }

    pub fn new(engine: Arc<dyn Engine>) -> Self {
        let mut state = Self::with_engine(engine);
        state.load_config(None);
        state
    }

    pub fn with_engine(engine: Arc<dyn Engine>) -> Self {
        let client_identity = Self::new_client_identity();
        let mut state = Self {
            engine,
            project_paths: None,
            config: None,
            working_copy: None,
            errors: Vec::new(),
            notices: Vec::new(),
            should_quit: false,
            coordinator_run_auto_quit: true,
            screen_stack: vec![Screen::Home],
            selected_tool_index: 0,
            tool_field_index: 0,
            current_tool_id: None,
            tool_descriptors: Vec::new(),
            tool_field_editing: false,
            tool_field_input: String::new(),
            tool_install_confirm_id: None,
            automation_field_index: 0,
            automation_field_editing: false,
            automation_field_input: String::new(),
            coordinator_tool_cycle_idx: 0,
            tool_priority_editor_active: false,
            tool_priority_editor_index: 0,
            tool_priority_editor_grabbed: false,
            tool_parallel_editor_active: false,
            tool_parallel_editor_index: 0,
            settings_field_index: 0,
            settings_field_editing: false,
            settings_field_input: String::new(),
            config_tab_index: 0,
            config_view_index: 0,
            skills: Vec::new(),
            agents: Vec::new(),
            skill_selection_index: 0,
            agent_selection_index: 0,
            skill_target_path: None,
            agent_target_path: None,
            mcp_selection_index: 0,
            mcp_entries: Vec::new(),
            log_selection_index: 0,
            log_content_scroll: 0,
            log_entries: Vec::new(),
            log_view_content: String::new(),
            preview_ops: Vec::new(),
            preview_selection_index: 0,
            preview_error: None,
            preview_diff_cache: HashMap::new(),
            preview_diff_scroll: HashMap::new(),
            apply_context: None,
            apply_consent_input: String::new(),
            apply_user_consent_granted: false,
            apply_feedback: None,
            apply_error: None,
            apply_progress: None,
            help_open: false,
            tool_checks: Vec::new(),
            last_screen: None,
            worktree_status: None,
            ui_status: None,
            coordinator_snapshot: None,
            coordinator_last_refresh: None,
            coordinator_running_command: None,
            coordinator_last_result: None,
            coordinator_pause_error: None,
            coordinator_pause_command: None,
            coordinator_pause_task_id: None,
            coordinator_pause_phase: None,
            coordinator_spinner_tick: 0,
            coordinator_events: Vec::new(),
            coordinator_events_last_refresh: None,
            coordinator_events_per_sec: None,
            coordinator_last_event_age: None,
            coordinator_paused: false,
            coordinator_current_run_id: None,
            coordinator_events_last_seen_count: 0,
            search_query: String::new(),
            search_editing: false,
            undo_stack: Vec::new(),
            redo_stack: Vec::new(),
            coordinator_client_id: client_identity.client_id.clone(),
            coordinator_running_elapsed_secs: None,
            coordinator_pause_next_action: None,
            coordinator_throttled_tools: Vec::new(),
            coordinator_effective_max_parallel: None,
            client_identity: client_identity.clone(),
            ownership_state: crate::ownership::TuiOwnershipState::new(),
            ownership_guard: None,
            viewer_guards: Vec::new(),
            coordinator_selected_task_index: 0,
            coordinator_log_pane_visible: true,
            coordinator_task_diff_popup: None,
            coordinator_task_explain_popup: None,
            watch_control_enabled: false,
            watch_logs_only: false,
            watch_events_only: false,
            watch_selected_worker: 0,
            watch_snapshot: None,
            watch_log_tail: Vec::new(),
            watch_last_refresh: None,
            home_doctor_summary: None,
            client_context: ClientContext {
                client_id: client_identity.client_id.clone(),
                project_root: PathBuf::new(),
            },
            last_ownership_refresh: None,
            coordinator_ownership: crate::ownership::TuiOwnershipState::new(),
            coordinator_ownership_last_heartbeat: None,
            coordinator_ownership_last_refresh: None,
            coordinator_stop_dialog_open: false,
            coordinator_stop_dialog_selection: 0,
            coordinator_recover_dialog_open: false,
            coordinator_recover_dialog_selection: 0,
            coordinator_phase_overrides: None,
        };

        state.refresh_tools();
        state.refresh_tool_checks();
        state.refresh_skills();
        state.refresh_mcp_entries();
        state.refresh_logs();
        state.agents = state.engine.builtin_agents();

        state
    }

    fn new_client_identity() -> ClientIdentity {
        let now = chrono::Utc::now().to_rfc3339();
        ClientIdentity {
            client_id: format!("tui-{}", Self::uuid_v4_like()),
            kind: ClientKind::Tui,
            connected_at: now.clone(),
            last_heartbeat: now,
        }
    }

    fn uuid_v4_like() -> String {
        use std::time::{SystemTime, UNIX_EPOCH};

        let since_epoch = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("time went backwards");
        format!("{:x}", since_epoch.as_nanos())
    }

    pub fn refresh_tools(&mut self) {
        let paths = self
            .project_paths
            .clone()
            .unwrap_or_else(|| ProjectPaths::from_root("."));
        let (descriptors, diagnostics) = self.engine.list_tools(&paths);
        self.tool_descriptors = descriptors;

        for diag in diagnostics {
            let location = match (diag.line, diag.column) {
                (Some(l), Some(c)) => format!(" at {}:{}", l, c),
                (Some(l), None) => format!(" at line {}", l),
                _ => "".to_string(),
            };
            self.errors.push(format!(
                "Tool Spec Error ({}{}): {}",
                diag.path.display(),
                location,
                diag.error
            ));
        }
    }

    pub fn refresh_skills(&mut self) {
        let mut skills_map: BTreeMap<String, Skill> = BTreeMap::new();

        if let Some(paths) = &self.project_paths {
            match macc_core::catalog::load_skills_catalog_with_local(paths) {
                Ok(catalog) => {
                    for entry in catalog.entries {
                        skills_map.insert(
                            entry.id.clone(),
                            Skill {
                                id: entry.id,
                                name: entry.name,
                                description: entry.description,
                                mandatory: entry.mandatory,
                            },
                        );
                    }
                }
                Err(err) => {
                    self.errors
                        .push(format!("Failed to load skills catalog: {}", err));
                }
            }
        }

        let mut skills: Vec<Skill> = skills_map.into_values().collect();
        skills.sort_by(|a, b| a.id.cmp(&b.id));
        self.skills = skills;
        if self.skill_selection_index >= self.skills.len() {
            self.skill_selection_index = 0;
        }
    }

    pub fn refresh_mcp_entries(&mut self) {
        if let Some(paths) = &self.project_paths {
            match macc_core::catalog::McpCatalog::load(&paths.mcp_catalog_path()) {
                Ok(mut catalog) => {
                    catalog.entries.sort_by(|a, b| a.id.cmp(&b.id));
                    self.mcp_entries = catalog.entries;
                }
                Err(err) => {
                    self.errors
                        .push(format!("Failed to load MCP catalog: {}", err));
                    self.mcp_entries = Vec::new();
                }
            }
        } else {
            self.mcp_entries = Vec::new();
        }

        if self.mcp_selection_index >= self.mcp_entries.len() {
            self.mcp_selection_index = 0;
        }
    }

    /// Run doctor checks synchronously and store a summary for the Home readiness panel.
    /// Called when the user presses 'd' on the Home screen (spec §13.1).
    pub fn run_home_doctor_check(&mut self) {
        let Some(paths) = self.project_paths.clone() else {
            self.set_status(
                UiStatusLevel::Warning,
                "No project loaded — run macc init first.",
                Some(std::time::Duration::from_secs(4)),
            );
            return;
        };

        let max_parallel = 2u32;
        let findings = self
            .engine
            .collect_diagnostic_findings(&paths, max_parallel);

        let errors: Vec<_> = findings
            .iter()
            .filter(|f| matches!(f.severity, macc_core::doctor::DiagnosticSeverity::Error))
            .collect();
        let warnings: Vec<_> = findings
            .iter()
            .filter(|f| matches!(f.severity, macc_core::doctor::DiagnosticSeverity::Warning))
            .collect();

        let summary = if errors.is_empty() && warnings.is_empty() {
            "✅ All checks passed".to_string()
        } else {
            let mut parts = Vec::new();
            if !errors.is_empty() {
                parts.push(format!("{} error(s)", errors.len()));
            }
            if !warnings.is_empty() {
                parts.push(format!("{} warning(s)", warnings.len()));
            }
            format!("⚠ Doctor: {}", parts.join(", "))
        };

        // Build detailed text for the Home panel.
        let mut detail = String::from("Last doctor check\n\n");
        for f in &findings {
            let sym = match f.severity {
                macc_core::doctor::DiagnosticSeverity::Ok => "✅",
                macc_core::doctor::DiagnosticSeverity::Info => "ℹ",
                macc_core::doctor::DiagnosticSeverity::Warning => "⚠",
                macc_core::doctor::DiagnosticSeverity::Error => "❌",
            };
            detail.push_str(&format!("{} {}\n", sym, f.title));
            if !f.message.is_empty()
                && !matches!(f.severity, macc_core::doctor::DiagnosticSeverity::Ok)
            {
                for line in f.message.lines() {
                    detail.push_str(&format!("   {}\n", line));
                }
            }
        }
        if errors.is_empty() {
            detail.push_str("\n✅ Ready to dispatch a task\n");
        } else {
            detail.push_str(&format!("\n❌ {} blocking issue(s)\n", errors.len()));
        }

        self.home_doctor_summary = Some(detail);
        let (level, ttl) = if errors.is_empty() {
            (UiStatusLevel::Success, std::time::Duration::from_secs(4))
        } else {
            (UiStatusLevel::Warning, std::time::Duration::from_secs(6))
        };
        self.set_status(level, summary, Some(ttl));
    }

    pub fn refresh_logs(&mut self) {
        let Some(paths) = &self.project_paths else {
            self.log_entries.clear();
            self.log_view_content.clear();
            self.log_selection_index = 0;
            self.log_content_scroll = 0;
            return;
        };
        match self.engine.logs_list_entries(paths) {
            Ok(entries) => {
                self.log_entries = entries
                    .into_iter()
                    .map(|e| LogEntry {
                        path: e.path,
                        relative: e.relative,
                    })
                    .collect();
            }
            Err(err) => {
                self.log_entries.clear();
                self.log_view_content = format!(
                    "Failed to list logs.\n\nCause: {}",
                    format_actionable_error(&err.to_string())
                );
                return;
            }
        }
        if self.log_entries.is_empty() {
            self.log_selection_index = 0;
            self.log_content_scroll = 0;
            self.log_view_content = "No log files found in .macc/log/.".to_string();
            return;
        }
        if self.log_selection_index >= self.log_entries.len() {
            self.log_selection_index = 0;
        }
        let filtered = self.filtered_log_indices();
        if let Some(first) = filtered.first() {
            if !filtered.contains(&self.log_selection_index) {
                self.log_selection_index = *first;
            }
        }
        self.log_content_scroll = 0;
        self.load_selected_log_content();
    }

    fn load_selected_log_content(&mut self) {
        let Some(entry) = self.log_entries.get(self.log_selection_index) else {
            self.log_view_content = "No log selected.".to_string();
            return;
        };
        match self.engine.logs_read_file(&entry.path) {
            Ok(content) => {
                self.log_view_content = content;
            }
            Err(err) => {
                self.log_view_content = format!(
                    "Failed to read log '{}'.\n\nCause: {}\nSuggested fix: verify file permissions and refresh logs with 'r'.",
                    entry.path.display(),
                    err
                );
            }
        }
    }

    pub fn next_log(&mut self) {
        let visible = self.filtered_log_indices();
        self.log_selection_index = next_visible_index(self.log_selection_index, &visible);
        self.log_content_scroll = 0;
        self.load_selected_log_content();
    }

    pub fn prev_log(&mut self) {
        let visible = self.filtered_log_indices();
        self.log_selection_index = prev_visible_index(self.log_selection_index, &visible);
        self.log_content_scroll = 0;
        self.load_selected_log_content();
    }

    pub fn scroll_log_content(&mut self, delta: isize) {
        let current = self.log_content_scroll as isize;
        let next = (current + delta).max(0) as usize;
        self.log_content_scroll = next;
    }

    pub fn refresh_worktree_status(&mut self) {
        let Some(paths) = &self.project_paths else {
            self.worktree_status = None;
            return;
        };

        match self.engine.list_worktrees(&paths.root) {
            Ok(entries) => {
                let current = macc_core::current_worktree(&paths.root, &entries);
                self.worktree_status = Some(WorktreeStatus {
                    current,
                    total: entries.len(),
                    error: None,
                });
            }
            Err(err) => {
                self.worktree_status = Some(WorktreeStatus {
                    current: None,
                    total: 0,
                    error: Some(err.to_string()),
                });
            }
        }
    }

    fn allow_legacy_json_fallback(&self) -> bool {
        let raw = self
            .engine
            .env_var("COORDINATOR_LEGACY_JSON_FALLBACK")
            .unwrap_or_else(|| "0".to_string());
        !matches!(
            raw.trim().to_ascii_lowercase().as_str(),
            "0" | "false" | "no" | "off"
        )
    }

    pub fn load_coordinator_storage_snapshot(&self) -> Result<CoordinatorSnapshot, String> {
        let paths = self
            .project_paths
            .as_ref()
            .ok_or_else(|| "No project loaded.".to_string())?;
        let storage_paths = CoordinatorStoragePaths::from_project_paths(paths);
        match SqliteStorage::new(storage_paths.clone()).load_snapshot() {
            Ok(snapshot) => Ok(snapshot),
            Err(err) if self.allow_legacy_json_fallback() => JsonStorage::new(storage_paths)
                .load_snapshot()
                .map_err(|json_err| {
                    format!(
                        "failed to load coordinator snapshot (sqlite={}, json={})",
                        err, json_err
                    )
                }),
            Err(err) => Err(format!(
                "failed to load coordinator snapshot from sqlite: {}",
                err
            )),
        }
    }

    fn resolve_task_model(
        task: &macc_core::coordinator::model::Task,
        canonical: &CanonicalConfig,
    ) -> String {
        let tool_id = task
            .tool
            .as_deref()
            .or_else(|| task.coordinator_tool.as_deref())
            .unwrap_or("");
        if tool_id.is_empty() {
            return "-".to_string();
        }

        // 1. Resolve model tier via routing engine
        let routing_cfg = canonical.automation.model_routing.as_ref();
        let phase = task
            .task_runtime
            .current_phase
            .as_deref()
            .unwrap_or("implementation");
        let decision = macc_core::coordinator::model_routing::decide(task, phase, routing_cfg);
        let tier_str = decision.tier.as_str();

        // 2. Lookup in tools.config.<tool_id>
        if let Some(tool_cfg) = canonical.tools.config.get(tool_id) {
            // Try model_tiers[tier].model
            if let Some(model_tiers) = tool_cfg.get("model_tiers").and_then(|t| t.as_object()) {
                if let Some(tier_spec) = model_tiers.get(tier_str) {
                    if let Some(model) = tier_spec.get("model").and_then(|m| m.as_str()) {
                        if !model.is_empty() {
                            return model.to_string();
                        }
                    }
                }
            }

            // Try model
            if let Some(model) = tool_cfg.get("model").and_then(|m| m.as_str()) {
                if !model.is_empty() {
                    return model.to_string();
                }
            }

            // Try settings.model_name or settings.model
            if let Some(settings) = tool_cfg.get("settings").and_then(|s| s.as_object()) {
                if let Some(model) = settings
                    .get("model_name")
                    .or_else(|| settings.get("model"))
                    .and_then(|m| m.as_str())
                {
                    if !model.is_empty() {
                        return model.to_string();
                    }
                }
            }
        }

        // 3. Fallback to default tool models
        let fallback = match tool_id {
            "claude" => "sonnet",         // macc:allow-tool-name
            "agy" => "auto-gemini-3",     // macc:allow-tool-name
            "gemini" => "gemini-1.5-pro", // macc:allow-tool-name
            "codex" => "gpt-4o",          // macc:allow-tool-name
            _ => tier_str,
        };
        fallback.to_string()
    }

    fn read_registry_snapshot(
        &self,
        root: &macc_core::coordinator::model::TaskRegistry,
    ) -> Result<CoordinatorTaskSnapshot, String> {
        let mut snapshot = CoordinatorTaskSnapshot {
            total: root.tasks.len(),
            todo: 0,
            active: 0,
            blocked: 0,
            merged: 0,
            active_tasks: Vec::new(),
            throttled_tools: Vec::new(),
        };
        // RL-TUI-007: collect throttled tool info from tasks whose delayed_until is in the future.
        let now_iso = Utc::now().to_rfc3339();
        let mut throttle_map: BTreeMap<String, ThrottledToolInfo> = BTreeMap::new();
        for task in &root.tasks {
            if let (Some(delayed_until), Some(tool_id)) = (
                task.task_runtime.delayed_until.as_deref(),
                task.tool.as_deref(),
            ) {
                if !tool_id.is_empty() && delayed_until > now_iso.as_str() {
                    let (backoff_seconds, consecutive_count) = task
                        .task_runtime
                        .extra
                        .get("throttle_state")
                        .map(|v| {
                            let bs = v
                                .get("backoff_seconds")
                                .and_then(|x| x.as_u64())
                                .unwrap_or(0);
                            let cc = v
                                .get("consecutive_429_count")
                                .and_then(|x| x.as_u64())
                                .unwrap_or(0) as u32;
                            (bs, cc)
                        })
                        .unwrap_or((0, 0));
                    let entry = throttle_map.entry(tool_id.to_string()).or_insert_with(|| {
                        ThrottledToolInfo {
                            tool_id: tool_id.to_string(),
                            throttled_until: delayed_until.to_string(),
                            display_until: throttle_until_hms(delayed_until),
                            backoff_seconds,
                            consecutive_count,
                        }
                    });
                    // Keep the latest expiry for this tool.
                    if delayed_until > entry.throttled_until.as_str() {
                        *entry = ThrottledToolInfo {
                            tool_id: tool_id.to_string(),
                            throttled_until: delayed_until.to_string(),
                            display_until: throttle_until_hms(delayed_until),
                            backoff_seconds,
                            consecutive_count,
                        };
                    }
                }
            }
        }
        snapshot.throttled_tools = throttle_map.into_values().collect();
        for task in &root.tasks {
            let state = if task.state.is_empty() {
                "todo".to_string()
            } else {
                task.state.to_ascii_lowercase()
            };
            let runtime_status = task.task_runtime.status.as_deref().unwrap_or("-");
            let is_live_active = matches!(
                state.as_str(),
                "claimed"
                    | "in_progress"
                    | "testing"
                    | "reviewing"
                    | "pr_open"
                    | "changes_requested"
                    | "queued"
            ) && !(state == "claimed" && runtime_status == "phase_done");

            match state.as_str() {
                "todo" => snapshot.todo += 1,
                "claimed" | "in_progress" | "testing" | "reviewing" | "pr_open"
                | "changes_requested" | "queued"
                    if is_live_active =>
                {
                    let model = if let Some(ref canonical) = self.working_copy {
                        Self::resolve_task_model(task, canonical)
                    } else {
                        "-".to_string()
                    };
                    snapshot.active += 1;
                    snapshot.active_tasks.push(
                        macc_core::coordinator::view_model::LiveTaskRow::from_task(
                            task,
                            Utc::now(),
                            model,
                        ),
                    );
                }
                "claimed" => {
                    // Claimed + phase_done can happen after coordinator restart before reconciliation.
                    // Keep it out of live-active rendering to avoid a misleading "still running" signal.
                }
                "blocked" => snapshot.blocked += 1,
                "merged" => snapshot.merged += 1,
                _ => {}
            }
        }
        Ok(snapshot)
    }

    pub fn refresh_coordinator_snapshot(&mut self) {
        self.refresh_coordinator_pause_state();
        match self
            .load_coordinator_storage_snapshot()
            .and_then(|snapshot| self.read_registry_snapshot(&snapshot.registry))
        {
            Ok(snapshot) => {
                self.coordinator_throttled_tools = snapshot.throttled_tools.clone();
                self.coordinator_snapshot = Some(snapshot);
                self.coordinator_last_refresh = Some(Instant::now());
            }
            Err(err) => {
                self.coordinator_last_result = Some(format_actionable_error(&err));
            }
        }
    }

    pub fn refresh_watch_snapshot(&mut self) {
        let Some(paths) = self.project_paths.as_ref() else {
            return;
        };
        match self.engine.runtime_snapshot(paths) {
            Ok(snapshot) => {
                self.watch_snapshot = Some(snapshot);
                self.watch_last_refresh = Some(Instant::now());
            }
            Err(_) => {
                // Snapshot unavailable (coordinator not running or storage missing).
                // Leave the previous snapshot in place so the screen is not blanked.
                self.watch_last_refresh = Some(Instant::now());
            }
        }
    }

    fn refresh_coordinator_pause_state(&mut self) {
        let paused = self
            .project_paths
            .as_ref()
            .map(|p| {
                self.engine
                    .path_exists(&p.root.join(Self::COORDINATOR_PAUSE_REL_PATH))
            })
            .unwrap_or(false);
        self.coordinator_paused = paused;
    }

    fn is_essential_coordinator_event(event: &str) -> bool {
        matches!(
            event,
            "command_start"
                | "command_end"
                | "command_error"
                | "task_transition"
                | "task_dispatched"
                | "sanitize_done"
                | "performer_complete"
                | "task_blocked"
                | "dispatch_complete"
                | "started"
                | "progress"
                | "phase_result"
                | "commit_created"
                | "review_done"
                | "integrate_done"
                | "failed"
                | "heartbeat"
                | "task_runtime_retry"
                | "task_runtime_requeue"
                | "task_runtime_stale"
                | "task_retry_count"
                | "task_slo_warning"
                | "phase_retry"
                | "phase_skipped"
                | "events_rotated"
                | "events_compacted"
                // RL-TUI-007: rate-limit visibility events
                | "concurrency_adjusted"
                | "tool_fallback"
                | "quota_exhausted"
                // Periodic coordinator-alive signal (emitted every 30s while the
                // coordinator is running, so viewer TUIs see activity even when
                // no task lifecycle events are occurring).
                | "coordinator_heartbeat"
        )
    }

    fn resolve_current_run_id(events: &[CoordinatorEvent]) -> Option<String> {
        events
            .iter()
            .rev()
            .filter_map(|event| event.run_id.as_deref())
            .find(|run_id| !run_id.trim().is_empty())
            .map(|run_id| run_id.to_string())
    }

    fn event_matches_current_run(event: &CoordinatorEvent, run_id: Option<&str>) -> bool {
        match run_id {
            Some(expected) if !expected.is_empty() => event
                .run_id
                .as_deref()
                .map(|value| value == expected)
                .unwrap_or(false),
            _ => true,
        }
    }

    pub fn refresh_coordinator_events(&mut self) {
        let Some(paths) = self.project_paths.as_ref() else {
            self.coordinator_events.clear();
            self.coordinator_events_per_sec = None;
            self.coordinator_last_event_age = None;
            self.coordinator_current_run_id = None;
            self.coordinator_events_last_seen_count = 0;
            return;
        };
        let events = match self.engine.get_coordinator_events(paths) {
            Ok(events) => events,
            Err(_) => {
                self.coordinator_events.clear();
                self.coordinator_events_per_sec = None;
                self.coordinator_last_event_age = None;
                self.coordinator_current_run_id = None;
                self.coordinator_events_last_seen_count = 0;
                return;
            }
        };
        self.coordinator_current_run_id = Self::resolve_current_run_id(&events);
        let current_run_id = self.coordinator_current_run_id.as_deref();
        let now = Instant::now();
        let filtered: Vec<&CoordinatorEvent> = events
            .iter()
            .filter(|v| Self::event_matches_current_run(v, current_run_id))
            .collect();
        let mut lines: Vec<String> = filtered
            .iter()
            .filter_map(|event| {
                if !Self::is_essential_coordinator_event(&event.event_type) {
                    return None;
                }
                let msg = event
                    .message
                    .as_deref()
                    .or_else(|| {
                        event
                            .raw
                            .get("msg")
                            .or_else(|| event.raw.get("payload").and_then(|p| p.get("message")))
                            .or_else(|| event.raw.get("payload").and_then(|p| p.get("reason")))
                            .and_then(|x| x.as_str())
                    })
                    .unwrap_or(event.event_type.as_str())
                    .to_string();
                let mut rendered = format!("[{}] {}", event.event_type, msg);
                if let Some(task) = event.task_id.as_deref() {
                    if !task.is_empty() {
                        rendered.push_str(&format!(" | task={}", task));
                    }
                }
                if let Some(state) = event.status.as_deref() {
                    if !state.is_empty() {
                        rendered.push_str(&format!(" | state={}", state));
                    }
                }
                if let Some(phase) = event.phase.as_deref() {
                    if !phase.is_empty() {
                        rendered.push_str(&format!(" | phase={}", phase));
                    }
                }
                if let Some(detail) = event.raw.get("detail").and_then(|x| x.as_str()) {
                    if !detail.is_empty() {
                        rendered.push_str(&format!(" | {}", detail));
                    }
                }
                if let Some(source) = event.raw.get("source").and_then(|x| x.as_str()) {
                    if !source.is_empty() {
                        rendered.push_str(&format!(" | src={}", source));
                    }
                }
                if let Some(ts) = event.ts.as_deref() {
                    if !ts.is_empty() {
                        rendered.push_str(&format!(" | {}", ts));
                    }
                }
                Some(rendered)
            })
            .collect();
        let total_count = lines.len();
        if let Some(prev_refresh) = self.coordinator_events_last_refresh {
            let elapsed_secs = now.saturating_duration_since(prev_refresh).as_secs_f64();
            if elapsed_secs > 0.0 {
                let delta_events =
                    total_count.saturating_sub(self.coordinator_events_last_seen_count);
                let instant_rate = delta_events as f64 / elapsed_secs;
                self.coordinator_events_per_sec = Some(match self.coordinator_events_per_sec {
                    Some(previous) => {
                        let alpha = Self::COORDINATOR_EVENTS_EWMA_ALPHA;
                        (1.0 - alpha) * previous + alpha * instant_rate
                    }
                    None => instant_rate,
                });
            }
        } else {
            self.coordinator_events_per_sec = Some(0.0);
        }

        self.coordinator_last_event_age = filtered
            .iter()
            .rev()
            .find_map(|event| event.ts.as_deref())
            .and_then(|ts| DateTime::parse_from_rfc3339(ts).ok())
            .and_then(|ts| {
                Utc::now()
                    .signed_duration_since(ts.with_timezone(&Utc))
                    .to_std()
                    .ok()
            });

        let keep = 120usize;
        if lines.len() > keep {
            lines = lines.split_off(lines.len() - keep);
        }
        self.coordinator_events = lines;
        self.coordinator_events_last_refresh = Some(now);
        self.coordinator_events_last_seen_count = total_count;
        // RL-TUI-007: parse effective_max_parallel from the most recent concurrency_adjusted event.
        if let Some(msg) = filtered
            .iter()
            .rev()
            .find(|e| e.event_type == "concurrency_adjusted")
            .and_then(|e| e.message.as_deref())
        {
            if let Some(effective) = msg
                .split_whitespace()
                .find(|s| s.starts_with("effective_max_parallel="))
                .and_then(|s| s.split('=').nth(1))
                .and_then(|v| v.parse::<usize>().ok())
            {
                let original = self
                    .config
                    .as_ref()
                    .and_then(|c| c.automation.coordinator.as_ref())
                    .and_then(|c| c.max_parallel)
                    .unwrap_or(effective);
                self.coordinator_effective_max_parallel = Some((effective, original));
            }
        }
    }

    pub fn refresh_tool_checks(&mut self) {
        let paths = self
            .project_paths
            .clone()
            .unwrap_or_else(|| ProjectPaths::from_root("."));
        self.tool_checks = self.engine.doctor(&paths);
    }

    fn coordinator_env_cfg(&self) -> CoordinatorEnvConfig {
        CoordinatorEnvConfig::default()
    }

    fn start_managed_coordinator_command(&mut self, command: CoordinatorCommand) {
        let command_name = coordinator_command_display_name(&command).to_string();
        if self.is_coordinator_running() {
            self.set_status(
                UiStatusLevel::Warning,
                "Coordinator already running.",
                Some(Duration::from_secs(3)),
            );
            return;
        }
        let Some(paths) = self.project_paths.as_ref() else {
            self.set_status(
                UiStatusLevel::Error,
                "No project loaded.",
                Some(Duration::from_secs(4)),
            );
            return;
        };
        if let Err(err) = self.gate_coordinator_action(&command) {
            self.set_status(
                UiStatusLevel::Error,
                format!(
                    "Failed to run '{}': {}",
                    command_name,
                    format_actionable_error(&err.to_string())
                ),
                Some(Duration::from_secs(6)),
            );
            return;
        }
        self.coordinator_pause_error = None;
        self.coordinator_pause_command = None;
        self.coordinator_pause_task_id = None;
        self.coordinator_pause_phase = None;
        let coordinator_cfg = self
            .working_copy
            .as_ref()
            .and_then(|c| c.automation.coordinator.as_ref());
        match self.engine.coordinator_start_managed_command_process(
            paths,
            &command,
            coordinator_cfg,
        ) {
            Ok(()) => {
                self.wait_for_coordinator_registration(Duration::from_secs(2));
                if let Some(handle) = self.coordinator_handle() {
                    self.claim_process_ownership(handle);
                    self.refresh_ownership_state();
                }
                self.coordinator_running_command = Some(command_name.clone());
                self.coordinator_running_elapsed_secs = Some(0);
                self.coordinator_last_result = Some(if command_name == "run" {
                    "Started 'run' loop.".to_string()
                } else {
                    format!("Started '{}'.", command_name)
                });
                self.refresh_coordinator_snapshot();
                self.refresh_coordinator_events();
                self.set_status(
                    UiStatusLevel::Info,
                    format!("Coordinator '{}' started.", command_name),
                    Some(Duration::from_secs(3)),
                );
            }
            Err(err) => {
                self.coordinator_last_result = Some(format_actionable_error(&format!(
                    "Failed to start '{}': {}",
                    command_name, err
                )));
                self.set_status(
                    UiStatusLevel::Error,
                    format!("Failed to start '{}'.", command_name),
                    Some(Duration::from_secs(8)),
                );
            }
        }
    }

    fn execute_coordinator_command(&mut self, command: CoordinatorCommand) {
        let action = coordinator_command_display_name(&command).to_string();
        if matches!(command, CoordinatorCommand::ResumePausedRun) {
            if let Err(err) = self.gate_coordinator_action(&command) {
                self.set_status(
                    UiStatusLevel::Error,
                    format!(
                        "Failed to run '{}': {}",
                        action,
                        format_actionable_error(&err.to_string())
                    ),
                    Some(Duration::from_secs(6)),
                );
                return;
            }
        }
        let Some(paths) = self.project_paths.as_ref() else {
            self.set_status(
                UiStatusLevel::Error,
                "No project loaded.",
                Some(Duration::from_secs(4)),
            );
            return;
        };
        let env_cfg = self.coordinator_env_cfg();
        let canonical = self.working_copy.as_ref();
        let coordinator_cfg = canonical.and_then(|c| c.automation.coordinator.as_ref());
        match self.engine.coordinator_execute_command(
            paths,
            command,
            CoordinatorCommandRequest {
                canonical,
                coordinator_cfg,
                env_cfg: &env_cfg,
                logger: None,
            },
        ) {
            Ok(response) => {
                self.refresh_coordinator_snapshot();
                self.refresh_coordinator_events();
                if let Some(resumed) = response.resumed {
                    let message = if resumed {
                        "Resume signal sent to coordinator."
                    } else {
                        "Coordinator is not paused."
                    };
                    self.set_status(
                        UiStatusLevel::Success,
                        message,
                        Some(Duration::from_secs(4)),
                    );
                } else {
                    self.set_status(
                        UiStatusLevel::Success,
                        format!("Coordinator '{}' completed.", action),
                        Some(Duration::from_secs(4)),
                    );
                }
            }
            Err(err) => {
                self.set_status(
                    UiStatusLevel::Error,
                    format!(
                        "Failed to run '{}': {}",
                        action,
                        format_actionable_error(&err.to_string())
                    ),
                    Some(Duration::from_secs(6)),
                );
            }
        }
    }

    pub fn start_coordinator_command(&mut self, command: CoordinatorCommand) {
        self.coordinator_pause_next_action = None;
        match command {
            CoordinatorCommand::ResumePausedRun => self.execute_coordinator_command(command),
            _ => self.start_managed_coordinator_command(command),
        }
    }

    pub fn start_named_coordinator_command(&mut self, command_name: &str) {
        let command = match command_name {
            "run" => CoordinatorCommand::Run,
            "sync" => CoordinatorCommand::SyncRegistry,
            "reconcile" => CoordinatorCommand::ReconcileRuntime,
            "cleanup" => CoordinatorCommand::CleanupMaintenance,
            "dispatch" => CoordinatorCommand::DispatchReadyTasks,
            "advance" => CoordinatorCommand::AdvanceTasks,
            "resume" => CoordinatorCommand::ResumePausedRun,
            other => {
                self.set_status(
                    UiStatusLevel::Error,
                    format!("Unsupported coordinator command '{}'.", other),
                    Some(Duration::from_secs(5)),
                );
                return;
            }
        };
        self.start_coordinator_command(command);
    }

    pub fn stop_coordinator_command(&mut self) {
        let Some(paths) = self.project_paths.clone() else {
            self.set_status(
                UiStatusLevel::Warning,
                "No project loaded.",
                Some(Duration::from_secs(4)),
            );
            return;
        };
        if let Err(err) = self.gate_coordinator_action(&CoordinatorCommand::Stop {
            drain: false,
            graceful: false,
            force: false,
            remove_worktrees: false,
            remove_branches: false,
            reason: "tui stop".to_string(),
        }) {
            self.set_status(
                UiStatusLevel::Error,
                format!(
                    "Failed to run 'stop': {}",
                    format_actionable_error(&err.to_string())
                ),
                Some(Duration::from_secs(6)),
            );
            return;
        }

        let stop_result = self
            .engine
            .coordinator_stop_managed_command_process(&paths, false);

        self.coordinator_pause_next_action = None;
        self.coordinator_running_command = None;
        self.coordinator_running_elapsed_secs = None;

        // Reconcile registry so dead-PID tasks are transitioned out of in_progress.
        let env_cfg = self.coordinator_env_cfg();
        let _ = self
            .engine
            .coordinator_reconcile_workflow(&paths, &env_cfg, None, None);

        self.refresh_coordinator_snapshot();
        self.refresh_coordinator_events();
        match stop_result {
            Ok(result) => {
                let mode = if result.used_group {
                    "process-group"
                } else {
                    "process-tree"
                };
                self.coordinator_last_result = Some(format!(
                    "Coordinator stopped via {} ({} process target(s)).",
                    mode, result.targets
                ));
                self.set_status(
                    UiStatusLevel::Success,
                    format!("Coordinator stopped via {}.", mode),
                    Some(Duration::from_secs(4)),
                );
            }
            Err(err) => {
                self.coordinator_last_result = Some(format!(
                    "Coordinator process stopped with fallback: {}",
                    err
                ));
                self.set_status(
                    UiStatusLevel::Warning,
                    "Coordinator stopped, but child cleanup may be incomplete.",
                    Some(Duration::from_secs(6)),
                );
            }
        }
    }

    pub fn open_coordinator_stop_dialog(&mut self) {
        self.coordinator_stop_dialog_open = true;
        self.coordinator_stop_dialog_selection = 0;
    }

    pub fn close_coordinator_stop_dialog(&mut self) {
        self.coordinator_stop_dialog_open = false;
    }

    pub fn open_coordinator_recover_dialog(&mut self) {
        self.coordinator_recover_dialog_open = true;
        self.coordinator_recover_dialog_selection = 0;
    }

    pub fn close_coordinator_recover_dialog(&mut self) {
        self.coordinator_recover_dialog_open = false;
    }

    pub fn stop_coordinator_with_selected_mode(&mut self) {
        let mode = match self.coordinator_stop_dialog_selection {
            0 => "drain",
            1 => "graceful",
            2 => "force",
            3 => "force_cleanup",
            _ => return,
        };
        self.close_coordinator_stop_dialog();
        self.stop_coordinator_command_with_mode(mode);
    }

    pub fn stop_coordinator_command_with_mode(&mut self, mode: &str) {
        let Some(paths) = self.project_paths.clone() else {
            self.set_status(
                UiStatusLevel::Warning,
                "No project loaded.",
                Some(Duration::from_secs(4)),
            );
            return;
        };

        let cmd = match mode {
            "drain" => CoordinatorCommand::Stop {
                drain: true,
                graceful: false,
                force: false,
                remove_worktrees: false,
                remove_branches: false,
                reason: "tui drain".to_string(),
            },
            "graceful" => CoordinatorCommand::Stop {
                drain: false,
                graceful: true,
                force: false,
                remove_worktrees: false,
                remove_branches: false,
                reason: "tui graceful stop".to_string(),
            },
            "force" => CoordinatorCommand::Stop {
                drain: false,
                graceful: false,
                force: true,
                remove_worktrees: false,
                remove_branches: false,
                reason: "tui force stop".to_string(),
            },
            "force_cleanup" => CoordinatorCommand::Stop {
                drain: false,
                graceful: false,
                force: true,
                remove_worktrees: true,
                remove_branches: true,
                reason: "tui force stop + cleanup".to_string(),
            },
            _ => return,
        };

        if let Err(err) = self.gate_coordinator_action(&cmd) {
            self.set_status(
                UiStatusLevel::Error,
                format!(
                    "Failed to run '{}': {}",
                    mode,
                    format_actionable_error(&err.to_string())
                ),
                Some(Duration::from_secs(6)),
            );
            return;
        }

        let env_cfg = self.coordinator_env_cfg();
        let req = macc_core::service::coordinator_workflow::CoordinatorCommandRequest {
            canonical: self.config.as_ref(),
            coordinator_cfg: self
                .config
                .as_ref()
                .and_then(|c| c.automation.coordinator.as_ref()),
            env_cfg: &env_cfg,
            logger: None,
        };

        match self.engine.coordinator_execute_command(&paths, cmd, req) {
            Ok(_) => {
                self.coordinator_pause_next_action = None;
                self.coordinator_running_command = None;
                self.coordinator_running_elapsed_secs = None;
                let _ = self
                    .engine
                    .coordinator_reconcile_workflow(&paths, &env_cfg, None, None);
                self.refresh_coordinator_snapshot();
                self.refresh_coordinator_events();
                self.set_status(
                    UiStatusLevel::Success,
                    format!("Stop mode '{}' applied successfully.", mode),
                    Some(Duration::from_secs(4)),
                );
            }
            Err(err) => {
                self.set_status(
                    UiStatusLevel::Error,
                    format!("Stop command failed: {}", err),
                    Some(Duration::from_secs(6)),
                );
            }
        }
    }

    pub fn recover_coordinator_with_selected_mode(&mut self) {
        let dry_run = match self.coordinator_recover_dialog_selection {
            0 => false,
            1 => true,
            _ => return,
        };
        self.close_coordinator_recover_dialog();
        self.recover_coordinator_command(dry_run);
    }

    pub fn recover_coordinator_command(&mut self, dry_run: bool) {
        let Some(paths) = self.project_paths.clone() else {
            self.set_status(
                UiStatusLevel::Warning,
                "No project loaded.",
                Some(Duration::from_secs(4)),
            );
            return;
        };

        let cmd = CoordinatorCommand::Recover { dry_run };

        if let Err(err) = self.gate_coordinator_action(&cmd) {
            self.set_status(
                UiStatusLevel::Error,
                format!(
                    "Failed to run recover: {}",
                    format_actionable_error(&err.to_string())
                ),
                Some(Duration::from_secs(6)),
            );
            return;
        }

        let env_cfg = self.coordinator_env_cfg();
        let req = macc_core::service::coordinator_workflow::CoordinatorCommandRequest {
            canonical: self.config.as_ref(),
            coordinator_cfg: self
                .config
                .as_ref()
                .and_then(|c| c.automation.coordinator.as_ref()),
            env_cfg: &env_cfg,
            logger: None,
        };

        match self.engine.coordinator_execute_command(&paths, cmd, req) {
            Ok(res) => {
                self.refresh_coordinator_snapshot();
                self.refresh_coordinator_events();
                let report_msg = if let Some(reports) = res.recovery_report {
                    format!("Recovery complete: {} tasks classified.", reports.len())
                } else {
                    "Recovery command succeeded.".to_string()
                };
                self.set_status(
                    UiStatusLevel::Success,
                    report_msg,
                    Some(Duration::from_secs(6)),
                );
            }
            Err(err) => {
                self.set_status(
                    UiStatusLevel::Error,
                    format!("Recovery failed: {}", err),
                    Some(Duration::from_secs(6)),
                );
            }
        }
    }

    fn gate_coordinator_action(&self, command: &CoordinatorCommand) -> macc_core::Result<()> {
        if !requires_owner_gate(command) {
            return Ok(());
        }

        let Some(handle) = self.coordinator_handle() else {
            return Ok(());
        };

        gate_owner_action(&self.client_context, &handle)
    }

    fn gate_project_mutation(&self) -> macc_core::Result<()> {
        let Some(handle) = self.project_handle() else {
            return Ok(());
        };

        gate_owner_action(&self.client_context, &handle)
    }

    fn coordinator_client_identity(&self) -> ClientIdentity {
        let mut identity = self.client_identity.clone();
        identity.client_id = self.coordinator_client_id.clone();
        identity.last_heartbeat = chrono::Utc::now().to_rfc3339();
        identity
    }

    pub(crate) fn coordinator_handle(&self) -> Option<ProcessHandle> {
        self.project_paths.as_ref().map(|paths| ProcessHandle {
            kind: ProcessKind::Coordinator,
            project_root: paths.root.clone(),
            pid: None,
        })
    }

    fn project_handle(&self) -> Option<ProcessHandle> {
        self.project_paths.as_ref().map(|paths| ProcessHandle {
            kind: ProcessKind::Project,
            project_root: paths.root.clone(),
            pid: None,
        })
    }

    fn sync_coordinator_ownership_view(&mut self) {
        self.coordinator_ownership.record = self.ownership_state.record.clone();
        self.coordinator_ownership.is_owner = self.ownership_state.is_owner;
        self.coordinator_ownership.pending_incoming_request =
            self.ownership_state.pending_incoming_request.clone();
        self.coordinator_ownership.dismissed_request_id =
            self.ownership_state.dismissed_request_id.clone();
        self.coordinator_ownership.last_refresh = self.ownership_state.last_refresh;
    }

    fn claim_process_ownership(&mut self, handle: ProcessHandle) {
        let Some(paths) = self.project_paths.clone() else {
            return;
        };

        let identity = self.coordinator_client_identity();
        match self
            .engine
            .process_ownership_claim(&paths.root, handle.clone(), identity.clone())
        {
            Ok((OwnershipStatus::Owner, owner_guard, _)) => {
                if self.ownership_guard.is_none() {
                    self.ownership_guard = Some(owner_guard.unwrap_or_else(|| {
                        ProcessOwnershipGuard::new(&paths.root, handle, identity.client_id.clone())
                    }));
                }
            }
            Ok((OwnershipStatus::Viewer, _, viewer_guard)) => {
                self.viewer_guards.push(viewer_guard.unwrap_or_else(|| {
                    ProcessViewerGuard::new(&paths.root, handle, identity.client_id.clone())
                }));
            }
            Ok((OwnershipStatus::Unregistered, _, _)) | Err(_) => {}
        }
    }

    fn wait_for_coordinator_registration(&self, timeout: Duration) -> bool {
        let Some(paths) = self.project_paths.as_ref() else {
            return false;
        };
        let Some(handle) = self.coordinator_handle() else {
            return false;
        };

        let deadline = Instant::now() + timeout;
        while Instant::now() <= deadline {
            match self.engine.process_ownership_status(&paths.root, &handle) {
                Ok(Some(_)) => return true,
                Ok(None) | Err(_) => thread::sleep(Duration::from_millis(100)),
            }
        }

        false
    }

    fn cleanup_ownership_guards(&mut self) {
        self.ownership_guard = None;
        self.viewer_guards.clear();
    }

    pub fn scan_and_attach_to_running_processes(&mut self) {
        let Some(paths) = self.project_paths.clone() else {
            self.cleanup_ownership_guards();
            self.ownership_state.record = None;
            self.ownership_state.is_owner = false;
            self.ownership_state.pending_incoming_request = None;
            self.sync_coordinator_ownership_view();
            return;
        };

        self.cleanup_ownership_guards();

        if let Ok(records) = self.engine.process_list_running(&paths.root) {
            for record in records {
                if matches!(record.process.kind, ProcessKind::Project) {
                    continue;
                }
                self.claim_process_ownership(record.process);
            }
        }

        self.refresh_ownership_state();
    }

    pub fn refresh_ownership_state(&mut self) {
        let Some(paths) = self.project_paths.clone() else {
            self.ownership_state.record = None;
            self.ownership_state.is_owner = false;
            self.ownership_state.pending_incoming_request = None;
            self.sync_coordinator_ownership_view();
            return;
        };

        let mut record = self
            .coordinator_handle()
            .and_then(|handle| {
                self.engine
                    .process_ownership_status(&paths.root, &handle)
                    .ok()
                    .flatten()
            })
            .filter(|record| record.owner.is_some() || record.takeover_request.is_some());

        if record.is_none() {
            record = self.project_handle().and_then(|handle| {
                self.engine
                    .process_ownership_status(&paths.root, &handle)
                    .ok()
                    .flatten()
            });
        }

        let is_owner = record
            .as_ref()
            .and_then(|r| r.owner.as_ref())
            .map(|o| o.client_id == self.client_identity.client_id)
            .unwrap_or(false);

        let active_request_id = record
            .as_ref()
            .and_then(|r| r.takeover_request.as_ref())
            .map(|request| request.request_id.clone());
        if self.ownership_state.dismissed_request_id.as_ref() != active_request_id.as_ref() {
            self.ownership_state.dismissed_request_id = None;
        }

        self.ownership_state.record = record;
        self.ownership_state.is_owner = is_owner;
        self.ownership_state.last_refresh = Instant::now();
        self.ownership_state.pending_incoming_request = self
            .ownership_state
            .record
            .as_ref()
            .and_then(|r| r.takeover_request.as_ref())
            .filter(|request| {
                is_owner
                    && self.ownership_state.dismissed_request_id.as_deref()
                        != Some(request.request_id.as_str())
            })
            .cloned();
        self.last_ownership_refresh = Some(Instant::now());
        self.coordinator_ownership_last_refresh = self.last_ownership_refresh;
        self.sync_coordinator_ownership_view();
    }

    pub fn tick_ownership(&mut self) {
        let Some(paths) = self.project_paths.clone() else {
            return;
        };
        let Some(handle) = self.coordinator_handle().or_else(|| self.project_handle()) else {
            return;
        };

        let now = Instant::now();
        let should_refresh = self
            .last_ownership_refresh
            .map(|t| now.duration_since(t) >= Duration::from_secs(5))
            .unwrap_or(true);
        if should_refresh {
            self.refresh_ownership_state();
            if self.ownership_state.is_owner && self.ownership_guard.is_none() {
                self.claim_process_ownership(handle.clone());
            }

            if self.ownership_state.pending_incoming_request.is_none() {
                if let Some(request) = self
                    .ownership_state
                    .record
                    .as_ref()
                    .and_then(|record| record.takeover_request.clone())
                    .filter(|request| {
                        self.ownership_state.is_owner
                            && self.ownership_state.dismissed_request_id.as_deref()
                                != Some(request.request_id.as_str())
                    })
                {
                    self.handle_takeover_request_received(request);
                }
            }

            let heartbeat_count =
                usize::from(self.ownership_guard.is_some()) + self.viewer_guards.len();
            for _ in 0..heartbeat_count {
                let _ = self.engine.process_heartbeat(
                    &paths.root,
                    &handle,
                    &self.client_identity.client_id,
                );
            }
            self.coordinator_ownership_last_heartbeat = Some(now);
        }
    }

    pub fn ownership_request_takeover(&mut self) {
        let Some(paths) = self.project_paths.clone() else {
            return;
        };
        let Some(handle) = self.coordinator_handle() else {
            return;
        };
        let identity = self.coordinator_client_identity();
        match self
            .engine
            .process_ownership_request_takeover(&paths.root, &handle, identity)
        {
            Ok(_request_id) => {
                self.set_status(
                    UiStatusLevel::Info,
                    "Takeover request sent to current owner.",
                    Some(Duration::from_secs(4)),
                );
                self.refresh_ownership_state();
            }
            Err(err) => {
                self.set_status(
                    UiStatusLevel::Warning,
                    format!("Failed to request takeover: {err}"),
                    Some(Duration::from_secs(5)),
                );
            }
        }
    }

    pub fn ownership_respond_takeover(&mut self, accept: bool) {
        let Some(paths) = self.project_paths.clone() else {
            return;
        };
        let Some(handle) = self.coordinator_handle() else {
            return;
        };
        let Some(request) = self.ownership_state.pending_incoming_request.clone() else {
            return;
        };
        match self.engine.process_ownership_respond_takeover(
            &paths.root,
            &handle,
            &self.client_identity.client_id,
            &request.request_id,
            accept,
        ) {
            Ok(()) => {
                self.ownership_state.dismissed_request_id = None;
                let msg = if accept {
                    "Takeover accepted — control transferred."
                } else {
                    "Takeover rejected."
                };
                self.set_status(UiStatusLevel::Info, msg, Some(Duration::from_secs(4)));
                self.refresh_ownership_state();
            }
            Err(err) => {
                self.set_status(
                    UiStatusLevel::Warning,
                    format!("Failed to respond to takeover: {err}"),
                    Some(Duration::from_secs(5)),
                );
            }
        }
    }

    pub fn release_ownership_on_exit(&mut self) {
        let Some(paths) = self.project_paths.clone() else {
            self.cleanup_ownership_guards();
            return;
        };
        let Some(handle) = self.coordinator_handle().or_else(|| self.project_handle()) else {
            self.cleanup_ownership_guards();
            return;
        };
        self.cleanup_ownership_guards();
        let _ = self.engine.process_ownership_release(
            &paths.root,
            &handle,
            &self.client_identity.client_id,
        );
        let _ = macc_core::service::process_ownership::unregister_viewer(
            &paths.root,
            &handle,
            &self.client_identity.client_id,
        );
        self.ownership_state.record = None;
        self.ownership_state.is_owner = false;
        self.ownership_state.pending_incoming_request = None;
        self.ownership_state.dismissed_request_id = None;
        self.sync_coordinator_ownership_view();
    }

    fn ensure_working_copy(&mut self) {
        if self.working_copy.is_none() {
            self.working_copy = Some(CanonicalConfig::default());
        }
    }

    pub fn load_config(&mut self, start_dir: Option<&std::path::Path>) {
        self.release_ownership_on_exit();

        let current_dir = if let Some(d) = start_dir {
            d.to_path_buf()
        } else {
            self.engine.current_dir()
        };

        match find_project_root(&current_dir) {
            Ok(paths) => {
                self.project_paths = Some(paths.clone());
                self.client_context.project_root = paths.root.clone();
                self.refresh_tools();
                self.refresh_skills();
                self.refresh_mcp_entries();
                self.refresh_logs();
                self.refresh_worktree_status();
                self.refresh_coordinator_snapshot();
                self.refresh_coordinator_events();
                self.scan_and_attach_to_running_processes();
                match self.engine.load_canonical_config(&paths) {
                    Ok(config) => {
                        self.config = Some(config.clone());
                        self.working_copy = Some(config);
                    }
                    Err(e) => {
                        self.errors.push(format!("Failed to load config: {}", e));
                    }
                }
            }
            Err(_) => {
                self.errors.push(
                    "MACC project not found. Run 'macc init' in your repository root to start."
                        .to_string(),
                );
                self.client_context.project_root = current_dir;
                self.worktree_status = None;
                self.refresh_logs();
            }
        }
    }
}

impl AppState {
    pub fn current_screen(&self) -> Screen {
        *self.screen_stack.last().unwrap_or(&Screen::Home)
    }

    pub fn interaction_mode_label(&self) -> &'static str {
        let screen = self.current_screen();
        if (screen == Screen::ToolSettings && self.is_tool_field_editing())
            || (screen == Screen::Automation && self.is_automation_field_editing())
        {
            "edit"
        } else if screen == Screen::Apply {
            "confirm"
        } else {
            "browse"
        }
    }

    pub fn breadcrumbs(&self) -> String {
        if self.screen_stack.is_empty() {
            return "Home".to_string();
        }
        self.screen_stack
            .iter()
            .map(|s| s.title())
            .collect::<Vec<_>>()
            .join(" > ")
    }

    pub fn active_tool_label(&self) -> String {
        if let Some(desc) = self.tool_descriptors.get(self.selected_tool_index) {
            return desc.id.to_string();
        }
        self.working_copy
            .as_ref()
            .and_then(|wc| wc.tools.enabled.first().cloned())
            .unwrap_or_else(|| "(none)".to_string())
    }

    pub fn status_badges(&self) -> Vec<String> {
        let mut badges = Vec::new();
        badges.push(if self.project_paths.is_some() {
            "project:ok".to_string()
        } else {
            "project:none".to_string()
        });
        badges.push(format!("warnings:{}", self.errors.len()));
        if self.is_coordinator_running() {
            let action = self.coordinator_running_command.as_deref().unwrap_or("run");
            badges.push(format!("coord:{}", action));
        } else if self.coordinator_paused {
            badges.push("coord:paused".to_string());
        } else {
            badges.push("coord:off".to_string());
        }
        let offline = self
            .working_copy
            .as_ref()
            .map(|c| c.settings.offline)
            .unwrap_or(false);
        badges.push(if offline {
            "offline:on".to_string()
        } else {
            "offline:off".to_string()
        });
        let cache_ok = self
            .project_paths
            .as_ref()
            .map(|p| self.engine.path_exists(&p.cache_dir))
            .unwrap_or(false);
        badges.push(if cache_ok {
            "cache:ok".to_string()
        } else {
            "cache:missing".to_string()
        });
        if !self.search_query.is_empty() {
            badges.push(format!("search:'{}'", self.search_query));
        }
        badges
    }

    pub fn set_status(
        &mut self,
        level: UiStatusLevel,
        message: impl Into<String>,
        ttl: Option<Duration>,
    ) {
        let expires_at = ttl.map(|d| Instant::now() + d);
        self.ui_status = Some(UiStatus {
            level,
            message: message.into(),
            expires_at,
        });
    }

    pub fn status_line(&self) -> Option<(UiStatusLevel, String)> {
        let status = self.ui_status.as_ref()?;
        Some((status.level, status.message.clone()))
    }

    pub fn is_coordinator_running(&self) -> bool {
        self.coordinator_running_command.is_some()
    }

    pub fn has_coordinator_pause_prompt(&self) -> bool {
        self.coordinator_pause_error.is_some()
    }

    pub fn handle_takeover_request_received(&mut self, request: TakeoverRequest) {
        if self.ownership_state.dismissed_request_id.as_deref() == Some(request.request_id.as_str())
        {
            return;
        }
        self.ownership_state.pending_incoming_request = Some(request);
        self.sync_coordinator_ownership_view();
    }

    pub fn dismiss_takeover_request_modal(&mut self) {
        self.ownership_state.dismissed_request_id = self
            .ownership_state
            .pending_incoming_request
            .as_ref()
            .map(|request| request.request_id.clone());
        self.ownership_state.pending_incoming_request = None;
        self.sync_coordinator_ownership_view();
    }

    pub fn try_owner_action<F>(&mut self, handle: &ProcessHandle, f: F) -> bool
    where
        F: FnOnce(&mut Self),
    {
        match gate_owner_action(&self.client_context, handle) {
            Ok(()) => {
                f(self);
                true
            }
            Err(MaccError::NotProcessOwner { .. }) => {
                self.set_status(
                    UiStatusLevel::Warning,
                    "Viewer mode — press T to request takeover",
                    Some(Duration::from_secs(4)),
                );
                false
            }
            Err(err) => {
                self.set_status(
                    UiStatusLevel::Error,
                    format_actionable_error(&err.to_string()),
                    Some(Duration::from_secs(6)),
                );
                false
            }
        }
    }

    pub fn is_coordinator_paused(&self) -> bool {
        self.coordinator_paused
    }

    pub fn retry_after_coordinator_pause(&mut self) {
        let Some(task_id) = self.coordinator_pause_task_id.clone() else {
            self.resume_after_coordinator_pause();
            return;
        };
        let phase = self
            .coordinator_pause_phase
            .clone()
            .unwrap_or_else(|| "dev".to_string());
        self.coordinator_pause_error = None;
        self.coordinator_pause_command = None;
        self.coordinator_pause_task_id = None;
        self.coordinator_pause_phase = None;
        self.coordinator_pause_next_action = Some(CoordinatorPauseNextAction::RetryPhaseAndRun);
        self.start_managed_coordinator_command(CoordinatorCommand::RetryTaskPhase {
            task_id,
            phase,
            skip: false,
        });
    }

    pub fn skip_after_coordinator_pause(&mut self) {
        let Some(task_id) = self.coordinator_pause_task_id.clone() else {
            self.resume_after_coordinator_pause();
            return;
        };
        let phase = self
            .coordinator_pause_phase
            .clone()
            .unwrap_or_else(|| "dev".to_string());
        self.coordinator_pause_error = None;
        self.coordinator_pause_command = None;
        self.coordinator_pause_task_id = None;
        self.coordinator_pause_phase = None;
        self.coordinator_pause_next_action = Some(CoordinatorPauseNextAction::ResumeRun);
        self.start_managed_coordinator_command(CoordinatorCommand::RetryTaskPhase {
            task_id,
            phase,
            skip: true,
        });
    }

    pub fn open_logs_after_coordinator_pause(&mut self) {
        self.coordinator_pause_error = None;
        self.coordinator_pause_command = None;
        self.coordinator_pause_task_id = None;
        self.coordinator_pause_phase = None;
        self.coordinator_pause_next_action = None;
        self.goto_screen(Screen::Logs);
        self.refresh_logs();
        self.set_status(
            UiStatusLevel::Info,
            "Opened logs for investigation.",
            Some(Duration::from_secs(4)),
        );
    }

    pub fn resume_signal_after_coordinator_pause(&mut self) {
        self.coordinator_pause_error = None;
        self.coordinator_pause_command = None;
        self.coordinator_pause_task_id = None;
        self.coordinator_pause_phase = None;
        self.coordinator_pause_next_action = None;
        self.start_coordinator_command(CoordinatorCommand::ResumePausedRun);
    }

    pub fn resume_after_coordinator_pause(&mut self) {
        let command_name = self
            .coordinator_pause_command
            .clone()
            .unwrap_or_else(|| "run".to_string());
        self.coordinator_pause_error = None;
        self.coordinator_pause_command = None;
        self.coordinator_pause_task_id = None;
        self.coordinator_pause_phase = None;
        self.start_named_coordinator_command(&command_name);
    }

    pub fn stop_after_coordinator_pause(&mut self) {
        self.coordinator_pause_error = None;
        self.coordinator_pause_command = None;
        self.coordinator_pause_task_id = None;
        self.coordinator_pause_phase = None;
        self.coordinator_pause_next_action = None;
        self.set_status(
            UiStatusLevel::Warning,
            "Coordinator paused. Stopped by user.",
            Some(Duration::from_secs(5)),
        );
    }

    pub fn coordinator_elapsed_seconds(&self) -> Option<u64> {
        self.coordinator_running_elapsed_secs
    }

    pub fn coordinator_spinner_frame(&self) -> &'static str {
        if !self.is_coordinator_running() {
            return "";
        }
        let frames = ["|", "/", "-", "\\"];
        let idx = (self.coordinator_spinner_tick as usize) % frames.len();
        frames[idx]
    }

    pub fn tick(&mut self) {
        self.refresh_coordinator_pause_state();
        self.tick_ownership();
        // Advance spinner globally so live task animation also moves when
        // observing a coordinator process started outside this TUI instance.
        self.coordinator_spinner_tick = self.coordinator_spinner_tick.wrapping_add(1);

        if let Some(status) = &self.ui_status {
            if let Some(expire) = status.expires_at {
                if Instant::now() >= expire {
                    self.ui_status = None;
                }
            }
        }

        let mut finished_message: Option<(UiStatusLevel, String)> = None;
        let mut post_success_action: Option<CoordinatorPauseNextAction> = None;
        if let Some(paths) = self.project_paths.as_ref() {
            match self.engine.coordinator_managed_command_state(paths) {
                Ok(CoordinatorManagedCommandState::Succeeded {
                    command,
                    elapsed_secs: elapsed,
                    finish_reason,
                }) => {
                    let base = format!(
                        "Coordinator '{}' finished in {}.",
                        command,
                        format_hms(elapsed)
                    );
                    let msg = match finish_reason {
                        Some(ref reason) => format!("{} {}", base, reason),
                        None => base,
                    };
                    finished_message = Some((UiStatusLevel::Success, msg));
                    post_success_action = self.coordinator_pause_next_action.take();
                    self.coordinator_pause_error = None;
                    self.coordinator_pause_command = None;
                    self.coordinator_pause_task_id = None;
                    self.coordinator_pause_phase = None;
                    self.coordinator_last_result = Some(
                        finished_message
                            .as_ref()
                            .map(|(_, msg)| msg.clone())
                            .unwrap_or_default(),
                    );
                    self.coordinator_running_command = None;
                    self.coordinator_running_elapsed_secs = None;
                    self.refresh_coordinator_snapshot();
                    self.refresh_coordinator_events();
                    if self.coordinator_run_auto_quit {
                        self.release_ownership_on_exit();
                        self.should_quit = true;
                    }
                }
                Ok(CoordinatorManagedCommandState::Failed {
                    command,
                    elapsed_secs: elapsed,
                    reason,
                    task_id,
                    phase,
                }) => {
                    let msg = format!(
                        "Coordinator '{}' failed in {}.\n\nCause: {}",
                        command,
                        format_hms(elapsed),
                        reason.trim()
                    );
                    finished_message = Some((UiStatusLevel::Error, msg.clone()));
                    self.coordinator_pause_error = Some(msg);
                    self.coordinator_pause_command = Some(command);
                    self.coordinator_pause_next_action = None;
                    if let Some(task_id) = task_id {
                        self.coordinator_pause_task_id = Some(task_id);
                        self.coordinator_pause_phase =
                            Some(phase.unwrap_or_else(|| "dev".to_string()));
                    } else {
                        self.coordinator_pause_task_id = None;
                        self.coordinator_pause_phase = None;
                    }
                    self.coordinator_last_result = Some(
                        finished_message
                            .as_ref()
                            .map(|(_, msg)| msg.clone())
                            .unwrap_or_default(),
                    );
                    self.coordinator_running_command = None;
                    self.coordinator_running_elapsed_secs = None;
                    self.refresh_coordinator_snapshot();
                    self.refresh_coordinator_events();
                }
                Ok(CoordinatorManagedCommandState::Running {
                    command,
                    elapsed_secs,
                }) => {
                    self.coordinator_running_command = Some(command);
                    self.coordinator_running_elapsed_secs = Some(elapsed_secs);
                    let should_refresh = self
                        .coordinator_last_refresh
                        .map(|ts| ts.elapsed() >= Duration::from_secs(1))
                        .unwrap_or(true);
                    if should_refresh {
                        self.refresh_coordinator_snapshot();
                        self.refresh_coordinator_events();
                    }
                }
                Ok(CoordinatorManagedCommandState::Idle) => {
                    self.coordinator_running_command = None;
                    self.coordinator_running_elapsed_secs = None;
                }
                Err(err) => {
                    let command_name = self
                        .coordinator_running_command
                        .clone()
                        .unwrap_or_else(|| "run".to_string());
                    self.coordinator_last_result = Some(format_actionable_error(&format!(
                        "Coordinator '{}' poll error: {}",
                        command_name, err
                    )));
                    self.coordinator_running_command = None;
                    self.coordinator_running_elapsed_secs = None;
                    self.coordinator_pause_error = Some(format!(
                        "Coordinator '{}' polling error: {}",
                        command_name, err
                    ));
                    self.coordinator_pause_command = Some(command_name);
                    self.coordinator_pause_task_id = None;
                    self.coordinator_pause_phase = None;
                    self.coordinator_pause_next_action = None;
                    finished_message = Some((
                        UiStatusLevel::Error,
                        "Coordinator polling failed.".to_string(),
                    ));
                }
            }
        }

        if let Some((level, msg)) = finished_message {
            self.set_status(level, msg, Some(Duration::from_secs(5)));
        }

        if let Some(next_action) = post_success_action {
            match next_action {
                CoordinatorPauseNextAction::RetryPhaseAndRun
                | CoordinatorPauseNextAction::ResumeRun => {
                    self.start_coordinator_command(CoordinatorCommand::Run);
                }
            }
        }

        let should_refresh_events = self
            .coordinator_events_last_refresh
            .map(|ts| ts.elapsed() >= Duration::from_secs(1))
            .unwrap_or(true);
        if should_refresh_events {
            self.refresh_coordinator_events();
            self.scan_for_takeover_requests();
        }

        // Watch screen: refresh RuntimeSnapshot every 2 seconds, independent of
        // the CoordinatorLive snapshot so the Observer works without the coordinator
        // live screen ever being visited.
        if self.current_screen() == Screen::Watch {
            let should_refresh_watch = self
                .watch_last_refresh
                .map(|ts| ts.elapsed() >= Duration::from_secs(2))
                .unwrap_or(true);
            if should_refresh_watch {
                self.refresh_watch_snapshot();
            }
        }
    }

    fn scan_for_takeover_requests(&mut self) {
        if !self.ownership_state.is_owner || self.ownership_state.pending_incoming_request.is_some()
        {
            return;
        }
        let Some(paths) = self.project_paths.as_ref() else {
            return;
        };
        let Some(active_request) = self
            .ownership_state
            .record
            .as_ref()
            .and_then(|record| record.takeover_request.clone())
        else {
            return;
        };
        if self.ownership_state.dismissed_request_id.as_deref()
            == Some(active_request.request_id.as_str())
        {
            return;
        }

        let Ok(events) = self.engine.get_coordinator_events(paths) else {
            return;
        };
        let Some(request) = events
            .iter()
            .rev()
            .filter(|event| event.event_type == "takeover_requested")
            .find_map(Self::parse_takeover_request_event)
            .filter(|request| {
                request.request_id == active_request.request_id
                    && request.requester.client_id == active_request.requester.client_id
                    && request.requested_at == active_request.requested_at
            })
        else {
            return;
        };

        self.handle_takeover_request_received(request);
    }

    fn parse_takeover_request_event(event: &CoordinatorEvent) -> Option<TakeoverRequest> {
        let payload = event
            .raw
            .get("payload")
            .and_then(|payload| payload.get("message"))
            .and_then(Value::as_str)
            .and_then(|message| serde_json::from_str::<Value>(message).ok())?;
        let process: ProcessHandle =
            serde_json::from_value(payload.get("process")?.clone()).ok()?;
        if !matches!(
            process.kind,
            ProcessKind::Coordinator | ProcessKind::Project
        ) {
            return None;
        }
        let requester: ClientIdentity =
            serde_json::from_value(payload.get("client")?.clone()).ok()?;
        let request_id = payload.get("request_id")?.as_str()?.to_string();
        Some(TakeoverRequest {
            request_id,
            requester,
            requested_at: event.ts.clone().unwrap_or_default(),
        })
    }

    pub fn take_full_clear(&mut self) -> bool {
        let current = self.current_screen();
        let needs_clear = self.last_screen != Some(current);
        self.last_screen = Some(current);
        needs_clear
    }

    pub fn push_screen(&mut self, screen: Screen) {
        if self.screen_stack.last() == Some(&screen) {
            return;
        }
        self.screen_stack.push(screen);
        self.search_editing = false;
    }

    pub fn pop_screen(&mut self) {
        if self.screen_stack.len() > 1 {
            self.screen_stack.pop();
        }
        self.search_editing = false;
    }

    pub fn goto_screen(&mut self, screen: Screen) {
        self.screen_stack.clear();
        self.screen_stack.push(screen);
        self.search_editing = false;
    }

    pub fn is_searchable_screen(&self) -> bool {
        matches!(
            self.current_screen(),
            Screen::Tools | Screen::Skills | Screen::Agents | Screen::Mcp | Screen::Logs
        )
    }

    pub fn begin_search(&mut self) {
        if self.is_searchable_screen() {
            self.search_editing = true;
        }
    }

    pub fn clear_search(&mut self) {
        self.search_query.clear();
        self.search_editing = false;
    }

    pub fn append_search_char(&mut self, ch: char) {
        if self.search_editing {
            self.search_query.push(ch);
        }
    }

    pub fn pop_search_char(&mut self) {
        if self.search_editing {
            self.search_query.pop();
        }
    }

    pub fn commit_search(&mut self) {
        self.search_editing = false;
    }

    pub fn cancel_search(&mut self) {
        self.search_editing = false;
    }

    pub fn undo_config_change(&mut self) {
        let Some(previous) = self.undo_stack.pop() else {
            self.set_status(
                UiStatusLevel::Info,
                "Undo stack is empty.",
                Some(Duration::from_secs(2)),
            );
            return;
        };
        if let Some(current) = self.working_copy.take() {
            self.redo_stack.push(current);
        }
        self.working_copy = Some(previous);
        self.set_status(
            UiStatusLevel::Success,
            "Undid last config change.",
            Some(Duration::from_secs(3)),
        );
    }

    pub fn redo_config_change(&mut self) {
        let Some(next) = self.redo_stack.pop() else {
            self.set_status(
                UiStatusLevel::Info,
                "Redo stack is empty.",
                Some(Duration::from_secs(2)),
            );
            return;
        };
        if let Some(current) = self.working_copy.take() {
            self.undo_stack.push(current);
        }
        self.working_copy = Some(next);
        self.set_status(
            UiStatusLevel::Success,
            "Redid config change.",
            Some(Duration::from_secs(3)),
        );
    }

    fn snapshot_before_config_change(&mut self) {
        let Some(cfg) = self.working_copy.as_ref() else {
            return;
        };
        self.undo_stack.push(cfg.clone());
        if self.undo_stack.len() > 128 {
            let _ = self.undo_stack.remove(0);
        }
        self.redo_stack.clear();
    }

    pub fn next_tool(&mut self) {
        let visible = self.filtered_tool_indices();
        self.selected_tool_index = next_visible_index(self.selected_tool_index, &visible);
    }

    pub fn prev_tool(&mut self) {
        let visible = self.filtered_tool_indices();
        self.selected_tool_index = prev_visible_index(self.selected_tool_index, &visible);
    }

    pub fn toggle_selected_tool(&mut self) {
        let selected_index = self
            .filtered_tool_indices()
            .into_iter()
            .find(|idx| *idx == self.selected_tool_index)
            .or_else(|| self.filtered_tool_indices().first().copied())
            .unwrap_or(self.selected_tool_index);
        let tool_id = match self.tool_descriptors.get(selected_index) {
            Some(desc) => desc.id.to_string(),
            None => return,
        };
        self.ensure_working_copy();
        self.snapshot_before_config_change();
        if let Some(ref mut wc) = self.working_copy {
            wc.tools.enabled = toggle_vec_item(wc.tools.enabled.clone(), tool_id);
        }
    }

    pub fn is_tool_install_confirmation_open(&self) -> bool {
        self.tool_install_confirm_id.is_some()
    }

    pub fn begin_tool_install_confirmation(&mut self) {
        let Some(descriptor) = self.selected_tool_descriptor() else {
            return;
        };
        let tool_id = descriptor.id.clone();
        let has_install_steps = descriptor.install.is_some();
        let status = self
            .tool_checks
            .iter()
            .find(|tc| tc.tool_id.as_deref() == Some(tool_id.as_str()))
            .map(|tc| tc.status.clone())
            .unwrap_or(macc_core::doctor::ToolStatus::Missing);

        if self.project_paths.is_none() {
            self.errors
                .push("Cannot install tool: no project is loaded.".to_string());
            self.set_status(
                UiStatusLevel::Error,
                "Cannot install tool: no project is loaded.",
                Some(Duration::from_secs(5)),
            );
            return;
        }
        if !has_install_steps {
            self.errors
                .push(format!("Tool '{}' does not define install steps.", tool_id));
            self.set_status(
                UiStatusLevel::Error,
                format!("Tool '{}' has no install steps.", tool_id),
                Some(Duration::from_secs(5)),
            );
            return;
        }
        if matches!(status, macc_core::doctor::ToolStatus::Installed) {
            self.notices
                .push(format!("Tool '{}' is already installed.", tool_id));
            self.set_status(
                UiStatusLevel::Info,
                format!("Tool '{}' is already installed.", tool_id),
                Some(Duration::from_secs(4)),
            );
            return;
        }
        self.tool_install_confirm_id = Some(tool_id);
    }

    pub fn generate_context_for_selected_tool(&mut self) {
        let Some(descriptor) = self.selected_tool_descriptor() else {
            self.set_status(
                UiStatusLevel::Error,
                "No tool selected.",
                Some(Duration::from_secs(4)),
            );
            return;
        };
        let tool_id = descriptor.id.clone();

        let Some(paths) = self.project_paths.clone() else {
            self.errors
                .push("Cannot generate context: no project is loaded.".to_string());
            self.set_status(
                UiStatusLevel::Error,
                "Cannot generate context: no project is loaded.",
                Some(Duration::from_secs(5)),
            );
            return;
        };

        self.set_status(
            UiStatusLevel::Info,
            format!("Generating context for '{}'...", tool_id),
            Some(Duration::from_secs(3)),
        );

        match self.engine.context_generate(
            &paths,
            Some(&tool_id),
            &[],
            false,
            false,
            &macc_core::service::tooling::NoopReporter,
        ) {
            Ok(_) => {
                self.notices
                    .push(format!("Context generation completed for '{}'.", tool_id));
                self.set_status(
                    UiStatusLevel::Success,
                    format!("Generated context for '{}'.", tool_id),
                    Some(Duration::from_secs(4)),
                );
            }
            Err(e) => {
                let detail = e.to_string();
                let actionable = format_actionable_error(&detail);
                self.errors.push(format!(
                    "Context generation failed for '{}': {}",
                    tool_id, actionable
                ));
                self.set_status(
                    UiStatusLevel::Error,
                    format!("Context generation failed for '{}'.", tool_id),
                    Some(Duration::from_secs(8)),
                );
            }
        }
    }

    pub fn cancel_tool_install_confirmation(&mut self) {
        self.tool_install_confirm_id = None;
    }

    pub fn confirm_tool_install(&mut self) {
        let Some(tool_id) = self.tool_install_confirm_id.clone() else {
            return;
        };
        let Some(paths) = self.project_paths.clone() else {
            self.errors
                .push("Cannot install tool: no project is loaded.".to_string());
            return;
        };

        self.tool_install_confirm_id = None;

        match self.engine.tooling_install_tool(
            &paths,
            &tool_id,
            true,
            &macc_core::service::tooling::NoopReporter,
        ) {
            Ok(_) => {
                self.notices
                    .push(format!("Tool '{}' installation completed.", tool_id));
                self.set_status(
                    UiStatusLevel::Success,
                    format!("Installed tool '{}'.", tool_id),
                    Some(Duration::from_secs(4)),
                );
                self.refresh_tool_checks();
            }
            Err(e) => {
                self.errors
                    .push(format!("Tool '{}' installation failed: {}.", tool_id, e));
                self.set_status(
                    UiStatusLevel::Error,
                    format!("Tool '{}' install failed.", tool_id),
                    Some(Duration::from_secs(6)),
                );
                self.refresh_tool_checks();
            }
        }
    }

    pub fn next_tool_field(&mut self) {
        let len = self
            .current_tool_descriptor()
            .map(|d| d.fields.len())
            .unwrap_or(1)
            .max(1);
        self.tool_field_index = next_index(self.tool_field_index, len);
    }

    pub fn prev_tool_field(&mut self) {
        let len = self
            .current_tool_descriptor()
            .map(|d| d.fields.len())
            .unwrap_or(1)
            .max(1);
        self.tool_field_index = prev_index(self.tool_field_index, len);
    }

    pub fn toggle_tool_field(&mut self) {
        let Some(field) = self.current_tool_field().cloned() else {
            return;
        };

        self.ensure_working_copy();

        match field.kind {
            FieldKind::Bool => {
                let current = self
                    .read_bool_at(&field.path)
                    .or(match &field.default {
                        Some(FieldDefault::Bool(value)) => Some(*value),
                        _ => None,
                    })
                    .unwrap_or(false);
                let _ = self.set_value_at(&field.path, Value::Bool(!current));
            }
            FieldKind::Enum(ref options) => {
                let current = self
                    .read_string_at(&field.path)
                    .or_else(|| match &field.default {
                        Some(FieldDefault::Enum(value)) => Some(value.clone()),
                        _ => None,
                    });
                let opts_refs: Vec<&str> = options.iter().map(|s| s.as_str()).collect();
                let next = cycle_value(&opts_refs, current.as_deref().unwrap_or(&options[0]));
                let _ = self.set_value_at(&field.path, Value::String(next.to_string()));
            }
            FieldKind::Text | FieldKind::Number | FieldKind::Array => {
                self.begin_tool_field_edit();
            }
            FieldKind::Action(ref action) => {
                self.handle_action(action);
            }
        }
    }

    pub fn next_automation_field(&mut self) {
        self.automation_field_index =
            next_index(self.automation_field_index, Self::AUTOMATION_FIELD_COUNT);
    }

    pub fn prev_automation_field(&mut self) {
        self.automation_field_index =
            prev_index(self.automation_field_index, Self::AUTOMATION_FIELD_COUNT);
    }

    pub fn next_settings_field(&mut self) {
        self.settings_field_index = next_index(self.settings_field_index, 4);
    }

    pub fn prev_settings_field(&mut self) {
        self.settings_field_index = prev_index(self.settings_field_index, 4);
    }

    pub fn is_automation_field_editing(&self) -> bool {
        // Text editing AND special-mode editors all count as "editing" for
        // purposes of blocking global key handlers.
        self.automation_field_editing
            || self.tool_priority_editor_active
            || self.tool_parallel_editor_active
    }

    pub fn is_settings_field_editing(&self) -> bool {
        self.settings_field_editing
    }

    // ── Unified config screen ─────────────────────────────────────────────────

    /// Tab names shown in the tab bar.
    pub const CONFIG_TAB_NAMES: &'static [&'static str] = &[
        "General",
        "Coordinator",
        "Tools",
        "Phases",
        "Reliability",
        "Admin",
    ];

    /// Returns the field list for the given tab as `(source, field_index)` pairs.
    /// `source` 0 = global (settings), 1 = coordinator (automation).
    pub fn config_tab_fields(tab: usize) -> &'static [(u8, usize)] {
        match tab {
            // General: global settings (quiet, offline, debug, web port)
            0 => &[(0, 0), (0, 1), (0, 3), (0, 2)],
            // Coordinator: core coordinator settings
            1 => &[(1, 0), (1, 1), (1, 7), (1, 6), (1, 8), (1, 32), (1, 33)],
            // Tools: routing and priority
            2 => &[(1, 3), (1, 4), (1, 5), (1, 2)],
            // Phases: pipeline phase controls
            3 => &[
                (1, 34),
                (1, 35),
                (1, 36),
                (1, 37),
                (1, 31),
                (1, 38),
                (1, 39),
            ],
            // Reliability: stale, merge, retry, lifecycle
            4 => &[
                (1, 9),
                (1, 10),
                (1, 11),
                (1, 12),
                (1, 13),
                (1, 17),
                (1, 18),
                (1, 19),
                (1, 20),
                (1, 21),
                (1, 30),
                (1, 23),
                (1, 24),
            ],
            // Admin: rate-limiting, log flush, JSON compat
            5 => &[
                (1, 26),
                (1, 27),
                (1, 28),
                (1, 29),
                (1, 14),
                (1, 15),
                (1, 16),
                (1, 22),
                (1, 25),
            ],
            _ => &[],
        }
    }

    /// Returns the `(source, field_index)` pair for the currently selected config row,
    /// or `None` if the current tab is empty.
    pub fn current_config_field(&self) -> Option<(u8, usize)> {
        let fields = Self::config_tab_fields(self.config_tab_index);
        fields.get(self.config_view_index).copied()
    }

    /// Keep `automation_field_index` and `settings_field_index` in sync with `config_view_index`.
    pub fn sync_config_indices(&mut self) {
        if let Some((source, idx)) = self.current_config_field() {
            if source == 0 {
                self.settings_field_index = idx;
            } else {
                self.automation_field_index = idx;
            }
        }
    }

    /// Move the selection down within the current tab.
    pub fn navigate_config_next(&mut self) {
        let len = Self::config_tab_fields(self.config_tab_index).len();
        if len > 0 {
            self.config_view_index = (self.config_view_index + 1).min(len.saturating_sub(1));
            self.sync_config_indices();
        }
    }

    /// Move the selection up within the current tab.
    pub fn navigate_config_prev(&mut self) {
        self.config_view_index = self.config_view_index.saturating_sub(1);
        self.sync_config_indices();
    }

    /// Switch to the next config tab (wraps around).
    pub fn next_config_tab(&mut self) {
        self.config_tab_index = (self.config_tab_index + 1) % Self::CONFIG_TAB_NAMES.len();
        self.config_view_index = 0;
        self.sync_config_indices();
    }

    /// Switch to the previous config tab (wraps around).
    pub fn prev_config_tab(&mut self) {
        let n = Self::CONFIG_TAB_NAMES.len();
        self.config_tab_index = (self.config_tab_index + n - 1) % n;
        self.config_view_index = 0;
        self.sync_config_indices();
    }

    /// Jump directly to a specific tab by index.
    pub fn jump_config_tab(&mut self, tab: usize) {
        if tab < Self::CONFIG_TAB_NAMES.len() {
            self.config_tab_index = tab;
            self.config_view_index = 0;
            self.sync_config_indices();
        }
    }

    /// Returns the display label for a config field, with the `[Category] ` prefix stripped.
    pub fn config_field_label(&self, source: u8, idx: usize) -> &'static str {
        let raw = if source == 0 {
            self.settings_field_label(idx)
        } else {
            self.automation_field_label(idx)
        };
        // Strip leading "[Category] " prefix for display within a tab
        if let Some(rest) = raw.strip_prefix('[') {
            if let Some(end) = rest.find(']') {
                return rest[end + 1..].trim_start();
            }
        }
        raw
    }

    /// Returns the current display value for a config field.
    pub fn config_field_value(&self, source: u8, idx: usize) -> String {
        if source == 0 {
            self.settings_field_display_value(idx)
        } else {
            self.automation_field_display_value(idx)
        }
    }

    /// Returns the help text for a config field.
    pub fn config_field_help(&self, source: u8, idx: usize) -> &'static str {
        if source == 0 {
            self.settings_field_help(idx)
        } else {
            self.automation_field_help(idx)
        }
    }

    /// Whether any config field is currently in text-editing mode.
    pub fn is_config_editing(&self) -> bool {
        self.automation_field_editing
            || self.tool_priority_editor_active
            || self.tool_parallel_editor_active
            || self.settings_field_editing
    }

    /// Toggle or begin editing for whichever config field is currently selected.
    pub fn toggle_current_config_field(&mut self) {
        if let Some((source, idx)) = self.current_config_field() {
            if source == 0 {
                self.settings_field_index = idx;
                self.toggle_settings_field();
            } else {
                self.automation_field_index = idx;
                self.toggle_automation_field();
            }
        }
    }

    /// Begin text editing for the currently selected config field.
    pub fn begin_current_config_edit(&mut self) {
        if let Some((source, idx)) = self.current_config_field() {
            if source == 0 {
                self.settings_field_index = idx;
                self.begin_settings_field_edit();
            } else {
                self.automation_field_index = idx;
                self.begin_automation_field_edit();
            }
        }
    }

    /// Append a character to whichever config field input is active.
    pub fn append_config_char(&mut self, ch: char) {
        if self.automation_field_editing {
            self.append_automation_field_char(ch);
        } else if self.settings_field_editing {
            self.append_settings_field_char(ch);
        }
    }

    /// Pop the last character from whichever config field input is active.
    pub fn pop_config_char(&mut self) {
        if self.automation_field_editing {
            self.pop_automation_field_char();
        } else if self.settings_field_editing {
            self.pop_settings_field_char();
        }
    }

    /// Commit whichever config field edit is active.
    pub fn commit_config_edit(&mut self) {
        if self.automation_field_editing {
            self.commit_automation_field_edit();
        } else if self.settings_field_editing {
            self.commit_settings_field_edit();
        }
    }

    /// Cancel whichever config field edit is active.
    pub fn cancel_config_edit(&mut self) {
        if self.automation_field_editing {
            self.cancel_automation_field_edit();
        } else if self.settings_field_editing {
            self.cancel_settings_field_edit();
        }
    }

    /// Returns the current config text input value (for the active editing field).
    pub fn config_field_input_display(&self) -> Option<String> {
        if self.automation_field_editing {
            Some(format!("{}_", self.automation_field_input))
        } else if self.settings_field_editing {
            Some(format!("{}_", self.settings_field_input))
        } else {
            None
        }
    }

    pub fn automation_field_label(&self, index: usize) -> &'static str {
        match index {
            0 => "[Basic] Coordinator Tool",
            1 => "[Basic] Reference Branch",
            2 => "[Advanced] PRD File",
            3 => "[Basic] Tool Priority (CSV)",
            4 => "[Advanced] Max Parallel Per Tool (JSON)",
            5 => "[Advanced] Tool Specializations (JSON)",
            6 => "[Advanced] Max Dispatch",
            7 => "[Basic] Max Parallel",
            8 => "[Basic] Timeout Seconds",
            9 => "[Advanced] Phase Runner Max Attempts",
            10 => "[Advanced] Stale Claimed Seconds",
            11 => "[Advanced] Stale In Progress Seconds",
            12 => "[Advanced] Stale Changes Requested Seconds",
            13 => "[Advanced] Stale Action",
            14 => "[Admin] Log Flush Lines",
            15 => "[Admin] Log Flush Interval (ms)",
            16 => "[Admin] JSON Export Debounce (ms)",
            17 => "[Advanced] Merge AI Fix",
            18 => "[Advanced] Merge Job Timeout (s)",
            19 => "[Advanced] Merge Hook Timeout (s)",
            20 => "[Advanced] Ghost Heartbeat Grace (s)",
            21 => "[Advanced] Dispatch Cooldown (s)",
            22 => "[Admin] JSON Compatibility",
            23 => "[Advanced] Retry Error Codes (CSV)",
            24 => "[Advanced] Max Auto Retries",
            25 => "[Admin] Legacy JSON Fallback",
            26 => "[Advanced] RL Backoff Base (s)",
            27 => "[Advanced] RL Backoff Max (s)",
            28 => "[Advanced] RL Fallback Enabled",
            29 => "[Advanced] RL Throttle Parallel",
            30 => "[Advanced] Force-Kill Grace (s)",
            31 => "[Advanced] Max Review Cycles",
            32 => "[Basic] Safety Policy",
            33 => "[Basic] Destructive Actions",
            // §19: Phase pipeline toggles — saved config only; runtime CLI overrides take precedence
            34 => "[Phases] Testing Phase Enabled",
            35 => "[Phases] Testing Phase Mode",
            36 => "[Phases] Review Phase Enabled",
            37 => "[Phases] Review Phase Mode",
            38 => "[Preflight] Require Clean Reference Branch",
            39 => "[Preflight] Preflight Enabled",
            _ => "",
        }
    }

    pub fn automation_field_help(&self, index: usize) -> &'static str {
        match index {
            0 => "Fixed tool for coordinator phase hooks (review/fix). Empty means task/default tool.",
            1 => "Default git branch used when task.base_branch is not set (default: main).",
            2 => "Path to PRD JSON used by coordinator.sh (default: prd.json).",
            3 => "Tool priority order as comma-separated values, e.g. tool-a,tool-b,tool-c.",
            4 => "Per-tool concurrency caps as JSON object, e.g. {\"tool-a\":3,\"tool-b\":2}.",
            5 => "Category routing as JSON object, e.g. {\"frontend\":[\"tool-b\",\"tool-c\"]}.",
            6 => "Total tasks to dispatch per run, 0 means no cap, unset uses default 10.",
            7 => "Maximum concurrent performer runs.",
            8 => "Lock wait timeout in seconds, 0 disables timeout.",
            9 => "Max attempts for phase runner fallback.",
            10 => "Auto-stale timeout for claimed tasks in seconds, 0 disables.",
            11 => "Hard kill timeout for the performer process in seconds. Sends SIGTERM then SIGKILL after force_kill_grace_seconds. 0 disables (no timeout).",
            12 => "Auto-stale timeout for changes_requested tasks in seconds, 0 disables.",
            13 => "Action for stale tasks: block, retry, requeue.",
            14 => "Flush coordinator logs every N lines (0 uses runtime default).",
            15 => "Flush coordinator logs every N milliseconds (0 uses runtime default).",
            16 => "Debounce SQLite -> JSON compatibility export in ms (0 disables debounce).",
            17 => "Enable AI-driven resolution for merge conflicts.",
            18 => "Timeout for git merge operations in seconds.",
            19 => "Timeout for AI merge-fix hook execution in seconds.",
            20 => "Grace period before considering a dead process a 'ghost' in seconds.",
            21 => "Wait time between task dispatch cycles in seconds.",
            22 => "Enable JSON snapshot export for external tool compatibility.",
            23 => "Comma-separated list of error codes that trigger an automatic task retry.",
            24 => "Maximum number of automatic retries for a failed task.",
            25 => "Allow falling back to JSON task registry if SQLite is corrupted or missing.",
            26 => "Minimum backoff delay in seconds on first E601 rate-limit (default: 30).",
            27 => "Maximum backoff delay cap in seconds for exponential growth (default: 300).",
            28 => "When the primary tool is throttled, dispatch to the next tool in priority order.",
            29 => "Reduce effective_max_parallel by 1 on each E601; restore on recovery.",
            30 => "Seconds to wait after a performer signals failure via IPC before force-killing it (default: 30).",
            31 => "Max review cycles per task. 0=skip review, 1=one review+fix (no loopback), N=N loops. Empty=unlimited.",
            32 => "Permitted tool write scopes and validations (strict, standard).",
            33 => "Risk policy for destructive actions (single_confirm, double_confirm).",
            // §19: phase controls — CLI flags (--disable-testing etc.) override these saved settings
            34 => "Enable the dedicated Tester agent phase after the Performer. \
Saved in .macc/macc.yaml; CLI flag --disable-testing/--testing= overrides at runtime without modifying this file.",
            35 => "Tester activation mode: disabled | required | risk_based | manual. \
Saved in .macc/macc.yaml; CLI flag --testing=<mode> overrides at runtime.",
            36 => "Enable the dedicated Reviewer agent phase after testing (or after dev if testing disabled). \
Saved in .macc/macc.yaml; CLI flag --disable-review/--review= overrides at runtime.",
            37 => "Reviewer activation mode: disabled | required | risk_based | manual. \
Saved in .macc/macc.yaml; CLI flag --review=<mode> overrides at runtime.",
            38 => "Block coordinator run when the reference branch worktree has uncommitted changes. \
Default: true. Override per-run with --allow-dirty-reference.",
            39 => "Enable the reference branch preflight gate before coordinator run. \
Default: true. Can be disabled via reference_branch_preflight.enabled: false.",
            _ => "",
        }
    }

    /// §18/§19: Returns a human-readable description of any active CLI runtime override
    /// that affects the given phase settings field (indices 34-37).
    /// Returns `None` when no override is active for that field.
    pub fn phase_override_notice_for_field(&self, index: usize) -> Option<String> {
        let overrides = self.coordinator_phase_overrides.as_deref()?;
        // Only relevant for phase fields
        if !matches!(index, 34..=37) {
            return None;
        }
        // Parse the override tokens to extract testing/review info
        let testing_override = if overrides.contains("[testing:") {
            overrides
                .split_whitespace()
                .find(|t| t.starts_with("[testing:"))
                .and_then(|t| {
                    t.strip_prefix("[testing:")
                        .and_then(|s| s.strip_suffix(']'))
                })
                .map(|m| m.to_string())
        } else {
            None
        };
        let review_override = if overrides.contains("[review:") {
            overrides
                .split_whitespace()
                .find(|t| t.starts_with("[review:"))
                .and_then(|t| t.strip_prefix("[review:").and_then(|s| s.strip_suffix(']')))
                .map(|m| m.to_string())
        } else {
            None
        };
        match index {
            34 | 35 => testing_override.map(|m| {
                format!(
                    "CLI OVERRIDE ACTIVE: --testing={m} (or --disable-testing)\n\
                     Effective mode: {m}\n\
                     The saved config below is NOT in effect for the running coordinator."
                )
            }),
            36 | 37 => review_override.map(|m| {
                format!(
                    "CLI OVERRIDE ACTIVE: --review={m} (or --disable-review)\n\
                     Effective mode: {m}\n\
                     The saved config below is NOT in effect for the running coordinator."
                )
            }),
            _ => None,
        }
    }

    pub fn automation_field_display_value(&self, index: usize) -> String {
        let coordinator = self
            .working_copy
            .as_ref()
            .and_then(|wc| wc.automation.coordinator.as_ref());
        match index {
            0 => coordinator
                .and_then(|c| c.coordinator_tool.clone())
                .unwrap_or_default(),
            1 => coordinator
                .and_then(|c| c.reference_branch.clone())
                .unwrap_or_else(|| "main".to_string()),
            2 => coordinator
                .and_then(|c| c.prd_file.clone())
                .unwrap_or_else(|| "prd.json".to_string()),
            3 => coordinator
                .map(|c| c.tool_priority.join(", "))
                .unwrap_or_default(),
            4 => coordinator
                .map(|c| {
                    serde_json::to_string(&c.max_parallel_per_tool)
                        .unwrap_or_else(|_| "{}".to_string())
                })
                .unwrap_or_else(|| "{}".to_string()),
            5 => coordinator
                .map(|c| {
                    serde_json::to_string(&c.tool_specializations)
                        .unwrap_or_else(|_| "{}".to_string())
                })
                .unwrap_or_else(|| "{}".to_string()),
            6 => coordinator
                .and_then(|c| c.max_dispatch)
                .unwrap_or(10)
                .to_string(),
            7 => coordinator
                .and_then(|c| c.max_parallel)
                .unwrap_or(3)
                .to_string(),
            8 => coordinator
                .and_then(|c| c.timeout_seconds)
                .unwrap_or(0)
                .to_string(),
            9 => coordinator
                .and_then(|c| c.phase_runner_max_attempts)
                .unwrap_or(1)
                .to_string(),
            10 => coordinator
                .and_then(|c| c.stale_claimed_seconds)
                .unwrap_or(0)
                .to_string(),
            11 => coordinator
                .and_then(|c| c.stale_in_progress_seconds)
                .unwrap_or(0)
                .to_string(),
            12 => coordinator
                .and_then(|c| c.stale_changes_requested_seconds)
                .unwrap_or(0)
                .to_string(),
            13 => coordinator
                .and_then(|c| c.stale_action.clone())
                .unwrap_or_else(|| "block".to_string()),
            14 => coordinator
                .and_then(|c| c.log_flush_lines)
                .unwrap_or(0)
                .to_string(),
            15 => coordinator
                .and_then(|c| c.log_flush_ms)
                .unwrap_or(0)
                .to_string(),
            16 => coordinator
                .and_then(|c| c.mirror_json_debounce_ms)
                .unwrap_or(0)
                .to_string(),
            17 => coordinator
                .and_then(|c| c.merge_ai_fix)
                .unwrap_or(false)
                .to_string(),
            18 => coordinator
                .and_then(|c| c.merge_job_timeout_seconds)
                .unwrap_or(0)
                .to_string(),
            19 => coordinator
                .and_then(|c| c.merge_hook_timeout_seconds)
                .unwrap_or(90)
                .to_string(),
            20 => coordinator
                .and_then(|c| c.ghost_heartbeat_grace_seconds)
                .unwrap_or(30)
                .to_string(),
            21 => coordinator
                .and_then(|c| c.dispatch_cooldown_seconds)
                .unwrap_or(2)
                .to_string(),
            22 => coordinator
                .and_then(|c| c.json_compat)
                .unwrap_or(false)
                .to_string(),
            23 => coordinator
                .and_then(|c| c.error_code_retry_list.clone())
                .unwrap_or_else(|| "E101,E102,E103,E301,E302,E303,E601,E603".to_string()),
            24 => coordinator
                .and_then(|c| c.error_code_retry_max)
                .unwrap_or(2)
                .to_string(),
            25 => coordinator
                .and_then(|c| c.legacy_json_fallback)
                .unwrap_or(false)
                .to_string(),
            26 => coordinator
                .and_then(|c| c.rate_limit_backoff_base_seconds)
                .unwrap_or(30)
                .to_string(),
            27 => coordinator
                .and_then(|c| c.rate_limit_backoff_max_seconds)
                .unwrap_or(300)
                .to_string(),
            28 => coordinator
                .and_then(|c| c.rate_limit_fallback_enabled)
                .unwrap_or(true)
                .to_string(),
            29 => coordinator
                .and_then(|c| c.rate_limit_throttle_parallel)
                .unwrap_or(true)
                .to_string(),
            30 => coordinator
                .and_then(|c| c.force_kill_grace_seconds)
                .unwrap_or(30)
                .to_string(),
            31 => coordinator
                .and_then(|c| c.max_review_cycles)
                .map(|v| v.to_string())
                .unwrap_or_default(),
            32 => coordinator
                .and_then(|c| c.safety_policy.clone())
                .unwrap_or_else(|| "standard".to_string()),
            33 => coordinator
                .and_then(|c| c.destructive_actions.clone())
                .unwrap_or_else(|| "double_confirm".to_string()),
            // §19: phase pipeline toggles — show saved config value; annotate if overridden at runtime
            34 => {
                let saved = coordinator
                    .map(|c| c.phases.testing.enabled.to_string())
                    .unwrap_or_else(|| "false".to_string());
                if self.phase_override_notice_for_field(34).is_some() {
                    format!("{saved} [CLI OVERRIDE ACTIVE]")
                } else {
                    saved
                }
            }
            35 => {
                let saved = coordinator
                    .map(|c| c.phases.testing.mode.clone())
                    .unwrap_or_else(|| "disabled".to_string());
                if self.phase_override_notice_for_field(35).is_some() {
                    format!("{saved} [CLI OVERRIDE ACTIVE]")
                } else {
                    saved
                }
            }
            36 => {
                let saved = coordinator
                    .map(|c| c.phases.review.enabled.to_string())
                    .unwrap_or_else(|| "true".to_string());
                if self.phase_override_notice_for_field(36).is_some() {
                    format!("{saved} [CLI OVERRIDE ACTIVE]")
                } else {
                    saved
                }
            }
            37 => {
                let saved = coordinator
                    .map(|c| c.phases.review.mode.clone())
                    .unwrap_or_else(|| "required".to_string());
                if self.phase_override_notice_for_field(37).is_some() {
                    format!("{saved} [CLI OVERRIDE ACTIVE]")
                } else {
                    saved
                }
            }
            38 => {
                let require_clean = coordinator
                    .and_then(|c| c.require_clean_reference_branch)
                    .unwrap_or(true);
                if require_clean {
                    "true (block)".to_string()
                } else {
                    "false (warn)".to_string()
                }
            }
            39 => {
                let enabled = coordinator
                    .and_then(|c| c.reference_branch_preflight.as_ref())
                    .and_then(|p| p.enabled)
                    .unwrap_or(true);
                if enabled {
                    "enabled".to_string()
                } else {
                    "disabled".to_string()
                }
            }
            _ => String::new(),
        }
    }

    pub fn settings_field_count(&self) -> usize {
        3
    }

    pub fn settings_field_label(&self, index: usize) -> &'static str {
        match index {
            0 => "[Basic] Silent Mode",
            1 => "[Basic] Offline Mode",
            2 => "[Basic] Web Interface Port",
            3 => "[Basic] Debug Mode",
            _ => "",
        }
    }

    pub fn settings_field_help(&self, index: usize) -> &'static str {
        match index {
            0 => "Suppress all non-essential output from AI tools.",
            1 => "Disable all remote fetching and updates.",
            2 => "The port number for the MACC web interface.",
            3 => "Enable verbose performer logs: prompt dump, runner line, [MACC] invoke. Equivalent to MACC_DEBUG=1 or macc --verbose.",
            _ => "",
        }
    }

    pub fn settings_field_display_value(&self, index: usize) -> String {
        let Some(config) = &self.working_copy else {
            return String::new();
        };
        match index {
            0 => config.settings.quiet.to_string(),
            1 => config.settings.offline.to_string(),
            2 => config
                .settings
                .web_port
                .map(|p| p.to_string())
                .unwrap_or_else(|| "default (3450)".to_string()),
            3 => config.settings.debug.to_string(),
            _ => String::new(),
        }
    }

    pub fn begin_settings_field_edit(&mut self) {
        self.settings_field_input = self.settings_field_display_value(self.settings_field_index);
        self.settings_field_editing = true;
    }

    pub fn append_settings_field_char(&mut self, ch: char) {
        if self.settings_field_editing {
            self.settings_field_input.push(ch);
        }
    }

    pub fn pop_settings_field_char(&mut self) {
        if self.settings_field_editing {
            self.settings_field_input.pop();
        }
    }

    pub fn cancel_settings_field_edit(&mut self) {
        self.settings_field_editing = false;
    }

    pub fn toggle_settings_field(&mut self) {
        if matches!(self.settings_field_index, 0 | 1 | 3) {
            let current = self.settings_field_display_value(self.settings_field_index) == "true";
            self.set_settings_field_bool(self.settings_field_index, !current);
            return;
        }
        self.begin_settings_field_edit();
    }

    pub fn commit_settings_field_edit(&mut self) {
        if !self.settings_field_editing {
            return;
        }
        let idx = self.settings_field_index;
        let input = self.settings_field_input.trim().to_string();
        self.settings_field_editing = false;

        let result = match idx {
            0 | 1 | 3 => {
                let value = input.to_lowercase();
                if value == "true" {
                    self.set_settings_field_bool(idx, true);
                    Ok(())
                } else if value == "false" {
                    self.set_settings_field_bool(idx, false);
                    Ok(())
                } else {
                    Err("Value must be 'true' or 'false'.".to_string())
                }
            }
            2 => match input.parse::<u16>() {
                Ok(value) => {
                    self.set_settings_field_u16(idx, value);
                    Ok(())
                }
                Err(_) => Err("Invalid port number.".to_string()),
            },
            _ => Ok(()),
        };

        if let Err(err) = result {
            self.errors.push(err);
        }
    }

    fn set_settings_field_bool(&mut self, idx: usize, value: bool) {
        self.snapshot_before_config_change();
        if let Some(config) = &mut self.working_copy {
            match idx {
                0 => config.settings.quiet = value,
                1 => config.settings.offline = value,
                3 => config.settings.debug = value,
                _ => {}
            }
        }
    }

    fn set_settings_field_u16(&mut self, idx: usize, value: u16) {
        self.snapshot_before_config_change();
        if let Some(config) = &mut self.working_copy {
            if idx == 2 {
                config.settings.web_port = Some(value);
            }
        }
    }

    pub fn begin_automation_field_edit(&mut self) {
        // Fields 0, 3, 4 use special editors — never enter text mode for them.
        match self.automation_field_index {
            0 => {
                self.cycle_coordinator_tool(true);
                return;
            }
            3 => {
                self.start_tool_priority_editor();
                return;
            }
            4 => {
                self.start_tool_parallel_editor();
                return;
            }
            _ => {}
        }
        self.automation_field_input =
            self.automation_field_display_value(self.automation_field_index);
        self.automation_field_editing = true;
    }

    pub fn append_automation_field_char(&mut self, ch: char) {
        if self.automation_field_editing {
            self.automation_field_input.push(ch);
        }
    }

    pub fn pop_automation_field_char(&mut self) {
        if self.automation_field_editing {
            self.automation_field_input.pop();
        }
    }

    pub fn cancel_automation_field_edit(&mut self) {
        self.automation_field_editing = false;
    }

    // ── Tool-aware field helpers ──────────────────────────────────────────────

    /// Return the list `["", ...enabled_tools]` used for coordinator tool cycling.
    fn coordinator_tool_options(&self) -> Vec<String> {
        let mut opts = vec![String::new()];
        if let Some(wc) = &self.working_copy {
            opts.extend(wc.tools.enabled.iter().cloned());
        }
        opts
    }

    /// Field 0 – cycle coordinator tool (next/prev) without free-form text.
    pub fn cycle_coordinator_tool(&mut self, forward: bool) {
        let opts = self.coordinator_tool_options();
        if opts.is_empty() {
            return;
        }
        let n = opts.len();
        let current = self
            .working_copy
            .as_ref()
            .and_then(|wc| wc.automation.coordinator.as_ref())
            .and_then(|c| c.coordinator_tool.clone())
            .unwrap_or_default();
        let idx = opts.iter().position(|o| o == &current).unwrap_or(0);
        self.coordinator_tool_cycle_idx = if forward {
            (idx + 1) % n
        } else {
            (idx + n - 1) % n
        };
        let chosen = opts[self.coordinator_tool_cycle_idx].clone();
        self.snapshot_before_config_change();
        if let Some(coordinator) = self.coordinator_config_mut() {
            coordinator.coordinator_tool = if chosen.is_empty() {
                None
            } else {
                Some(chosen)
            };
        }
    }

    /// Return enabled tools ordered by current priority setting (unordered tools appended at end).
    pub fn tool_priority_ordered_list(&self) -> Vec<String> {
        let enabled: Vec<String> = self
            .working_copy
            .as_ref()
            .map(|wc| wc.tools.enabled.clone())
            .unwrap_or_default();
        let explicit: Vec<String> = self
            .working_copy
            .as_ref()
            .and_then(|wc| wc.automation.coordinator.as_ref())
            .map(|c| {
                c.tool_priority
                    .iter()
                    .filter(|t| enabled.contains(t))
                    .cloned()
                    .collect()
            })
            .unwrap_or_default();
        let mut result = explicit;
        for t in &enabled {
            if !result.contains(t) {
                result.push(t.clone());
            }
        }
        result
    }

    /// Field 3 – enter the priority reorder editor.
    pub fn start_tool_priority_editor(&mut self) {
        self.tool_priority_editor_index = 0;
        self.tool_priority_editor_grabbed = false;
        self.tool_priority_editor_active = true;
    }

    /// Field 3 – commit priority editor (saves current order to config).
    pub fn commit_tool_priority_editor(&mut self) {
        let ordered = self.tool_priority_ordered_list();
        self.snapshot_before_config_change();
        if let Some(coordinator) = self.coordinator_config_mut() {
            coordinator.tool_priority = ordered;
        }
        self.tool_priority_editor_grabbed = false;
        self.tool_priority_editor_active = false;
    }

    /// Field 3 – cancel priority editor (releases grab first if held).
    pub fn cancel_tool_priority_editor(&mut self) {
        if self.tool_priority_editor_grabbed {
            // First Esc releases grab without closing the editor.
            self.tool_priority_editor_grabbed = false;
        } else {
            self.tool_priority_editor_active = false;
        }
    }

    /// Field 3 – toggle the "grabbed" state for the currently-selected tool.
    ///
    /// When grabbed = false → ↑/↓ only navigate the cursor.
    /// When grabbed = true  → ↑/↓ move the tool in the list.
    pub fn tool_priority_toggle_grab(&mut self) {
        self.tool_priority_editor_grabbed = !self.tool_priority_editor_grabbed;
    }

    /// Field 3 – ↑ key: navigate cursor up OR move grabbed tool up.
    pub fn tool_priority_editor_up(&mut self) {
        if self.tool_priority_editor_grabbed {
            // Move the grabbed tool to a higher-priority position.
            let mut list = self.tool_priority_ordered_list();
            let idx = self.tool_priority_editor_index;
            if idx == 0 {
                return;
            }
            list.swap(idx - 1, idx);
            self.tool_priority_editor_index = idx - 1;
            self.snapshot_before_config_change();
            if let Some(coordinator) = self.coordinator_config_mut() {
                coordinator.tool_priority = list;
            }
        } else {
            // Navigate cursor without reordering.
            let count = self.tool_priority_ordered_list().len();
            if count > 0 {
                self.tool_priority_editor_index = self.tool_priority_editor_index.saturating_sub(1);
            }
        }
    }

    /// Field 3 – ↓ key: navigate cursor down OR move grabbed tool down.
    pub fn tool_priority_editor_down(&mut self) {
        if self.tool_priority_editor_grabbed {
            // Move the grabbed tool to a lower-priority position.
            let mut list = self.tool_priority_ordered_list();
            let idx = self.tool_priority_editor_index;
            if idx + 1 >= list.len() {
                return;
            }
            list.swap(idx, idx + 1);
            self.tool_priority_editor_index = idx + 1;
            self.snapshot_before_config_change();
            if let Some(coordinator) = self.coordinator_config_mut() {
                coordinator.tool_priority = list;
            }
        } else {
            // Navigate cursor without reordering.
            let count = self.tool_priority_ordered_list().len();
            if count > 0 {
                self.tool_priority_editor_index =
                    (self.tool_priority_editor_index + 1).min(count - 1);
            }
        }
    }

    /// Field 4 – enter the per-tool parallel count editor.
    pub fn start_tool_parallel_editor(&mut self) {
        self.tool_parallel_editor_index = 0;
        self.tool_parallel_editor_active = true;
    }

    /// Field 4 – cancel per-tool parallel editor.
    pub fn cancel_tool_parallel_editor(&mut self) {
        self.tool_parallel_editor_active = false;
    }

    /// Field 4 – navigate between tools in the parallel editor.
    pub fn tool_parallel_editor_select(&mut self, forward: bool) {
        let count = self
            .working_copy
            .as_ref()
            .map(|wc| wc.tools.enabled.len())
            .unwrap_or(0);
        if count == 0 {
            return;
        }
        if forward {
            self.tool_parallel_editor_index = (self.tool_parallel_editor_index + 1).min(count - 1);
        } else {
            self.tool_parallel_editor_index = self.tool_parallel_editor_index.saturating_sub(1);
        }
    }

    /// Field 4 – adjust the parallel count for the currently selected tool by `delta`.
    pub fn tool_parallel_editor_adjust(&mut self, delta: i32) {
        let tool = {
            let enabled = self
                .working_copy
                .as_ref()
                .map(|wc| wc.tools.enabled.clone())
                .unwrap_or_default();
            enabled.into_iter().nth(self.tool_parallel_editor_index)
        };
        let Some(tool) = tool else { return };
        self.snapshot_before_config_change();
        if let Some(coordinator) = self.coordinator_config_mut() {
            let current = coordinator
                .max_parallel_per_tool
                .get(&tool)
                .copied()
                .unwrap_or(1) as i32;
            let next = (current + delta).max(1) as usize;
            coordinator.max_parallel_per_tool.insert(tool, next);
        }
    }

    pub fn toggle_automation_field(&mut self) {
        // Field 0 (Coordinator Tool): cycle through enabled tools instead of free-text.
        if self.automation_field_index == 0 {
            self.cycle_coordinator_tool(true);
            return;
        }
        // Field 3 (Tool Priority): open the reorder editor.
        if self.automation_field_index == 3 {
            self.start_tool_priority_editor();
            return;
        }
        // Field 4 (Max Parallel Per Tool): open the per-tool count editor.
        if self.automation_field_index == 4 {
            self.start_tool_parallel_editor();
            return;
        }
        if self.automation_field_index == 13 {
            let current = self.automation_field_display_value(13);
            let next = match current.as_str() {
                "block" => "retry",
                "retry" => "requeue",
                _ => "block",
            };
            self.set_automation_field_string(13, next.to_string());
            return;
        }
        if self.automation_field_index == 32 {
            let current = self.automation_field_display_value(32);
            let next = match current.as_str() {
                "standard" => "strict",
                _ => "standard",
            };
            self.set_automation_field_string(32, next.to_string());
            return;
        }
        if self.automation_field_index == 33 {
            let current = self.automation_field_display_value(33);
            let next = match current.as_str() {
                "double_confirm" => "single_confirm",
                _ => "double_confirm",
            };
            self.set_automation_field_string(33, next.to_string());
            return;
        }
        if matches!(self.automation_field_index, 17 | 22 | 25 | 28 | 29) {
            let current =
                self.automation_field_display_value(self.automation_field_index) == "true";
            self.set_automation_field_bool(self.automation_field_index, !current);
            return;
        }
        // §19: phase bool toggles (fields 34 = testing.enabled, 36 = review.enabled)
        if matches!(self.automation_field_index, 34 | 36) {
            let raw = self.automation_field_display_value(self.automation_field_index);
            // Strip any "[CLI OVERRIDE ACTIVE]" suffix before parsing
            let current = raw.split_whitespace().next().unwrap_or("false") == "true";
            self.set_automation_phase_bool(self.automation_field_index, !current);
            return;
        }
        // §19: phase mode cycling (fields 35 = testing.mode, 37 = review.mode)
        if matches!(self.automation_field_index, 35 | 37) {
            let raw = self.automation_field_display_value(self.automation_field_index);
            let current = raw.split_whitespace().next().unwrap_or("disabled");
            let next = match current {
                "disabled" => "required",
                "required" => "risk_based",
                "risk_based" => "manual",
                _ => "disabled",
            };
            self.set_automation_phase_mode(self.automation_field_index, next.to_string());
            return;
        }
        self.begin_automation_field_edit();
    }

    pub fn commit_automation_field_edit(&mut self) {
        if !self.automation_field_editing {
            return;
        }
        let idx = self.automation_field_index;
        let input = self.automation_field_input.trim().to_string();
        let result = match idx {
            0..=2 | 23 => {
                if input.is_empty() && idx != 23 {
                    Err("Value cannot be empty.".to_string())
                } else {
                    self.set_automation_field_string(idx, input);
                    Ok(())
                }
            }
            32 => {
                let val = input.to_lowercase();
                if !matches!(val.as_str(), "standard" | "strict") {
                    Err("safety_policy must be standard or strict.".to_string())
                } else {
                    self.set_automation_field_string(32, val);
                    Ok(())
                }
            }
            33 => {
                let val = input.to_lowercase();
                if !matches!(val.as_str(), "single_confirm" | "double_confirm") {
                    Err("destructive_actions must be single_confirm or double_confirm.".to_string())
                } else {
                    self.set_automation_field_string(33, val);
                    Ok(())
                }
            }
            3 => {
                self.set_automation_field_tool_priority(input);
                Ok(())
            }
            4 => self.set_automation_field_tool_caps(input),
            5 => self.set_automation_field_tool_specializations(input),
            6..=12 | 14 | 18 | 24 | 31 => match input.parse::<usize>() {
                Ok(value) => {
                    self.set_automation_field_usize(idx, value);
                    Ok(())
                }
                Err(_) => Err("Invalid integer value.".to_string()),
            },
            15 | 16 | 19 | 21 | 26 | 27 | 30 => match input.parse::<u64>() {
                Ok(value) => {
                    self.set_automation_field_u64(idx, value);
                    Ok(())
                }
                Err(_) => Err("Invalid integer value.".to_string()),
            },
            20 => match input.parse::<i64>() {
                Ok(value) => {
                    self.set_automation_field_i64(idx, value);
                    Ok(())
                }
                Err(_) => Err("Invalid integer value.".to_string()),
            },
            13 => {
                let value = input.to_lowercase();
                if !matches!(value.as_str(), "block" | "retry" | "requeue") {
                    Err("stale_action must be one of: block, retry, requeue.".to_string())
                } else {
                    self.set_automation_field_string(13, value);
                    Ok(())
                }
            }
            17 | 22 | 25 | 28 | 29 => {
                let value = input.to_lowercase();
                if value == "true" {
                    self.set_automation_field_bool(idx, true);
                    Ok(())
                } else if value == "false" {
                    self.set_automation_field_bool(idx, false);
                    Ok(())
                } else {
                    Err("Value must be 'true' or 'false'.".to_string())
                }
            }
            // §19: phase bool/mode fields 34-37 editable as text
            34 | 36 => {
                let value = input.to_lowercase();
                if value == "true" {
                    self.set_automation_phase_bool(idx, true);
                    Ok(())
                } else if value == "false" {
                    self.set_automation_phase_bool(idx, false);
                    Ok(())
                } else {
                    Err("Value must be 'true' or 'false'.".to_string())
                }
            }
            35 | 37 => {
                let value = input.to_lowercase();
                if matches!(
                    value.as_str(),
                    "disabled" | "required" | "risk_based" | "manual"
                ) {
                    self.set_automation_phase_mode(idx, value);
                    Ok(())
                } else {
                    Err("Mode must be one of: disabled, required, risk_based, manual.".to_string())
                }
            }
            // Preflight fields
            38 => {
                let value = input.to_lowercase();
                if value == "true" {
                    self.set_require_clean_reference_branch(true);
                    Ok(())
                } else if value == "false" {
                    self.set_require_clean_reference_branch(false);
                    Ok(())
                } else {
                    Err("Value must be 'true' or 'false'.".to_string())
                }
            }
            39 => {
                let value = input.to_lowercase();
                if value == "true" {
                    self.set_preflight_enabled(true);
                    Ok(())
                } else if value == "false" {
                    self.set_preflight_enabled(false);
                    Ok(())
                } else {
                    Err("Value must be 'true' or 'false'.".to_string())
                }
            }
            _ => Ok(()),
        };

        if let Err(err) = result {
            self.errors.push(err);
            self.set_status(
                UiStatusLevel::Error,
                "Invalid field value.",
                Some(Duration::from_secs(4)),
            );
            return;
        }
        self.automation_field_editing = false;
        self.set_status(
            UiStatusLevel::Success,
            "Automation value updated.",
            Some(Duration::from_secs(3)),
        );
    }

    fn coordinator_config_mut(&mut self) -> Option<&mut CoordinatorConfig> {
        self.ensure_working_copy();
        let wc = self.working_copy.as_mut()?;
        if wc.automation.coordinator.is_none() {
            wc.automation.coordinator = Some(CoordinatorConfig::default());
        }
        wc.automation.coordinator.as_mut()
    }

    fn set_automation_field_string(&mut self, idx: usize, value: String) {
        self.snapshot_before_config_change();
        if let Some(coordinator) = self.coordinator_config_mut() {
            match idx {
                0 => coordinator.coordinator_tool = Some(value),
                1 => coordinator.reference_branch = Some(value),
                2 => coordinator.prd_file = Some(value),
                13 => coordinator.stale_action = Some(value),
                23 => coordinator.error_code_retry_list = Some(value),
                32 => coordinator.safety_policy = Some(value),
                33 => coordinator.destructive_actions = Some(value),
                _ => {}
            }
        }
    }

    fn set_automation_field_usize(&mut self, idx: usize, value: usize) {
        self.snapshot_before_config_change();
        if let Some(coordinator) = self.coordinator_config_mut() {
            match idx {
                6 => coordinator.max_dispatch = Some(value),
                7 => coordinator.max_parallel = Some(value),
                8 => coordinator.timeout_seconds = Some(value),
                9 => coordinator.phase_runner_max_attempts = Some(value),
                10 => coordinator.stale_claimed_seconds = Some(value),
                11 => coordinator.stale_in_progress_seconds = Some(value),
                12 => coordinator.stale_changes_requested_seconds = Some(value),
                14 => coordinator.log_flush_lines = Some(value),
                18 => coordinator.merge_job_timeout_seconds = Some(value),
                24 => coordinator.error_code_retry_max = Some(value),
                31 => coordinator.max_review_cycles = Some(value),
                _ => {}
            }
        }
    }

    fn set_automation_field_u64(&mut self, idx: usize, value: u64) {
        self.snapshot_before_config_change();
        if let Some(coordinator) = self.coordinator_config_mut() {
            match idx {
                15 => coordinator.log_flush_ms = Some(value),
                16 => coordinator.mirror_json_debounce_ms = Some(value),
                19 => coordinator.merge_hook_timeout_seconds = Some(value),
                21 => coordinator.dispatch_cooldown_seconds = Some(value),
                26 => coordinator.rate_limit_backoff_base_seconds = Some(value),
                27 => coordinator.rate_limit_backoff_max_seconds = Some(value),
                30 => coordinator.force_kill_grace_seconds = Some(value),
                _ => {}
            }
        }
    }

    fn set_automation_field_i64(&mut self, idx: usize, value: i64) {
        self.snapshot_before_config_change();
        if let Some(coordinator) = self.coordinator_config_mut() {
            if idx == 20 {
                coordinator.ghost_heartbeat_grace_seconds = Some(value);
            }
        }
    }

    fn set_automation_field_bool(&mut self, idx: usize, value: bool) {
        self.snapshot_before_config_change();
        if let Some(coordinator) = self.coordinator_config_mut() {
            match idx {
                17 => coordinator.merge_ai_fix = Some(value),
                22 => coordinator.json_compat = Some(value),
                25 => coordinator.legacy_json_fallback = Some(value),
                28 => coordinator.rate_limit_fallback_enabled = Some(value),
                29 => coordinator.rate_limit_throttle_parallel = Some(value),
                _ => {}
            }
        }
    }

    /// §19: Set a phase enabled/disabled bool (fields 34 = testing.enabled, 36 = review.enabled).
    fn set_automation_phase_bool(&mut self, idx: usize, value: bool) {
        self.snapshot_before_config_change();
        if let Some(coordinator) = self.coordinator_config_mut() {
            match idx {
                34 => {
                    coordinator.phases.testing.enabled = value;
                    // Keep mode in sync: disabled when unchecked, required when checked
                    if !value {
                        coordinator.phases.testing.mode = "disabled".to_string();
                    } else if coordinator.phases.testing.mode == "disabled" {
                        coordinator.phases.testing.mode = "required".to_string();
                    }
                }
                36 => {
                    coordinator.phases.review.enabled = value;
                    if !value {
                        coordinator.phases.review.mode = "disabled".to_string();
                    } else if coordinator.phases.review.mode == "disabled" {
                        coordinator.phases.review.mode = "required".to_string();
                    }
                }
                _ => {}
            }
        }
    }

    /// §19: Set a phase mode string (fields 35 = testing.mode, 37 = review.mode).
    fn set_automation_phase_mode(&mut self, idx: usize, value: String) {
        self.snapshot_before_config_change();
        if let Some(coordinator) = self.coordinator_config_mut() {
            match idx {
                35 => {
                    coordinator.phases.testing.enabled = value != "disabled";
                    coordinator.phases.testing.mode = value;
                }
                37 => {
                    coordinator.phases.review.enabled = value != "disabled";
                    coordinator.phases.review.mode = value;
                }
                _ => {}
            }
        }
    }

    fn set_require_clean_reference_branch(&mut self, value: bool) {
        self.snapshot_before_config_change();
        if let Some(coordinator) = self.coordinator_config_mut() {
            coordinator.require_clean_reference_branch = Some(value);
        }
    }

    fn set_preflight_enabled(&mut self, value: bool) {
        self.snapshot_before_config_change();
        if let Some(coordinator) = self.coordinator_config_mut() {
            let preflight = coordinator
                .reference_branch_preflight
                .get_or_insert_with(Default::default);
            preflight.enabled = Some(value);
        }
    }

    fn set_automation_field_tool_priority(&mut self, value: String) {
        let parsed = parse_csv_list(&value);
        self.snapshot_before_config_change();
        if let Some(coordinator) = self.coordinator_config_mut() {
            coordinator.tool_priority = parsed;
        }
    }

    fn set_automation_field_tool_caps(&mut self, value: String) -> Result<(), String> {
        let parsed: BTreeMap<String, usize> =
            serde_json::from_str(&value).map_err(|e| format!("Invalid tool caps JSON: {}", e))?;
        self.snapshot_before_config_change();
        if let Some(coordinator) = self.coordinator_config_mut() {
            coordinator.max_parallel_per_tool = parsed;
        }
        Ok(())
    }

    fn set_automation_field_tool_specializations(&mut self, value: String) -> Result<(), String> {
        let parsed: BTreeMap<String, Vec<String>> = serde_json::from_str(&value)
            .map_err(|e| format!("Invalid tool specializations JSON: {}", e))?;
        self.snapshot_before_config_change();
        if let Some(coordinator) = self.coordinator_config_mut() {
            coordinator.tool_specializations = parsed;
        }
        Ok(())
    }

    pub fn is_tool_field_editing(&self) -> bool {
        self.tool_field_editing
    }

    pub fn begin_tool_field_edit(&mut self) {
        let Some(field) = self.current_tool_field() else {
            return;
        };
        match field.kind {
            FieldKind::Text | FieldKind::Number | FieldKind::Array => {
                self.tool_field_input = self.tool_field_display_value(field);
                self.tool_field_editing = true;
            }
            _ => {}
        }
    }

    pub fn append_tool_field_char(&mut self, ch: char) {
        if self.tool_field_editing {
            self.tool_field_input.push(ch);
        }
    }

    pub fn pop_tool_field_char(&mut self) {
        if self.tool_field_editing {
            self.tool_field_input.pop();
        }
    }

    pub fn cancel_tool_field_edit(&mut self) {
        self.tool_field_editing = false;
    }

    pub fn commit_tool_field_edit(&mut self) {
        if !self.tool_field_editing {
            return;
        }
        let Some(field) = self.current_tool_field().cloned() else {
            self.tool_field_editing = false;
            return;
        };

        self.ensure_working_copy();
        let input = self.tool_field_input.trim().to_string();

        let result = match field.kind {
            FieldKind::Text => {
                let _ = self.set_value_at(&field.path, Value::String(input));
                Ok(())
            }
            FieldKind::Number => {
                if input.is_empty() {
                    Err("Number is required.".to_string())
                } else {
                    match input.parse::<f64>() {
                        Ok(value) => match serde_json::Number::from_f64(value) {
                            Some(num) => {
                                let _ = self.set_value_at(&field.path, Value::Number(num));
                                Ok(())
                            }
                            None => Err("Number is out of range.".to_string()),
                        },
                        Err(_) => Err("Invalid number.".to_string()),
                    }
                }
            }
            FieldKind::Array => {
                let items = parse_csv_list(&input);
                let values = items.into_iter().map(Value::String).collect();
                let _ = self.set_value_at(&field.path, Value::Array(values));
                Ok(())
            }
            _ => Ok(()),
        };

        if let Err(err) = result {
            self.errors.push(err);
            self.set_status(
                UiStatusLevel::Error,
                "Invalid field value.",
                Some(Duration::from_secs(4)),
            );
            return;
        }

        self.tool_field_editing = false;
        self.set_status(
            UiStatusLevel::Success,
            "Tool field updated.",
            Some(Duration::from_secs(3)),
        );
    }

    fn handle_action(&mut self, action: &ActionKind) {
        match action {
            ActionKind::OpenSkills { target_pointer } => {
                self.skill_target_path = Some(target_pointer.to_string());
                self.skill_selection_index = 0;
                self.push_screen(Screen::Skills);
            }
            ActionKind::OpenAgents { target_pointer } => {
                self.agent_target_path = Some(target_pointer.to_string());
                self.agent_selection_index = 0;
                self.push_screen(Screen::Agents);
            }
            ActionKind::OpenMcp { .. } => {
                self.goto_screen(Screen::Mcp);
            }
            ActionKind::Custom { .. } => {
                // TODO: handle custom actions
            }
        }
    }

    pub fn current_tool_descriptor(&self) -> Option<&ToolDescriptor> {
        let id = self.current_tool_id.as_deref()?;
        self.tool_descriptors.iter().find(|d| d.id == id)
    }

    pub fn selected_tool_descriptor(&self) -> Option<&ToolDescriptor> {
        let selected_index = self
            .filtered_tool_indices()
            .into_iter()
            .find(|idx| *idx == self.selected_tool_index)
            .or_else(|| self.filtered_tool_indices().first().copied())
            .unwrap_or(self.selected_tool_index);
        self.tool_descriptors.get(selected_index)
    }

    pub fn current_tool_field(&self) -> Option<&ToolField> {
        self.current_tool_descriptor()
            .and_then(|d| d.fields.get(self.tool_field_index))
    }

    pub fn tool_field_display_value(&self, field: &ToolField) -> String {
        match field.kind {
            FieldKind::Bool => {
                let current = self
                    .read_bool_at(&field.path)
                    .or(match &field.default {
                        Some(FieldDefault::Bool(value)) => Some(*value),
                        _ => None,
                    })
                    .unwrap_or(false);
                if current {
                    "on".to_string()
                } else {
                    "off".to_string()
                }
            }
            FieldKind::Enum(ref options) => self
                .read_string_at(&field.path)
                .or(match &field.default {
                    Some(FieldDefault::Enum(value)) => Some(value.clone()),
                    _ => None,
                })
                .unwrap_or_else(|| options[0].to_string()),
            FieldKind::Text => self
                .read_string_at(&field.path)
                .or(match &field.default {
                    Some(FieldDefault::Text(value)) => Some(value.clone()),
                    _ => None,
                })
                .unwrap_or_default(),
            FieldKind::Number => self
                .read_number_at(&field.path)
                .or(match &field.default {
                    Some(FieldDefault::Number(value)) => Some(*value),
                    _ => None,
                })
                .map(format_number)
                .unwrap_or_default(),
            FieldKind::Array => self
                .read_array_at(&field.path)
                .or(match &field.default {
                    Some(FieldDefault::Array(value)) => Some(value.clone()),
                    _ => None,
                })
                .map(|items| items.join(", "))
                .unwrap_or_default(),
            FieldKind::Action(_) => "open...".to_string(),
        }
    }

    pub fn selected_skills(&self) -> Vec<String> {
        let Some(path) = self.skill_target_path.as_deref() else {
            return Vec::new();
        };
        let mut selected = self.read_string_list_at(path);
        selected.extend(self.mandatory_skill_ids());
        selected.sort();
        selected.dedup();
        selected
    }

    pub fn selected_agents(&self) -> Vec<String> {
        let Some(path) = self.agent_target_path.as_deref() else {
            return Vec::new();
        };
        self.read_string_list_at(path)
    }

    pub fn filtered_tool_indices(&self) -> Vec<usize> {
        self.tool_descriptors
            .iter()
            .enumerate()
            .filter_map(|(i, t)| {
                if matches_search(&self.search_query, &[&t.id, &t.title, &t.description]) {
                    Some(i)
                } else {
                    None
                }
            })
            .collect()
    }

    pub fn filtered_skill_indices(&self) -> Vec<usize> {
        self.skills
            .iter()
            .enumerate()
            .filter_map(|(i, s)| {
                if matches_search(&self.search_query, &[&s.id, &s.name, &s.description]) {
                    Some(i)
                } else {
                    None
                }
            })
            .collect()
    }

    pub fn filtered_agent_indices(&self) -> Vec<usize> {
        self.agents
            .iter()
            .enumerate()
            .filter_map(|(i, a)| {
                if matches_search(&self.search_query, &[&a.id, &a.name, &a.description]) {
                    Some(i)
                } else {
                    None
                }
            })
            .collect()
    }

    pub fn filtered_mcp_indices(&self) -> Vec<usize> {
        self.mcp_entries
            .iter()
            .enumerate()
            .filter_map(|(i, m)| {
                if matches_search(&self.search_query, &[&m.id, &m.name, &m.description]) {
                    Some(i)
                } else {
                    None
                }
            })
            .collect()
    }

    pub fn filtered_log_indices(&self) -> Vec<usize> {
        self.log_entries
            .iter()
            .enumerate()
            .filter_map(|(i, e)| {
                if matches_search(&self.search_query, &[&e.relative]) {
                    Some(i)
                } else {
                    None
                }
            })
            .collect()
    }

    fn read_value_at(&self, pointer: &str) -> Option<Value> {
        if pointer.is_empty() {
            return None;
        }
        let wc = self.working_copy.as_ref()?;
        let value = serde_json::to_value(wc).ok()?;
        value.pointer(pointer).cloned()
    }

    fn read_string_at(&self, pointer: &str) -> Option<String> {
        self.read_value_at(pointer)
            .and_then(|v| v.as_str().map(|s| s.to_string()))
    }

    fn read_bool_at(&self, pointer: &str) -> Option<bool> {
        self.read_value_at(pointer).and_then(|v| v.as_bool())
    }

    fn read_number_at(&self, pointer: &str) -> Option<f64> {
        let value = self.read_value_at(pointer)?;
        if let Some(num) = value.as_f64() {
            return Some(num);
        }
        value
            .as_str()
            .and_then(|text| f64::from_str(text.trim()).ok())
    }

    fn read_array_at(&self, pointer: &str) -> Option<Vec<String>> {
        let value = self.read_value_at(pointer)?;
        if let Some(arr) = value.as_array() {
            let mut items = Vec::new();
            for entry in arr {
                if let Some(text) = entry.as_str() {
                    items.push(text.to_string());
                } else {
                    items.push(entry.to_string());
                }
            }
            return Some(items);
        }
        value.as_str().map(parse_csv_list)
    }

    fn read_string_list_at(&self, pointer: &str) -> Vec<String> {
        match self.read_value_at(pointer) {
            Some(Value::Array(values)) => values
                .into_iter()
                .filter_map(|v| v.as_str().map(|s| s.to_string()))
                .collect(),
            _ => Vec::new(),
        }
    }

    fn set_string_list_at(&mut self, pointer: &str, values: Vec<String>) {
        let mut normalized = values;
        if self.skill_target_path.as_deref() == Some(pointer) {
            normalized.extend(self.mandatory_skill_ids());
        }
        normalized.sort();
        normalized.dedup();
        let array = normalized.into_iter().map(Value::String).collect();
        let _ = self.set_value_at(pointer, Value::Array(array));
    }

    fn set_value_at(&mut self, pointer: &str, new_value: Value) -> Result<(), String> {
        if pointer.is_empty() {
            return Ok(());
        }
        self.snapshot_before_config_change();
        let wc = self
            .working_copy
            .as_ref()
            .ok_or_else(|| "No working config loaded.".to_string())?;
        let mut value = serde_json::to_value(wc).map_err(|e| e.to_string())?;
        set_json_pointer(&mut value, pointer, new_value)?;
        let updated: CanonicalConfig = serde_json::from_value(value).map_err(|e| e.to_string())?;
        self.working_copy = Some(updated);
        Ok(())
    }

    pub fn next_mcp(&mut self) {
        let visible = self.filtered_mcp_indices();
        self.mcp_selection_index = next_visible_index(self.mcp_selection_index, &visible);
    }

    pub fn prev_mcp(&mut self) {
        let visible = self.filtered_mcp_indices();
        self.mcp_selection_index = prev_visible_index(self.mcp_selection_index, &visible);
    }

    pub fn toggle_mcp(&mut self) {
        let selected_index = self
            .filtered_mcp_indices()
            .into_iter()
            .find(|idx| *idx == self.mcp_selection_index)
            .or_else(|| self.filtered_mcp_indices().first().copied())
            .unwrap_or(self.mcp_selection_index);
        if self.mcp_entries.is_empty() {
            return;
        }
        let template_id = self.mcp_entries[selected_index].id.to_string();
        self.ensure_working_copy();
        self.snapshot_before_config_change();
        if let Some(ref mut wc) = self.working_copy {
            let selections = wc
                .selections
                .get_or_insert_with(macc_core::config::SelectionsConfig::default);
            selections.mcp = toggle_vec_item(selections.mcp.clone(), template_id);
        }
    }

    pub fn select_all_mcp(&mut self) {
        self.ensure_working_copy();
        self.snapshot_before_config_change();
        if let Some(ref mut wc) = self.working_copy {
            let selections = wc
                .selections
                .get_or_insert_with(macc_core::config::SelectionsConfig::default);
            selections.mcp = self.mcp_entries.iter().map(|t| t.id.to_string()).collect();
            selections.mcp.sort();
        }
    }

    pub fn select_no_mcp(&mut self) {
        self.ensure_working_copy();
        self.snapshot_before_config_change();
        if let Some(ref mut wc) = self.working_copy {
            if let Some(ref mut selections) = wc.selections {
                selections.mcp.clear();
            }
        }
    }

    pub fn next_skill(&mut self) {
        let visible = self.filtered_skill_indices();
        self.skill_selection_index = next_visible_index(self.skill_selection_index, &visible);
    }

    pub fn prev_skill(&mut self) {
        let visible = self.filtered_skill_indices();
        self.skill_selection_index = prev_visible_index(self.skill_selection_index, &visible);
    }

    pub fn toggle_skill(&mut self) {
        let Some(path) = self.skill_target_path.clone() else {
            return;
        };
        let selected_index = self
            .filtered_skill_indices()
            .into_iter()
            .find(|idx| *idx == self.skill_selection_index)
            .or_else(|| self.filtered_skill_indices().first().copied())
            .unwrap_or(self.skill_selection_index);
        self.ensure_working_copy();
        let skill = self.skills[selected_index].clone();
        let skill_id = skill.id;
        if skill.mandatory {
            self.set_status(
                UiStatusLevel::Warning,
                format!("cannot disable mandatory skill '{}'", skill_id),
                Some(Duration::from_secs(4)),
            );
            return;
        }
        let mut skills = self.read_string_list_at(&path);
        skills = toggle_vec_item(skills, skill_id);
        self.set_string_list_at(&path, skills);
    }

    pub fn select_all_skills(&mut self) {
        let Some(path) = self.skill_target_path.clone() else {
            return;
        };
        self.ensure_working_copy();
        let mut skills: Vec<String> = self.skills.iter().map(|s| s.id.to_string()).collect();
        skills.sort();
        skills.dedup();
        self.set_string_list_at(&path, skills);
    }

    pub fn select_no_skills(&mut self) {
        let Some(path) = self.skill_target_path.clone() else {
            return;
        };
        self.ensure_working_copy();
        self.set_string_list_at(&path, self.mandatory_skill_ids());
        self.set_status(
            UiStatusLevel::Info,
            "mandatory skills remain enabled",
            Some(Duration::from_secs(4)),
        );
    }

    pub fn next_agent(&mut self) {
        let visible = self.filtered_agent_indices();
        self.agent_selection_index = next_visible_index(self.agent_selection_index, &visible);
    }

    pub fn prev_agent(&mut self) {
        let visible = self.filtered_agent_indices();
        self.agent_selection_index = prev_visible_index(self.agent_selection_index, &visible);
    }

    pub fn toggle_agent(&mut self) {
        let Some(path) = self.agent_target_path.clone() else {
            return;
        };
        let selected_index = self
            .filtered_agent_indices()
            .into_iter()
            .find(|idx| *idx == self.agent_selection_index)
            .or_else(|| self.filtered_agent_indices().first().copied())
            .unwrap_or(self.agent_selection_index);
        self.ensure_working_copy();
        let agent_id = self.agents[selected_index].id.to_string();
        let mut agents = self.read_string_list_at(&path);
        agents = toggle_vec_item(agents, agent_id);
        self.set_string_list_at(&path, agents);
    }

    pub fn select_all_agents(&mut self) {
        let Some(path) = self.agent_target_path.clone() else {
            return;
        };
        self.ensure_working_copy();
        let mut agents: Vec<String> = self.agents.iter().map(|a| a.id.to_string()).collect();
        agents.sort();
        agents.dedup();
        self.set_string_list_at(&path, agents);
    }

    pub fn select_no_agents(&mut self) {
        let Some(path) = self.agent_target_path.clone() else {
            return;
        };
        self.ensure_working_copy();
        self.set_string_list_at(&path, Vec::new());
    }

    pub fn navigate_next(&mut self) {
        match self.current_screen() {
            Screen::Tools => self.next_tool(),
            // Both Automation and Settings use the unified config navigation.
            Screen::Automation | Screen::Settings => self.navigate_config_next(),
            Screen::Logs => self.next_log(),
            Screen::Skills => self.next_skill(),
            Screen::Agents => self.next_agent(),
            Screen::ToolSettings => self.next_tool_field(),
            Screen::Preview => self.next_preview_op(),
            Screen::Mcp => self.next_mcp(),
            Screen::CoordinatorLive => self.next_live_task(),
            Screen::Watch => {
                let max = self
                    .watch_snapshot
                    .as_ref()
                    .map(|s| s.workers.len())
                    .unwrap_or(0);
                if max > 0 {
                    self.watch_selected_worker =
                        (self.watch_selected_worker + 1).min(max.saturating_sub(1));
                }
            }
            _ => {}
        }
    }

    pub fn navigate_prev(&mut self) {
        match self.current_screen() {
            Screen::Tools => self.prev_tool(),
            Screen::Automation | Screen::Settings => self.navigate_config_prev(),
            Screen::Logs => self.prev_log(),
            Screen::Skills => self.prev_skill(),
            Screen::Agents => self.prev_agent(),
            Screen::ToolSettings => self.prev_tool_field(),
            Screen::Preview => self.prev_preview_op(),
            Screen::Mcp => self.prev_mcp(),
            Screen::CoordinatorLive => self.prev_live_task(),
            Screen::Watch => {
                self.watch_selected_worker = self.watch_selected_worker.saturating_sub(1);
            }
            _ => {}
        }
    }

    pub fn navigate_toggle(&mut self) {
        match self.current_screen() {
            Screen::Tools => self.toggle_selected_tool(),
            Screen::Automation | Screen::Settings => self.toggle_current_config_field(),
            Screen::Skills => self.toggle_skill(),
            Screen::Agents => self.toggle_agent(),
            Screen::ToolSettings => self.toggle_tool_field(),
            Screen::Mcp => self.toggle_mcp(),
            _ => {}
        }
    }

    pub fn navigate_enter(&mut self) {
        match self.current_screen() {
            Screen::Tools => {
                let selected_index = self
                    .filtered_tool_indices()
                    .into_iter()
                    .find(|idx| *idx == self.selected_tool_index)
                    .or_else(|| self.filtered_tool_indices().first().copied())
                    .unwrap_or(self.selected_tool_index);
                let tool_id = match self.tool_descriptors.get(selected_index) {
                    Some(desc) => desc.id.clone(),
                    None => return,
                };
                let is_enabled = self
                    .working_copy
                    .as_ref()
                    .map(|c| c.tools.enabled.contains(&tool_id.to_string()))
                    .unwrap_or(false);

                if is_enabled {
                    self.current_tool_id = Some(tool_id.to_string());
                    self.tool_field_index = 0;
                    self.push_screen(Screen::ToolSettings);
                }
            }
            Screen::Automation | Screen::Settings => self.toggle_current_config_field(),
            Screen::Skills => self.toggle_skill(),
            Screen::Agents => self.toggle_agent(),
            Screen::ToolSettings => self.toggle_tool_field(),
            Screen::Mcp => self.toggle_mcp(),
            Screen::Apply => self.attempt_apply(),
            _ => {}
        }
    }

    pub fn save_config(&mut self) {
        if let Err(err) = self.gate_project_mutation() {
            self.errors.push(format!("Config save blocked: {}", err));
            self.set_status(
                UiStatusLevel::Error,
                format!(
                    "Save blocked: {}",
                    format_actionable_error(&err.to_string())
                ),
                Some(Duration::from_secs(6)),
            );
            return;
        }

        let paths = match &self.project_paths {
            Some(p) => p.clone(),
            None => {
                self.errors.push("No project loaded to save.".to_string());
                return;
            }
        };

        if self.working_copy.is_none() {
            self.errors.push("No project loaded to save.".to_string());
            return;
        }

        self.apply_tool_defaults();
        self.ensure_mandatory_skills_selected();
        let yaml = match self
            .working_copy
            .as_ref()
            .expect("working_copy checked above")
            .to_yaml()
        {
            Ok(y) => y,
            Err(e) => {
                self.errors
                    .push(format!("Failed to serialize config: {}", e));
                return;
            }
        };

        match macc_core::write_if_changed(
            &paths,
            paths.config_path.to_string_lossy().as_ref(),
            &paths.config_path,
            yaml.as_bytes(),
            |_| Ok(()),
        ) {
            Ok(status) => {
                self.config = self.working_copy.clone();
                if status == macc_core::plan::ActionStatus::Unchanged {
                    self.notices
                        .push("Config unchanged, no save needed.".to_string());
                    self.set_status(
                        UiStatusLevel::Info,
                        "Config unchanged.",
                        Some(Duration::from_secs(3)),
                    );
                } else {
                    self.notices.push("Config saved successfully.".to_string());
                    self.set_status(
                        UiStatusLevel::Success,
                        "Config saved.",
                        Some(Duration::from_secs(3)),
                    );
                }
            }
            Err(e) => {
                self.errors.push(format!("Failed to save config: {}", e));
                self.set_status(
                    UiStatusLevel::Error,
                    format!("Save failed: {}", e),
                    Some(Duration::from_secs(6)),
                );
            }
        }
    }

    fn apply_tool_defaults(&mut self) {
        let Some(working_copy) = &self.working_copy else {
            return;
        };

        let enabled = working_copy.tools.enabled.clone();
        let mut defaults = Vec::new();
        for descriptor in &self.tool_descriptors {
            if !enabled.contains(&descriptor.id) {
                continue;
            }
            for field in &descriptor.fields {
                if field.default.is_none() {
                    continue;
                }
                if field.path.is_empty() {
                    continue;
                }
                if self.read_value_at(&field.path).is_some() {
                    continue;
                }
                if let Some(value) = field_default_json(field) {
                    defaults.push((field.path.clone(), value));
                }
            }
        }

        for (path, value) in defaults {
            let _ = self.set_value_at(&path, value);
        }

        self.apply_tool_normalizations();
    }

    fn mandatory_skill_ids(&self) -> Vec<String> {
        self.skills
            .iter()
            .filter(|skill| skill.mandatory)
            .map(|skill| skill.id.clone())
            .collect()
    }

    fn ensure_mandatory_skills_selected(&mut self) {
        let mandatory = self.mandatory_skill_ids();
        if mandatory.is_empty() {
            return;
        }
        let Some(ref mut wc) = self.working_copy else {
            return;
        };
        let selections = wc
            .selections
            .get_or_insert_with(macc_core::config::SelectionsConfig::default);
        selections.skills.extend(mandatory);
        selections.skills.sort();
        selections.skills.dedup();
    }

    fn apply_tool_normalizations(&mut self) {
        let Some(working_copy) = &self.working_copy else {
            return;
        };

        let enabled = working_copy.tools.enabled.clone();
        let mut updates = Vec::new();
        for descriptor in &self.tool_descriptors {
            if !enabled.contains(&descriptor.id) {
                continue;
            }
            for field in &descriptor.fields {
                if field.path.is_empty() {
                    continue;
                }
                match field.kind {
                    FieldKind::Number => {
                        if let Some(Value::String(text)) = self.read_value_at(&field.path) {
                            if let Ok(parsed) = text.trim().parse::<f64>() {
                                if let Some(num) = serde_json::Number::from_f64(parsed) {
                                    updates.push((field.path.clone(), Value::Number(num)));
                                }
                            }
                        }
                    }
                    FieldKind::Array => {
                        if let Some(Value::String(text)) = self.read_value_at(&field.path) {
                            let items = parse_csv_list(&text);
                            let values = items.into_iter().map(Value::String).collect();
                            updates.push((field.path.clone(), Value::Array(values)));
                        }
                    }
                    _ => {}
                }
            }
        }

        for (path, value) in updates {
            let _ = self.set_value_at(&path, value);
        }
    }

    pub fn open_preview(&mut self) {
        if self.current_screen() != Screen::Preview {
            self.push_screen(Screen::Preview);
        }
        self.refresh_preview_plan();
    }

    pub fn refresh_preview_plan(&mut self) {
        self.preview_ops.clear();
        self.preview_diff_cache.clear();
        self.preview_diff_scroll.clear();
        self.preview_error = None;
        self.preview_selection_index = 0;

        let paths = match &self.project_paths {
            Some(paths) => paths,
            None => {
                self.preview_error = Some(
                    "Preview requires a loaded MACC project. Run 'macc init' in the repo root."
                        .to_string(),
                );
                return;
            }
        };

        let canonical = match &self.working_copy {
            Some(cfg) => cfg,
            None => {
                self.preview_error =
                    Some("No canonical configuration available to plan.".to_string());
                return;
            }
        };

        let resolved = resolve(canonical, &CliOverrides::default());
        let fetch_units = match resolve_fetch_units(paths, &resolved) {
            Ok(units) => units,
            Err(e) => {
                self.preview_error = Some(format!("Failed to resolve catalog selections: {}", e));
                return;
            }
        };

        let (quiet, offline) = self
            .working_copy
            .as_ref()
            .map(|c| (c.settings.quiet, c.settings.offline))
            .unwrap_or((false, false));

        let materialized_units = match materialize_fetch_units(paths, fetch_units, quiet, offline) {
            Ok(units) => units,
            Err(e) => {
                self.preview_error = Some(format!("Failed to materialize catalog sources: {}", e));
                return;
            }
        };

        match self.engine.plan(
            paths,
            canonical,
            &materialized_units,
            &CliOverrides::default(),
        ) {
            Ok(plan) => {
                self.preview_ops = self.engine.plan_operations(paths, &plan);
                self.set_preview_selection(0);
            }
            Err(e) => {
                self.preview_error = Some(format!("Planning failed: {}", e));
            }
        }
    }

    fn build_apply_context(&self) -> Result<ApplyContext, String> {
        let paths = self
            .project_paths
            .as_ref()
            .ok_or_else(|| "Apply requires a loaded MACC project.".to_string())?;
        let canonical = self
            .working_copy
            .as_ref()
            .ok_or_else(|| "No configuration available to build an apply plan.".to_string())?;

        let resolved = resolve(canonical, &CliOverrides::default());
        let fetch_units = resolve_fetch_units(paths, &resolved)
            .map_err(|e| format!("Failed to resolve catalog selections: {}", e))?;
        let materialized_units = materialize_fetch_units(
            paths,
            fetch_units,
            canonical.settings.quiet,
            canonical.settings.offline,
        )
        .map_err(|e| format!("Failed to materialize catalog sources: {}", e))?;

        let plan = self
            .engine
            .plan(
                paths,
                canonical,
                &materialized_units,
                &CliOverrides::default(),
            )
            .map_err(|e| format!("Failed to build apply plan: {}", e))?;

        let operations = self.engine.plan_operations(paths, &plan);
        let mut project_ops = 0;
        let mut user_ops = 0;
        for op in &operations {
            match op.scope {
                Scope::Project => project_ops += 1,
                Scope::User => user_ops += 1,
            }
        }

        let backup_preview = format!("{}/<timestamp>", paths.backups_dir.display());
        Ok(ApplyContext {
            plan,
            operations,
            project_ops,
            user_ops,
            backup_preview,
        })
    }

    pub fn open_apply_screen(&mut self) {
        self.apply_consent_input.clear();
        self.apply_user_consent_granted = false;
        self.apply_feedback = None;
        self.apply_error = None;
        self.apply_progress = None;

        match self.build_apply_context() {
            Ok(context) => self.apply_context = Some(context),
            Err(err) => {
                self.apply_context = None;
                self.apply_error = Some(err);
            }
        }

        if self.current_screen() != Screen::Apply {
            self.push_screen(Screen::Apply);
        }
    }

    pub fn append_apply_consent_char(&mut self, ch: char) {
        self.apply_consent_input.push(ch);
        self.apply_user_consent_granted = self.apply_consent_input.eq_ignore_ascii_case("YES");
    }

    pub fn pop_apply_consent_char(&mut self) {
        self.apply_consent_input.pop();
        self.apply_user_consent_granted = self.apply_consent_input.eq_ignore_ascii_case("YES");
    }

    pub fn attempt_apply(&mut self) {
        if let Err(err) = self.gate_project_mutation() {
            self.apply_error = Some(format!(
                "Apply blocked: {}",
                format_actionable_error(&err.to_string())
            ));
            self.set_status(
                UiStatusLevel::Error,
                "Apply blocked by project ownership.",
                Some(Duration::from_secs(6)),
            );
            return;
        }

        let paths = match &self.project_paths {
            Some(paths) => paths,
            None => {
                self.apply_error = Some("No project loaded for apply.".to_string());
                return;
            }
        };

        let context = match &self.apply_context {
            Some(ctx) => ctx,
            None => {
                self.apply_error =
                    Some("No apply context available. Refresh and try again.".to_string());
                return;
            }
        };

        if context.needs_user_consent() && !self.apply_user_consent_granted {
            self.apply_error =
                Some("User-level operations require typing YES before applying.".to_string());
            return;
        }

        let allow_user_scope = !context.needs_user_consent() || self.apply_user_consent_granted;
        let mut plan = context.plan.clone();

        let operations = context.operations.clone();
        self.apply_feedback = None;
        self.apply_error = None;
        self.apply_progress = Some(ApplyProgress {
            current: 0,
            total: operations.len(),
            path: None,
        });

        let result = {
            // For now, engine.apply doesn't support progress callback yet,
            // but we could add it to Engine trait if needed.
            self.engine.apply(paths, &mut plan, allow_user_scope)
        };

        match result {
            Ok(report) => {
                self.apply_feedback = Some(report.render_cli());
                self.apply_error = None;
                self.notices
                    .push("TUI apply completed successfully.".to_string());
                self.set_status(
                    UiStatusLevel::Success,
                    "Apply completed.",
                    Some(Duration::from_secs(5)),
                );
            }
            Err(err) => {
                self.apply_feedback = None;
                self.apply_error = Some(format!("Apply failed: {}", err));
                self.set_status(
                    UiStatusLevel::Error,
                    format!("Apply failed: {}", err),
                    Some(Duration::from_secs(8)),
                );
            }
        }
    }

    pub fn selected_preview_op(&self) -> Option<&PlannedOp> {
        self.preview_ops.get(self.preview_selection_index)
    }

    fn preview_diff_key(op: &PlannedOp) -> String {
        format!("{}|{:?}", op.path, op.kind)
    }

    fn preview_diff_key_for_selected(&self) -> Option<String> {
        self.selected_preview_op().map(Self::preview_diff_key)
    }

    fn ensure_selected_diff_cached(&mut self) {
        if let Some(op) = self.selected_preview_op().cloned() {
            let key = Self::preview_diff_key(&op);
            self.preview_diff_cache
                .entry(key.clone())
                .or_insert_with(|| render_diff(&op));
            self.preview_diff_scroll.entry(key).or_insert(0);
        }
    }

    fn set_preview_selection(&mut self, index: usize) {
        if self.preview_ops.is_empty() {
            self.preview_selection_index = 0;
            return;
        }
        let bounded = index.min(self.preview_ops.len() - 1);
        self.preview_selection_index = bounded;
        self.ensure_selected_diff_cached();
    }

    pub fn next_preview_op(&mut self) {
        if self.preview_ops.is_empty() {
            return;
        }
        let next = (self.preview_selection_index + 1) % self.preview_ops.len();
        self.set_preview_selection(next);
    }

    pub fn prev_preview_op(&mut self) {
        if self.preview_ops.is_empty() {
            return;
        }
        let next = if self.preview_selection_index == 0 {
            self.preview_ops.len() - 1
        } else {
            self.preview_selection_index - 1
        };
        self.set_preview_selection(next);
    }

    pub fn preview_diff_for_selected(&self) -> Option<&DiffView> {
        let key = self.preview_diff_key_for_selected()?;
        self.preview_diff_cache.get(&key)
    }

    pub fn preview_diff_scroll_position(&self) -> usize {
        self.preview_diff_key_for_selected()
            .and_then(|key| self.preview_diff_scroll.get(&key).copied())
            .unwrap_or(0)
    }

    pub fn scroll_preview_diff(&mut self, delta: isize) {
        self.ensure_selected_diff_cached();
        if let Some(key) = self.preview_diff_key_for_selected() {
            if let Some(view) = self.preview_diff_cache.get(&key) {
                let entry = self.preview_diff_scroll.entry(key.clone()).or_insert(0);
                let line_count = view.diff.lines().count();
                let next = if delta < 0 {
                    entry.saturating_sub((-delta) as usize)
                } else {
                    entry.saturating_add(delta as usize)
                };
                *entry = next.min(line_count);
            }
        }
    }

    pub fn toggle_help(&mut self) {
        self.help_open = !self.help_open;
    }

    pub fn current_tool_field_validation(&self) -> Option<String> {
        if !self.is_tool_field_editing() {
            return None;
        }
        let field = self.current_tool_field()?;
        let input = self.tool_field_input.trim();
        match field.kind {
            FieldKind::Number => {
                if input.is_empty() {
                    Some("Number is required.".to_string())
                } else if input.parse::<f64>().is_err() {
                    Some("Invalid number.".to_string())
                } else {
                    None
                }
            }
            FieldKind::Array => None,
            FieldKind::Text => None,
            _ => None,
        }
    }

    pub fn current_automation_field_validation(&self) -> Option<String> {
        if !self.is_automation_field_editing() {
            return None;
        }
        let idx = self.automation_field_index;
        let input = self.automation_field_input.trim();
        match idx {
            0..=2 => {
                if input.is_empty() {
                    Some("Value cannot be empty.".to_string())
                } else {
                    None
                }
            }
            4 => serde_json::from_str::<BTreeMap<String, usize>>(input)
                .err()
                .map(|e| format!("Invalid JSON: {}", e)),
            5 => serde_json::from_str::<BTreeMap<String, Vec<String>>>(input)
                .err()
                .map(|e| format!("Invalid JSON: {}", e)),
            6..=12 | 14 | 18 | 24 => {
                if input.parse::<usize>().is_err() {
                    Some("Invalid integer value.".to_string())
                } else {
                    None
                }
            }
            15 | 16 | 19 | 21 => {
                if input.parse::<u64>().is_err() {
                    Some("Invalid integer value.".to_string())
                } else {
                    None
                }
            }
            20 => {
                if input.parse::<i64>().is_err() {
                    Some("Invalid integer value.".to_string())
                } else {
                    None
                }
            }
            13 => {
                let value = input.to_lowercase();
                if !matches!(value.as_str(), "block" | "retry" | "requeue") {
                    Some("Allowed: block | retry | requeue".to_string())
                } else {
                    None
                }
            }
            // §19: phase bool/mode validation
            34 | 36 => {
                let v = input.to_lowercase();
                if !matches!(v.as_str(), "true" | "false") {
                    Some("Value must be 'true' or 'false'.".to_string())
                } else {
                    None
                }
            }
            35 | 37 => {
                let v = input.to_lowercase();
                if !matches!(
                    v.as_str(),
                    "disabled" | "required" | "risk_based" | "manual"
                ) {
                    Some("Mode must be one of: disabled, required, risk_based, manual.".to_string())
                } else {
                    None
                }
            }
            38 | 39 => {
                let v = input.to_lowercase();
                if !matches!(v.as_str(), "true" | "false") {
                    Some("Value must be 'true' or 'false'.".to_string())
                } else {
                    None
                }
            }
            _ => None,
        }
    }

    pub fn filtered_active_tasks(&self) -> Vec<macc_core::coordinator::view_model::LiveTaskRow> {
        let Some(ref snap) = self.coordinator_snapshot else {
            return Vec::new();
        };
        snap.active_tasks
            .iter()
            .filter(|task| {
                let msg = task.current_message.as_deref().unwrap_or("");
                let phase = task.phase.compact_label();
                let status = task.runtime_status.as_str();
                matches_search(
                    &self.search_query,
                    &[
                        &task.task_id,
                        msg,
                        &task.worker_id,
                        &task.tool,
                        phase,
                        status,
                    ],
                )
            })
            .cloned()
            .collect()
    }

    pub fn selected_live_task(&self) -> Option<macc_core::coordinator::view_model::LiveTaskRow> {
        let tasks = self.filtered_active_tasks();
        if tasks.is_empty() {
            return None;
        }
        let index = self.coordinator_selected_task_index.min(tasks.len() - 1);
        Some(tasks[index].clone())
    }

    pub fn next_live_task(&mut self) {
        let tasks = self.filtered_active_tasks();
        if !tasks.is_empty() {
            self.coordinator_selected_task_index =
                (self.coordinator_selected_task_index + 1) % tasks.len();
        }
    }

    pub fn prev_live_task(&mut self) {
        let tasks = self.filtered_active_tasks();
        if !tasks.is_empty() {
            if self.coordinator_selected_task_index == 0 {
                self.coordinator_selected_task_index = tasks.len() - 1;
            } else {
                self.coordinator_selected_task_index -= 1;
            }
        }
    }

    pub fn toggle_log_pane(&mut self) {
        self.coordinator_log_pane_visible = !self.coordinator_log_pane_visible;
    }

    pub fn get_task_diff(&self, task: &macc_core::coordinator::view_model::LiveTaskRow) -> String {
        let paths = match &self.project_paths {
            Some(p) => p,
            None => return "No project loaded.".to_string(),
        };
        let snap = match self.load_coordinator_storage_snapshot() {
            Ok(s) => s,
            Err(e) => return format!("Failed to load coordinator snapshot: {}", e),
        };
        let reg_task = match snap.registry.tasks.iter().find(|t| t.id == task.task_id) {
            Some(t) => t,
            None => return format!("Task '{}' not found in registry.", task.task_id),
        };

        let worktree_path = reg_task
            .task_runtime
            .worktree
            .as_deref()
            .filter(|s| !s.is_empty())
            .or_else(|| {
                reg_task
                    .worktree
                    .as_ref()
                    .and_then(|w| w.worktree_path.as_deref())
                    .filter(|s| !s.is_empty())
            })
            .map(|p| paths.root.join(p));

        let base_branch = reg_task
            .worktree
            .as_ref()
            .and_then(|w| w.base_branch.clone())
            .filter(|s| !s.is_empty())
            .or_else(|| reg_task.base_branch.clone().filter(|s| !s.is_empty()))
            .unwrap_or_else(|| "main".to_string());

        let commit = reg_task
            .worktree
            .as_ref()
            .and_then(|w| w.last_commit.as_ref());

        if let Some(ref wt) = worktree_path {
            if wt.exists() {
                let diff_target = format!("{}...HEAD", base_branch);
                let args = vec!["diff", &diff_target];
                match macc_core::git::run_git_output_mapped(wt, &args, "git diff worktree") {
                    Ok(output) => return String::from_utf8_lossy(&output.stdout).into_owned(),
                    Err(e) => return format!("Failed to run git diff in worktree: {}", e),
                }
            }
        }

        if let Some(commit_sha) = commit {
            let diff_target = format!("{}...{}", base_branch, commit_sha);
            let args = vec!["diff", &diff_target];
            match macc_core::git::run_git_output_mapped(&paths.root, &args, "git diff commit") {
                Ok(output) => return String::from_utf8_lossy(&output.stdout).into_owned(),
                Err(e) => {
                    return format!("Failed to run git diff for commit {}: {}", commit_sha, e)
                }
            }
        }
        "No diff available (worktree does not exist and no commit recorded).".to_string()
    }

    pub fn get_task_explain(
        &self,
        task: &macc_core::coordinator::view_model::LiveTaskRow,
    ) -> String {
        let paths = match &self.project_paths {
            Some(p) => p,
            None => return "No project loaded.".to_string(),
        };
        let snap = match self.load_coordinator_storage_snapshot() {
            Ok(s) => s,
            Err(e) => return format!("Failed to load coordinator snapshot: {}", e),
        };
        let reg_task = match snap.registry.tasks.iter().find(|t| t.id == task.task_id) {
            Some(t) => t,
            None => return format!("Task '{}' not found in registry.", task.task_id),
        };

        let mut output = String::new();
        let rt = &reg_task.task_runtime;
        let title = reg_task.title.as_deref().unwrap_or("(no title)");
        output.push_str(&format!("{} — {}\n\n", reg_task.id, title));
        output.push_str(&format!("State:     {}\n", reg_task.state));
        if let Some(status) = &rt.status {
            output.push_str(&format!("Runtime:   {}\n", status));
        }
        if let Some(phase) = &rt.current_phase {
            if !phase.is_empty() {
                output.push_str(&format!("Phase:     {}\n", phase));
            }
        }
        if let Some(tool) = &reg_task.tool {
            output.push_str(&format!("Tool:      {}\n", tool));
        }
        if let Some(worker) = &rt.worker_id {
            if !worker.is_empty() {
                output.push_str(&format!("Worker:    {}\n", worker));
            }
        }
        if let Some(worktree) = &rt.worktree {
            if !worktree.is_empty() {
                output.push_str(&format!("Worktree:  {}\n", worktree));
            }
        }
        if let Some(branch) = &rt.branch {
            if !branch.is_empty() {
                output.push_str(&format!("Branch:    {}\n", branch));
            }
        }
        if let Some(started) = &rt.started_at {
            if !started.is_empty() {
                output.push_str(&format!("Started:   {}\n", started));
            }
        }
        if let Some(hb) = &rt.last_heartbeat {
            if !hb.is_empty() {
                output.push_str(&format!("Heartbeat: {}\n", hb));
            }
        }
        if let Some(msg) = &rt.message {
            if !msg.is_empty() {
                output.push_str(&format!("Message:   {}\n", msg));
            }
        }
        if let Some(err) = &rt.last_error {
            if !err.is_empty() {
                output.push_str(&format!("Error:     {}\n", err));
            }
        }

        output.push_str("\nTimeline:\n");
        let events_log_path = rt.events_log.as_deref().map(|p| paths.root.join(p));
        let events_resolved_path = if let Some(ref path) = events_log_path {
            if path.exists() {
                Some(path.clone())
            } else {
                None
            }
        } else {
            let global_events = paths.root.join(".macc/log/events.jsonl");
            if global_events.exists() {
                Some(global_events)
            } else {
                None
            }
        };

        if let Some(ref path) = events_resolved_path {
            use std::io::{BufRead, BufReader};
            if let Ok(file) = std::fs::File::open(path) {
                let reader = BufReader::new(file);
                let mut found = false;
                for line in reader.lines() {
                    let Ok(line) = line else { continue };
                    let Ok(val) = serde_json::from_str::<serde_json::Value>(&line) else {
                        continue;
                    };
                    if events_log_path.is_none() {
                        let event_task = val.get("task_id").and_then(|v| v.as_str()).unwrap_or("");
                        if !event_task.eq_ignore_ascii_case(&task.task_id) {
                            continue;
                        }
                    }
                    let ts = val
                        .get("timestamp")
                        .and_then(|v| v.as_str())
                        .or_else(|| val.get("ts").and_then(|v| v.as_str()))
                        .unwrap_or("-");
                    let phase = val.get("phase").and_then(|v| v.as_str()).unwrap_or("-");
                    let sev = val
                        .get("severity")
                        .and_then(|v| v.as_str())
                        .unwrap_or("info");
                    let message = val
                        .get("message")
                        .and_then(|v| v.as_str())
                        .or_else(|| val.get("msg").and_then(|v| v.as_str()))
                        .unwrap_or("");
                    let time_part = if ts.len() >= 19 { &ts[11..19] } else { ts };
                    output.push_str(&format!(
                        "  {}  {:<6} {:<8} {}\n",
                        time_part, sev, phase, message
                    ));
                    found = true;
                }
                if !found {
                    output.push_str("  (no events found)\n");
                }
            } else {
                output.push_str("  (failed to open events log)\n");
            }
        } else {
            output.push_str("  (no events log found)\n");
        }

        output
    }

    pub fn requeue_selected_task(&mut self, task_id: String) -> Result<(), String> {
        let paths = self
            .project_paths
            .as_ref()
            .ok_or_else(|| "No project loaded.".to_string())?;
        let storage_paths = CoordinatorStoragePaths::from_project_paths(paths);

        let mut snapshot = self.load_coordinator_storage_snapshot()?;

        let task = match snapshot.registry.tasks.iter_mut().find(|t| t.id == task_id) {
            Some(t) => t,
            None => return Err(format!("Task '{}' not found in registry.", task_id)),
        };

        task.state = "queued".to_string();
        task.task_runtime.status = Some("idle".to_string());
        task.task_runtime.pid = None;
        task.task_runtime.started_at = None;
        task.task_runtime.current_phase = None;
        task.task_runtime.message = Some("Requeued by operator via TUI".to_string());
        task.task_runtime.last_error = None;
        task.task_runtime.last_error_code = None;

        snapshot.registry.updated_at = Some(macc_core::coordinator::helpers::now_iso_coordinator());

        let store_sqlite = SqliteStorage::new(storage_paths.clone());
        if let Err(e) = store_sqlite.save_snapshot(&snapshot) {
            return Err(format!("Failed to save snapshot to SQLite: {}", e));
        }

        if self.allow_legacy_json_fallback() {
            let store_json = JsonStorage::new(storage_paths);
            let _ = store_json.save_snapshot(&snapshot);
        }

        self.refresh_coordinator_snapshot();
        Ok(())
    }

    pub fn stop_selected_task(&mut self, task_id: String) {
        self.start_managed_coordinator_command(CoordinatorCommand::KillTask { task_id });
    }
}

// --- Pure Reducer Helpers ---

fn format_actionable_error(raw: &str) -> String {
    let lower = raw.to_ascii_lowercase();
    let (cause, fix) = if lower.contains("registry is missing tasks array")
        || lower.contains("invalid registry json")
    {
        (
            "The coordinator registry is malformed.",
            "Run 'macc coordinator sync' to rebuild .macc/automation/task/task_registry.json from PRD, then retry.",
        )
    } else if lower.contains("not found") || lower.contains("no such file") {
        (
            "A required file or command is missing.",
            "Check paths in Automation settings, run 'macc init' in project root, then retry.",
        )
    } else if lower.contains("permission denied") {
        (
            "MACC cannot execute a required script/binary.",
            "Ensure executable permissions (chmod +x) and that your user can access the project files.",
        )
    } else if lower.contains("failed with status") {
        (
            "A coordinator action exited with a non-zero status.",
            "Open the latest file in .macc/log/coordinator/ and resolve the first reported failure cause.",
        )
    } else {
        (
            "Coordinator command failed.",
            "Open logs in .macc/log/coordinator/ and .macc/log/performer/, then rerun the action.",
        )
    };
    format!("{}\n\nCause: {}\nSuggested fix: {}", raw, cause, fix)
}

fn next_index(current: usize, total: usize) -> usize {
    if total == 0 {
        return 0;
    }
    (current + 1) % total
}

fn prev_index(current: usize, total: usize) -> usize {
    if total == 0 {
        return 0;
    }
    if current == 0 {
        total - 1
    } else {
        current - 1
    }
}

fn next_visible_index(current: usize, visible: &[usize]) -> usize {
    if visible.is_empty() {
        return current;
    }
    if let Some(pos) = visible.iter().position(|idx| *idx == current) {
        return visible[(pos + 1) % visible.len()];
    }
    visible[0]
}

fn prev_visible_index(current: usize, visible: &[usize]) -> usize {
    if visible.is_empty() {
        return current;
    }
    if let Some(pos) = visible.iter().position(|idx| *idx == current) {
        if pos == 0 {
            return visible[visible.len() - 1];
        }
        return visible[pos - 1];
    }
    visible[0]
}

fn matches_search(query: &str, fields: &[&str]) -> bool {
    let q = query.trim().to_ascii_lowercase();
    if q.is_empty() {
        return true;
    }
    fields
        .iter()
        .any(|f| f.to_ascii_lowercase().contains(q.as_str()))
}

fn toggle_vec_item(mut vec: Vec<String>, item: String) -> Vec<String> {
    if vec.contains(&item) {
        vec.retain(|i| i != &item);
    } else {
        vec.push(item);
        vec.sort();
        vec.dedup();
    }
    vec
}

fn field_default_json(field: &ToolField) -> Option<Value> {
    match &field.default {
        Some(FieldDefault::Bool(value)) => Some(Value::Bool(*value)),
        Some(FieldDefault::Text(value)) => Some(Value::String(value.clone())),
        Some(FieldDefault::Enum(value)) => Some(Value::String(value.clone())),
        Some(FieldDefault::Number(value)) => {
            serde_json::Number::from_f64(*value).map(Value::Number)
        }
        Some(FieldDefault::Array(value)) => Some(Value::Array(
            value.iter().cloned().map(Value::String).collect(),
        )),
        None => None,
    }
}

fn cycle_value<'a>(options: &'a [&'a str], current: &str) -> &'a str {
    let current_idx = options.iter().position(|&m| m == current).unwrap_or(0);
    let next_idx = (current_idx + 1) % options.len();
    options[next_idx]
}

fn set_json_pointer(root: &mut Value, pointer: &str, new_value: Value) -> Result<(), String> {
    if pointer.is_empty() {
        return Ok(());
    }
    let tokens = pointer
        .trim_start_matches('/')
        .split('/')
        .map(decode_pointer_token)
        .collect::<Vec<_>>();

    let mut current = root;
    for (idx, token) in tokens.iter().enumerate() {
        let is_last = idx == tokens.len() - 1;
        match current {
            Value::Object(map) => {
                if is_last {
                    map.insert(token.clone(), new_value);
                    return Ok(());
                }
                current = map
                    .entry(token.clone())
                    .or_insert_with(|| Value::Object(Map::new()));
            }
            _ => {
                return Err(format!("Cannot set pointer at non-object: {}", pointer));
            }
        }
    }
    Ok(())
}

fn decode_pointer_token(token: &str) -> String {
    token.replace("~1", "/").replace("~0", "~")
}

fn parse_csv_list(value: &str) -> Vec<String> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return Vec::new();
    }
    trimmed
        .split(',')
        .map(|entry| entry.trim())
        .filter(|entry| !entry.is_empty())
        .map(|entry| entry.to_string())
        .collect()
}

fn format_number(value: f64) -> String {
    if value.fract() == 0.0 {
        format!("{:.0}", value)
    } else {
        value.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use macc_core::plan::{PlannedOpKind, PlannedOpMetadata, Scope};
    use macc_core::process_ownership::{ClientIdentity, ClientKind, ProcessHandle, ProcessKind};
    use macc_core::service::process_ownership::{claim_owner, register_process};
    use macc_core::tool::ToolDiagnostic;
    use macc_core::{MaccEngine, ToolRegistry};
    use std::cell::RefCell;
    use std::fs;
    use tempfile::tempdir;

    fn fixture_ids() -> Vec<String> {
        macc_core::TestEngine::generate_fixture_ids(2)
    }

    fn fixture_engine(ids: &[String]) -> Arc<macc_core::TestEngine> {
        Arc::new(macc_core::TestEngine::with_fixtures_for_ids(ids))
    }

    #[derive(Default)]
    struct NoopEngine {
        stop_calls: RefCell<usize>,
    }

    impl Engine for NoopEngine {
        fn list_tools(&self, _paths: &ProjectPaths) -> (Vec<ToolDescriptor>, Vec<ToolDiagnostic>) {
            (Vec::new(), Vec::new())
        }

        fn doctor(&self, _paths: &ProjectPaths) -> Vec<ToolCheck> {
            Vec::new()
        }

        fn plan(
            &self,
            _paths: &ProjectPaths,
            _config: &CanonicalConfig,
            _materialized_units: &[macc_core::resolve::MaterializedFetchUnit],
            _overrides: &macc_core::resolve::CliOverrides,
        ) -> macc_core::Result<ActionPlan> {
            Ok(ActionPlan::default())
        }

        fn plan_operations(&self, _paths: &ProjectPaths, _plan: &ActionPlan) -> Vec<PlannedOp> {
            Vec::new()
        }

        fn apply(
            &self,
            _paths: &ProjectPaths,
            _plan: &mut ActionPlan,
            _allow_user_scope: bool,
        ) -> macc_core::Result<macc_core::ApplyReport> {
            Ok(macc_core::ApplyReport::default())
        }

        fn builtin_agents(&self) -> Vec<Agent> {
            Vec::new()
        }

        fn coordinator_stop_managed_command_process(
            &self,
            _paths: &ProjectPaths,
            _graceful: bool,
        ) -> macc_core::Result<macc_core::service::coordinator::CoordinatorStopResult> {
            *self.stop_calls.borrow_mut() += 1;
            Ok(macc_core::service::coordinator::CoordinatorStopResult {
                targets: 0,
                used_group: false,
            })
        }
    }

    fn sample_cli_client(client_id: &str) -> ClientIdentity {
        let now = Utc::now().to_rfc3339();
        ClientIdentity {
            client_id: client_id.to_string(),
            kind: ClientKind::Cli,
            connected_at: now.clone(),
            last_heartbeat: now,
        }
    }

    fn sample_project(dir: &std::path::Path) {
        let macc_dir = dir.join(".macc");
        fs::create_dir_all(&macc_dir).expect("create .macc");
        fs::write(macc_dir.join("macc.yaml"), "tools:\n  enabled: []\n").expect("write config");
    }

    #[test]
    fn test_navigation_stack() {
        let engine = Arc::new(MaccEngine::new(ToolRegistry::new()));
        let mut state = AppState::with_engine(engine);
        assert_eq!(state.current_screen(), Screen::Home);

        state.push_screen(Screen::About);
        assert_eq!(state.current_screen(), Screen::About);
        assert_eq!(state.screen_stack.len(), 2);

        state.pop_screen();
        assert_eq!(state.current_screen(), Screen::Home);
        assert_eq!(state.screen_stack.len(), 1);

        // Cannot pop last screen
        state.pop_screen();
        assert_eq!(state.current_screen(), Screen::Home);
        assert_eq!(state.screen_stack.len(), 1);
    }

    #[test]
    fn test_goto_screen() {
        let engine = Arc::new(MaccEngine::new(ToolRegistry::new()));
        let mut state = AppState::with_engine(engine);
        state.push_screen(Screen::About);
        state.goto_screen(Screen::Home);
        assert_eq!(state.current_screen(), Screen::Home);
        assert_eq!(state.screen_stack.len(), 1);
    }

    #[test]
    fn test_toggle_help() {
        let engine = Arc::new(MaccEngine::new(ToolRegistry::new()));
        let mut state = AppState::with_engine(engine);
        assert!(!state.help_open);
        state.toggle_help();
        assert!(state.help_open);
        state.toggle_help();
        assert!(!state.help_open);
    }

    #[test]
    fn test_load_config_valid() {
        let dir = tempdir().unwrap();
        let macc_dir = dir.path().join(".macc");
        fs::create_dir(&macc_dir).unwrap();
        let ids = fixture_ids();
        let tool_one = ids[0].clone();
        fs::write(
            macc_dir.join("macc.yaml"),
            format!("tools:\n  enabled:\n    - {}\n", tool_one),
        )
        .unwrap();

        let engine = fixture_engine(&ids);
        let mut state = AppState::with_engine(engine);
        state.load_config(Some(dir.path()));

        assert!(state.errors.is_empty());
        assert!(state.config.is_some());
        assert_eq!(state.config.unwrap().tools.enabled, vec![tool_one]);
    }

    #[test]
    fn test_load_config_missing() {
        let dir = tempdir().unwrap();
        let engine = Arc::new(macc_core::TestEngine::with_fixtures());
        let mut state = AppState::with_engine(engine);
        state.load_config(Some(dir.path()));

        assert!(!state.errors.is_empty());
        assert!(state.errors[0].contains("MACC project not found"));
        assert!(state.config.is_none());
    }

    #[test]
    fn test_load_config_invalid_yaml() {
        let dir = tempdir().unwrap();
        let macc_dir = dir.path().join(".macc");
        fs::create_dir(&macc_dir).unwrap();
        fs::write(macc_dir.join("macc.yaml"), "tools: [invalid").unwrap();

        let engine = Arc::new(macc_core::TestEngine::with_fixtures());
        let mut state = AppState::with_engine(engine);
        state.load_config(Some(dir.path()));

        assert!(!state.errors.is_empty());
        assert!(state.errors[0].contains("Failed to load config"));
        assert!(state.config.is_none());
    }

    #[test]
    fn test_save_config() {
        let dir = tempdir().unwrap();
        let macc_dir = dir.path().join(".macc");
        fs::create_dir(&macc_dir).unwrap();
        let config_path = macc_dir.join("macc.yaml");
        let ids = fixture_ids();
        let tool_one = ids[0].clone();
        let tool_two = ids[1].clone();
        fs::write(
            &config_path,
            format!("tools:\n  enabled:\n    - {}\n", tool_one),
        )
        .unwrap();

        let engine = fixture_engine(&ids);
        let mut state = AppState::with_engine(engine);
        state.load_config(Some(dir.path()));

        // Modify working copy
        if let Some(ref mut wc) = state.working_copy {
            wc.tools.enabled.push(tool_two.clone());
        }

        state.save_config();

        assert!(state.errors.is_empty());
        assert!(state.notices[0].contains("saved successfully"));

        // Verify file content
        let saved_yaml = fs::read_to_string(&config_path).unwrap();
        assert!(saved_yaml.contains(&tool_one));
        assert!(saved_yaml.contains(&tool_two));

        // Verify idempotence
        state.notices.clear();
        state.save_config();
        assert!(state.notices[0].contains("unchanged"));
    }

    #[test]
    fn stop_coordinator_command_shows_not_owner_error_for_viewer() {
        let dir = tempdir().expect("tempdir");
        sample_project(dir.path());
        let handle = ProcessHandle {
            kind: ProcessKind::Coordinator,
            project_root: dir.path().to_path_buf(),
            pid: Some(4242),
        };
        register_process(dir.path(), handle.clone()).expect("register process");
        claim_owner(dir.path(), &handle, sample_cli_client("client-A")).expect("claim owner");

        let engine = Arc::new(NoopEngine::default());
        let mut state = AppState::with_engine(engine.clone());
        state.project_paths = Some(ProjectPaths::from_root(dir.path()));
        state.coordinator_client_id = "client-B".to_string();
        state.client_identity.client_id = "client-B".to_string();
        state.client_context = ClientContext {
            client_id: "client-B".to_string(),
            project_root: dir.path().to_path_buf(),
        };

        state.stop_coordinator_command();

        let status = state.ui_status.expect("status");
        assert_eq!(status.level, UiStatusLevel::Error);
        assert!(status.message.contains("Failed to run 'stop'"));
        assert!(status.message.contains("not the owner of this process"));
        assert_eq!(*engine.stop_calls.borrow(), 0);
    }

    #[test]
    fn scan_and_attach_to_running_processes_claims_unowned_running_coordinator() {
        let dir = tempdir().expect("tempdir");
        sample_project(dir.path());
        let handle = ProcessHandle {
            kind: ProcessKind::Coordinator,
            project_root: dir.path().to_path_buf(),
            pid: Some(4242),
        };
        register_process(dir.path(), handle).expect("register process");

        let engine = Arc::new(NoopEngine::default());
        let mut state = AppState::with_engine(engine);
        state.project_paths = Some(ProjectPaths::from_root(dir.path()));
        state.client_context.project_root = dir.path().to_path_buf();

        state.scan_and_attach_to_running_processes();

        assert!(state.ownership_guard.is_some());
        assert!(state.ownership_state.is_owner);
        assert!(state.viewer_guards.is_empty());
    }

    #[test]
    fn second_tui_attaches_as_viewer_when_owner_already_exists() {
        let dir = tempdir().expect("tempdir");
        sample_project(dir.path());
        let handle = ProcessHandle {
            kind: ProcessKind::Coordinator,
            project_root: dir.path().to_path_buf(),
            pid: Some(4242),
        };
        register_process(dir.path(), handle).expect("register process");

        let engine = Arc::new(NoopEngine::default());
        let mut first = AppState::with_engine(engine.clone());
        first.project_paths = Some(ProjectPaths::from_root(dir.path()));
        first.client_context.project_root = dir.path().to_path_buf();
        first.scan_and_attach_to_running_processes();
        assert!(first.ownership_state.is_owner);

        let mut second = AppState::with_engine(engine);
        second.project_paths = Some(ProjectPaths::from_root(dir.path()));
        second.client_context.project_root = dir.path().to_path_buf();
        second.scan_and_attach_to_running_processes();

        assert!(!second.ownership_state.is_owner);
        assert!(second.ownership_guard.is_none());
        assert_eq!(second.viewer_guards.len(), 1);
    }

    #[test]
    fn test_tool_selection_and_toggling() {
        let ids = fixture_ids();
        let tool_one = ids[0].clone();
        let tool_two = ids[1].clone();
        let engine = fixture_engine(&ids);
        let mut state = AppState::with_engine(engine);
        // Mock working copy
        state.working_copy = Some(CanonicalConfig::default());

        // Initial state
        assert_eq!(state.selected_tool_index, 0);
        assert!(state
            .working_copy
            .as_ref()
            .unwrap()
            .tools
            .enabled
            .is_empty());

        // Toggle first tool
        state.toggle_selected_tool();
        assert_eq!(
            state.working_copy.as_ref().unwrap().tools.enabled,
            vec![tool_one.clone()]
        );

        // Move to next tool
        state.next_tool();
        assert_eq!(state.selected_tool_index, 1);

        // Toggle second tool
        state.toggle_selected_tool();
        assert_eq!(
            state.working_copy.as_ref().unwrap().tools.enabled,
            vec![tool_one.clone(), tool_two.clone()]
        );

        // Toggle second tool again (disable)
        state.toggle_selected_tool();
        assert_eq!(
            state.working_copy.as_ref().unwrap().tools.enabled,
            vec![tool_one]
        );

        // Prev tool (back to first)
        state.prev_tool();
        assert_eq!(state.selected_tool_index, 0);

        // Prev tool (loops back to second)
        state.prev_tool();
        assert_eq!(state.selected_tool_index, 1);
    }

    #[test]
    fn test_non_blocking_failed_event_does_not_trigger_pause_context() {
        assert!(!macc_core::service::diagnostic::is_blocking_failure_event(
            "branch_cleanup",
            "failed",
            "warning"
        ));
        assert!(!macc_core::service::diagnostic::is_blocking_failure_event(
            "branch_cleanup",
            "failed",
            "info"
        ));
    }

    #[test]
    fn test_blocking_failed_event_triggers_pause_context() {
        assert!(macc_core::service::diagnostic::is_blocking_failure_event(
            "phase_result",
            "failed",
            "blocking"
        ));
        // Backward compatibility when severity is missing.
        assert!(macc_core::service::diagnostic::is_blocking_failure_event(
            "failed", "failed", ""
        ));
    }

    #[test]
    fn test_resolve_current_run_id_uses_latest_event() {
        let events = vec![
            CoordinatorEvent {
                event_id: None,
                run_id: Some("run-1".to_string()),
                seq: 0,
                event_type: "heartbeat".to_string(),
                task_id: None,
                phase: None,
                status: None,
                ts: None,
                message: None,
                raw: serde_json::json!({"type":"heartbeat","run_id":"run-1"}),
            },
            CoordinatorEvent {
                event_id: None,
                run_id: Some("run-2".to_string()),
                seq: 1,
                event_type: "phase_result".to_string(),
                task_id: None,
                phase: None,
                status: None,
                ts: None,
                message: None,
                raw: serde_json::json!({"type":"phase_result","run_id":"run-2"}),
            },
        ];
        assert_eq!(
            AppState::resolve_current_run_id(&events),
            Some("run-2".to_string())
        );
    }

    #[test]
    fn test_event_matches_current_run_filters_legacy_events() {
        let with_run = CoordinatorEvent {
            event_id: None,
            run_id: Some("run-2".to_string()),
            seq: 0,
            event_type: "heartbeat".to_string(),
            task_id: None,
            phase: None,
            status: None,
            ts: None,
            message: None,
            raw: serde_json::json!({"type":"heartbeat","run_id":"run-2"}),
        };
        let without_run = CoordinatorEvent {
            event_id: None,
            run_id: None,
            seq: 0,
            event_type: "heartbeat".to_string(),
            task_id: None,
            phase: None,
            status: None,
            ts: None,
            message: None,
            raw: serde_json::json!({"type":"heartbeat"}),
        };
        assert!(AppState::event_matches_current_run(
            &with_run,
            Some("run-2")
        ));
        assert!(!AppState::event_matches_current_run(
            &without_run,
            Some("run-2")
        ));
        assert!(AppState::event_matches_current_run(&without_run, None));
    }

    #[test]
    fn test_preview_plan_requires_project() {
        let engine = Arc::new(MaccEngine::new(ToolRegistry::new()));
        let mut state = AppState::with_engine(engine);
        state.refresh_preview_plan();
        assert!(state.preview_ops.is_empty());
        assert!(state.preview_error.is_some());
    }

    #[test]
    fn test_preview_diff_cached_on_selection() {
        let engine = Arc::new(MaccEngine::new(ToolRegistry::new()));
        let mut state = AppState::with_engine(engine);
        let op = PlannedOp {
            path: "docs/example.txt".to_string(),
            scope: Scope::Project,
            consent_required: false,
            kind: PlannedOpKind::Write,
            metadata: PlannedOpMetadata::default(),
            before: Some(b"line\n".to_vec()),
            after: Some(b"line\nnew content\n".to_vec()),
        };

        state.preview_ops = vec![op];
        state.set_preview_selection(0);

        let diff = state.preview_diff_for_selected();
        assert!(diff.is_some());
        let diff = diff.unwrap();
        assert!(diff.diff.contains("new content"));
        assert_eq!(state.preview_diff_scroll_position(), 0);
    }

    #[test]
    fn test_tool_settings_navigation_and_cycling() {
        let ids = fixture_ids();
        let tool_two = ids[1].clone();
        let engine = fixture_engine(&ids);
        let mut state = AppState::with_engine(engine);
        state.working_copy = Some(CanonicalConfig::default());

        state.current_tool_id = Some(tool_two.clone());
        state.tool_field_index = 1; // Index 1 is 'model' in tool two

        // Cycle model (from default None to next)
        // options: [smart, small]
        // None -> uses smart -> returns small
        state.toggle_tool_field();

        let settings = state
            .working_copy
            .as_ref()
            .unwrap()
            .tools
            .config
            .get(&tool_two)
            .unwrap();
        assert_eq!(
            settings
                .get("settings")
                .unwrap()
                .get("model_name")
                .unwrap()
                .as_str()
                .unwrap(),
            "small"
        );

        // Cycle model again (loops back)
        state.toggle_tool_field();
        let settings = state
            .working_copy
            .as_ref()
            .unwrap()
            .tools
            .config
            .get(&tool_two)
            .unwrap();
        assert_eq!(
            settings
                .get("settings")
                .unwrap()
                .get("model_name")
                .unwrap()
                .as_str()
                .unwrap(),
            "smart"
        );
    }

    #[test]
    fn test_skills_selection() {
        use macc_core::catalog::{Selector, SkillEntry, SkillsCatalog, Source, SourceKind};

        let ids = fixture_ids();
        let tool_one = ids[0].clone();
        let engine = fixture_engine(&ids);
        let mut state = AppState::with_engine(engine);
        let temp_dir = tempdir().unwrap();
        let paths = ProjectPaths::from_root(temp_dir.path());
        fs::create_dir_all(&paths.catalog_dir).unwrap();
        let mut catalog = SkillsCatalog::default();
        for (id, name) in [
            ("mock-skill-one", "Mock Skill One"),
            ("mock-skill-two", "Mock Skill Two"),
        ] {
            catalog.entries.push(SkillEntry {
                id: id.to_string(),
                name: name.to_string(),
                description: format!("{name} from catalog."),
                tags: vec![],
                selector: Selector {
                    subpath: format!("skills/{id}"),
                },
                source: Source {
                    kind: SourceKind::Git,
                    url: "https://example.com/catalog.git".to_string(),
                    reference: "main".to_string(),
                    checksum: None,
                    subpaths: vec![],
                },
                tools: vec![],
                recommended_ref: None,
                risk: None,
                requires_mcp: false,
                writes_user_level_config: false,
                mandatory: id == "mock-skill-one",
                targets: Default::default(),
                category: None,
                compatibility: None,
            });
        }
        catalog
            .save_atomically(&paths, &paths.skills_catalog_path())
            .unwrap();
        state.project_paths = Some(paths);
        state.refresh_skills();
        state.working_copy = Some(CanonicalConfig::default());
        state.skill_target_path = Some(format!("/tools/config/{}/skills", tool_one));
        state.goto_screen(Screen::Skills);
        state.search_query = "mock-skill".to_string();
        state.skill_selection_index = state.filtered_skill_indices()[0];

        // Initial state
        assert_eq!(
            state.skills[state.skill_selection_index].id,
            "mock-skill-one"
        );

        let empty_vec: Vec<String> = Vec::new();
        let current_skills = state
            .working_copy
            .as_ref()
            .unwrap()
            .tools
            .config
            .get(&tool_one)
            .and_then(|v| v.get("skills"))
            .and_then(|v| serde_json::from_value(v.clone()).ok())
            .unwrap_or(empty_vec);
        assert!(current_skills.is_empty());

        assert!(state
            .selected_skills()
            .contains(&"mock-skill-one".to_string()));

        // Toggle first skill (mock-skill-one); mandatory skills are read-only.
        state.toggle_skill();
        assert!(state
            .working_copy
            .as_ref()
            .unwrap()
            .tools
            .config
            .get(&tool_one)
            .and_then(|v| v.get("skills"))
            .is_none());

        assert!(state
            .selected_skills()
            .contains(&"mock-skill-one".to_string()));

        // Move to next skill
        state.next_skill();
        assert_eq!(
            state.skills[state.skill_selection_index].id,
            "mock-skill-two"
        );

        // Toggle second skill (mock-skill-two)
        state.toggle_skill();
        let current_skills: Vec<String> = serde_json::from_value(
            state
                .working_copy
                .as_ref()
                .unwrap()
                .tools
                .config
                .get(&tool_one)
                .unwrap()
                .get("skills")
                .unwrap()
                .clone(),
        )
        .unwrap();
        assert!(current_skills.contains(&"mock-skill-one".to_string()));
        assert!(current_skills.contains(&"mock-skill-two".to_string()));

        // Select none
        state.select_no_skills();
        let current_skills: Vec<String> = serde_json::from_value(
            state
                .working_copy
                .as_ref()
                .unwrap()
                .tools
                .config
                .get(&tool_one)
                .unwrap()
                .get("skills")
                .unwrap()
                .clone(),
        )
        .unwrap();
        assert_eq!(
            current_skills,
            vec![
                "macc-performer".to_string(),
                "macc-prd-planner".to_string(),
                "macc-reviewer".to_string(),
                "mock-skill-one".to_string(),
            ]
        );

        // Select all
        state.select_all_skills();
        let current_skills: Vec<String> = serde_json::from_value(
            state
                .working_copy
                .as_ref()
                .unwrap()
                .tools
                .config
                .get(&tool_one)
                .unwrap()
                .get("skills")
                .unwrap()
                .clone(),
        )
        .unwrap();
        assert!(current_skills.len() >= 2);
        assert!(current_skills.contains(&"mock-skill-one".to_string()));
        assert!(current_skills.contains(&"mock-skill-two".to_string()));
    }

    #[test]
    fn test_agents_selection() {
        let ids = fixture_ids();
        let tool_one = ids[0].clone();
        let engine = fixture_engine(&ids);
        let mut state = AppState::with_engine(engine);
        state.working_copy = Some(CanonicalConfig::default());
        state.agent_target_path = Some(format!("/tools/config/{}/agents", tool_one));
        state.goto_screen(Screen::Agents);

        // Initial state
        assert_eq!(state.agent_selection_index, 0);

        let empty_vec: Vec<String> = Vec::new();
        let current_agents = state
            .working_copy
            .as_ref()
            .unwrap()
            .tools
            .config
            .get(&tool_one)
            .and_then(|v| v.get("agents"))
            .and_then(|v| serde_json::from_value(v.clone()).ok())
            .unwrap_or(empty_vec);
        assert!(current_agents.is_empty());

        // Toggle first agent (mock-agent-one)
        state.toggle_agent();
        let current_agents: Vec<String> = serde_json::from_value(
            state
                .working_copy
                .as_ref()
                .unwrap()
                .tools
                .config
                .get(&tool_one)
                .unwrap()
                .get("agents")
                .unwrap()
                .clone(),
        )
        .unwrap();
        assert_eq!(current_agents, vec!["mock-agent-one"]);

        // Move to next agent
        state.next_agent();
        assert_eq!(state.agent_selection_index, 1);

        // Toggle second agent (mock-agent-two)
        state.toggle_agent();
        let current_agents: Vec<String> = serde_json::from_value(
            state
                .working_copy
                .as_ref()
                .unwrap()
                .tools
                .config
                .get(&tool_one)
                .unwrap()
                .get("agents")
                .unwrap()
                .clone(),
        )
        .unwrap();
        assert_eq!(current_agents, vec!["mock-agent-one", "mock-agent-two"]);

        // Select none
        state.select_no_agents();
        let current_agents: Vec<String> = serde_json::from_value(
            state
                .working_copy
                .as_ref()
                .unwrap()
                .tools
                .config
                .get(&tool_one)
                .unwrap()
                .get("agents")
                .unwrap()
                .clone(),
        )
        .unwrap();
        assert!(current_agents.is_empty());

        // Select all
        state.select_all_agents();
        let current_agents: Vec<String> = serde_json::from_value(
            state
                .working_copy
                .as_ref()
                .unwrap()
                .tools
                .config
                .get(&tool_one)
                .unwrap()
                .get("agents")
                .unwrap()
                .clone(),
        )
        .unwrap();
        assert_eq!(current_agents.len(), 2);
        assert!(current_agents.contains(&"mock-agent-one".to_string()));
        assert!(current_agents.contains(&"mock-agent-two".to_string()));
    }

    #[test]
    fn test_mcp_selection_toggle_and_bulk() {
        let temp = tempdir().unwrap();
        let paths = ProjectPaths::from_root(temp.path());
        std::fs::create_dir_all(&paths.macc_dir).unwrap();
        std::fs::create_dir_all(&paths.catalog_dir).unwrap();
        std::fs::write(paths.macc_dir.join("macc.yaml"), "tools:\n  enabled: []\n").unwrap();

        let mut catalog = macc_core::catalog::McpCatalog::default();
        catalog.entries.push(macc_core::catalog::McpEntry {
            id: "mcp-a".into(),
            name: "MCP A".into(),
            description: "First MCP".into(),
            tags: vec!["alpha".into()],
            selector: macc_core::catalog::Selector {
                subpath: "path/a".into(),
            },
            source: macc_core::catalog::Source {
                kind: macc_core::catalog::SourceKind::Git,
                url: "https://example.com/a.git".into(),
                reference: "main".into(),
                checksum: None,
                subpaths: vec![],
            },
        });
        catalog.entries.push(macc_core::catalog::McpEntry {
            id: "mcp-b".into(),
            name: "MCP B".into(),
            description: "Second MCP".into(),
            tags: vec!["beta".into()],
            selector: macc_core::catalog::Selector {
                subpath: "path/b".into(),
            },
            source: macc_core::catalog::Source {
                kind: macc_core::catalog::SourceKind::Git,
                url: "https://example.com/b.git".into(),
                reference: "main".into(),
                checksum: None,
                subpaths: vec![],
            },
        });
        catalog
            .save_atomically(&paths, &paths.mcp_catalog_path())
            .unwrap();

        let ids = fixture_ids();
        let engine = fixture_engine(&ids);
        let mut state = AppState::with_engine(engine);
        state.load_config(Some(temp.path()));
        state.working_copy = Some(CanonicalConfig::default());
        assert_eq!(state.mcp_entries.len(), 2);

        state.toggle_mcp();
        let selected = state
            .working_copy
            .as_ref()
            .unwrap()
            .selections
            .as_ref()
            .unwrap()
            .mcp
            .clone();
        assert_eq!(selected, vec!["mcp-a".to_string()]);

        state.select_all_mcp();
        let selected = state
            .working_copy
            .as_ref()
            .unwrap()
            .selections
            .as_ref()
            .unwrap()
            .mcp
            .clone();
        assert_eq!(selected.len(), 2);

        state.select_no_mcp();
        let selected = state
            .working_copy
            .as_ref()
            .unwrap()
            .selections
            .as_ref()
            .unwrap()
            .mcp
            .clone();
        assert!(selected.is_empty());
    }

    #[test]
    fn test_pure_helpers() {
        // next_index
        assert_eq!(next_index(0, 3), 1);
        assert_eq!(next_index(2, 3), 0);
        assert_eq!(next_index(0, 0), 0);

        // prev_index
        assert_eq!(prev_index(1, 3), 0);
        assert_eq!(prev_index(0, 3), 2);
        assert_eq!(prev_index(0, 0), 0);

        // toggle_vec_item
        let v = vec!["a".to_string(), "c".to_string()];
        let v = toggle_vec_item(v, "b".to_string());
        assert_eq!(v, vec!["a", "b", "c"]);
        let v = toggle_vec_item(v, "a".to_string());
        assert_eq!(v, vec!["b", "c"]);

        // cycle_value
        let options = &["a", "b", "c"];
        assert_eq!(cycle_value(options, "a"), "b");
        assert_eq!(cycle_value(options, "c"), "a");
        assert_eq!(cycle_value(options, "unknown"), "b"); // defaults to 0 + 1
    }

    #[test]
    fn test_unified_navigation() {
        let ids = fixture_ids();
        let tool_one = ids[0].clone();
        let engine = fixture_engine(&ids);
        let mut state = AppState::with_engine(engine);
        state.working_copy = Some(CanonicalConfig::default());

        // Home screen (no-op for these)
        state.navigate_next();
        assert_eq!(state.current_screen(), Screen::Home);

        // Tools screen
        state.goto_screen(Screen::Tools);
        state.navigate_next();
        assert_eq!(state.selected_tool_index, 1);
        state.navigate_prev();
        assert_eq!(state.selected_tool_index, 0);

        // Toggle tool
        state.navigate_toggle();
        assert_eq!(
            state.working_copy.as_ref().unwrap().tools.enabled,
            vec![tool_one.clone()]
        );

        // Enter sub-screen
        state.navigate_enter();
        assert_eq!(state.current_screen(), Screen::ToolSettings);

        // Tool settings fields
        state.navigate_next();
        // First tool has 4 fields, so it should move to 1.
        assert_eq!(state.tool_field_index, 1);
        state.navigate_prev();
        assert_eq!(state.tool_field_index, 0);

        state.navigate_toggle(); // toggle enabled
        let settings = state
            .working_copy
            .as_ref()
            .unwrap()
            .tools
            .config
            .get(&tool_one)
            .unwrap();
        assert_eq!(settings.get("enabled").unwrap().as_bool().unwrap(), true);

        // MCP screen (no catalog entries loaded in this test)
        state.goto_screen(Screen::Mcp);
        state.navigate_next();
        if state.mcp_entries.len() > 1 {
            assert_eq!(state.mcp_selection_index, 1);
        } else {
            assert_eq!(state.mcp_selection_index, 0);
        }
    }

    #[test]
    fn test_config_golden_serialization() {
        let mut config = CanonicalConfig::default();
        let ids = fixture_ids();
        let tool_one = ids[0].clone();
        let tool_two = ids[1].clone();
        config.tools.enabled = vec![tool_one.clone(), tool_two];

        config.tools.settings.insert(
            tool_one,
            serde_json::json!({
                "model": "smart",
                "language": "English",
                "permissions": "strict",
                "skills": ["create-plan", "implement"],
                "agents": ["architect"],
                "rules_enabled": false
            }),
        );

        config.selections = Some(macc_core::config::SelectionsConfig {
            mcp: vec!["local-notes".to_string()],
            ..Default::default()
        });

        let yaml = config.to_yaml().expect("Serialization failed");

        // Golden check: verify specific deterministic properties
        assert!(yaml.contains("model: smart"));
        assert!(yaml.contains("language: English"));
        assert!(yaml.contains("- create-plan"));
        assert!(yaml.contains("- implement")); // alphabetical sort check
        assert!(yaml.find("create-plan").unwrap() < yaml.find("implement").unwrap());

        // Roundtrip
        let config2 = CanonicalConfig::from_yaml(&yaml).expect("Deserialization failed");
        assert_eq!(config, config2);

        // Idempotence
        let yaml2 = config2.to_yaml().expect("Second serialization failed");
        assert_eq!(yaml, yaml2);
    }

    #[test]
    fn test_interaction_mode_labels() {
        let engine = Arc::new(MaccEngine::new(ToolRegistry::new()));
        let mut state = AppState::with_engine(engine);
        assert_eq!(state.interaction_mode_label(), "browse");

        state.push_screen(Screen::Apply);
        assert_eq!(state.interaction_mode_label(), "confirm");

        state.pop_screen();
        state.push_screen(Screen::Automation);
        state.automation_field_editing = true;
        assert_eq!(state.interaction_mode_label(), "edit");
    }

    #[test]
    fn test_inline_validation_for_automation_number_field() {
        let engine = Arc::new(MaccEngine::new(ToolRegistry::new()));
        let mut state = AppState::with_engine(engine);
        state.working_copy = Some(CanonicalConfig::default());
        state.push_screen(Screen::Automation);

        state.automation_field_index = 7; // Max Parallel
        state.automation_field_editing = true;
        state.automation_field_input = "abc".to_string();
        assert!(state.current_automation_field_validation().is_some());

        state.automation_field_input = "3".to_string();
        assert!(state.current_automation_field_validation().is_none());
    }

    #[test]
    fn test_format_actionable_error_includes_cause_and_fix() {
        let msg = format_actionable_error("invalid registry JSON");
        assert!(msg.contains("Cause:"));
        assert!(msg.contains("Suggested fix:"));
        assert!(msg.contains("registry"));
    }

    #[test]
    fn test_resolve_task_model_and_worker_fallback() {
        use macc_core::coordinator::model::{Task, TaskRuntime, TaskWorktree};
        use macc_core::coordinator::view_model::LiveTaskRow;

        // Setup test CanonicalConfig
        let mut config = CanonicalConfig::default();
        config.tools.config.insert(
            "claude".to_string(),
            serde_json::json!({
                "model": "sonnet-default",
                "model_tiers": {
                    "mini": {
                        "model": "haiku-override"
                    },
                    "heavy": {
                        "model": "opus-override"
                    }
                }
            }),
        );

        // Task with no routing hints -> uses phase defaults (standard tier for implementation)
        let mut task = Task {
            id: "task-1".to_string(),
            tool: Some("claude".to_string()),
            task_runtime: TaskRuntime {
                current_phase: Some("implementation".to_string()),
                ..Default::default()
            },
            ..Default::default()
        };

        // Standard tier claude should fallback to "sonnet-default" because standard is not in model_tiers
        let resolved = AppState::resolve_task_model(&task, &config);
        assert_eq!(resolved, "sonnet-default");

        // Set routing hint to heavy
        task.extra.insert(
            "routing_hints".to_string(),
            serde_json::json!({
                "risk_level": "high" // high risk maps to heavy tier
            }),
        );
        let resolved_heavy = AppState::resolve_task_model(&task, &config);
        assert_eq!(resolved_heavy, "opus-override");

        // Verify fallback to default tool models when tool is unknown/missing config
        task.tool = Some("unknown-tool".to_string());
        let resolved_fallback = AppState::resolve_task_model(&task, &config);
        // "unknown-tool" with heavy tier should resolve to tier name "heavy"
        assert_eq!(resolved_fallback, "heavy");

        // Now verify worker fallback logic in LiveTaskRow::from_task
        task.task_runtime.worker_id = None;
        task.worktree = Some(TaskWorktree {
            worktree_path: Some("/path/to/.macc/worktree/worker-05".to_string()),
            ..Default::default()
        });

        let now = chrono::Utc::now();
        let row = LiveTaskRow::from_task(&task, now, "some-model".to_string());
        assert_eq!(row.worker_id, "worker-05");
        assert_eq!(row.model, "some-model");
    }
}
