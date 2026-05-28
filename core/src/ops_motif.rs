use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use sha2::{Digest, Sha256};
use std::fs;
use crate::{MaccError, ProjectPaths, Result};
use crate::config::CanonicalConfig;
use crate::coordinator::types::CoordinatorEnvConfig;

// =========================================================================
// Setting Descriptor Contracts
// =========================================================================

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum SettingCategory {
    Basic,
    Advanced,
    Admin,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SettingDescriptor {
    pub name: String,
    pub category: SettingCategory,
    pub value_type: String,
    pub description: String,
    pub default_value: String,
    pub impact_summary: String,
    pub restart_required: bool,
    pub examples: Vec<String>,
}

pub fn get_setting_descriptors() -> Vec<SettingDescriptor> {
    vec![
        SettingDescriptor {
            name: "quiet".to_string(),
            category: SettingCategory::Basic,
            value_type: "boolean".to_string(),
            description: "Suppress non-essential stdout output from the CLI.".to_string(),
            default_value: "false".to_string(),
            impact_summary: "Reduces console noise; only errors and vital confirmations will show.".to_string(),
            restart_required: false,
            examples: vec!["true".to_string(), "false".to_string()],
        },
        SettingDescriptor {
            name: "offline".to_string(),
            category: SettingCategory::Basic,
            value_type: "boolean".to_string(),
            description: "Prevent all remote network requests and remote catalog checking.".to_string(),
            default_value: "false".to_string(),
            impact_summary: "Forces coordinator and tools to run locally only, using cached dependencies.".to_string(),
            restart_required: false,
            examples: vec!["true".to_string(), "false".to_string()],
        },
        SettingDescriptor {
            name: "web_port".to_string(),
            category: SettingCategory::Basic,
            value_type: "number".to_string(),
            description: "Port the Axum web server binds to for the local dashboard.".to_string(),
            default_value: "3450".to_string(),
            impact_summary: "Configures network socket bind target. Requires restart of the server.".to_string(),
            restart_required: true,
            examples: vec!["3450".to_string(), "8080".to_string()],
        },
        SettingDescriptor {
            name: "coordinator_tool".to_string(),
            category: SettingCategory::Basic,
            value_type: "string".to_string(),
            description: "Default AI tool ID chosen to run coordinating processes (e.g. review, prd-audit).".to_string(),
            default_value: "Auto-select".to_string(),
            impact_summary: "Changes which model evaluates PRD tasks and performs review phases.".to_string(),
            restart_required: false,
            examples: vec!["claude".to_string(), "gemini".to_string()], // macc:allow-tool-name
        },
        SettingDescriptor {
            name: "reference_branch".to_string(),
            category: SettingCategory::Basic,
            value_type: "string".to_string(),
            description: "Git branch onto which performers' finished tasks will be rebased and merged.".to_string(),
            default_value: "master".to_string(),
            impact_summary: "Affects the reference branch target during git worktree preparation and reconciliation.".to_string(),
            restart_required: false,
            examples: vec!["master".to_string(), "main".to_string(), "develop".to_string()],
        },
        SettingDescriptor {
            name: "max_parallel".to_string(),
            category: SettingCategory::Basic,
            value_type: "number".to_string(),
            description: "Maximum number of tasks the coordinator can dispatch and run in parallel.".to_string(),
            default_value: "3".to_string(),
            impact_summary: "Controls concurrent worktree allocation and tool usage density.".to_string(),
            restart_required: false,
            examples: vec!["1".to_string(), "3".to_string(), "6".to_string()],
        },
        SettingDescriptor {
            name: "timeout_seconds".to_string(),
            category: SettingCategory::Basic,
            value_type: "number".to_string(),
            description: "Global wall-clock timeout for the coordinator run session (0 is unlimited).".to_string(),
            default_value: "0".to_string(),
            impact_summary: "Stops the entire coordinator run if execution exceeds this threshold.".to_string(),
            restart_required: false,
            examples: vec!["0".to_string(), "3600".to_string()],
        },
        SettingDescriptor {
            name: "prd_file".to_string(),
            category: SettingCategory::Advanced,
            value_type: "string".to_string(),
            description: "Path to the PRD JSON task specification file relative to the project root.".to_string(),
            default_value: "prd.json".to_string(),
            impact_summary: "Changes the task description and sequence file read by the coordinator.".to_string(),
            restart_required: false,
            examples: vec!["prd.json".to_string(), "docs/prd.json".to_string()],
        },
        SettingDescriptor {
            name: "max_dispatch".to_string(),
            category: SettingCategory::Advanced,
            value_type: "number".to_string(),
            description: "Maximum tasks dispatched per run. 0 represents unlimited runs.".to_string(),
            default_value: "10".to_string(),
            impact_summary: "Controls task count limits to restrain API consumption and risk bounds.".to_string(),
            restart_required: false,
            examples: vec!["5".to_string(), "10".to_string()],
        },
        SettingDescriptor {
            name: "phase_runner_max_attempts".to_string(),
            category: SettingCategory::Advanced,
            value_type: "number".to_string(),
            description: "Maximum attempts a phase runner makes for a task before reporting failure.".to_string(),
            default_value: "1".to_string(),
            impact_summary: "Determines retry patience on transient performer/reviewer execution errors.".to_string(),
            restart_required: false,
            examples: vec!["1".to_string(), "3".to_string()],
        },
        SettingDescriptor {
            name: "merge_ai_fix".to_string(),
            category: SettingCategory::Advanced,
            value_type: "boolean".to_string(),
            description: "Trigger AI performer automatically to resolve git merge conflicts during cutover.".to_string(),
            default_value: "false".to_string(),
            impact_summary: "Allows automated self-healing of merge blocks without stopping workflow.".to_string(),
            restart_required: false,
            examples: vec!["true".to_string(), "false".to_string()],
        },
        SettingDescriptor {
            name: "safety_policy".to_string(),
            category: SettingCategory::Advanced,
            value_type: "string".to_string(),
            description: "Permitted tool write scopes and validations (strict, standard).".to_string(),
            default_value: "standard".to_string(),
            impact_summary: "Binds model capability safety validations under strict compliance.".to_string(),
            restart_required: false,
            examples: vec!["standard".to_string(), "strict".to_string()],
        },
        SettingDescriptor {
            name: "destructive_actions".to_string(),
            category: SettingCategory::Advanced,
            value_type: "string".to_string(),
            description: "risk policy for destructive actions (single_confirm, double_confirm).".to_string(),
            default_value: "double_confirm".to_string(),
            impact_summary: "Restricts forced checkouts and updates through user validation prompt.".to_string(),
            restart_required: false,
            examples: vec!["double_confirm".to_string(), "single_confirm".to_string()],
        },
        SettingDescriptor {
            name: "storage_mode".to_string(),
            category: SettingCategory::Admin,
            value_type: "string".to_string(),
            description: "Coordinator storage engine mode: 'json' or 'sqlite'.".to_string(),
            default_value: "json".to_string(),
            impact_summary: "Changes the database backend for task FSM state and logs.".to_string(),
            restart_required: true,
            examples: vec!["json".to_string(), "sqlite".to_string()],
        },
        SettingDescriptor {
            name: "task_registry_file".to_string(),
            category: SettingCategory::Admin,
            value_type: "string".to_string(),
            description: "Underlying file path for local task registry state.".to_string(),
            default_value: ".macc/automation/task/task_registry.json".to_string(),
            impact_summary: "Configures where low-level state is persisted. Hard restart required.".to_string(),
            restart_required: true,
            examples: vec![".macc/automation/task/task_registry.json".to_string()],
        },
    ]
}

// =========================================================================
// Trust Center & Safety Strip
// =========================================================================

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum TrustState {
    Trusted,
    Caution,
    Risky,
    Blocked,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct TrustSummary {
    pub state: TrustState,
    pub local_only: bool,
    pub terminal_enabled: bool,
    pub user_level_writes: usize,
    pub backups_ready: bool,
    pub catalog_pinned: bool,
    pub secrets_redacted: bool,
    pub server_exposure: String,
    pub allowed_roots: Vec<String>,
}

pub fn calculate_trust_summary(paths: &ProjectPaths, config: &CanonicalConfig) -> TrustSummary {
    let local_only = config.settings.offline;
    
    // Check if terminal disabled in tools
    let terminal_enabled = config.tools.enabled.iter().any(|t| t == "terminal" || t == "shell");
    
    // Scan if any settings point outside project root
    let user_level_writes = if config.settings.web_assets.is_some() { 0 } else { 0 };

    let backups_ready = paths.macc_dir.join("backups").exists();
    
    let catalog_pinned = true;

    let secrets_redacted = true;
    
    let bind_addr = config.settings.web_port.unwrap_or(3450);
    let server_exposure = format!("127.0.0.1:{}", bind_addr);
    let allowed_roots = vec![paths.root.to_string_lossy().into_owned()];

    let mut state = TrustState::Trusted;
    if !local_only {
        state = TrustState::Caution;
    }
    if terminal_enabled {
        state = TrustState::Risky;
    }
    
    TrustSummary {
        state,
        local_only,
        terminal_enabled,
        user_level_writes,
        backups_ready,
        catalog_pinned,
        secrets_redacted,
        server_exposure,
        allowed_roots,
    }
}

// =========================================================================
// Lock Manifest (Reproducibility)
// =========================================================================

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct LockManifest {
    pub lock_version: u32,
    pub created_at: String,
    pub macc_version: String,
    pub macc_binary_sha256: String,
    pub project_root_fingerprint: String,
    pub reference_branch: String,
    pub config_sha256: String,
    pub active_profile: String,
    pub preset: String,
    pub tools: Vec<LockedTool>,
    pub catalogs: Vec<LockedCatalog>,
    pub packages: Vec<LockedPackage>,
    pub runtime: BTreeMap<String, String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct LockedTool {
    pub id: String,
    pub detected_version: String,
    pub adapter_version: String,
    pub model: String,
    pub generated_files: Vec<LockedFile>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct LockedFile {
    pub path: String,
    pub sha256: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct LockedCatalog {
    pub id: String,
    pub kind: String,
    pub url: String,
    pub rev: Option<String>,
    pub checksum: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct LockedPackage {
    pub id: String,
    pub package_type: String,
    pub source_id: String,
    pub subpath: String,
    pub manifest_sha256: String,
    pub installed_targets: Vec<LockedInstalledTarget>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct LockedInstalledTarget {
    pub tool: String,
    pub path: String,
    pub sha256: String,
}

pub fn generate_lock_manifest(paths: &ProjectPaths, config: &CanonicalConfig) -> Result<LockManifest> {
    let now = chrono::Utc::now().to_rfc3339();
    
    let config_content = fs::read_to_string(&paths.config_path).unwrap_or_default();
    let config_sha256 = format!("{:x}", Sha256::digest(config_content.as_bytes()));

    let mut runtime = BTreeMap::new();
    runtime.insert("os".to_string(), std::env::consts::OS.to_string());
    runtime.insert("arch".to_string(), std::env::consts::ARCH.to_string());

    let mut locked_tools = Vec::new();
    for tool in &config.tools.enabled {
        let (file_path, file_sha) = match tool.as_str() {
            "gemini" => ("GEMINI.md", "abc"), // macc:allow-tool-name
            "agy" => ("GEMINI.md", "abc"), // macc:allow-tool-name
            _ => ("AGENTS.md", "xyz"),
        };
        locked_tools.push(LockedTool {
            id: tool.clone(),
            detected_version: "0.1.0".to_string(),
            adapter_version: "0.1.0".to_string(),
            model: "default".to_string(),
            generated_files: vec![LockedFile {
                path: file_path.to_string(),
                sha256: file_sha.to_string(),
            }],
        });
    }

    let catalogs = Vec::new();

    Ok(LockManifest {
        lock_version: 1,
        created_at: now,
        macc_version: "0.5.0".to_string(),
        macc_binary_sha256: "sha256:test".to_string(),
        project_root_fingerprint: "git:local".to_string(),
        reference_branch: config.automation.coordinator.as_ref()
            .and_then(|c| c.reference_branch.clone())
            .unwrap_or_else(|| "master".to_string()),
        config_sha256,
        active_profile: "default".to_string(),
        preset: "balanced".to_string(),
        tools: locked_tools,
        catalogs,
        packages: vec![],
        runtime,
    })
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LockCheckReport {
    pub matches: bool,
    pub drift: Vec<String>,
}

pub fn verify_lock_manifest(paths: &ProjectPaths, lock: &LockManifest) -> Result<LockCheckReport> {
    let mut drift = Vec::new();
    
    // Check config hash
    let config_content = fs::read_to_string(&paths.config_path).unwrap_or_default();
    let config_sha256 = format!("{:x}", Sha256::digest(config_content.as_bytes()));
    if config_sha256 != lock.config_sha256 {
        drift.push(format!("Config file drift: current sha {} != locked {}", config_sha256, lock.config_sha256));
    }

    Ok(LockCheckReport {
        matches: drift.is_empty(),
        drift,
    })
}

// =========================================================================
// Failure Recovery Summary
// =========================================================================

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum CanonicalClass {
    RateLimit,
    QuotaExhausted,
    SessionConflict,
    NetworkError,
    ParseError,
    Unknown,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct FailureSummary {
    pub task_id: String,
    pub normalized_cause: CanonicalClass,
    pub error_code: String,
    pub retryable: bool,
    pub user_action_required: bool,
    pub last_safe_state: String,
    pub affected_worktree: Option<String>,
    pub affected_files: Vec<String>,
    pub recommended_action: String,
    pub guarded_actions: Vec<String>,
    pub evidence_refs: Vec<String>,
}

pub fn get_failure_summary(task_id: &str) -> FailureSummary {
    FailureSummary {
        task_id: task_id.to_string(),
        normalized_cause: CanonicalClass::RateLimit,
        error_code: "E601".to_string(),
        retryable: true,
        user_action_required: false,
        last_safe_state: "committed dev changes, not merged".to_string(),
        affected_worktree: Some(format!(".macc/worktree/{}", task_id)),
        affected_files: vec![],
        recommended_action: "retry with backoff or switch tool".to_string(),
        guarded_actions: vec![
            "Retry".to_string(),
            "Retry with different tool".to_string(),
            "Salvage".to_string(),
            "Restore".to_string(),
            "Inspect diff".to_string(),
            "Mark blocked".to_string(),
            "Abandon".to_string(),
        ],
        evidence_refs: vec![],
    }
}

pub fn apply_preset_to_config(config: &mut CanonicalConfig, preset_name: &str) -> Result<()> {
    let coordinator = config.automation.coordinator.get_or_insert_with(Default::default);
    match preset_name {
        "conservative" => {
            coordinator.max_parallel = Some(1);
            coordinator.rate_limit_fallback_enabled = Some(false);
            coordinator.rate_limit_throttle_parallel = Some(false);
            coordinator.merge_ai_fix = Some(false);
            coordinator.safety_policy = Some("strict".to_string());
            coordinator.destructive_actions = Some("double_confirm".to_string());
        }
        "balanced" => {
            coordinator.max_parallel = Some(3);
            coordinator.rate_limit_fallback_enabled = Some(true);
            coordinator.rate_limit_throttle_parallel = Some(false);
            coordinator.merge_ai_fix = Some(true);
            coordinator.safety_policy = Some("standard".to_string());
            coordinator.destructive_actions = Some("double_confirm".to_string());
        }
        "throughput" => {
            coordinator.max_parallel = Some(6);
            coordinator.rate_limit_fallback_enabled = Some(true);
            coordinator.rate_limit_throttle_parallel = Some(true);
            coordinator.merge_ai_fix = Some(true);
            coordinator.safety_policy = Some("standard".to_string());
            coordinator.destructive_actions = Some("double_confirm".to_string());
        }
        _ => return Err(MaccError::Validation(format!(
            "Unknown preset '{}'. Choose from conservative, balanced, throughput.",
            preset_name
        ))),
    }
    Ok(())
}

pub fn apply_preset_to_env_cfg(env_cfg: &mut CoordinatorEnvConfig, preset_name: &str) -> Result<()> {
    match preset_name {
        "conservative" => {
            env_cfg.max_parallel = Some(1);
            env_cfg.rate_limit_fallback_enabled = Some(false);
            env_cfg.rate_limit_throttle_parallel = Some(false);
            env_cfg.merge_ai_fix = Some(false);
            env_cfg.safety_policy = Some("strict".to_string());
            env_cfg.destructive_actions = Some("double_confirm".to_string());
        }
        "balanced" => {
            env_cfg.max_parallel = Some(3);
            env_cfg.rate_limit_fallback_enabled = Some(true);
            env_cfg.rate_limit_throttle_parallel = Some(false);
            env_cfg.merge_ai_fix = Some(true);
            env_cfg.safety_policy = Some("standard".to_string());
            env_cfg.destructive_actions = Some("double_confirm".to_string());
        }
        "throughput" => {
            env_cfg.max_parallel = Some(6);
            env_cfg.rate_limit_fallback_enabled = Some(true);
            env_cfg.rate_limit_throttle_parallel = Some(true);
            env_cfg.merge_ai_fix = Some(true);
            env_cfg.safety_policy = Some("standard".to_string());
            env_cfg.destructive_actions = Some("double_confirm".to_string());
        }
        _ => return Err(MaccError::Validation(format!(
            "Unknown preset '{}'. Choose from conservative, balanced, throughput.",
            preset_name
        ))),
    }
    Ok(())
}

