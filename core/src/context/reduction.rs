//! Shared pure output transforms for explicit pulls and normal execution.
use crate::Result;
pub fn apply(raw: &str, bundles: &[String]) -> Result<String> {
    let mut text = raw.to_string();
    for bundle in bundles {
        text = match bundle.as_str() {
            "test" | "test-output-failures-only" => super::test_output_failures_only(&text),
            "lint" | "lint-errors-only" => super::lint_errors_only(&text),
            "trace" | "stacktrace-collapse" => super::stacktrace_collapse(&text),
            "diff" | "git-diff-stat-before-full-diff" => super::git_diff_stat_before_full(&text, 0),
            "log" | "log-grep-error-first" => super::log_grep_error_first(&text),
            _ => {
                tracing::warn!(bundle=%bundle,"Output transform unavailable; retaining bounded raw evidence");
                text
            }
        };
    }
    Ok(text)
}
/// Protocol streams must retain their exact framing/markers for runner parsers.
pub fn is_protocol(raw: &str) -> bool {
    raw.lines().any(|line| {
        line.trim_start().starts_with("MACC_")
            || serde_json::from_str::<serde_json::Value>(line)
                .is_ok_and(|v| v.is_object() || v.is_array())
    })
}

pub fn known(bundle: &str) -> bool {
    matches!(
        bundle,
        "test"
            | "test-output-failures-only"
            | "lint"
            | "lint-errors-only"
            | "trace"
            | "stacktrace-collapse"
            | "diff"
            | "git-diff-stat-before-full-diff"
            | "log"
            | "log-grep-error-first"
    )
}
