use crate::{MaccError, ProjectPaths, Result};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};
use sha2::{Digest, Sha256};
use chrono::Utc;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum MatchStrength {
    None,
    Weak,
    Medium,
    Strong,
}

impl MatchStrength {
    pub fn as_str(&self) -> &'static str {
        match self {
            MatchStrength::None => "none",
            MatchStrength::Weak => "weak",
            MatchStrength::Medium => "medium",
            MatchStrength::Strong => "strong",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct SaveBundleManifest {
    pub version: u32,
    pub kind: String,
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    pub created_at: String,
    pub updated_at: String,
    pub macc_version: String,
    pub repository: RepositoryIdentity,
    pub includes: SaveIncludes,
    pub excludes: SaveExcludes,
    pub paths: SavePaths,
    pub hashes: SaveHashes,
    pub security: SaveSecurity,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct RepositoryIdentity {
    pub root_name: String,
    pub root_path_hash: String,
    pub git_remote_url_hash: String,
    pub git_default_branch: String,
    pub git_current_branch: String,
    pub git_head_sha: String,
    pub identity_strength: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct SaveIncludes {
    pub config: bool,
    pub coordinator_sessions: bool,
    pub catalogs: bool,
    pub logs: bool,
    pub prd: bool,
    pub automation_state: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct SaveExcludes {
    pub worktrees: bool,
    pub cache: bool,
    pub generated_files: bool,
    pub secrets: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct SavePaths {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub config: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub coordinator_sessions: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub logs_archive: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub task_registry: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct SaveHashes {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub config: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub coordinator_sessions: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub task_registry: Option<String>,
    pub manifest_payload: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct SaveSecurity {
    pub secret_scan: SecretScanMetadata,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct SecretScanMetadata {
    pub performed: bool,
    pub findings: usize,
    pub redacted_logs: bool,
}

pub struct SaveOptions {
    pub description: Option<String>,
    pub overwrite: bool,
    pub only: Option<String>, // comma separated
    pub no_sessions: bool,
    pub include_logs: bool,
    pub log_max_size: String,
    pub log_since: String,
    pub redact_logs: bool,
    pub dry_run: bool,
    pub include_prd: bool,
    pub include_state: bool,
}

pub struct RestoreOptions {
    pub apply: bool,
    pub config_only: bool,
    pub sessions: bool,
    pub no_sessions: bool,
    pub include_logs: bool,
    pub dry_run: bool,
    pub yes: bool,
}

pub fn user_saves_dir() -> Option<PathBuf> {
    crate::user_backup::find_user_home().map(|home| home.join(".macc").join("saves"))
}

fn normalize_git_url(url: &str) -> String {
    let mut s = url.trim().to_ascii_lowercase();
    if s.starts_with("https://") {
        s = s["https://".len()..].to_string();
    } else if s.starts_with("http://") {
        s = s["http://".len()..].to_string();
    } else if s.starts_with("ssh://") {
        s = s["ssh://".len()..].to_string();
    }
    if s.starts_with("git@") {
        s = s["git@".len()..].to_string();
    }
    s = s.replace(':', "/");
    if s.ends_with(".git") {
        s = s[..s.len() - 4].to_string();
    }
    s
}

fn compute_file_sha256(path: &Path) -> std::io::Result<String> {
    let content = fs::read(path)?;
    let mut hasher = Sha256::new();
    hasher.update(&content);
    Ok(format!("sha256:{:x}", hasher.finalize()))
}

fn hash_string(s: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(s.as_bytes());
    format!("sha256:{:x}", hasher.finalize())
}

fn get_git_remote_url(repo_root: &Path) -> Option<String> {
    if let Ok(out) = crate::git::run_git_output_mapped(repo_root, &["remote", "get-url", "origin"], "get remote url") {
        let url = String::from_utf8_lossy(&out.stdout).trim().to_string();
        if !url.is_empty() {
            return Some(url);
        }
    }
    None
}

fn get_git_current_branch(repo_root: &Path) -> Option<String> {
    if let Ok(out) = crate::git::run_git_output_mapped(repo_root, &["rev-parse", "--abbrev-ref", "HEAD"], "get current branch") {
        let branch = String::from_utf8_lossy(&out.stdout).trim().to_string();
        if !branch.is_empty() {
            return Some(branch);
        }
    }
    None
}

fn get_git_default_branch(repo_root: &Path) -> Option<String> {
    if let Ok(out) = crate::git::run_git_output_mapped(repo_root, &["symbolic-ref", "refs/remotes/origin/HEAD"], "get default branch") {
        let ref_path = String::from_utf8_lossy(&out.stdout).trim().to_string();
        if let Some(branch) = ref_path.split('/').last() {
            return Some(branch.to_string());
        }
    }
    None
}

fn get_git_head_sha(repo_root: &Path) -> Option<String> {
    if let Ok(out) = crate::git::run_git_output_mapped(repo_root, &["rev-parse", "HEAD"], "get HEAD sha") {
        let sha = String::from_utf8_lossy(&out.stdout).trim().to_string();
        if !sha.is_empty() {
            return Some(sha);
        }
    }
    None
}

pub fn get_repository_identity(repo_root: &Path) -> RepositoryIdentity {
    let root_name = repo_root.file_name()
        .map(|s| s.to_string_lossy().to_string())
        .unwrap_or_else(|| "unknown".to_string());
    
    let root_path_hash = hash_string(&repo_root.to_string_lossy());
    
    let remote_url = get_git_remote_url(repo_root);
    let git_remote_url_hash = remote_url
        .as_ref()
        .map(|url| hash_string(&normalize_git_url(url)))
        .unwrap_or_else(|| "".to_string());
        
    let git_current_branch = get_git_current_branch(repo_root).unwrap_or_else(|| "main".to_string());
    let git_default_branch = get_git_default_branch(repo_root).unwrap_or_else(|| "main".to_string());
    let git_head_sha = get_git_head_sha(repo_root).unwrap_or_else(|| "".to_string());
    
    let identity_strength = if !git_remote_url_hash.is_empty() {
        "strong".to_string()
    } else if !git_head_sha.is_empty() {
        "medium".to_string()
    } else {
        "weak".to_string()
    };

    RepositoryIdentity {
        root_name,
        root_path_hash,
        git_remote_url_hash,
        git_default_branch,
        git_current_branch,
        git_head_sha,
        identity_strength,
    }
}

pub fn compute_match_strength(current: &RepositoryIdentity, manifest: &RepositoryIdentity) -> MatchStrength {
    if !current.git_remote_url_hash.is_empty() && current.git_remote_url_hash == manifest.git_remote_url_hash {
        MatchStrength::Strong
    } else if current.root_name == manifest.root_name && current.root_path_hash == manifest.root_path_hash {
        MatchStrength::Medium
    } else if current.root_name == manifest.root_name {
        MatchStrength::Weak
    } else {
        MatchStrength::None
    }
}

pub fn redact_secrets_in_text(text: &str) -> String {
    let mut redacted = text.to_string();
    let regexes = [
        regex::Regex::new(r"AKIA[0-9A-Z]{16}").unwrap(),
        regex::Regex::new(r"sk-[a-zA-Z0-9]{20,}").unwrap(),
        regex::Regex::new(r"ghp_[a-zA-Z0-9]{36}").unwrap(),
    ];
    for re in &regexes {
        redacted = re.replace_all(&redacted, "[REDACTED]").into_owned();
    }
    redacted
}

fn parse_size_to_bytes(s: &str) -> u64 {
    let s = s.trim().to_ascii_uppercase();
    if s.is_empty() {
        return 50 * 1024 * 1024; // Default fallback
    }
    
    let (num_part, multiplier) = if s.ends_with("GB") {
        (&s[..s.len() - 2], 1024 * 1024 * 1024)
    } else if s.ends_with("MB") {
        (&s[..s.len() - 2], 1024 * 1024)
    } else if s.ends_with("KB") {
        (&s[..s.len() - 2], 1024)
    } else if s.ends_with('B') {
        (&s[..s.len() - 1], 1)
    } else {
        (s.as_str(), 1)
    };
    
    match num_part.trim().parse::<u64>() {
        Ok(val) => val * multiplier,
        Err(_) => 50 * 1024 * 1024, // Fallback
    }
}

fn parse_duration_to_seconds(s: &str) -> u64 {
    let s = s.trim().to_ascii_lowercase();
    if s.is_empty() {
        return 7 * 24 * 3600; // Default fallback (7 days)
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
        (s.as_str(), 1)
    };
    
    match num_part.trim().parse::<u64>() {
        Ok(val) => val * multiplier,
        Err(_) => 7 * 24 * 3600, // Fallback
    }
}

fn copy_dir_all(src: impl AsRef<Path>, dst: impl AsRef<Path>) -> std::io::Result<()> {
    fs::create_dir_all(&dst)?;
    for entry in fs::read_dir(src)? {
        let entry = entry?;
        let ty = entry.file_type()?;
        if ty.is_dir() {
            let name = entry.file_name().to_string_lossy().to_lowercase();
            if name == "cache" || name == "download" || name == "installed" || name == "worktree" || name == "worktrees" {
                continue;
            }
            copy_dir_all(entry.path(), dst.as_ref().join(entry.file_name()))?;
        } else {
            fs::copy(entry.path(), dst.as_ref().join(entry.file_name()))?;
        }
    }
    Ok(())
}

pub fn create_save_bundle(paths: &ProjectPaths, name: &str, opts: &SaveOptions) -> Result<SaveBundleManifest> {
    if name.is_empty() || name.chars().any(|c| !c.is_alphanumeric() && c != '-' && c != '_') {
        return Err(MaccError::Validation(format!("MACC-SAVE-1000: Invalid save name: {}", name)));
    }

    let saves_dir = user_saves_dir().ok_or(MaccError::HomeDirNotFound)?;
    let target_save_dir = saves_dir.join(name);
    
    if target_save_dir.exists() && !opts.overwrite {
        return Err(MaccError::Validation(format!("MACC-SAVE-1001: Save already exists: {}", name)));
    }

    if !paths.config_path.exists() {
        return Err(MaccError::Validation("MACC-SAVE-1002: No .macc/macc.yaml found in project root.".to_string()));
    }

    // Determine what to include
    let only_sections: Option<BTreeSet<String>> = opts.only.as_ref().map(|s| {
        s.split(',').map(|sec| sec.trim().to_string()).collect()
    });

    let include_config = only_sections.as_ref().map(|s| s.contains("config")).unwrap_or(true);
    let include_sessions = !opts.no_sessions && only_sections.as_ref().map(|s| s.contains("sessions")).unwrap_or(true);
    let include_catalogs = only_sections.as_ref().map(|s| s.contains("catalogs")).unwrap_or(true);
    let include_logs = opts.include_logs || only_sections.as_ref().map(|s| s.contains("logs")).unwrap_or(false);
    let include_prd = opts.include_prd || only_sections.as_ref().map(|s| s.contains("prd")).unwrap_or(false);
    let include_state = opts.include_state || only_sections.as_ref().map(|s| s.contains("automation_state")).unwrap_or(false);

    let tmp_parent = saves_dir.join(".tmp");
    fs::create_dir_all(&tmp_parent).ok();
    let tmp_save_dir = tmp_parent.join(format!("{}.{}", name, uuid::Uuid::new_v4()));
    fs::create_dir_all(&tmp_save_dir).map_err(|e| MaccError::Io {
        path: tmp_save_dir.to_string_lossy().into(),
        action: "create temp save directory".into(),
        source: e,
    })?;

    let mut includes = SaveIncludes {
        config: false,
        coordinator_sessions: false,
        catalogs: false,
        logs: false,
        prd: false,
        automation_state: false,
    };

    let mut paths_meta = SavePaths {
        config: None,
        coordinator_sessions: None,
        logs_archive: None,
        task_registry: None,
    };

    let mut hashes = SaveHashes {
        config: None,
        coordinator_sessions: None,
        task_registry: None,
        manifest_payload: "".to_string(),
    };

    let mut findings_count = 0;

    // 1. Copy config
    if include_config && paths.config_path.exists() {
        let config_dir = tmp_save_dir.join("config");
        fs::create_dir_all(&config_dir).ok();
        let dest = config_dir.join("macc.yaml");
        fs::copy(&paths.config_path, &dest).map_err(|e| MaccError::Io {
            path: dest.to_string_lossy().into(),
            action: "copy config file".into(),
            source: e,
        })?;
        
        let config_bytes = fs::read(&dest).unwrap();
        let findings = crate::security::scan_bytes("config/macc.yaml", &config_bytes);
        findings_count += findings.iter().filter(|f| f.severity == crate::security::Severity::Error).count();

        let sha = compute_file_sha256(&dest).unwrap();
        hashes.config = Some(sha);
        paths_meta.config = Some("config/macc.yaml".to_string());
        includes.config = true;
    }

    // 2. Copy sessions
    let sessions_src = paths.macc_dir.join("state").join("tool-sessions.json");
    if include_sessions && sessions_src.exists() {
        let state_dir = tmp_save_dir.join("state");
        fs::create_dir_all(&state_dir).ok();
        let dest = state_dir.join("tool-sessions.json");
        fs::copy(&sessions_src, &dest).map_err(|e| MaccError::Io {
            path: dest.to_string_lossy().into(),
            action: "copy tool-sessions file".into(),
            source: e,
        })?;

        let sessions_bytes = fs::read(&dest).unwrap();
        let findings = crate::security::scan_bytes("state/tool-sessions.json", &sessions_bytes);
        findings_count += findings.iter().filter(|f| f.severity == crate::security::Severity::Error).count();

        let sha = compute_file_sha256(&dest).unwrap();
        hashes.coordinator_sessions = Some(sha);
        paths_meta.coordinator_sessions = Some("state/tool-sessions.json".to_string());
        includes.coordinator_sessions = true;
    }

    // 3. Copy catalogs
    if include_catalogs && paths.project_catalog_dir().exists() {
        let catalogs_dest = tmp_save_dir.join("catalogs");
        if let Err(e) = copy_dir_all(&paths.project_catalog_dir(), &catalogs_dest) {
            fs::remove_dir_all(&tmp_save_dir).ok();
            return Err(MaccError::Io {
                path: catalogs_dest.to_string_lossy().into(),
                action: "copy catalog directory".into(),
                source: e,
            });
        }
        includes.catalogs = true;
    }

    // 4. Copy task registry / state
    let registry_src = paths.macc_dir.join("automation").join("task");
    if (include_prd || include_state) && registry_src.exists() {
        let registry_dest = tmp_save_dir.join("automation").join("task");
        fs::create_dir_all(registry_dest.parent().unwrap()).ok();
        if let Err(e) = copy_dir_all(&registry_src, &registry_dest) {
            fs::remove_dir_all(&tmp_save_dir).ok();
            return Err(MaccError::Io {
                path: registry_dest.to_string_lossy().into(),
                action: "copy task registry directory".into(),
                source: e,
            });
        }
        includes.prd = include_prd;
        includes.automation_state = include_state;
        paths_meta.task_registry = Some("automation/task".to_string());
    }

    // 5. Copy logs
    let logs_src = paths.macc_dir.join("log");
    if include_logs && logs_src.exists() {
        let logs_dest = tmp_save_dir.join("logs");
        fs::create_dir_all(&logs_dest).ok();
        
        // Custom copy logs with size limit, time range, and redaction
        let max_bytes = parse_size_to_bytes(&opts.log_max_size);
        let max_age_seconds = parse_duration_to_seconds(&opts.log_since);
        let mut total_copied = 0;

        let mut read_logs = |dir: &Path| -> std::io::Result<()> {
            if !dir.exists() { return Ok(()); }
            for entry in fs::read_dir(dir)? {
                let entry = entry?;
                let path = entry.path();
                if path.is_file() {
                    let meta = entry.metadata()?;
                    if let Ok(modified_time) = meta.modified() {
                        if let Ok(elapsed) = modified_time.elapsed() {
                            if elapsed.as_secs() > max_age_seconds {
                                continue;
                            }
                        }
                    }
                    if total_copied + meta.len() > max_bytes {
                        break;
                    }
                    if let Ok(mut text) = fs::read_to_string(&path) {
                        if opts.redact_logs {
                            text = redact_secrets_in_text(&text);
                        }
                        let rel = path.strip_prefix(&logs_src).unwrap();
                        let target_path = logs_dest.join(rel);
                        fs::create_dir_all(target_path.parent().unwrap()).ok();
                        fs::write(&target_path, text)?;
                        total_copied += meta.len();
                    }
                }
            }
            Ok(())
        };

        let _ = read_logs(&logs_src);
        let _ = read_logs(&logs_src.join("coordinator"));
        let _ = read_logs(&logs_src.join("performer"));
        
        includes.logs = true;
        paths_meta.logs_archive = Some("logs".to_string());
    }

    // If dry run, clean up temp dir and abort
    if opts.dry_run {
        fs::remove_dir_all(&tmp_save_dir).ok();
        return Ok(SaveBundleManifest {
            version: 1,
            kind: "macc.save_bundle".to_string(),
            name: name.to_string(),
            description: opts.description.clone(),
            created_at: Utc::now().to_rfc3339(),
            updated_at: Utc::now().to_rfc3339(),
            macc_version: "0.1.0".to_string(),
            repository: get_repository_identity(&paths.root),
            includes,
            excludes: SaveExcludes {
                worktrees: true,
                cache: true,
                generated_files: true,
                secrets: true,
            },
            paths: paths_meta,
            hashes,
            security: SaveSecurity {
                secret_scan: SecretScanMetadata {
                    performed: true,
                    findings: findings_count,
                    redacted_logs: opts.redact_logs,
                },
            },
        });
    }

    // Abort if secrets were found in config/session files
    if findings_count > 0 {
        fs::remove_dir_all(&tmp_save_dir).ok();
        return Err(MaccError::Validation(format!("MACC-SAVE-1003: Secret scan failed. Found {} potential secrets in save files. Aborted.", findings_count)));
    }

    // Write manifest
    let manifest_path = tmp_save_dir.join("manifest.yaml");
    let mut manifest = SaveBundleManifest {
        version: 1,
        kind: "macc.save_bundle".to_string(),
        name: name.to_string(),
        description: opts.description.clone(),
        created_at: Utc::now().to_rfc3339(),
        updated_at: Utc::now().to_rfc3339(),
        macc_version: "0.1.0".to_string(),
        repository: get_repository_identity(&paths.root),
        includes,
        excludes: SaveExcludes {
            worktrees: true,
            cache: true,
            generated_files: true,
            secrets: true,
        },
        paths: paths_meta,
        hashes: hashes.clone(),
        security: SaveSecurity {
            secret_scan: SecretScanMetadata {
                performed: true,
                findings: 0,
                redacted_logs: opts.redact_logs,
            },
        },
    };

    let manifest_str = serde_yaml::to_string(&manifest).map_err(|e| MaccError::Validation(e.to_string()))?;
    fs::write(&manifest_path, manifest_str).map_err(|e| MaccError::Io {
        path: manifest_path.to_string_lossy().into(),
        action: "write manifest".into(),
        source: e,
    })?;

    // Calculate manifest payload hash
    let manifest_payload = compute_file_sha256(&manifest_path).unwrap();
    manifest.hashes.manifest_payload = manifest_payload;

    // Rewrite manifest with its own hash updated
    let manifest_str = serde_yaml::to_string(&manifest).map_err(|e| MaccError::Validation(e.to_string()))?;
    fs::write(&manifest_path, manifest_str).map_err(|e| MaccError::Io {
        path: manifest_path.to_string_lossy().into(),
        action: "rewrite manifest with hashes".into(),
        source: e,
    })?;

    // Atomic move to saves dir
    if target_save_dir.exists() {
        let backup_dir = saves_dir.join(format!("{}.old.{}", name, uuid::Uuid::new_v4()));
        fs::rename(&target_save_dir, &backup_dir).ok();
        if let Err(e) = fs::rename(&tmp_save_dir, &target_save_dir) {
            fs::rename(&backup_dir, &target_save_dir).ok(); // Rollback
            return Err(MaccError::Io {
                path: target_save_dir.to_string_lossy().into(),
                action: "atomic replace save directory".into(),
                source: e,
            });
        }
        fs::remove_dir_all(&backup_dir).ok();
    } else {
        fs::create_dir_all(&saves_dir).ok();
        fs::rename(&tmp_save_dir, &target_save_dir).map_err(|e| MaccError::Io {
            path: target_save_dir.to_string_lossy().into(),
            action: "atomic create save directory".into(),
            source: e,
        })?;
    }

    Ok(manifest)
}

pub fn restore_save_bundle(paths: &ProjectPaths, name: &str, opts: &RestoreOptions) -> Result<()> {
    let saves_dir = user_saves_dir().ok_or(MaccError::HomeDirNotFound)?;
    let target_save_dir = saves_dir.join(name);
    
    if !target_save_dir.exists() {
        return Err(MaccError::Validation(format!("MACC-RESTORE-2000: Save bundle not found: {}", name)));
    }

    let manifest_path = target_save_dir.join("manifest.yaml");
    let manifest_str = fs::read_to_string(&manifest_path).map_err(|e| MaccError::Io {
        path: manifest_path.to_string_lossy().into(),
        action: "read manifest for restore".into(),
        source: e,
    })?;

    let manifest: SaveBundleManifest = serde_yaml::from_str(&manifest_str).map_err(|e| {
        MaccError::Validation(format!("MACC-RESTORE-2001: Unsupported or malformed manifest: {}", e))
    })?;

    // Repository match validation
    let current_repo = get_repository_identity(&paths.root);
    let match_strength = compute_match_strength(&current_repo, &manifest.repository);
    if match_strength == MatchStrength::None && !opts.yes {
        return Err(MaccError::Validation("MACC-RESTORE-2002: Repository mismatch. Choose config-only restore or confirm with yes.".to_string()));
    }

    // Verify config file checksum if present
    if let Some(config_rel) = &manifest.paths.config {
        let config_src = target_save_dir.join(config_rel);
        if config_src.exists() {
            let actual_sha = compute_file_sha256(&config_src).unwrap();
            if let Some(expected_sha) = &manifest.hashes.config {
                if &actual_sha != expected_sha {
                    return Err(MaccError::Validation("MACC-RESTORE-2003: Checksum mismatch. Save configuration file is corrupted.".to_string()));
                }
            }
        }
    }

    // Verify session file checksum if present
    if let Some(sessions_rel) = &manifest.paths.coordinator_sessions {
        let sessions_src = target_save_dir.join(sessions_rel);
        if sessions_src.exists() {
            let actual_sha = compute_file_sha256(&sessions_src).unwrap();
            if let Some(expected_sha) = &manifest.hashes.coordinator_sessions {
                if &actual_sha != expected_sha {
                    return Err(MaccError::Validation("MACC-RESTORE-2003: Checksum mismatch. Save sessions file is corrupted.".to_string()));
                }
            }
        }
    }

    if opts.dry_run {
        return Ok(());
    }

    // 1. Restore config
    let config_only = opts.config_only || match_strength == MatchStrength::Weak;
    if manifest.includes.config {
        if let Some(config_rel) = &manifest.paths.config {
            let config_src = target_save_dir.join(config_rel);
            if config_src.exists() {
                fs::create_dir_all(paths.config_path.parent().unwrap()).ok();
                
                // Back up overwritten file first
                if paths.config_path.exists() {
                    let backup_name = format!("macc.yaml.backup.{}", Utc::now().timestamp());
                    let backup_path = paths.config_path.parent().unwrap().join(backup_name);
                    fs::copy(&paths.config_path, &backup_path).ok();
                }

                fs::copy(&config_src, &paths.config_path).map_err(|e| MaccError::Io {
                    path: paths.config_path.to_string_lossy().into(),
                    action: "restore config file".into(),
                    source: e,
                })?;
            }
        }
    }

    if config_only {
        // Skip sessions, catalogs, logs, state
        return Ok(());
    }

    // 2. Restore sessions
    let restore_sessions = opts.sessions || (!opts.no_sessions && manifest.includes.coordinator_sessions);
    if restore_sessions {
        if let Some(sessions_rel) = &manifest.paths.coordinator_sessions {
            let sessions_src = target_save_dir.join(sessions_rel);
            if sessions_src.exists() {
                let sessions_dest = paths.macc_dir.join("state").join("tool-sessions.json");
                fs::create_dir_all(sessions_dest.parent().unwrap()).ok();
                
                // Session normalization: drop PIDs / stale leases
                if let Ok(content) = fs::read_to_string(&sessions_src) {
                    if let Ok(mut val) = serde_json::from_str::<serde_json::Value>(&content) {
                        if let Some(obj) = val.as_object_mut() {
                            // Drop PID and state fields that are live PIDs
                            obj.remove("pid");
                            obj.remove("active_worktree");
                        }
                        let normalized_bytes = serde_json::to_vec_pretty(&val).unwrap();
                        fs::write(&sessions_dest, normalized_bytes).ok();
                    } else {
                        fs::copy(&sessions_src, &sessions_dest).ok();
                    }
                }
            }
        }
    }

    // 3. Restore catalogs
    if manifest.includes.catalogs {
        let catalogs_src = target_save_dir.join("catalogs");
        if catalogs_src.exists() {
            let _ = copy_dir_all(&catalogs_src, &paths.project_catalog_dir());
        }
    }

    // 4. Restore logs to restored-logs directory
    if opts.include_logs && manifest.includes.logs {
        let logs_src = target_save_dir.join("logs");
        if logs_src.exists() {
            let restored_logs_dest = paths.macc_dir.join("restored-logs").join(name);
            let _ = copy_dir_all(&logs_src, &restored_logs_dest);
        }
    }

    // 5. Restore task registry / state
    if manifest.includes.prd || manifest.includes.automation_state {
        let registry_src = target_save_dir.join("automation").join("task");
        if registry_src.exists() {
            let registry_dest = paths.macc_dir.join("automation").join("task");
            let _ = copy_dir_all(&registry_src, &registry_dest);
        }
    }

    Ok(())
}

pub fn list_save_bundles() -> Result<Vec<SaveBundleManifest>> {
    let saves_dir = match user_saves_dir() {
        Some(d) => d,
        None => return Ok(Vec::new()),
    };
    if !saves_dir.exists() {
        return Ok(Vec::new());
    }
    let mut list = Vec::new();
    for entry in fs::read_dir(saves_dir).unwrap().flatten() {
        let path = entry.path();
        if path.is_dir() {
            let manifest_path = path.join("manifest.yaml");
            if manifest_path.exists() {
                if let Ok(manifest_str) = fs::read_to_string(&manifest_path) {
                    if let Ok(manifest) = serde_yaml::from_str::<SaveBundleManifest>(&manifest_str) {
                        list.push(manifest);
                    }
                }
            }
        }
    }
    Ok(list)
}

pub fn delete_save_bundle(name: &str) -> Result<()> {
    let saves_dir = user_saves_dir().ok_or(MaccError::HomeDirNotFound)?;
    let target_save_dir = saves_dir.join(name);
    if !target_save_dir.exists() {
        return Err(MaccError::Validation(format!("MACC-RESTORE-2000: Save not found: {}", name)));
    }
    fs::remove_dir_all(target_save_dir).map_err(|e| MaccError::Io {
        path: name.to_string(),
        action: "delete save bundle".into(),
        source: e,
    })
}

pub fn detect_matching_saves(paths: &ProjectPaths) -> Result<Vec<(SaveBundleManifest, MatchStrength)>> {
    let current_repo = get_repository_identity(&paths.root);
    let list = list_save_bundles()?;
    let mut matches = Vec::new();
    for m in list {
        let strength = compute_match_strength(&current_repo, &m.repository);
        if strength != MatchStrength::None {
            matches.push((m, strength));
        }
    }
    matches.sort_by(|a, b| b.1.cmp(&a.1)); // Sort by match strength descending
    Ok(matches)
}

pub fn is_macc_state_unsaved(paths: &ProjectPaths) -> Result<bool> {
    if !paths.config_path.exists() {
        return Ok(false);
    }
    let saves = list_save_bundles()?;
    if saves.is_empty() {
        return Ok(true);
    }

    let config_sha = compute_file_sha256(&paths.config_path).ok();
    let sessions_path = paths.macc_dir.join("state").join("tool-sessions.json");
    let sessions_sha = if sessions_path.exists() {
        compute_file_sha256(&sessions_path).ok()
    } else {
        None
    };

    for m in &saves {
        let config_matches = match (&config_sha, &m.hashes.config) {
            (Some(c), Some(m_c)) => c == m_c,
            (None, None) => true,
            _ => false,
        };
        let sessions_matches = match (&sessions_sha, &m.hashes.coordinator_sessions) {
            (Some(s), Some(m_s)) => s == m_s,
            (None, None) => true,
            _ => false,
        };
        if config_matches && sessions_matches {
            return Ok(false); // Found a match, so it's not unsaved
        }
    }

    Ok(true)
}
