use crate::config::CanonicalConfig;
use crate::find_user_home;
use crate::{MaccError, Result};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

/// Envelope wrapping a full CanonicalConfig with metadata.
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct ProfileWrapper {
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    pub created_at: String,
    /// Absolute path of the project this profile was saved from.
    /// Set by `save_auto`; not set for manually-saved global profiles.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub project_root: Option<String>,
    pub config: CanonicalConfig,
}

/// Sections that can be selectively saved/restored via `--only`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProfileSection {
    Tools,
    Standards,
    Selections,
    Automation,
    Settings,
    McpTemplates,
}

impl ProfileSection {
    pub fn parse(s: &str) -> Option<Self> {
        match s.to_ascii_lowercase().as_str() {
            "tools" => Some(Self::Tools),
            "standards" => Some(Self::Standards),
            "selections" => Some(Self::Selections),
            "automation" => Some(Self::Automation),
            "settings" => Some(Self::Settings),
            "mcp_templates" | "mcp-templates" | "mcp" => Some(Self::McpTemplates),
            _ => None,
        }
    }

    pub fn all_names() -> &'static str {
        "tools, standards, selections, automation, settings, mcp_templates"
    }
}

pub fn parse_sections(csv: &str) -> Result<Vec<ProfileSection>> {
    let mut out = Vec::new();
    for tok in csv.split(',') {
        let tok = tok.trim();
        if tok.is_empty() {
            continue;
        }
        match ProfileSection::parse(tok) {
            Some(s) => out.push(s),
            None => {
                return Err(MaccError::Validation(format!(
                    "Unknown profile section '{}'. Valid sections: {}",
                    tok,
                    ProfileSection::all_names()
                )));
            }
        }
    }
    if out.is_empty() {
        return Err(MaccError::Validation("No valid sections specified".into()));
    }
    Ok(out)
}

/// Manages user-level profiles stored under `~/.macc/profiles/`.
pub struct ProfileManager {
    profiles_dir: PathBuf,
}

impl ProfileManager {
    pub fn new() -> Result<Self> {
        let home = find_user_home().ok_or(MaccError::HomeDirNotFound)?;
        let profiles_dir = home.join(".macc").join("profiles");
        Ok(Self { profiles_dir })
    }

    #[cfg(test)]
    pub fn with_dir(dir: PathBuf) -> Self {
        Self { profiles_dir: dir }
    }

    pub fn profile_path(&self, name: &str) -> PathBuf {
        self.profiles_dir.join(format!("{}.yaml", name))
    }

    /// Save a profile. If `sections` is Some, only the listed sections are
    /// saved (others get defaults). Paths are sanitized for portability.
    pub fn save(
        &self,
        name: &str,
        description: Option<&str>,
        config: &CanonicalConfig,
        sections: Option<&[ProfileSection]>,
    ) -> Result<()> {
        validate_profile_name(name)?;

        let config_to_save = match sections {
            Some(secs) => extract_sections(config, secs),
            None => config.clone(),
        };

        let sanitized = sanitize_paths(config_to_save);

        let wrapper = ProfileWrapper {
            name: name.to_string(),
            description: description.map(|s| s.to_string()),
            created_at: chrono::Utc::now().to_rfc3339(),
            project_root: None,
            config: sanitized,
        };

        let yaml = serde_yaml::to_string(&wrapper).map_err(|e| MaccError::Config {
            path: name.to_string(),
            source: e,
        })?;

        std::fs::create_dir_all(&self.profiles_dir).map_err(|e| MaccError::Io {
            path: self.profiles_dir.to_string_lossy().into(),
            action: "create profiles directory".into(),
            source: e,
        })?;

        let path = self.profile_path(name);
        std::fs::write(&path, yaml.as_bytes()).map_err(|e| MaccError::Io {
            path: path.to_string_lossy().into(),
            action: "write profile".into(),
            source: e,
        })?;

        Ok(())
    }

    /// Load a profile by name.
    pub fn load(&self, name: &str) -> Result<ProfileWrapper> {
        let path = self.profile_path(name);
        let content = std::fs::read_to_string(&path).map_err(|e| MaccError::Io {
            path: path.to_string_lossy().into(),
            action: "read profile".into(),
            source: e,
        })?;
        let wrapper: ProfileWrapper =
            serde_yaml::from_str(&content).map_err(|e| MaccError::Config {
                path: path.to_string_lossy().into(),
                source: e,
            })?;
        Ok(wrapper)
    }

