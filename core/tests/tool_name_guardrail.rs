use std::path::{Path, PathBuf};

fn collect_rs_files(dir: &Path, out: &mut Vec<PathBuf>) {
    let entries = match std::fs::read_dir(dir) {
        Ok(entries) => entries,
        Err(_) => return,
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            collect_rs_files(&path, out);
        } else if path.extension().and_then(|ext| ext.to_str()) == Some("rs") {
            out.push(path);
        }
    }
}

#[test]
fn guardrail_no_tool_names_in_core_cli_tui() {
    let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let repo_root = manifest_dir
        .parent()
        .expect("core crate should live one level below repo root");

    let targets = [
        repo_root.join("core/src"),
        repo_root.join("cli/src"),
        repo_root.join("tui/src"),
    ];
    let forbidden = load_forbidden_tokens(&repo_root);
    let mut violations = Vec::new();

    for target in targets {
        let mut files = Vec::new();
        collect_rs_files(&target, &mut files);

        for file in files {
            let Ok(contents) = std::fs::read_to_string(&file) else {
                continue;
            };
            let mut in_test_scope = false;
            let mut pending_test_scope = false;
            let mut test_scope_depth = 0usize;

            for (line_no, line) in contents.lines().enumerate() {
                let trimmed = line.trim();
                if !in_test_scope && (trimmed == "#[cfg(test)]" || trimmed.starts_with("#[test]")) {
                    pending_test_scope = true;
                    continue;
                }

                if pending_test_scope {
                    let opens = line.matches('{').count();
                    let closes = line.matches('}').count();
                    if opens > 0 {
                        in_test_scope = true;
                        test_scope_depth = opens.saturating_sub(closes);
                        pending_test_scope = false;
                    }
                    continue;
                }

                if in_test_scope {
                    let opens = line.matches('{').count();
                    let closes = line.matches('}').count();
                    test_scope_depth = test_scope_depth.saturating_add(opens);
                    test_scope_depth = test_scope_depth.saturating_sub(closes);
                    if test_scope_depth == 0 {
                        in_test_scope = false;
                    }
                    continue;
                }

                let lower = line.to_lowercase();
                for token in &forbidden {
                    if !lower.contains(token) {
                        continue;
                    }
                    if is_allowed_occurrence(&file, token, &lower) {
                        continue;
                    }
                    violations.push(format!(
                        "{}:{} contains '{}'",
                        file.display(),
                        line_no + 1,
                        token
                    ));
                }
            }
        }
    }

    assert!(
        violations.is_empty(),
        "Forbidden tool names found:\n{}",
        violations.join("\n")
    );
}

fn load_forbidden_tokens(repo_root: &Path) -> Vec<String> {
    let denylist = repo_root.join("scripts").join("ui-denylist.txt");
    let Ok(contents) = std::fs::read_to_string(denylist) else {
        return vec![
            "claude".to_string(),
            "gemini".to_string(),
            "codex".to_string(),
            "openai".to_string(),
            "anthropic".to_string(),
            "google".to_string(),
        ];
    };

    contents
        .lines()
        .map(|line| line.trim().to_lowercase())
        .filter(|line| !line.is_empty() && !line.starts_with('#'))
        .collect()
}

fn is_allowed_occurrence(file: &Path, token: &str, line: &str) -> bool {
    let path = file.to_string_lossy().replace('\\', "/");

    // Built-in ToolSpec embedding references concrete spec filenames.
    if path.ends_with("core/src/tool/loader.rs") {
        if line.contains("registry/tools.d/") && line.contains(".tool.yaml") {
            return true;
        }
        if line.contains("embedded:") && line.contains(".tool.yaml") {
            return true;
        }
    }

    // Existing clear tests use pre-existing CLAUDE.md fixtures.
    if path.ends_with("core/src/lib.rs") && token == "claude" && line.contains("claude.md") {
        return true;
    }

    // normalizers.rs is the per-provider error normalizer implementation layer.
    // It necessarily contains provider identifiers ("claude", "codex", "gemini")
    // as Rust type names and as the `provider` field values it produces.
    if path.ends_with("core/src/coordinator/normalizers.rs") {
        return true;
    }

    // error_normalizer.rs defines the canonical classification table and helper
    // functions. Its doc comments list provider names as illustrative examples.
    if path.ends_with("core/src/coordinator/error_normalizer.rs") {
        return true;
    }

    // engine.rs references per-provider normalizer struct types in its import
    // block and in the `get_normalizer_for_tool` dispatch function.
    if path.ends_with("core/src/coordinator/engine.rs") {
        // Import of per-provider normalizer types (e.g. ClaudeErrorNormalizer).
        if line.contains("errornormalizer") {
            return true;
        }
        // Code comments may name specific tools in bug/context descriptions.
        let trimmed = line.trim_start();
        if trimmed.starts_with("//") {
            return true;
        }
    }

    // rate_limit.rs has a doc-comment field description listing example tool IDs.
    if path.ends_with("core/src/coordinator/rate_limit.rs") {
        let trimmed = line.trim_start();
        if trimmed.starts_with("///") || trimmed.starts_with("//") {
            return true;
        }
    }

    false
}
