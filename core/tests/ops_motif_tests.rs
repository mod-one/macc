use macc_core::ops_motif::{
    calculate_trust_summary, get_setting_descriptors, generate_lock_manifest,
    verify_lock_manifest, TrustState, SettingCategory, apply_preset_to_config,
    apply_preset_to_env_cfg
};
use macc_core::coordinator::types::CoordinatorEnvConfig;
use macc_core::ProjectPaths;
use macc_core::config::CanonicalConfig;
use tempfile::tempdir;
use std::fs;

#[test]
fn test_setting_descriptors_exist_and_categorized() {
    let descriptors = get_setting_descriptors();
    assert!(!descriptors.is_empty());
    
    // Check categorization presence
    let has_basic = descriptors.iter().any(|d| d.category == SettingCategory::Basic);
    let has_advanced = descriptors.iter().any(|d| d.category == SettingCategory::Advanced);
    let has_admin = descriptors.iter().any(|d| d.category == SettingCategory::Admin);
    
    assert!(has_basic);
    assert!(has_advanced);
    assert!(has_admin);
}

#[test]
fn test_trust_summary_computation() {
    let temp_dir = tempdir().unwrap();
    let paths = ProjectPaths::from_root(temp_dir.path());
    
    // Create necessary folders
    fs::create_dir_all(paths.macc_dir.join("backups")).unwrap();
    
    let mut config = CanonicalConfig::default();
    config.settings.offline = true; // Local only
    
    let trust = calculate_trust_summary(&paths, &config);
    assert_eq!(trust.state, TrustState::Trusted);
    assert!(trust.local_only);
    assert!(trust.backups_ready);
}

#[test]
fn test_lock_manifest_generation_and_drift_detection() {
    let temp_dir = tempdir().unwrap();
    let paths = ProjectPaths::from_root(temp_dir.path());
    
    fs::create_dir_all(&paths.macc_dir).unwrap();
    fs::write(&paths.config_path, "settings:\n  offline: true\ntools:\n  enabled: [\"gemini\"]\n").unwrap();
    
    let config = macc_core::config::load_canonical_config(&paths.config_path).unwrap();
    let lock = generate_lock_manifest(&paths, &config).unwrap();
    
    assert_eq!(lock.lock_version, 1);
    assert_eq!(lock.preset, "balanced");
    
    // Verify lock file integrity
    let report = verify_lock_manifest(&paths, &lock).unwrap();
    assert!(report.matches);
    assert!(report.drift.is_empty());
    
    // Introduce drift by changing config
    fs::write(&paths.config_path, "settings:\n  offline: false\ntools:\n  enabled: [\"gemini\"]\n").unwrap();
    let report_drifted = verify_lock_manifest(&paths, &lock).unwrap();
    assert!(!report_drifted.matches);
    assert!(!report_drifted.drift.is_empty());
}

#[test]
fn test_presets_application() {
    let mut config = CanonicalConfig::default();
    apply_preset_to_config(&mut config, "conservative").unwrap();
    let coord_conservative = config.automation.coordinator.as_ref().unwrap();
    assert_eq!(coord_conservative.max_parallel, Some(1));
    assert_eq!(coord_conservative.rate_limit_fallback_enabled, Some(false));
    
    apply_preset_to_config(&mut config, "throughput").unwrap();
    let coord_throughput = config.automation.coordinator.as_ref().unwrap();
    assert_eq!(coord_throughput.max_parallel, Some(6));
    assert_eq!(coord_throughput.rate_limit_fallback_enabled, Some(true));
}

#[test]
fn test_preset_to_env_cfg() {
    let mut env_cfg = CoordinatorEnvConfig::default();
    apply_preset_to_env_cfg(&mut env_cfg, "balanced").unwrap();
    assert_eq!(env_cfg.max_parallel, Some(3));
    assert_eq!(env_cfg.rate_limit_fallback_enabled, Some(true));
    assert_eq!(env_cfg.merge_ai_fix, Some(true));
}