    /// List all saved profile names, sorted by name.
    pub fn list(&self) -> Result<Vec<ProfileInfo>> {
        if !self.profiles_dir.exists() {
            return Ok(Vec::new());
        }
        let mut profiles = Vec::new();
        let entries = std::fs::read_dir(&self.profiles_dir).map_err(|e| MaccError::Io {
            path: self.profiles_dir.to_string_lossy().into(),
            action: "list profiles".into(),
            source: e,
        })?;
        for entry in entries {
            let entry = entry.map_err(|e| MaccError::Io {
                path: self.profiles_dir.to_string_lossy().into(),
                action: "read profile entry".into(),
                source: e,
            })?;
            let path = entry.path();
            if path.extension().and_then(|e| e.to_str()) == Some("yaml") {
                if let Some(stem) = path.file_stem().and_then(|s| s.to_str()) {
                    let name = stem.to_string();
                    let (description, created_at, project_root) =
                        match std::fs::read_to_string(&path) {
                            Ok(content) => match serde_yaml::from_str::<ProfileWrapper>(&content) {
                                Ok(w) => (w.description, Some(w.created_at), w.project_root),
                                Err(_) => (None, None, None),
                            },
                            Err(_) => (None, None, None),
                        };
                    profiles.push(ProfileInfo {
                        name,
                        description,
                        created_at,
                        project_root,
                    });
                }
            }
        }
        profiles.sort_by(|a, b| a.name.cmp(&b.name));
        Ok(profiles)
    }

    /// List profiles that were saved for a specific project root.
    /// Returns entries sorted by creation date, newest first.
    pub fn list_for_project(&self, project_root: &Path) -> Result<Vec<ProfileInfo>> {
        let root_str = project_root.to_string_lossy().to_string();
        let mut profiles: Vec<ProfileInfo> = self
            .list()?
            .into_iter()
            .filter(|p| p.project_root.as_deref() == Some(root_str.as_str()))
            .collect();
        // Newest first so the most relevant option appears first.
        profiles.sort_by(|a, b| {
            b.created_at
                .as_deref()
                .unwrap_or("")
                .cmp(a.created_at.as_deref().unwrap_or(""))
        });
        Ok(profiles)
    }

    /// Auto-save the current config as `<project-slug>-auto` linked to the given
    /// project root. Always overwrites the previous auto-save for this project so
    /// only one entry accumulates per project.
    pub fn save_auto(&self, project_root: &Path, config: &CanonicalConfig) -> Result<()> {
        let slug = profile_project_slug(project_root);
        let name = format!("{}-auto", slug);
        let now = chrono::Utc::now().to_rfc3339();
        let description = format!("Auto-saved after 'macc apply' on {}", &now[..10]);

        let sanitized = sanitize_paths(config.clone());
        let wrapper = ProfileWrapper {
            name: name.clone(),
            description: Some(description),
            created_at: now,
            project_root: Some(project_root.to_string_lossy().to_string()),
            config: sanitized,
        };

        let yaml = serde_yaml::to_string(&wrapper).map_err(|e| MaccError::Config {
            path: name.clone(),
            source: e,
        })?;
        std::fs::create_dir_all(&self.profiles_dir).map_err(|e| MaccError::Io {
            path: self.profiles_dir.to_string_lossy().into(),
            action: "create profiles directory for auto-save".into(),
            source: e,
        })?;
        let path = self.profile_path(&name);
        std::fs::write(&path, yaml.as_bytes()).map_err(|e| MaccError::Io {
            path: path.to_string_lossy().into(),
            action: "write auto-save profile".into(),
            source: e,
        })?;
        Ok(())
    }

    /// Delete a profile by name.
    pub fn delete(&self, name: &str) -> Result<()> {
        let path = self.profile_path(name);
        if !path.exists() {
            return Err(MaccError::Validation(format!(
                "Profile '{}' does not exist",
                name
            )));
        }
        std::fs::remove_file(&path).map_err(|e| MaccError::Io {
            path: path.to_string_lossy().into(),
            action: "delete profile".into(),
            source: e,
        })?;
        Ok(())
    }

    /// Restore (merge) a profile into a target config. If `sections` is Some,
    /// only the listed sections are restored.
    pub fn restore(
        &self,
        name: &str,
        target: &CanonicalConfig,
        sections: Option<&[ProfileSection]>,
    ) -> Result<CanonicalConfig> {
        let wrapper = self.load(name)?;
        Ok(merge_profile(target, &wrapper.config, sections))
    }
}

#[derive(Debug, Clone)]
pub struct ProfileInfo {
    pub name: String,
    pub description: Option<String>,
    pub created_at: Option<String>,
    pub project_root: Option<String>,
}

