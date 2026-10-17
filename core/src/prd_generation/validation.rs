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
        Self {
            ok: true,
            ..Default::default()
        }
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
            if let Some(deps) = task
                .get("dependencies")
                .or_else(|| task.get("depends_on"))
                .and_then(|d| d.as_array())
            {
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
            check_scheduler_contract(task, &mut result);
        }
    }

    result
}

fn check_scheduler_contract(task: &Value, result: &mut ValidationResult) {
    let task_id = task
        .get("id")
        .and_then(Value::as_str)
        .unwrap_or("<unknown>");
    if let Some(block) = task.get("blocked_on_external") {
        let valid = block.as_object().is_some_and(|object| {
            ["reason", "clears_when"].iter().all(|field| {
                object
                    .get(*field)
                    .and_then(Value::as_str)
                    .is_some_and(|value| !value.trim().is_empty())
            })
        });
        if !valid {
            result.add_error(format!(
                "Task '{task_id}' has invalid blocked_on_external; non-empty reason and clears_when are required"
            ));
        }
    }
    if let Some(gate) = task.get("gate") {
        let valid = gate.as_object().is_some_and(|object| {
            object
                .get("required_verdict")
                .is_none_or(|value| value.as_str() == Some("accepted"))
        });
        if !valid {
            result.add_error(format!(
                "Task '{task_id}' has invalid gate.required_verdict; only 'accepted' is currently supported"
            ));
        }
        check_human_approval_gate(task_id, task, gate, result);
    }
    let notes = task
        .get("notes")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_ascii_lowercase();
    if task.get("blocked_on_external").is_none()
        && (notes.contains("blocked on external") || notes.contains("do not retry"))
    {
        result.add_warning(format!(
            "Task '{task_id}' describes a scheduler block only in prose; add blocked_on_external"
        ));
    }
    let category = task
        .get("category")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_ascii_lowercase();
    if task.get("gate").is_none() && matches!(category.as_str(), "gate" | "acceptance") {
        result.add_warning(format!(
            "Task '{task_id}' has category '{category}' but no gate declaration"
        ));
    }
}

/// A `human_approval` gate is scheduler-enforced: it must parse, pass the
/// same structural rules the coordinator applies at sync time, and carry no
/// executable work (a performer never runs it).
fn check_human_approval_gate(
    task_id: &str,
    task: &Value,
    gate: &Value,
    result: &mut ValidationResult,
) {
    if gate.get("kind").and_then(Value::as_str) != Some("human_approval") {
        return;
    }
    let parsed: crate::coordinator::model::TaskGate = match serde_json::from_value(gate.clone()) {
        Ok(parsed) => parsed,
        Err(err) => {
            result.add_error(format!(
                "Task '{task_id}' has an unreadable human_approval gate: {err}"
            ));
            return;
        }
    };
    let dependencies: Vec<String> = task
        .get("dependencies")
        .and_then(Value::as_array)
        .map(|deps| {
            deps.iter()
                .filter_map(Value::as_str)
                .map(str::to_string)
                .collect()
        })
        .unwrap_or_default();
    for problem in parsed.validate_human_approval(task_id, &dependencies) {
        result.add_error(format!("Invalid human approval gate: {problem}"));
    }
    let writes = task
        .pointer("/change_scope/allowed_paths")
        .and_then(Value::as_array)
        .is_some_and(|paths| !paths.is_empty());
    if writes {
        result.add_error(format!(
            "Task '{task_id}' is a human_approval gate but declares change_scope.allowed_paths; a gate is never executed by a performer — move the work to the subject task"
        ));
    }
    if parsed.governance_ref.is_none() {
        result.add_warning(format!(
            "Task '{task_id}' does not cite gate.governance_ref; approver roles should be derived from the specification governance, not assumed"
        ));
    }
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
    lower.contains("claude-") // macc:allow-tool-name
        || lower.contains("gpt-")
        || lower.contains("gemini-") // macc:allow-tool-name
        || lower.contains("codex-") // macc:allow-tool-name
        || lower.contains("opus")
        || lower.contains("sonnet")
        || lower.contains("haiku")
        || lower.contains("mistral") // macc:allow-tool-name
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn validate(value: Value) -> ValidationResult {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos();
        let root = std::env::temp_dir().join(format!("macc_prd_validation_{nonce}"));
        fs::create_dir_all(&root).unwrap();
        let path = root.join("prd.json");
        fs::write(&path, serde_json::to_vec(&value).unwrap()).unwrap();
        let result = validate_prd_file(&path, Some(&root));
        let _ = fs::remove_dir_all(root);
        result
    }

    #[test]
    fn warns_when_external_block_exists_only_in_prose() {
        let result = validate(serde_json::json!({
            "lot": "L1",
            "tasks": [{
                "id": "TASK-1",
                "notes": "BLOCKED ON EXTERNAL EVIDENCE - do not retry."
            }]
        }));

        assert!(result.ok);
        assert!(result
            .warnings
            .iter()
            .any(|warning| warning.contains("add blocked_on_external")));
    }

    #[test]
    fn accepts_structured_gate_and_external_block() {
        let result = validate(serde_json::json!({
            "lot": "L1",
            "tasks": [{
                "id": "GATE-1",
                "category": "acceptance",
                "gate": {},
                "blocked_on_external": {
                    "reason": "Observation window has not run",
                    "clears_when": "GAP-WP4-017 is accepted",
                    "tracking_id": "GAP-WP4-017"
                }
            }]
        }));

        assert!(result.ok, "unexpected errors: {:?}", result.errors);
        assert!(result.warnings.is_empty(), "{:?}", result.warnings);
    }

    #[test]
    fn rejects_incomplete_scheduler_contracts() {
        let result = validate(serde_json::json!({
            "lot": "L1",
            "tasks": [{
                "id": "GATE-1",
                "gate": { "required_verdict": "maybe" },
                "blocked_on_external": { "reason": "Waiting" }
            }]
        }));

        assert!(!result.ok);
        assert_eq!(result.errors.len(), 2);
    }
}

#[cfg(test)]
mod human_gate_validation_tests {
    use super::*;
    use serde_json::json;

    fn run(task: Value) -> ValidationResult {
        let mut result = ValidationResult::pass();
        check_scheduler_contract(&task, &mut result);
        result
    }

    #[test]
    fn a_valid_gate_passes_with_governance_cited() {
        let r = run(json!({"id":"SEC-APP-003","dependencies":["SEC-ADR-003"],
            "gate":{"kind":"human_approval","subject_task":"SEC-ADR-003",
                    "required_approvers":[{"role":"PRODUCT_OWNER"}],
                    "governance_ref":"docs/16-decisions.md"}}));
        assert!(r.ok, "{:?}", r.errors);
        assert!(r.warnings.is_empty(), "{:?}", r.warnings);
    }

    #[test]
    fn structural_defects_and_executable_scope_are_errors() {
        let r = run(json!({"id":"SEC-APP-003","dependencies":[],
            "change_scope":{"allowed_paths":["docs/**"]},
            "gate":{"kind":"human_approval","subject_task":"SEC-ADR-003","required_approvers":[]}}));
        assert!(!r.ok);
        let text = r.errors.join("\n");
        assert!(
            text.contains("must also be listed in dependencies"),
            "{text}"
        );
        assert!(text.contains("at least one role"), "{text}");
        assert!(text.contains("allowed_paths"), "{text}");
        assert!(r.warnings.join(" ").contains("governance_ref"));
    }
}
