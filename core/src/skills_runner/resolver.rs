use super::model::*;
use std::path::Path;

pub struct SkillResolver;

impl SkillResolver {
    pub fn resolve(id: &str, macc_dir: &Path) -> Option<SkillDefinition> {
        // 1. Look in .macc/skills/<id>.yaml or .macc/skills/<id>/skill.yaml
        let skill_yaml = macc_dir.join("skills").join(format!("{}.yaml", id));
        if skill_yaml.exists() {
            if let Ok(content) = std::fs::read_to_string(&skill_yaml) {
                if let Ok(def) = serde_yaml::from_str::<SkillDefinition>(&content) {
                    return Some(def);
                }
            }
        }

        let skill_dir_yaml = macc_dir.join("skills").join(id).join("skill.yaml");
        if skill_dir_yaml.exists() {
            if let Ok(content) = std::fs::read_to_string(&skill_dir_yaml) {
                if let Ok(def) = serde_yaml::from_str::<SkillDefinition>(&content) {
                    return Some(def);
                }
            }
        }

        // 2. Built-in skill stubs (validate, implement, security-check)
        Self::builtin(id)
    }

    pub fn list(macc_dir: &Path) -> Vec<SkillDefinition> {
        let mut skills = Vec::new();

        let skills_dir = macc_dir.join("skills");
        if let Ok(entries) = std::fs::read_dir(&skills_dir) {
            for entry in entries.flatten() {
                let path = entry.path();
                if path.extension().map(|e| e == "yaml").unwrap_or(false) {
                    if let Ok(content) = std::fs::read_to_string(&path) {
                        if let Ok(def) = serde_yaml::from_str::<SkillDefinition>(&content) {
                            skills.push(def);
                        }
                    }
                }
                if path.is_dir() {
                    let inner = path.join("skill.yaml");
                    if inner.exists() {
                        if let Ok(content) = std::fs::read_to_string(&inner) {
                            if let Ok(def) = serde_yaml::from_str::<SkillDefinition>(&content) {
                                skills.push(def);
                            }
                        }
                    }
                }
            }
        }

        // Always include built-in stubs that are not already present.
        let present_ids: std::collections::HashSet<_> =
            skills.iter().map(|s| s.id.clone()).collect();
        for builtin_id in &["validate", "implement", "security-check"] {
            if !present_ids.contains(*builtin_id) {
                if let Some(def) = Self::builtin(builtin_id) {
                    skills.push(def);
                }
            }
        }

        skills.sort_by(|a, b| a.id.cmp(&b.id));
        skills
    }

    fn builtin(id: &str) -> Option<SkillDefinition> {
        match id {
            "validate" => Some(SkillDefinition {
                id: "validate".to_string(),
                title: "Validate project".to_string(),
                kind: SkillKind::LocalCommand,
                risk: SkillRisk::Safe,
                description: "Run lint, build, and tests.".to_string(),
                steps: vec![
                    SkillStep {
                        run: Some("pnpm lint".to_string()),
                        prompt: None,
                    },
                    SkillStep {
                        run: Some("pnpm build".to_string()),
                        prompt: None,
                    },
                    SkillStep {
                        run: Some("pnpm test".to_string()),
                        prompt: None,
                    },
                ],
                targets: Default::default(),
            }),
            "implement" => Some(SkillDefinition {
                id: "implement".to_string(),
                title: "Implement task".to_string(),
                kind: SkillKind::Prompt,
                risk: SkillRisk::Caution,
                description: "Implement the next pending task.".to_string(),
                steps: vec![SkillStep {
                    run: None,
                    prompt: Some(
                        "Implement the task described in the current context.".to_string(),
                    ),
                }],
                targets: Default::default(),
            }),
            "security-check" => Some(SkillDefinition {
                id: "security-check".to_string(),
                title: "Security review".to_string(),
                kind: SkillKind::Prompt,
                risk: SkillRisk::Safe,
                description: "Perform a security review of changed files.".to_string(),
                steps: vec![SkillStep {
                    run: None,
                    prompt: Some(
                        "Review the changed files for security vulnerabilities.".to_string(),
                    ),
                }],
                targets: Default::default(),
            }),
            _ => None,
        }
    }
}