fn profile_project_slug(project_root: &Path) -> String {
    let name = project_root
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_else(|| "unknown".to_string());
    name.chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.') {
                c
            } else {
                '_'
            }
        })
        .collect()
}

fn validate_profile_name(name: &str) -> Result<()> {
    if name.is_empty() {
        return Err(MaccError::Validation("Profile name cannot be empty".into()));
    }
    if name.contains('/') || name.contains('\\') || name.contains("..") {
        return Err(MaccError::Validation(format!(
            "Profile name '{}' contains invalid characters",
            name
        )));
    }
    if !name
        .chars()
        .all(|c| c.is_alphanumeric() || c == '-' || c == '_' || c == '.')
    {
        return Err(MaccError::Validation(format!(
            "Profile name '{}' must only contain alphanumeric characters, hyphens, underscores, or dots",
            name
        )));
    }
    Ok(())
}

/// Build a config containing only the selected sections, with defaults for the rest.
fn extract_sections(source: &CanonicalConfig, sections: &[ProfileSection]) -> CanonicalConfig {
    let def = CanonicalConfig::default();
    CanonicalConfig {
        version: source.version.clone(),
        tools: if sections.contains(&ProfileSection::Tools) {
            source.tools.clone()
        } else {
            def.tools
        },
        standards: if sections.contains(&ProfileSection::Standards) {
            source.standards.clone()
        } else {
            def.standards
        },
        selections: if sections.contains(&ProfileSection::Selections) {
            source.selections.clone()
        } else {
            def.selections
        },
        automation: if sections.contains(&ProfileSection::Automation) {
            source.automation.clone()
        } else {
            def.automation
        },
        settings: if sections.contains(&ProfileSection::Settings) {
            source.settings.clone()
        } else {
            def.settings
        },
        process_ownership: def.process_ownership,
        mcp_templates: if sections.contains(&ProfileSection::McpTemplates) {
            source.mcp_templates.clone()
        } else {
            def.mcp_templates
        },
        skills: source.skills.clone(),
        context: source.context.clone(),
        prd_generation: source.prd_generation.clone(),
    }
}

/// Merge profile config into target. If sections is None, replace everything.
/// If sections is Some, only replace the listed sections.
fn merge_profile(
    target: &CanonicalConfig,
    profile: &CanonicalConfig,
    sections: Option<&[ProfileSection]>,
) -> CanonicalConfig {
    match sections {
        None => {
            // Full restore: use profile config but keep target's version
            let mut merged = profile.clone();
            merged.version = target.version.clone();
            merged
        }
        Some(secs) => {
            let mut merged = target.clone();
            for sec in secs {
                match sec {
                    ProfileSection::Tools => merged.tools = profile.tools.clone(),
                    ProfileSection::Standards => merged.standards = profile.standards.clone(),
                    ProfileSection::Selections => merged.selections = profile.selections.clone(),
                    ProfileSection::Automation => merged.automation = profile.automation.clone(),
                    ProfileSection::Settings => merged.settings = profile.settings.clone(),
                    ProfileSection::McpTemplates => {
                        merged.mcp_templates = profile.mcp_templates.clone()
                    }
                }
            }
            merged
        }
    }
}

