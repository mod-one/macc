//! Locate where an unmet precondition must be fixed.
//!
//! A performer that stops with `precondition_unmet` often names the file that
//! must change ("apps/api/src/routes/session.ts must declare the web
//! audience"). When that file lies outside the task's `change_scope`, retrying
//! or unblocking the task cannot help: the fix belongs to another module. This
//! module detects that case so the blocked record, the run summary and the
//! supervisor diagnosis can say so instead of only reporting a stop.

use crate::coordinator::model::Task;
use serde_json::Value;

/// Runtime `extra` key holding the out-of-scope fix record.
pub const OUT_OF_SCOPE_FIX_KEY: &str = "out_of_scope_fix";

/// Paths named in `texts` that `task` may not change: outside every
/// `change_scope.allowed_paths` pattern, or inside a read-only or forbidden one.
///
/// Returns nothing when the task declares no `allowed_paths`, since there is
/// then no scope to compare against.
pub fn out_of_scope_paths(task: &Task, texts: &[&str]) -> Vec<String> {
    let scope = task.extra.get("change_scope");
    let list = |key: &str| -> Vec<String> {
        scope
            .and_then(|s| s.get(key))
            .and_then(Value::as_array)
            .map(|items| {
                items
                    .iter()
                    .filter_map(Value::as_str)
                    .map(|p| p.trim().trim_start_matches("./").to_string())
                    .filter(|p| !p.is_empty())
                    .collect()
            })
            .unwrap_or_default()
    };
    let allowed = list("allowed_paths");
    if allowed.is_empty() {
        return Vec::new();
    }
    let protected: Vec<String> = list("read_only_paths")
        .into_iter()
        .chain(list("forbidden_paths"))
        .collect();
    let mut out: Vec<String> = Vec::new();
    for path in texts.iter().flat_map(|text| mentioned_paths(text)) {
        let writable = allowed.iter().any(|pattern| path_matches(pattern, &path))
            && !protected.iter().any(|pattern| path_matches(pattern, &path));
        if !writable && !out.contains(&path) {
            out.push(path);
        }
    }
    out
}

/// The repository paths a free-text precondition names: tokens with a `/`
/// that look like relative file or directory paths (URLs and API routes such
/// as `/api/v1/session` are ignored).
pub fn mentioned_paths(text: &str) -> Vec<String> {
    let mut out = Vec::new();
    for raw in text.split(|c: char| {
        c.is_whitespace()
            || matches!(
                c,
                '(' | ')' | '[' | ']' | '{' | '}' | ',' | ';' | '`' | '"' | '\'' | '<' | '>'
            )
    }) {
        let token = raw
            .trim_end_matches(['.', ':', '!', '?'])
            .trim_start_matches("./");
        if token.contains("://") || token.starts_with('/') || !token.contains('/') {
            continue;
        }
        let looks_like_path = token
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '/' | '.' | '-' | '_' | '*' | '@'))
            && token.split('/').all(|segment| !segment.is_empty());
        if looks_like_path && !out.iter().any(|p: &String| p == token) {
            out.push(token.to_string());
        }
    }
    out
}

/// Whether `path` is covered by `pattern` (`*` within one segment, `**` across
/// segments; a plain directory pattern covers everything below it).
pub fn path_matches(pattern: &str, path: &str) -> bool {
    let pattern = pattern.trim_end_matches('/');
    if !pattern.contains('*') {
        return path == pattern || path.starts_with(&format!("{pattern}/"));
    }
    glob(pattern.as_bytes(), path.as_bytes())
}

fn glob(pattern: &[u8], text: &[u8]) -> bool {
    match pattern.first() {
        None => text.is_empty(),
        Some(b'*') if pattern.get(1) == Some(&b'*') => {
            // `**/` may also match zero directories.
            let rest = &pattern[2..];
            let rest_after_slash = rest.strip_prefix(b"/").unwrap_or(rest);
            (0..=text.len()).any(|i| glob(rest, &text[i..]) || glob(rest_after_slash, &text[i..]))
        }
        Some(b'*') => {
            let rest = &pattern[1..];
            (0..=text.len())
                .take_while(|&i| i == 0 || text[i - 1] != b'/')
                .any(|i| glob(rest, &text[i..]))
        }
        Some(&c) => text.first() == Some(&c) && glob(&pattern[1..], &text[1..]),
    }
}

/// Sentence appended to a precondition record whose fix is out of scope.
pub fn out_of_scope_note(task_id: &str, paths: &[String]) -> String {
    format!(
        "The fix lies outside {task_id}'s change scope ({}): it needs a source change in the module that owns {}, not a retry of {task_id}. A supervisor repair or a task owning {} must land first; dependants stay blocked until then.",
        paths.join(", "),
        if paths.len() == 1 { "that path" } else { "those paths" },
        if paths.len() == 1 { "it" } else { "them" },
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn task(scope: Value) -> Task {
        serde_json::from_value(json!({"id":"WEB-UI-005","change_scope":scope})).unwrap()
    }

    #[test]
    fn paths_are_extracted_from_prose_but_not_urls_or_routes() {
        let text = "apps/api/src/routes/session.ts must declare the `web` audience (see docs/12-api.md). GET /api/v1/session; https://x.dev/a/b.";
        assert_eq!(
            mentioned_paths(text),
            vec!["apps/api/src/routes/session.ts", "docs/12-api.md"]
        );
    }

    #[test]
    fn globs_cover_segments_and_directories() {
        assert!(path_matches("apps/web/**", "apps/web/src/a.tsx"));
        assert!(path_matches("apps/web/src/**/*.tsx", "apps/web/src/a.tsx"));
        assert!(path_matches("docs/1*.md", "docs/12-api.md"));
        assert!(!path_matches("docs/1*.md", "docs/sub/12.md"));
        assert!(path_matches("apps/web", "apps/web/src/a.tsx"));
        assert!(!path_matches("apps/web", "apps/website/a.tsx"));
    }

    #[test]
    fn a_fix_in_another_module_is_out_of_scope() {
        let t = task(
            json!({"allowed_paths":["apps/web/**"],"read_only_paths":["apps/web/generated/**"]}),
        );
        let paths = out_of_scope_paths(
            &t,
            &[
                "apps/api/src/routes/session.ts must expose the web audience",
                "apps/web/src/pages/login.tsx needs the session route",
                "regenerate apps/web/generated/client.ts",
            ],
        );
        assert_eq!(
            paths,
            vec![
                "apps/api/src/routes/session.ts",
                "apps/web/generated/client.ts"
            ]
        );
    }

    #[test]
    fn without_a_declared_scope_nothing_is_flagged() {
        let t: Task = serde_json::from_value(json!({"id":"X"})).unwrap();
        assert!(out_of_scope_paths(&t, &["apps/api/src/a.ts must change"]).is_empty());
    }
}
