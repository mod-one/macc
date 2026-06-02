use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashSet;
use std::path::Path;

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ValidationResult {
    pub ok: bool,
    pub warnings: Vec<String>,
    pub errors: Vec<String>,
}

impl ValidationResult {
    pub fn pass() -> Self {
        Self { ok: true, ..Default::default() }
    }

    pub fn add_warning(&mut self, w: impl Into<String>) {
        self.warnings.push(w.into());
    }

    pub fn add_error(&mut self, e: impl Into<String>) {
        self.ok = false;
        self.errors.push(e.into());
    }
}

/// Lightweight mandatory validation on a generated PRD file.
pub fn validate_prd_file(file_path: &Path, target_dir: Option<&Path>) -> ValidationResult {
    let mut result = ValidationResult::pass();

    if !file_path.exists() {
        result.add_error(format!(
            "PRD-GEN-OUTPUT-MISSING: '{}' was not generated",
            file_path.display()
        ));
        return result;
    }

    if let Some(dir) = target_dir {
        if !file_path.starts_with(dir) {
            result.add_error(format!(
                "PRD-GEN-OUTPUT-OUTSIDE: '{}' is outside the target directory '{}'",
                file_path.display(),
                dir.display()
            ));
            return result;
        }
    }

    let raw = match std::fs::read_to_string(file_path) {
        Ok(s) => s,
        Err(e) => {
            result.add_error(format!(
                "PRD-GEN-OUTPUT-MISSING: cannot read '{}': {}",
                file_path.display(),
                e
            ));
            return result;
        }
    };

    let value: Value = match serde_json::from_str(&raw) {
        Ok(v) => v,
        Err(e) => {
            result.add_error(format!(
                "PRD-GEN-VALIDATION-FAILED: '{}' is not valid JSON: {}",
                file_path.display(),
                e
            ));
            return result;
        }
    };

    let obj = match value.as_object() {
        Some(o) => o,
        None => {
            result.add_error("PRD-GEN-VALIDATION-FAILED: PRD root is not a JSON object");
            return result;
        }
    };

    for field in &["lot", "tasks"] {
        if !obj.contains_key(*field) {
            result.add_warning(format!("Missing recommended top-level field: '{}'", field));
        }
    }

    if let Some(tasks) = obj.get("tasks").and_then(|t| t.as_array()) {
        let mut seen_ids: HashSet<String> = HashSet::new();
        let mut all_ids: HashSet<String> = HashSet::new();
        for task in tasks {
            if let Some(id) = task.get("id").and_then(|v| v.as_str()) {
                all_ids.insert(id.to_string());
                if !seen_ids.insert(id.to_string()) {
                    result.add_error(format!("Duplicate task ID: '{}'", id));
                }
            }
        }
        for task in tasks {
            if let Some(deps) = task.get("depends_on").and_then(|d| d.as_array()) {
                for dep in deps {
                    if let Some(dep_id) = dep.as_str() {
                        if !all_ids.contains(dep_id) {
                            result.add_warning(format!(
                                "Task dependency '{}' references unknown task ID",
                                dep_id
                            ));
                        }
                    }
                }
            }
        }
        for task in tasks {
            check_routing_hints_neutral(task, &mut result);
        }
    }

    result
}

fn check_routing_hints_neutral(task: &Value, result: &mut ValidationResult) {
    let Some(hints) = task.get("routing_hints").and_then(|h| h.as_object()) else {
        return;
    };
    for (key, val) in hints {
        if matches!(key.as_str(), "model" | "provider_model") || key.ends_with("_model_name") {
            result.add_warning(format!(
                "routing_hints contains provider-specific key '{}'; only neutral hints are allowed.",
                key
            ));
        }
        if let Some(s) = val.as_str() {
            if looks_like_provider_model(s) {
                result.add_warning(format!(
                    "routing_hints value '{}' for '{}' appears to be a provider-specific model name.",
                    s, key
                ));
            }
        }
    }
}

fn looks_like_provider_model(s: &str) -> bool {
    let lower = s.to_lowercase();
    lower.contains("claude-") || lower.contains("gpt-") || lower.contains("gemini-")
        || lower.contains("codex-") || lower.contains("opus") || lower.contains("sonnet")
        || lower.contains("haiku") || lower.contains("mistral")
}
