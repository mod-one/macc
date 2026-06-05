use crate::config::CanonicalConfig;
use crate::coordinator::error_normalizer::CanonicalClass;
use crate::coordinator::types::CoordinatorEnvConfig;
use crate::{MaccError, ProjectPaths, Result};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::fs;

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
    pub audit_log: String,
}

fn is_pinned_git_ref(r: &str) -> bool {
    r.len() == 40 && r.chars().all(|c| c.is_ascii_hexdigit())
}

fn get_catalog_id_from_url(url: &str) -> String {
    if let Some(pos) = url.find("://") {
        let rest = &url[pos + 3..];
        if let Some(slash_pos) = rest.find('/') {
            let host = &rest[..slash_pos];
            let path = &rest[slash_pos..];
            let clean_host = host.replace('.', "-");
            let clean_path = path.trim_matches('/').replace('/', "-");
            if clean_path.is_empty() {
                clean_host
            } else {
                format!("{}-{}", clean_host, clean_path)
            }
        } else {
            rest.replace('.', "-")
        }
    } else {
        "remote-source".to_string()
    }
}

fn check_catalogs_pinned(paths: &ProjectPaths) -> bool {
    let check_file = |path: &std::path::Path| -> bool {
        if !path.exists() {
            return true;
        }
        if let Ok(content) = std::fs::read_to_string(path) {
            if let Ok(val) = serde_json::from_str::<serde_json::Value>(&content) {
                if let Some(entries) = val.get("entries").and_then(|e| e.as_array()) {
                    for entry in entries {
                        if let Some(source) = entry.get("source") {
                            let kind = source.get("kind").and_then(|k| k.as_str()).unwrap_or("");
                            if kind != "local" {
                                if kind == "git" {
                                    let reference =
                                        source.get("ref").and_then(|r| r.as_str()).unwrap_or("");
                                    if !is_pinned_git_ref(reference) {
                                        return false;
                                    }
                                } else if kind == "http" {
                                    let checksum = source
                                        .get("checksum")
                                        .and_then(|c| c.as_str())
                                        .unwrap_or("");
                                    if checksum.is_empty() {
                                        return false;
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
        true
    };

    if !check_file(&paths.skills_catalog_path()) {
        return false;
    }
    if !check_file(&paths.project_skills_catalog_path()) {
        return false;
    }
    if !check_file(&paths.mcp_catalog_path()) {
        return false;
    }
    if !check_file(&paths.project_mcp_catalog_path()) {
        return false;
    }
    true
}

fn check_secrets_redacted(paths: &ProjectPaths) -> bool {
    if paths.config_path.exists() {
        if let Ok(content) = std::fs::read_to_string(&paths.config_path) {
            let findings = crate::security::scan_bytes(
                &paths.config_path.to_string_lossy(),
                content.as_bytes(),
            );
            findings
                .iter()
                .all(|f| f.severity != crate::security::Severity::Error)
        } else {
            true
        }
    } else {
        true
    }
}

fn check_user_level_writes(paths: &ProjectPaths, config: &CanonicalConfig) -> usize {
    let registry = crate::tool::ToolRegistry::from_inventory();
    let resolved = crate::resolve::resolve(config, &Default::default());
    if let Ok(plan) = crate::build_plan(paths, &resolved, &[], &registry) {
        plan.actions
            .iter()
            .filter(|action| action.scope() == crate::plan::Scope::User)
            .count()
    } else {
        0
    }
}

pub fn calculate_trust_summary(paths: &ProjectPaths, config: &CanonicalConfig) -> TrustSummary {
    let local_only = config.settings.offline;

    let terminal_enabled = config
        .tools
        .enabled
        .iter()
        .any(|t| t == "terminal" || t == "shell");

    let user_level_writes = check_user_level_writes(paths, config);

    let backups_ready = paths.macc_dir.join("backups").exists();

    let catalog_pinned = check_catalogs_pinned(paths);

    let secrets_redacted = check_secrets_redacted(paths);

    let bind_addr = config.settings.web_port.unwrap_or(3450);
    let server_exposure = format!("127.0.0.1:{}", bind_addr);
    let allowed_roots = vec![paths.root.to_string_lossy().into_owned()];
    let audit_log = paths
        .macc_dir
        .join("log/coordinator/coordinator.log")
        .to_string_lossy()
        .into_owned();

    let resolved_safety = config
        .automation
        .coordinator
        .as_ref()
        .and_then(|c| c.safety_policy.clone())
        .unwrap_or_else(|| "standard".to_string());

    let mut state = TrustState::Trusted;
    if !local_only {
        state = TrustState::Caution;
    }
    if terminal_enabled || user_level_writes > 0 || !catalog_pinned {
        state = TrustState::Caution;
    }
    if !secrets_redacted || !backups_ready {
        state = TrustState::Risky;
    }
    if resolved_safety == "strict"
        && (user_level_writes > 0 || !catalog_pinned || !secrets_redacted)
    {
        state = TrustState::Blocked;
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
        audit_log,
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
    pub coordinator: LockedCoordinator,
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

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct LockedCoordinator {
    pub storage_mode: String,
    pub max_parallel: usize,
    pub retry_policy_hash: String,
    pub rate_limit_policy_hash: String,
}

pub fn generate_lock_manifest(
    paths: &ProjectPaths,
    config: &CanonicalConfig,
) -> Result<LockManifest> {
    use crate::resolve::{resolve_fetch_units, PlanningContext, SelectionKind};
    use crate::tool::loader::ToolSpecLoader;
    use crate::tool::registry::ToolRegistry;

    let now = chrono::Utc::now().to_rfc3339();

    let config_content = fs::read_to_string(&paths.config_path).unwrap_or_default();
    let config_sha256 = format!("sha256:{:x}", Sha256::digest(config_content.as_bytes()));

    // 1. Macc version & binary hash
    let macc_version = env!("CARGO_PKG_VERSION").to_string();
    let macc_binary_sha256 = std::env::current_exe()
        .ok()
        .and_then(|path| fs::read(&path).ok())
        .map(|bytes| format!("sha256:{:x}", Sha256::digest(&bytes)))
        .unwrap_or_else(|| "sha256:test".to_string());

    // 2. Git URL and commit fingerprint
    let repo_url = crate::git::run_git_output_mapped(
        paths.root.as_ref(),
        &["config", "--get", "remote.origin.url"],
        "get git remote URL",
    )
    .map(|out| String::from_utf8_lossy(&out.stdout).trim().to_string())
    .unwrap_or_else(|_| "local".to_string());
    let commit_hash = crate::git::run_git_output_mapped(
        paths.root.as_ref(),
        &["rev-parse", "HEAD"],
        "get git HEAD commit",
    )
    .map(|out| String::from_utf8_lossy(&out.stdout).trim().to_string())
    .unwrap_or_else(|_| "unknown".to_string());
    let project_root_fingerprint = format!("git:{}#{}", repo_url, commit_hash);

    // 3. Runtime versions
    let mut runtime = BTreeMap::new();
    runtime.insert("os".to_string(), std::env::consts::OS.to_string());
    runtime.insert("arch".to_string(), std::env::consts::ARCH.to_string());
    let get_cmd_version = |cmd: &str, args: &[&str]| -> Option<String> {
        std::process::Command::new(cmd)
            .args(args)
            .output()
            .ok()
            .and_then(|out| {
                if out.status.success() {
                    let s = String::from_utf8_lossy(&out.stdout).trim().to_string();
                    if !s.is_empty() {
                        return Some(s);
                    }
                }
                None
            })
    };
    if let Some(v) = get_cmd_version("git", &["--version"]) {
        runtime.insert("git_version".to_string(), v);
    }
    if let Some(v) = get_cmd_version("node", &["--version"]) {
        runtime.insert("node_version".to_string(), v);
    }
    if let Some(v) = get_cmd_version("pnpm", &["--version"]) {
        runtime.insert("pnpm_version".to_string(), v);
    }
    if let Some(v) = get_cmd_version("rustc", &["--version"]) {
        runtime.insert("rust_version".to_string(), v);
    }

    // 4. Locked tools & generated files
    let registry = ToolRegistry::from_inventory();
    let resolved = crate::resolve::resolve(config, &Default::default());
    let planning_ctx = PlanningContext {
        paths,
        resolved: &resolved,
        materialized_units: &[],
    };

    let spec_loader = ToolSpecLoader::new(ToolSpecLoader::default_search_paths(&paths.root));
    let (specs, _) = spec_loader.load_all_with_embedded();

    let mut locked_tools = Vec::new();
    for tool_id in &config.tools.enabled {
        let detected_version = specs
            .iter()
            .find(|s| &s.id == tool_id)
            .and_then(|spec| spec.version_check.as_ref())
            .and_then(|vc| crate::service::tooling::run_version_command(&vc.current))
            .unwrap_or_else(|| "0.1.0".to_string());

        let adapter_version = env!("CARGO_PKG_VERSION").to_string();

        let model = config
            .tools
            .config
            .get(tool_id)
            .and_then(|v| v.as_object())
            .and_then(|obj| obj.get("model"))
            .and_then(|m| m.as_str())
            .unwrap_or("default")
            .to_string();

        let mut generated_files = Vec::new();
        if let Some(adapter) = registry.get(tool_id) {
            if let Ok(plan) = adapter.plan(&planning_ctx) {
                for action in plan.actions {
                    if let crate::plan::Action::WriteFile { path, content, .. } = action {
                        let sha256 = format!("sha256:{:x}", Sha256::digest(&content));
                        generated_files.push(LockedFile { path, sha256 });
                    }
                }
            }
        }

        locked_tools.push(LockedTool {
            id: tool_id.clone(),
            detected_version,
            adapter_version,
            model,
            generated_files,
        });
    }

    // 5. Catalogs
    let mut catalogs = Vec::new();
    let mut catalog_map = std::collections::HashMap::new();

    if let Ok(skills_cat) = crate::catalog::load_effective_skills_catalog(paths) {
        for entry in skills_cat.entries {
            if entry.source.kind != crate::catalog::SourceKind::Local {
                catalog_map.insert(entry.source.url.clone(), entry.source.clone());
            }
        }
    }
    if let Ok(mcp_cat) = crate::catalog::load_effective_mcp_catalog(paths) {
        for entry in mcp_cat.entries {
            if entry.source.kind != crate::catalog::SourceKind::Local {
                catalog_map.insert(entry.source.url.clone(), entry.source.clone());
            }
        }
    }

    for (url, source) in catalog_map {
        let kind = match source.kind {
            crate::catalog::SourceKind::Git => "git".to_string(),
            crate::catalog::SourceKind::Http => "http".to_string(),
            crate::catalog::SourceKind::Local => "local".to_string(),
        };
        // Generate a nice ID
        let id = get_catalog_id_from_url(&url);

        catalogs.push(LockedCatalog {
            id,
            kind,
            url,
            rev: Some(source.reference.clone()),
            checksum: source.checksum.clone(),
        });
    }

    // 6. Packages
    let mut packages = Vec::new();
    if let Ok(fetch_units) = resolve_fetch_units(paths, &resolved) {
        for unit in fetch_units {
            // Find corresponding source_id from catalogs
            let source_id = catalogs
                .iter()
                .find(|c| c.url == unit.source.url)
                .map(|c| c.id.clone())
                .unwrap_or_else(|| "default-source".to_string());

            for selection in unit.selections {
                let package_type = match selection.kind {
                    SelectionKind::Skill => "skill".to_string(),
                    SelectionKind::Mcp => "mcp".to_string(),
                };

                let mut manifest_sha256 = "sha256:unknown".to_string();
                let mut installed_targets = Vec::new();

                if selection.kind == SelectionKind::Skill {
                    for tool in &config.tools.enabled {
                        let skill_dir = paths
                            .root
                            .join(format!(".{}", tool))
                            .join("skills")
                            .join(&selection.id);
                        let manifest_path = skill_dir.join("macc.package.json");
                        if manifest_path.exists() {
                            if let Ok(content) = fs::read(&manifest_path) {
                                manifest_sha256 = format!("sha256:{:x}", Sha256::digest(&content));
                            }
                        }
                        if skill_dir.is_dir() {
                            if let Ok(entries) = fs::read_dir(&skill_dir) {
                                for entry in entries.flatten() {
                                    let path = entry.path();
                                    if path.is_file() {
                                        if let Ok(content) = fs::read(&path) {
                                            let sha256 =
                                                format!("sha256:{:x}", Sha256::digest(&content));
                                            let rel_path = path
                                                .strip_prefix(&paths.root)
                                                .map(|p| p.to_string_lossy().into_owned())
                                                .unwrap_or_else(|_| {
                                                    path.to_string_lossy().into_owned()
                                                });
                                            installed_targets.push(LockedInstalledTarget {
                                                tool: tool.clone(),
                                                path: rel_path,
                                                sha256,
                                            });
                                        }
                                    }
                                }
                            }
                        }
                    }
                } else {
                    let mcp_dir = paths.macc_dir.join("mcp").join(&selection.id);
                    let manifest_path = mcp_dir.join("macc.package.json");
                    if manifest_path.exists() {
                        if let Ok(content) = fs::read(&manifest_path) {
                            manifest_sha256 = format!("sha256:{:x}", Sha256::digest(&content));
                        }
                    }
                }

                packages.push(LockedPackage {
                    id: selection.id.clone(),
                    package_type,
                    source_id: source_id.clone(),
                    subpath: selection.subpath,
                    manifest_sha256,
                    installed_targets,
                });
            }
        }
    }

    // 7. Coordinator hashes
    let coord = config.automation.coordinator.as_ref();
    let max_parallel = coord.and_then(|c| c.max_parallel).unwrap_or(0);

    let retry_input = format!(
        "{:?}|{:?}",
        coord.and_then(|c| c.error_code_retry_list.as_ref()),
        coord.and_then(|c| c.error_code_retry_max)
    );
    let retry_policy_hash = format!("sha256:{:x}", Sha256::digest(retry_input.as_bytes()));

    let rate_input = format!(
        "{:?}|{:?}|{:?}|{:?}",
        coord.and_then(|c| c.rate_limit_backoff_base_seconds),
        coord.and_then(|c| c.rate_limit_backoff_max_seconds),
        coord.and_then(|c| c.rate_limit_fallback_enabled),
        coord.and_then(|c| c.rate_limit_throttle_parallel)
    );
    let rate_limit_policy_hash = format!("sha256:{:x}", Sha256::digest(rate_input.as_bytes()));

    let coordinator = LockedCoordinator {
        storage_mode: coord
            .and_then(|c| c.storage_mode.clone())
            .unwrap_or_else(|| "sqlite".to_string()),
        max_parallel,
        retry_policy_hash,
        rate_limit_policy_hash,
    };

    Ok(LockManifest {
        lock_version: 1,
        created_at: now,
        macc_version,
        macc_binary_sha256,
        project_root_fingerprint,
        reference_branch: config
            .automation
            .coordinator
            .as_ref()
            .and_then(|c| c.reference_branch.clone())
            .unwrap_or_else(|| "master".to_string()),
        config_sha256,
        active_profile: "default".to_string(),
        preset: config
            .automation
            .coordinator
            .as_ref()
            .and_then(|c| c.preset.clone())
            .unwrap_or_else(|| "balanced".to_string()),
        tools: locked_tools,
        catalogs,
        packages,
        runtime,
        coordinator,
    })
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LockCheckReport {
    pub matches: bool,
    pub drift: Vec<String>,
}

pub fn verify_lock_manifest(paths: &ProjectPaths, lock: &LockManifest) -> Result<LockCheckReport> {
    use crate::resolve::resolve_fetch_units;
    use crate::tool::loader::ToolSpecLoader;

    let mut drift = Vec::new();

    // 1. Check config hash
    let config_content = fs::read_to_string(&paths.config_path).unwrap_or_default();
    let config_sha256 = format!("sha256:{:x}", Sha256::digest(config_content.as_bytes()));
    if config_sha256 != lock.config_sha256 {
        drift.push(format!(
            "Config file drift: current sha {} != locked {}",
            config_sha256, lock.config_sha256
        ));
    }

    // Load active config to compare other sections
    if let Ok(config) = crate::load_canonical_config(&paths.config_path) {
        let resolved = crate::resolve::resolve(&config, &Default::default());

        let spec_loader = ToolSpecLoader::new(ToolSpecLoader::default_search_paths(&paths.root));
        let (specs, _) = spec_loader.load_all_with_embedded();

        // 2. Verify tools & versions & generated files
        for locked_tool in &lock.tools {
            if !config.tools.enabled.contains(&locked_tool.id) {
                drift.push(format!(
                    "Tool '{}' is locked but currently disabled in config",
                    locked_tool.id
                ));
                continue;
            }

            let current_detected = specs
                .iter()
                .find(|s| s.id == locked_tool.id)
                .and_then(|spec| spec.version_check.as_ref())
                .and_then(|vc| crate::service::tooling::run_version_command(&vc.current))
                .unwrap_or_else(|| "0.1.0".to_string());

            if current_detected != locked_tool.detected_version {
                drift.push(format!(
                    "Tool '{}' version mismatch: current detected '{}' != locked '{}'",
                    locked_tool.id, current_detected, locked_tool.detected_version
                ));
            }

            // Verify generated files & hashes
            for locked_file in &locked_tool.generated_files {
                let file_path = paths.root.join(&locked_file.path);
                if !file_path.exists() {
                    drift.push(format!(
                        "Generated file '{}' for tool '{}' is missing from disk",
                        locked_file.path, locked_tool.id
                    ));
                } else if let Ok(content) = fs::read(&file_path) {
                    let sha256 = format!("sha256:{:x}", Sha256::digest(&content));
                    if sha256 != locked_file.sha256 {
                        drift.push(format!(
                            "Generated file '{}' hash drift: current '{}' != locked '{}'",
                            locked_file.path, sha256, locked_file.sha256
                        ));
                    }
                }
            }
        }

        // Check if any tool is enabled in config but missing from lock
        for enabled_tool in &config.tools.enabled {
            if !lock.tools.iter().any(|t| &t.id == enabled_tool) {
                drift.push(format!(
                    "Tool '{}' is enabled in config but missing from lockfile",
                    enabled_tool
                ));
            }
        }

        // 3. Verify catalogs
        let mut current_catalogs = std::collections::HashMap::new();
        if let Ok(skills_cat) = crate::catalog::load_effective_skills_catalog(paths) {
            for entry in skills_cat.entries {
                if entry.source.kind != crate::catalog::SourceKind::Local {
                    current_catalogs.insert(entry.source.url.clone(), entry.source.clone());
                }
            }
        }
        if let Ok(mcp_cat) = crate::catalog::load_effective_mcp_catalog(paths) {
            for entry in mcp_cat.entries {
                if entry.source.kind != crate::catalog::SourceKind::Local {
                    current_catalogs.insert(entry.source.url.clone(), entry.source.clone());
                }
            }
        }

        for locked_cat in &lock.catalogs {
            if let Some(source) = current_catalogs.get(&locked_cat.url) {
                if let Some(ref locked_rev) = locked_cat.rev {
                    if &source.reference != locked_rev {
                        drift.push(format!(
                            "Catalog '{}' revision drift: current '{}' != locked '{}'",
                            locked_cat.id, source.reference, locked_rev
                        ));
                    }
                }
                if locked_cat.checksum != source.checksum {
                    drift.push(format!(
                        "Catalog '{}' checksum drift: current '{:?}' != locked '{:?}'",
                        locked_cat.id, source.checksum, locked_cat.checksum
                    ));
                }
            } else {
                drift.push(format!(
                    "Catalog '{}' (url: {}) in lockfile is missing from current effective catalogs",
                    locked_cat.id, locked_cat.url
                ));
            }
        }

        // 4. Verify packages
        if let Ok(fetch_units) = resolve_fetch_units(paths, &resolved) {
            for unit in fetch_units {
                for selection in unit.selections {
                    if let Some(locked_pkg) = lock.packages.iter().find(|p| p.id == selection.id) {
                        // Compare subpath
                        if locked_pkg.subpath != selection.subpath {
                            drift.push(format!(
                                "Package '{}' subpath mismatch: current '{}' != locked '{}'",
                                selection.id, selection.subpath, locked_pkg.subpath
                            ));
                        }
                        // Verify target files' hashes
                        for target in &locked_pkg.installed_targets {
                            let target_path = paths.root.join(&target.path);
                            if !target_path.exists() {
                                drift.push(format!(
                                    "Package target file '{}' is missing from disk",
                                    target.path
                                ));
                            } else if let Ok(content) = fs::read(&target_path) {
                                let sha256 = format!("sha256:{:x}", Sha256::digest(&content));
                                if sha256 != target.sha256 {
                                    drift.push(format!("Package target file '{}' hash drift: current '{}' != locked '{}'", 
                                        target.path, sha256, target.sha256));
                                }
                            }
                        }
                    } else {
                        drift.push(format!(
                            "Package '{}' is currently selected but missing from lockfile",
                            selection.id
                        ));
                    }
                }
            }
        }

        // 5. Verify coordinator configuration
        let coord = config.automation.coordinator.as_ref();
        let max_parallel = coord.and_then(|c| c.max_parallel).unwrap_or(0);
        if max_parallel != lock.coordinator.max_parallel {
            drift.push(format!(
                "Coordinator max_parallel drift: current {} != locked {}",
                max_parallel, lock.coordinator.max_parallel
            ));
        }

        let current_storage_mode = coord
            .and_then(|c| c.storage_mode.clone())
            .unwrap_or_else(|| "sqlite".to_string());
        if current_storage_mode != lock.coordinator.storage_mode {
            drift.push(format!(
                "Coordinator storage_mode drift: current '{}' != locked '{}'",
                current_storage_mode, lock.coordinator.storage_mode
            ));
        }

        let retry_input = format!(
            "{:?}|{:?}",
            coord.and_then(|c| c.error_code_retry_list.as_ref()),
            coord.and_then(|c| c.error_code_retry_max)
        );
        let current_retry_hash = format!("sha256:{:x}", Sha256::digest(retry_input.as_bytes()));
        if current_retry_hash != lock.coordinator.retry_policy_hash {
            drift.push("Coordinator retry policy drift from lockfile".to_string());
        }

        let rate_input = format!(
            "{:?}|{:?}|{:?}|{:?}",
            coord.and_then(|c| c.rate_limit_backoff_base_seconds),
            coord.and_then(|c| c.rate_limit_backoff_max_seconds),
            coord.and_then(|c| c.rate_limit_fallback_enabled),
            coord.and_then(|c| c.rate_limit_throttle_parallel)
        );
        let current_rate_hash = format!("sha256:{:x}", Sha256::digest(rate_input.as_bytes()));
        if current_rate_hash != lock.coordinator.rate_limit_policy_hash {
            drift.push("Coordinator rate limit policy drift from lockfile".to_string());
        }

        // 6. Verify runtime
        for (key, val) in &lock.runtime {
            if key == "os" {
                let current_os = std::env::consts::OS.to_string();
                if &current_os != val {
                    drift.push(format!(
                        "Runtime OS mismatch: current '{}' != locked '{}'",
                        current_os, val
                    ));
                }
            } else if key == "arch" {
                let current_arch = std::env::consts::ARCH.to_string();
                if &current_arch != val {
                    drift.push(format!(
                        "Runtime arch mismatch: current '{}' != locked '{}'",
                        current_arch, val
                    ));
                }
            }
        }
    }

    Ok(LockCheckReport {
        matches: drift.is_empty(),
        drift,
    })
}

// =========================================================================
// Failure Recovery Summary
// =========================================================================

pub fn load_task_registry(
    paths: &ProjectPaths,
    config: &CanonicalConfig,
) -> Result<crate::coordinator::model::TaskRegistry> {
    let mut args = std::collections::BTreeMap::new();
    let coord = config.automation.coordinator.as_ref();
    if let Some(storage_mode) = coord.and_then(|c| c.storage_mode.as_ref()) {
        args.insert("storage-mode".to_string(), storage_mode.clone());
    }
    if let Some(fallback) = coord.and_then(|c| c.legacy_json_fallback) {
        if fallback {
            args.insert("legacy-json-fallback".to_string(), "true".to_string());
        }
    }
    let value = crate::coordinator::state::coordinator_state_registry_load(&paths.root, &args)?;
    let registry: crate::coordinator::model::TaskRegistry = serde_json::from_value(value)
        .map_err(|e| MaccError::Validation(format!("Failed to parse registry: {}", e)))?;
    Ok(registry)
}

pub fn log_ops_action(paths: &ProjectPaths, action: &str, task_id: &str) -> Result<()> {
    #[derive(Serialize)]
    struct AuditRecord {
        timestamp: String,
        actor: String,
        action: String,
        method: String,
        path: String,
        inputs_summary: serde_json::Value,
        result: AuditResult,
        duration_ms: u64,
        log_path: &'static str,
    }
    #[derive(Serialize)]
    struct AuditResult {
        status_code: u16,
    }

    let log_path = paths.root.join(".macc/log/ops.jsonl");
    if let Some(parent) = log_path.parent() {
        std::fs::create_dir_all(parent).ok();
    }

    let actor = std::env::var("USER")
        .or_else(|_| std::env::var("USERNAME"))
        .unwrap_or_else(|_| "unknown".to_string());

    let record = AuditRecord {
        timestamp: chrono::Utc::now().to_rfc3339(),
        actor,
        action: format!("cli failure {}", action),
        method: "CLI".to_string(),
        path: format!("/failure/{}", action),
        inputs_summary: serde_json::json!({ "task_id": task_id }),
        result: AuditResult { status_code: 200 },
        duration_ms: 0,
        log_path: ".macc/log/ops.jsonl",
    };

    if let Ok(mut file) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&log_path)
    {
        if let Ok(line) = serde_json::to_vec(&record) {
            use std::io::Write;
            let _ = file.write_all(&line);
            let _ = file.write_all(b"\n");
        }
    }
    Ok(())
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

pub fn get_failure_summary(
    paths: &ProjectPaths,
    config: &CanonicalConfig,
    task_id: &str,
) -> Result<FailureSummary> {
    let registry = load_task_registry(paths, config)?;
    let task = registry
        .tasks
        .iter()
        .find(|t| t.id == task_id)
        .ok_or_else(|| MaccError::Validation(format!("Task {} not found", task_id)))?;

    let runtime = &task.task_runtime;
    let error_code = runtime
        .last_error_code
        .clone()
        .unwrap_or_else(|| "E901".to_string());
    let normalized_cause =
        crate::coordinator::error_normalizer::error_code_to_canonical_class(&error_code);

    let retry_policy =
        crate::coordinator::error_normalizer::retry_policy_for_error_code(&error_code);
    let retryable = retry_policy == crate::coordinator::error_normalizer::RetryPolicy::Retryable;
    let user_action_required = retry_policy
        == crate::coordinator::error_normalizer::RetryPolicy::Conditional
        || retry_policy == crate::coordinator::error_normalizer::RetryPolicy::NotRetryable;

    let affected_worktree = task
        .worktree
        .as_ref()
        .and_then(|w| w.worktree_path.clone())
        .or_else(|| Some(format!(".macc/worktree/{}", task_id)));

    let last_safe_state = if task.state == "claimed" {
        "claimed task session initialized".to_string()
    } else if task.state == "in_progress" {
        "performer in-progress state snapshot".to_string()
    } else {
        "base branch workspace".to_string()
    };

    let recommended_action = if retryable {
        "retry with backoff or switch tool".to_string()
    } else {
        "inspect files and resolve conflicts manually".to_string()
    };

    Ok(FailureSummary {
        task_id: task_id.to_string(),
        normalized_cause,
        error_code,
        retryable,
        user_action_required,
        last_safe_state,
        affected_worktree,
        affected_files: vec![],
        recommended_action,
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
    })
}

pub fn apply_preset_to_config(config: &mut CanonicalConfig, preset_name: &str) -> Result<()> {
    let coordinator = config
        .automation
        .coordinator
        .get_or_insert_with(Default::default);
    coordinator.preset = Some(preset_name.to_string());
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
        _ => {
            return Err(MaccError::Validation(format!(
                "Unknown preset '{}'. Choose from conservative, balanced, throughput.",
                preset_name
            )))
        }
    }
    Ok(())
}

pub fn apply_preset_to_env_cfg(
    env_cfg: &mut CoordinatorEnvConfig,
    preset_name: &str,
) -> Result<()> {
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
        _ => {
            return Err(MaccError::Validation(format!(
                "Unknown preset '{}'. Choose from conservative, balanced, throughput.",
                preset_name
            )))
        }
    }
    Ok(())
}

pub fn print_trust_review_card(
    paths: &ProjectPaths,
    plan: &crate::plan::ActionPlan,
    allowed_user_scope: bool,
) {
    let has_user_level = plan
        .actions
        .iter()
        .any(|a| a.scope() == crate::plan::Scope::User);
    let scope_str = if has_user_level || allowed_user_scope {
        "user-level write"
    } else {
        "project-level write"
    };
    let files_to_change = plan
        .actions
        .iter()
        .filter(|a| {
            matches!(
                a,
                crate::plan::Action::WriteFile { .. } | crate::plan::Action::MergeJson { .. }
            )
        })
        .count();
    let user_files = plan
        .actions
        .iter()
        .filter(|a| {
            a.scope() == crate::plan::Scope::User
                && matches!(
                    a,
                    crate::plan::Action::WriteFile { .. } | crate::plan::Action::MergeJson { .. }
                )
        })
        .count();

    let backups_dir = paths.macc_dir.join("backups");
    let mut backup_str = "not found (will create)".to_string();
    if backups_dir.exists() {
        if let Ok(entries) = std::fs::read_dir(&backups_dir) {
            let mut latest = None;
            for entry in entries.flatten() {
                if let Ok(meta) = entry.metadata() {
                    if meta.is_dir() {
                        let name = entry.file_name().to_string_lossy().into_owned();
                        if latest.is_none() || name > latest.clone().unwrap() {
                            latest = Some(name);
                        }
                    }
                }
            }
            if let Some(l) = latest {
                backup_str = format!(".macc/backups/{}", l);
            } else {
                backup_str = "ready".to_string();
            }
        } else {
            backup_str = "ready".to_string();
        }
    }

    let mut pinned = 0;
    let mut unpinned = 0;
    if let Ok(skills) = crate::catalog::load_effective_skills_catalog(paths) {
        for entry in skills.entries {
            if entry.source.kind != crate::catalog::SourceKind::Local {
                if entry.source.checksum.is_some() && entry.source.checksum.as_deref() != Some("") {
                    pinned += 1;
                } else {
                    unpinned += 1;
                }
            }
        }
    }
    if let Ok(mcp) = crate::catalog::load_effective_mcp_catalog(paths) {
        for entry in mcp.entries {
            if entry.source.kind != crate::catalog::SourceKind::Local {
                if entry.source.checksum.is_some() && entry.source.checksum.as_deref() != Some("") {
                    pinned += 1;
                } else {
                    unpinned += 1;
                }
            }
        }
    }

    let mut secrets_count = 0;
    for action in &plan.actions {
        match action {
            crate::plan::Action::WriteFile { path, content, .. } => {
                let findings = crate::security::scan_bytes(path, content);
                secrets_count += findings
                    .iter()
                    .filter(|f| f.severity == crate::security::Severity::Error)
                    .count();
            }
            crate::plan::Action::MergeJson { path, patch, .. } => {
                let content = serde_json::to_vec(patch).unwrap_or_default();
                let findings = crate::security::scan_bytes(path, &content);
                secrets_count += findings
                    .iter()
                    .filter(|f| f.severity == crate::security::Severity::Error)
                    .count();
            }
            _ => {}
        }
    }
    let secrets_str = if secrets_count == 0 {
        "none".to_string()
    } else {
        format!("{} detected", secrets_count)
    };

    println!("\nTrust Review");
    println!("Scope:            {}", scope_str);
    println!("Files to change:  {}", files_to_change);
    println!("User-level files: {}", user_files);
    println!("Backups:          {}", backup_str);
    println!("Remote inputs:    {} pinned, {} unpinned", pinned, unpinned);
    println!("Secrets detected: {}", secrets_str);
    println!("Rollback:         macc restore --backup <id>\n");
}
