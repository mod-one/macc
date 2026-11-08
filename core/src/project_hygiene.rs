//! Portable metadata checks report field locations, never the private values found.
use crate::{doctor::DiagnosticFinding, ProjectPaths};
use serde_json::Value;
use std::path::Path;

fn portable(path: &str) -> bool {
    matches!(path, ".macc/macc.yaml" | ".macc/skills.lock.json")
        || path.starts_with(".macc/catalog/")
}
fn referenced(value: &str) -> bool {
    value.is_empty()
        || crate::security::contains_placeholder(value)
        || value.starts_with("env://")
        || value.starts_with('$')
}
fn scan(value: &Value, location: &str, findings: &mut Vec<DiagnosticFinding>) {
    match value {
        Value::Object(map) => {
            for (key, value) in map {
                let field = format!("{location}/{key}");
                let normalized = key.to_ascii_lowercase().replace('-', "_");
                if let Some(text) = value.as_str().filter(|s| !referenced(s)) {
                    if matches!(
                        normalized.as_str(),
                        "provider_session_id"
                            | "native_session_id"
                            | "session_id"
                            | "lease_id"
                            | "owner_process_id"
                            | "owner_worktree"
                    ) {
                        findings.push(DiagnosticFinding::warning("MACC-GIT-RUNTIME-METADATA", "git", "Machine-local execution metadata in portable setup", &format!("{field}: native session/lease/owner values must stay local."), Some("Remove execution metadata from shared setup; use local persistent storage.")));
                    } else if matches!(
                        normalized.as_str(),
                        "api_key"
                            | "apikey"
                            | "access_token"
                            | "refresh_token"
                            | "client_secret"
                            | "password"
                            | "secret"
                            | "authorization"
                    ) {
                        findings.push(DiagnosticFinding::warning("MACC-GIT-PRIVATE-VALUE", "git", "Literal credential in portable setup", &format!("{field}: credential field contains a literal value (redacted)."), Some("Use an environment placeholder such as ${API_KEY}; rotate credentials already published.")));
                    } else if Path::new(text).is_absolute()
                        || text.starts_with("~/")
                        || (text.len() > 2
                            && text.as_bytes()[1] == b':'
                            && matches!(text.as_bytes()[2], b'\\' | b'/'))
                    {
                        findings.push(DiagnosticFinding::warning("MACC-GIT-LOCAL-PATH", "git", "Machine-specific path in portable setup", &format!("{field}: absolute local path will not generally work in another checkout."), Some("Use a project-relative source or a pinned remote source. Put intentionally local sources in .macc/persistent/catalog overrides.")));
                    }
                }
                scan(value, &field, findings);
            }
        }
        Value::Array(values) => {
            for (index, value) in values.iter().enumerate() {
                scan(value, &format!("{location}/{index}"), findings);
            }
        }
        _ => {}
    }
}
pub fn findings(paths: &ProjectPaths) -> Vec<DiagnosticFinding> {
    fn walk(path: &Path, files: &mut Vec<std::path::PathBuf>) {
        let Ok(kind) = std::fs::symlink_metadata(path) else {
            return;
        };
        if kind.is_symlink() {
            return;
        }
        if kind.is_file() {
            files.push(path.into());
        } else if let Ok(entries) = std::fs::read_dir(path) {
            for entry in entries.flatten() {
                walk(&entry.path(), files);
            }
        }
    }
    let mut findings = vec![];
    let mut files = vec![
        paths.config_path.clone(),
        paths.macc_dir.join("skills.lock.json"),
    ];
    walk(&paths.macc_dir.join("catalog"), &mut files);
    for file in files.into_iter().filter(|p| p.is_file()) {
        let location = file
            .strip_prefix(&paths.root)
            .unwrap_or(&file)
            .to_string_lossy();
        if let Ok(bytes) = std::fs::read(&file) {
            for finding in crate::security::scan_bytes(&location, &bytes) {
                findings.push(DiagnosticFinding::warning(
                    "MACC-GIT-SECRET-PATTERN",
                    "git",
                    "Private content in portable setup",
                    &format!("{location}: {} detected (redacted).", finding.pattern_name),
                    Some("Use environment placeholders; review any already published credentials."),
                ));
            }
        }
        let parsed = std::fs::read(&file).ok().and_then(|bytes| {
            match file.extension().and_then(|e| e.to_str()) {
                Some("yaml" | "yml") => serde_yaml::from_slice::<Value>(&bytes).ok(),
                Some("json") => serde_json::from_slice::<Value>(&bytes).ok(),
                _ => None,
            }
        });
        if let Some(value) = parsed {
            scan(&value, &location, &mut findings);
        }
    }
    if crate::git::git_common_dir(&paths.root).is_ok() {
        match crate::git::run_git_output_mapped(
            &paths.root,
            &["ls-files", "-z", "--", ".macc"],
            "inspect tracked MACC runtime",
        ) {
            Ok(output) if output.status.success() => {
                for name in output.stdout.split(|b| *b == 0).filter(|b| !b.is_empty()) {
                    let name = String::from_utf8_lossy(name);
                    if !portable(&name) {
                        findings.push(DiagnosticFinding::warning("MACC-GIT-TRACKED-RUNTIME", "git", "Local MACC state is already tracked", &format!("{name}: Git ignore rules do not protect files already in the index."), Some("Review git ls-files -- .macc; explicitly untrack local runtime with git rm --cached -- <path>. Review published history separately; MACC never changes the index automatically.")));
                    }
                }
            }
            Err(error) => findings.push(DiagnosticFinding::warning(
                "MACC-GIT-HYGIENE-UNVERIFIED",
                "git",
                "Cannot inspect tracked local state",
                &error.to_string(),
                Some("Check Git access and retry Doctor."),
            )),
            _ => findings.push(DiagnosticFinding::warning(
                "MACC-GIT-HYGIENE-UNVERIFIED",
                "git",
                "Cannot inspect tracked local state",
                "git ls-files failed; Git hygiene is unverified.",
                Some("Check Git access and retry Doctor."),
            )),
        }
    }
    findings
}
/// Only project-contained local sources are rewritten. External machine paths are
/// diagnosed, not guessed or silently converted to a different resource.
pub(crate) fn normalize_source(paths: &ProjectPaths, source: &mut crate::catalog::Source) {
    if source.kind != crate::catalog::SourceKind::Local {
        return;
    }
    let path = Path::new(&source.url);
    if let Ok(relative) = path.strip_prefix(&paths.root) {
        if let Some(normalized) = crate::normalize_relative_path(&relative.to_string_lossy()) {
            source.url = normalized;
        }
    }
}
/// Include portability repairs in the reviewed Apply plan, preserving unknown
/// catalog fields and never mutating a file while previewing it.
pub(crate) fn plan_portable_catalogs(
    paths: &ProjectPaths,
    plan: &mut crate::plan::ActionPlan,
) -> crate::Result<()> {
    for file in [
        paths.project_skills_catalog_path(),
        paths.project_mcp_catalog_path(),
    ] {
        if !file.is_file() {
            continue;
        }
        let before = std::fs::read(&file).map_err(crate::persistent::error)?;
        let mut value: Value = serde_json::from_slice(&before).map_err(crate::persistent::error)?;
        let mut changed = false;
        if let Some(entries) = value.get_mut("entries").and_then(Value::as_array_mut) {
            for entry in entries {
                if entry.pointer("/source/kind").and_then(Value::as_str) != Some("local") {
                    continue;
                }
                if let Some(url) = entry.pointer_mut("/source/url") {
                    if let Some(text) = url.as_str() {
                        if let Ok(relative) = Path::new(text).strip_prefix(&paths.root) {
                            if let Some(relative) =
                                crate::normalize_relative_path(&relative.to_string_lossy())
                            {
                                *url = Value::String(relative);
                                changed = true;
                            }
                        }
                    }
                }
            }
        }
        if changed {
            let relative = crate::relative_to_root(paths, &file)
                .ok_or_else(|| crate::persistent::error("Project catalog is outside project"))?;
            plan.add_action(crate::plan::Action::WriteFile {
                path: relative,
                content: serde_json::to_vec_pretty(&value).map_err(crate::persistent::error)?,
                scope: crate::plan::Scope::Project,
            });
        }
    }
    Ok(())
}

#[cfg(test)]
#[path = "project_hygiene_tests.rs"]
mod test_cases;

pub(crate) fn portable_local_url(root: &Path, url: &str) -> Option<String> {
    let path = Path::new(url);
    if let Ok(relative) = path.strip_prefix(root) {
        return crate::normalize_relative_path(&relative.to_string_lossy());
    }
    if path.is_absolute() || url.starts_with("~/") {
        return None;
    }
    Some(url.into())
}
