/// Keeps only failed test output: test names, assertion messages, file/line refs, exit code.
pub fn test_output_failures_only(input: &str) -> String {
    let mut lines = Vec::new();
    let mut in_failure = false;
    for line in input.lines() {
        let lower = line.to_lowercase();
        if lower.contains("fail")
            || lower.contains("error")
            || lower.contains("assert")
            || lower.contains("panic")
            || lower.starts_with("not ok")
        {
            in_failure = true;
        }
        if in_failure || lower.contains("exit code") || lower.contains("exit status") {
            lines.push(line);
        }
        if line.trim().is_empty() && in_failure {
            in_failure = false;
        }
    }
    if lines.is_empty() {
        "No test failures detected.".to_string()
    } else {
        lines.join("\n")
    }
}

/// Keeps only error-level lint output (error code, file, line, rule, message).
pub fn lint_errors_only(input: &str) -> String {
    let mut lines: Vec<&str> = input
        .lines()
        .filter(|line| {
            let lower = line.to_lowercase();
            lower.contains("error")
                || lower.contains("err[")
                || lower.contains(" E")
                || lower.contains("✗")
        })
        .collect();
    lines.dedup();
    if lines.is_empty() {
        "No lint errors detected.".to_string()
    } else {
        lines.join("\n")
    }
}

/// Collapses a stack trace to: exception type, message, first app frame, first external frame,
/// and a count of omitted frames.
pub fn stacktrace_collapse(input: &str) -> String {
    let mut result = Vec::new();
    let mut app_frames: Vec<&str> = Vec::new();
    let mut external_frame: Option<&str> = None;
    let mut omitted = 0usize;
    let mut found_header = false;

    for line in input.lines() {
        let trimmed = line.trim();
        if !found_header {
            if trimmed.contains(':') || trimmed.starts_with("Error") || trimmed.starts_with("panic") {
                result.push(line);
                found_header = true;
            }
            continue;
        }

        if trimmed.starts_with("at ") || trimmed.starts_with("in ") || trimmed.contains("::") {
            let is_external = trimmed.contains("node_modules")
                || trimmed.contains("site-packages")
                || trimmed.contains("cargo/registry")
                || trimmed.contains(".rustup");

            if is_external {
                if external_frame.is_none() {
                    external_frame = Some(line);
                } else {
                    omitted += 1;
                }
            } else if app_frames.len() < 3 {
                app_frames.push(line);
            } else {
                omitted += 1;
            }
        } else if !trimmed.is_empty() {
            result.push(line);
        }
    }

    let mut out: Vec<String> = result.iter().map(|s| s.to_string()).collect();
    for f in app_frames {
        out.push(f.to_string());
    }
    if let Some(ef) = external_frame {
        out.push(ef.to_string());
    }
    if omitted > 0 {
        out.push(format!("... ({} frames omitted)", omitted));
    }
    out.join("\n")
}

/// Returns diff stat + file list; includes full diff only when it fits within `budget` chars.
pub fn git_diff_stat_before_full(input: &str, budget: usize) -> String {
    let mut stat_lines = Vec::new();
    let mut full_diff_lines = Vec::new();
    let mut in_stat = false;

    for line in input.lines() {
        if line.starts_with("diff --git") {
            in_stat = false;
        }
        if !in_stat
            && (line.contains("file changed")
                || line.contains("files changed")
                || line.contains("insertion")
                || line.contains("deletion"))
        {
            stat_lines.push(line);
        }
        full_diff_lines.push(line);
    }

    let stat = stat_lines.join("\n");
    let full = full_diff_lines.join("\n");

    if full.len() <= budget {
        full
    } else {
        format!(
            "{}\n\n[Full diff truncated: {} chars > budget {} chars]",
            stat,
            full.len(),
            budget
        )
    }
}

/// Returns error/warning lines with surrounding context; last non-zero exit first.
pub fn log_grep_error_first(input: &str) -> String {
    let mut errors = Vec::new();
    let mut warnings = Vec::new();
    let lines: Vec<&str> = input.lines().collect();

    for (i, line) in lines.iter().enumerate() {
        let lower = line.to_lowercase();
        if lower.contains("error") || lower.contains("fatal") || lower.contains("panic") {
            let ctx_start = i.saturating_sub(1);
            let ctx_end = (i + 2).min(lines.len());
            errors.extend_from_slice(&lines[ctx_start..ctx_end]);
            errors.push("---");
        } else if lower.contains("warn") {
            warnings.push(*line);
        }
    }

    let mut result = errors;
    if !warnings.is_empty() {
        result.push("Warnings:");
        result.extend(warnings.iter().take(10).copied());
    }
    if result.is_empty() {
        "No errors or warnings found.".to_string()
    } else {
        result.join("\n")
    }
}