/// Strip absolute paths from config fields to make profiles portable.
fn sanitize_paths(mut config: CanonicalConfig) -> CanonicalConfig {
    // standards.path: make relative if absolute
    if let Some(ref p) = config.standards.path {
        if Path::new(p).is_absolute() {
            // Keep only the filename or last component
            config.standards.path = Path::new(p)
                .file_name()
                .map(|f| f.to_string_lossy().to_string());
        }
    }
    // coordinator.prd_file: make relative if absolute
    if let Some(ref mut coord) = config.automation.coordinator {
        if let Some(ref p) = coord.prd_file {
            if Path::new(p).is_absolute() {
                coord.prd_file = Path::new(p)
                    .file_name()
                    .map(|f| f.to_string_lossy().to_string());
            }
        }
        if let Some(ref p) = coord.task_registry_file {
            if Path::new(p).is_absolute() {
                coord.task_registry_file = Path::new(p)
                    .file_name()
                    .map(|f| f.to_string_lossy().to_string());
            }
        }
    }
    config
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{SettingsConfig, StandardsConfig, ToolsConfig};

    fn temp_dir() -> PathBuf {
        let ts = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        std::env::temp_dir().join(format!("macc_profile_test_{}", ts))
    }

    fn sample_config() -> CanonicalConfig {
        CanonicalConfig {
            version: Some("v1".to_string()),
            tools: ToolsConfig {
                enabled: vec!["tool-a".to_string(), "tool-b".to_string()],
                ..Default::default()
            },
            standards: StandardsConfig {
                path: Some("standards.md".to_string()),
                ..Default::default()
            },
            selections: None,
            automation: Default::default(),
            settings: SettingsConfig {
                quiet: true,
                ..Default::default()
            },
            process_ownership: None,
            mcp_templates: Vec::new(),
            skills: None,
            context: None,
        }
    }

    #[test]
    fn test_save_load_roundtrip() {
        let dir = temp_dir();
        let mgr = ProfileManager::with_dir(dir.clone());
        let config = sample_config();

        mgr.save("test-profile", Some("A test"), &config, None)
            .unwrap();

        let loaded = mgr.load("test-profile").unwrap();
        assert_eq!(loaded.name, "test-profile");
        assert_eq!(loaded.description, Some("A test".to_string()));
        assert_eq!(loaded.config.tools.enabled, config.tools.enabled);
        assert_eq!(loaded.config.settings.quiet, true);

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn test_list_profiles() {
        let dir = temp_dir();
        let mgr = ProfileManager::with_dir(dir.clone());
        let config = sample_config();

        mgr.save("beta", None, &config, None).unwrap();
        mgr.save("alpha", Some("First"), &config, None).unwrap();

        let list = mgr.list().unwrap();
        assert_eq!(list.len(), 2);
        assert_eq!(list[0].name, "alpha");
        assert_eq!(list[1].name, "beta");

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn test_delete_profile() {
        let dir = temp_dir();
        let mgr = ProfileManager::with_dir(dir.clone());
        let config = sample_config();

        mgr.save("to-delete", None, &config, None).unwrap();
        assert!(mgr.load("to-delete").is_ok());

        mgr.delete("to-delete").unwrap();
        assert!(mgr.load("to-delete").is_err());

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn test_delete_nonexistent() {
        let dir = temp_dir();
        let mgr = ProfileManager::with_dir(dir.clone());
        let result = mgr.delete("nope");
        assert!(result.is_err());
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn test_selective_save() {
        let dir = temp_dir();
        let mgr = ProfileManager::with_dir(dir.clone());
        let config = sample_config();

        mgr.save(
            "partial",
            None,
            &config,
            Some(&[ProfileSection::Tools, ProfileSection::Settings]),
        )
        .unwrap();

        let loaded = mgr.load("partial").unwrap();
        assert_eq!(loaded.config.tools.enabled, config.tools.enabled);
        assert_eq!(loaded.config.settings.quiet, true);
        // Standards should be default (not saved)
        assert_eq!(loaded.config.standards.path, None);

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn test_selective_restore() {
        let dir = temp_dir();
        let mgr = ProfileManager::with_dir(dir.clone());

        let profile_config = sample_config();
        mgr.save("source", None, &profile_config, None).unwrap();

        let target = CanonicalConfig::default();
        let restored = mgr
            .restore("source", &target, Some(&[ProfileSection::Tools]))
            .unwrap();

        // Tools should come from profile
        assert_eq!(restored.tools.enabled, vec!["tool-a", "tool-b"]);
        // Settings should remain from target (default)
        assert_eq!(restored.settings.quiet, false);

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn test_full_restore() {
        let dir = temp_dir();
        let mgr = ProfileManager::with_dir(dir.clone());

        let profile_config = sample_config();
        mgr.save("full", None, &profile_config, None).unwrap();

        let target = CanonicalConfig::default();
        let restored = mgr.restore("full", &target, None).unwrap();

        assert_eq!(restored.tools.enabled, vec!["tool-a", "tool-b"]);
        assert_eq!(restored.settings.quiet, true);

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn test_sanitize_absolute_paths() {
        let mut config = sample_config();
        config.standards.path = Some("/home/user/project/standards.md".to_string());

        let sanitized = sanitize_paths(config);
        assert_eq!(sanitized.standards.path, Some("standards.md".to_string()));
    }

    #[test]
    fn test_invalid_profile_names() {
        assert!(validate_profile_name("").is_err());
        assert!(validate_profile_name("foo/bar").is_err());
        assert!(validate_profile_name("foo..bar").is_err());
        assert!(validate_profile_name("foo bar").is_err());
        assert!(validate_profile_name("valid-name_1.0").is_ok());
    }

    #[test]
    fn test_parse_sections() {
        let secs = parse_sections("tools,settings").unwrap();
        assert_eq!(secs.len(), 2);
        assert!(secs.contains(&ProfileSection::Tools));
        assert!(secs.contains(&ProfileSection::Settings));

        assert!(parse_sections("bogus").is_err());
        assert!(parse_sections("").is_err());
    }
}
