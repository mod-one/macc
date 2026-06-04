pub mod bundle;
pub mod classifier;
pub mod logs;
pub mod manifest;
pub mod repository_identity;
pub mod restore;
pub mod scanner;

use crate::{MaccError, ProjectPaths, Result};
use std::fs;
use std::path::PathBuf;

pub use bundle::{compute_file_sha256, compute_manifest_payload_hash, create_save_bundle};
pub use manifest::{
    MatchStrength, SaveBundleManifest, SaveExcludes, SaveHashes, SaveIncludes, SavePaths,
    SaveSecurity, SecretScanMetadata,
};
pub use repository_identity::{
    compute_match_strength, get_repository_identity, RepositoryIdentity,
};
pub use restore::restore_save_bundle;
pub use scanner::redact_secrets_in_text;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SecretHandlingChoice {
    Abort,
    Redact,
    Exclude,
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
    pub handle_secrets: Option<SecretHandlingChoice>,
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
        let file_name = entry.file_name();
        let name_str = file_name.to_string_lossy();
        if name_str.starts_with('.') {
            continue;
        }
        let path = entry.path();
        if path.is_dir() {
            let manifest_path = path.join("manifest.yaml");
            if manifest_path.exists() {
                if let Ok(manifest_str) = fs::read_to_string(&manifest_path) {
                    if let Ok(manifest) = serde_yaml::from_str::<SaveBundleManifest>(&manifest_str)
                    {
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
        return Err(MaccError::Validation(format!(
            "MACC-RESTORE-2000: Save not found: {}",
            name
        )));
    }
    fs::remove_dir_all(target_save_dir).map_err(|e| MaccError::Io {
        path: name.to_string(),
        action: "delete save bundle".into(),
        source: e,
    })
}

pub fn detect_matching_saves(
    paths: &ProjectPaths,
) -> Result<Vec<(SaveBundleManifest, MatchStrength)>> {
    let current_repo = get_repository_identity(&paths.root);
    let list = list_save_bundles()?;
    let mut matches = Vec::new();
    for m in list {
        let strength = compute_match_strength(&current_repo, &m.repository);
        if strength != MatchStrength::None {
            matches.push((m, strength));
        }
    }
    matches.sort_by(|a, b| {
        b.1.cmp(&a.1)
            .then_with(|| b.0.created_at.cmp(&a.0.created_at))
    });
    Ok(matches)
}

pub fn is_macc_state_unsaved(paths: &ProjectPaths) -> Result<bool> {
    if !paths.config_path.exists() {
        return Ok(false);
    }
    let saves = detect_matching_saves(paths)?;
    let saves: Vec<_> = saves
        .into_iter()
        .filter(|(_, strength)| *strength >= MatchStrength::Medium)
        .collect();
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

    for (m, _) in &saves {
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct UnsavedStateReport {
    pub config_changed: bool,
    pub sessions_changed: bool,
}

pub fn get_unsaved_state_report(paths: &ProjectPaths) -> Result<UnsavedStateReport> {
    let mut report = UnsavedStateReport {
        config_changed: false,
        sessions_changed: false,
    };
    if !paths.config_path.exists() {
        return Ok(report);
    }

    let config_sha = compute_file_sha256(&paths.config_path).ok();
    let sessions_path = paths.macc_dir.join("state").join("tool-sessions.json");
    let sessions_sha = if sessions_path.exists() {
        compute_file_sha256(&sessions_path).ok()
    } else {
        None
    };

    let matches = detect_matching_saves(paths)?;
    let matches: Vec<_> = matches
        .into_iter()
        .filter(|(_, strength)| *strength >= MatchStrength::Medium)
        .collect();
    if matches.is_empty() {
        report.config_changed = true;
        if sessions_path.exists() {
            report.sessions_changed = true;
        }
        return Ok(report);
    }

    let best_match = &matches[0].0;
    let config_matches = match (&config_sha, &best_match.hashes.config) {
        (Some(c), Some(m_c)) => c == m_c,
        (None, None) => true,
        _ => false,
    };
    let sessions_matches = match (&sessions_sha, &best_match.hashes.coordinator_sessions) {
        (Some(s), Some(m_s)) => s == m_s,
        (None, None) => true,
        _ => false,
    };

    report.config_changed = !config_matches;
    report.sessions_changed = !sessions_matches;

    Ok(report)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::tempdir;

    #[test]
    fn test_list_save_bundles_skips_dot_directories() {
        let temp = tempdir().unwrap();
        let old_home = std::env::var_os("HOME");
        std::env::set_var("HOME", temp.path());

        let saves_dir = temp.path().join(".macc").join("saves");
        fs::create_dir_all(&saves_dir).unwrap();

        // 1. Regular save bundle
        let bundle_dir = saves_dir.join("my-save");
        fs::create_dir_all(&bundle_dir).unwrap();
        let manifest_content = r#"
version: 1
kind: macc.save_bundle
name: my-save
created_at: "2026-01-01T00:00:00Z"
updated_at: "2026-01-01T00:00:00Z"
macc_version: "0.1.0"
repository:
  root_name: "test-repo"
  root_path_hash: "hash"
  git_remote_url_hash: "hash"
  git_default_branch: "main"
  git_current_branch: "main"
  git_head_sha: "head"
  identity_strength: "strong"
includes:
  config: true
  coordinator_sessions: false
  catalogs: false
  logs: false
  prd: false
  automation_state: false
excludes:
  worktrees: true
  cache: true
  generated_files: true
  secrets: true
paths: {}
hashes:
  manifest_payload: "hash"
security:
  secret_scan:
    performed: false
    findings: 0
    redacted_logs: false
"#;
        fs::write(bundle_dir.join("manifest.yaml"), manifest_content).unwrap();

        // 2. Temp / dot directory (should be skipped)
        let dot_dir = saves_dir.join(".tmp");
        fs::create_dir_all(&dot_dir).unwrap();
        fs::write(dot_dir.join("manifest.yaml"), manifest_content).unwrap();

        let bundles = list_save_bundles().unwrap();

        // Restore environment
        if let Some(h) = old_home {
            std::env::set_var("HOME", h);
        } else {
            std::env::remove_var("HOME");
        }

        assert_eq!(bundles.len(), 1);
        assert_eq!(bundles[0].name, "my-save");
    }

    #[test]
    fn test_is_macc_state_unsaved_filters_weak_matches() {
        let temp = tempdir().unwrap();
        let old_home = std::env::var_os("HOME");
        std::env::set_var("HOME", temp.path());

        let project_root = temp.path().join("my-project");
        fs::create_dir_all(&project_root).unwrap();
        let paths = ProjectPaths::from_root(&project_root);
        fs::create_dir_all(paths.config_path.parent().unwrap()).unwrap();
        fs::write(&paths.config_path, "local config").unwrap();
        let saves_dir = temp.path().join(".macc").join("saves");
        fs::create_dir_all(&saves_dir).unwrap();

        // Create a bundle that has the same root name ("my-project") but a completely different root_path_hash
        // and git_remote_url_hash, making it a Weak match.
        let bundle_dir = saves_dir.join("weak-match-save");
        fs::create_dir_all(&bundle_dir).unwrap();

        // Calculate hash of config file
        let config_sha = compute_file_sha256(&paths.config_path).unwrap();

        let manifest_content = format!(
            r#"
version: 1
kind: macc.save_bundle
name: weak-match-save
created_at: "2026-01-01T00:00:00Z"
updated_at: "2026-01-01T00:00:00Z"
macc_version: "0.1.0"
repository:
  root_name: "my-project"
  root_path_hash: "different-hash"
  git_remote_url_hash: "different-remote-hash"
  git_default_branch: "main"
  git_current_branch: "main"
  git_head_sha: "different-head"
  identity_strength: "strong"
includes:
  config: true
  coordinator_sessions: false
  catalogs: false
  logs: false
  prd: false
  automation_state: false
excludes:
  worktrees: true
  cache: true
  generated_files: true
  secrets: true
paths:
  config: "config/macc.yaml"
hashes:
  config: "{}"
  manifest_payload: "hash"
security:
  secret_scan:
    performed: false
    findings: 0
    redacted_logs: false
"#,
            config_sha
        );
        fs::write(bundle_dir.join("manifest.yaml"), manifest_content).unwrap();

        // Since the match is Weak, is_macc_state_unsaved should return true (unsaved)
        // because it ignores the Weak match, despite the config hash matching exactly.
        let unsaved = is_macc_state_unsaved(&paths).unwrap();
        let report = get_unsaved_state_report(&paths).unwrap();

        // Restore environment
        if let Some(h) = old_home {
            std::env::set_var("HOME", h);
        } else {
            std::env::remove_var("HOME");
        }

        assert!(unsaved);
        assert!(report.config_changed);
    }
}
