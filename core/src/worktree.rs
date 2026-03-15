use crate::tool::ToolSpecLoader;
use crate::{config::CanonicalConfig, MaccError, ProjectPaths, Result};
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorktreeEntry {
    pub path: PathBuf,
    pub head: Option<String>,
    pub branch: Option<String>,
    pub locked: bool,
    pub prunable: bool,
}

#[derive(Debug, Clone)]
pub struct WorktreeCreateSpec {
    pub slug: String,
    pub tool: String,
    pub count: usize,
    pub base: String,
    pub dir: PathBuf,
    pub scope: Option<String>,
    pub feature: Option<String>,
}

#[derive(Debug, Clone)]
pub struct WorktreeCreateResult {
    pub id: String,
    pub path: PathBuf,
    pub branch: String,
    pub base: String,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct WorktreeMetadata {
    pub id: String,
    pub tool: String,
    pub scope: Option<String>,
    pub feature: Option<String>,
    pub base: String,
    pub branch: String,
}

pub fn list_worktrees(cwd: &Path) -> Result<Vec<WorktreeEntry>> {
    let text = crate::git::worktree_list_porcelain(cwd)?;
    Ok(parse_porcelain(&text))
}

pub fn current_worktree(cwd: &Path, entries: &[WorktreeEntry]) -> Option<WorktreeEntry> {
    let cwd = cwd.canonicalize().unwrap_or_else(|_| cwd.to_path_buf());
    entries.iter().find(|entry| entry.path == cwd).cloned()
}

pub fn create_worktrees(
    root: &Path,
    spec: &WorktreeCreateSpec,
) -> Result<Vec<WorktreeCreateResult>> {
    if spec.count == 0 {
        return Err(MaccError::Validation("worktree count must be >= 1".into()));
    }

    let base_dir = root.join(&spec.dir);
    std::fs::create_dir_all(&base_dir).map_err(|e| MaccError::Io {
        path: base_dir.to_string_lossy().into(),
        action: "create worktree base dir".into(),
        source: e,
    })?;

    let mut results = Vec::new();
    let suffix = generate_suffix();
    for idx in 1..=spec.count {
        let id = if spec.count == 1 {
            format!("{}-{}", spec.slug, suffix)
        } else {
            format!("{}-{}-{:02}", spec.slug, suffix, idx)
        };
        let branch = format!("ai/{}/{}", spec.tool, id);
        let path = base_dir.join(&id);

        crate::git::worktree_add(root, &branch, &path, &spec.base)?;

        write_worktree_metadata(
            &path,
            WorktreeMetadata {
                id: id.clone(),
                tool: spec.tool.clone(),
                scope: spec.scope.clone(),
                feature: spec.feature.clone(),
                base: spec.base.clone(),
                branch: branch.clone(),
            },
        )?;

        if let Some(scope) = &spec.scope {
            write_scope_file(&path, scope)?;
        }

        results.push(WorktreeCreateResult {
            id,
            path,
            branch,
            base: spec.base.clone(),
        });
    }

    Ok(results)
}

pub fn remove_worktree(root: &Path, path: &Path, force: bool) -> Result<()> {
    crate::git::worktree_remove(root, path, force)
}

pub fn prune_worktrees(root: &Path) -> Result<()> {
    crate::git::worktree_prune(root)
}

fn parse_porcelain(output: &str) -> Vec<WorktreeEntry> {
    let mut entries = Vec::new();
    let mut current: Option<WorktreeEntry> = None;

    for raw in output.lines() {
        let line = raw.trim();
        if line.is_empty() {
            continue;
        }

        if let Some(rest) = line.strip_prefix("worktree ") {
            if let Some(entry) = current.take() {
                entries.push(entry);
            }
            current = Some(WorktreeEntry {
                path: PathBuf::from(rest),
                head: None,
                branch: None,
                locked: false,
                prunable: false,
            });
            continue;
        }

        let Some(entry) = current.as_mut() else {
            continue;
        };

        if let Some(rest) = line.strip_prefix("HEAD ") {
            entry.head = Some(rest.to_string());
        } else if let Some(rest) = line.strip_prefix("branch ") {
            entry.branch = Some(rest.to_string());
        } else if line.starts_with("locked") {
            entry.locked = true;
        } else if line.starts_with("prunable") {
            entry.prunable = true;
        }
    }

    if let Some(entry) = current.take() {
        entries.push(entry);
    }

    entries
}

fn write_worktree_metadata(path: &Path, metadata: WorktreeMetadata) -> Result<()> {
    let macc_dir = path.join(".macc");
    std::fs::create_dir_all(&macc_dir).map_err(|e| MaccError::Io {
        path: macc_dir.to_string_lossy().into(),
        action: "create .macc directory".into(),
        source: e,
    })?;

    let file_path = macc_dir.join("worktree.json");
    let content = serde_json::to_string_pretty(&metadata)
        .map_err(|e| MaccError::Validation(format!("Failed to serialize worktree.json: {}", e)))?;
    std::fs::write(&file_path, content).map_err(|e| MaccError::Io {
        path: file_path.to_string_lossy().into(),
        action: "write worktree.json".into(),
        source: e,
    })?;
    Ok(())
}

pub fn read_worktree_metadata(path: &Path) -> Result<Option<WorktreeMetadata>> {
    let file_path = path.join(".macc").join("worktree.json");
    if !file_path.exists() {
        return Ok(None);
    }
    let content = std::fs::read_to_string(&file_path).map_err(|e| MaccError::Io {
        path: file_path.to_string_lossy().into(),
        action: "read worktree.json".into(),
        source: e,
    })?;
    let metadata = serde_json::from_str(&content)
        .map_err(|e| MaccError::Validation(format!("Failed to parse worktree.json: {}", e)))?;
    Ok(Some(metadata))
}

pub fn write_tool_json(repo_root: &Path, worktree_path: &Path, tool_id: &str) -> Result<PathBuf> {
    let search_paths = ToolSpecLoader::default_search_paths(repo_root);
    let loader = ToolSpecLoader::new(search_paths);
    let (specs, _) = loader.load_all_with_embedded();

    let spec = specs
        .into_iter()
        .find(|spec| spec.id == tool_id)
        .ok_or_else(|| MaccError::Validation(format!("Tool spec not found: {}", tool_id)))?;
    let mut runtime = spec.to_runtime_config().ok_or_else(|| {
        MaccError::Validation(format!("Tool spec missing performer section: {}", tool_id))
    })?;
    let canonical = load_tool_runtime_config(repo_root, worktree_path)?;
    let placeholders = resolve_runtime_placeholders(&spec, &canonical);
    apply_runtime_placeholders(&mut runtime, &placeholders);

    let worktree_paths = ProjectPaths::from_root(worktree_path);
    let _ = crate::ensure_embedded_automation_scripts(&worktree_paths)?;
    if let Some(runner_path) =
        crate::embedded_runner_path_for_ref(&worktree_paths, &runtime.performer.runner)?
    {
        runtime.performer.runner = runner_path.to_string_lossy().into_owned();
    }

    let macc_dir = worktree_path.join(".macc");
    std::fs::create_dir_all(&macc_dir).map_err(|e| MaccError::Io {
        path: macc_dir.to_string_lossy().into(),
        action: "create .macc directory".into(),
        source: e,
    })?;

    let tool_json_path = macc_dir.join("tool.json");
    let content = serde_json::to_string_pretty(&runtime)
        .map_err(|e| MaccError::Validation(format!("Failed to serialize tool.json: {}", e)))?;
    std::fs::write(&tool_json_path, content).map_err(|e| MaccError::Io {
        path: tool_json_path.to_string_lossy().into(),
        action: "write tool.json".into(),
        source: e,
    })?;
    Ok(tool_json_path)
}

fn load_tool_runtime_config(repo_root: &Path, worktree_path: &Path) -> Result<CanonicalConfig> {
    let worktree_paths = ProjectPaths::from_root(worktree_path);
    if worktree_paths.config_path.exists() {
        return crate::load_canonical_config(&worktree_paths.config_path);
    }

    let repo_paths = ProjectPaths::from_root(repo_root);
    crate::load_canonical_config(&repo_paths.config_path)
}

fn resolve_runtime_placeholders(
    spec: &crate::tool::ToolSpec,
    canonical: &CanonicalConfig,
) -> BTreeMap<String, String> {
    let mut placeholders = BTreeMap::new();
    if let Some(model) = resolve_tool_model(spec, canonical) {
        placeholders.insert("model".to_string(), model);
    }
    placeholders
}

fn resolve_tool_model(spec: &crate::tool::ToolSpec, canonical: &CanonicalConfig) -> Option<String> {
    let field = spec.fields.iter().find(|field| field.id == "model");
    if let Some(field) = field {
        if let Some(pointer) = field.pointer.as_deref() {
            if let Some(value) = resolve_tool_pointer_value(canonical, &spec.id, pointer)
                .and_then(json_scalar_to_string)
            {
                return Some(value);
            }
        }

        if let Some(default) = field.default.as_ref().and_then(json_scalar_to_string) {
            return Some(default);
        }
    }

    spec.defaults
        .as_ref()
        .and_then(|defaults| defaults.get("model"))
        .and_then(json_scalar_to_string)
}

fn resolve_tool_pointer_value<'a>(
    canonical: &'a CanonicalConfig,
    tool_id: &str,
    pointer: &str,
) -> Option<&'a serde_json::Value> {
    let relative = relative_tool_pointer(tool_id, pointer)?;
    canonical
        .tools
        .config
        .get(tool_id)
        .and_then(|value| value.pointer(relative))
        .or_else(|| {
            canonical
                .tools
                .settings
                .get(tool_id)
                .and_then(|value| value.pointer(relative))
        })
}

fn relative_tool_pointer<'a>(tool_id: &str, pointer: &'a str) -> Option<&'a str> {
    let config_prefix = format!("/tools/config/{}", tool_id);
    let legacy_prefix = format!("/tools/{}", tool_id);
    if let Some(relative) = pointer.strip_prefix(&config_prefix) {
        return Some(if relative.is_empty() { "/" } else { relative });
    }
    if let Some(relative) = pointer.strip_prefix(&legacy_prefix) {
        return Some(if relative.is_empty() { "/" } else { relative });
    }
    None
}

fn json_scalar_to_string(value: &serde_json::Value) -> Option<String> {
    match value {
        serde_json::Value::String(text) => Some(text.clone()),
        serde_json::Value::Number(number) => Some(number.to_string()),
        serde_json::Value::Bool(flag) => Some(flag.to_string()),
        _ => None,
    }
}

fn apply_runtime_placeholders(
    runtime: &mut crate::tool::ToolRuntimeConfig,
    placeholders: &BTreeMap<String, String>,
) {
    replace_placeholders_in_args(&mut runtime.performer.args, placeholders);
    if let Some(retry) = runtime.performer.retry.as_mut() {
        replace_placeholders_in_args(&mut retry.args, placeholders);
    }
    if let Some(session) = runtime.performer.session.as_mut() {
        if let Some(resume) = session.resume.as_mut() {
            replace_placeholders_in_args(&mut resume.args, placeholders);
        }
        if let Some(discover) = session.discover.as_mut() {
            replace_placeholders_in_args(&mut discover.args, placeholders);
        }
    }
}

fn replace_placeholders_in_args(args: &mut [String], placeholders: &BTreeMap<String, String>) {
    for arg in args {
        *arg = replace_placeholders(arg, placeholders);
    }
}

fn replace_placeholders(value: &str, placeholders: &BTreeMap<String, String>) -> String {
    let mut rendered = value.to_string();
    for (key, replacement) in placeholders {
        rendered = rendered.replace(&format!("{{{}}}", key), replacement);
    }
    rendered
}

pub fn ensure_performer(worktree_path: &Path) -> Result<PathBuf> {
    let target = worktree_path.join("performer.sh");
    if target.exists() {
        return Ok(target);
    }

    let worktree_paths = ProjectPaths::from_root(worktree_path);
    let _ = crate::ensure_embedded_automation_scripts(&worktree_paths)?;
    let source = worktree_paths.automation_performer_path();

    std::fs::copy(&source, &target).map_err(|e| MaccError::Io {
        path: target.to_string_lossy().into(),
        action: "copy performer.sh".into(),
        source: e,
    })?;

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut perms = std::fs::metadata(&target)
            .map_err(|e| MaccError::Io {
                path: target.to_string_lossy().into(),
                action: "read performer permissions".into(),
                source: e,
            })?
            .permissions();
        perms.set_mode(0o755);
        std::fs::set_permissions(&target, perms).map_err(|e| MaccError::Io {
            path: target.to_string_lossy().into(),
            action: "set performer permissions".into(),
            source: e,
        })?;
    }

    Ok(target)
}

pub fn resolve_worktree_task_context(
    repo_root: &Path,
    worktree_path: &Path,
    fallback_id: &str,
) -> Result<(String, PathBuf)> {
    let prd_path = worktree_path.join("worktree.prd.json");
    if prd_path.exists() {
        let content = std::fs::read_to_string(&prd_path).map_err(|e| MaccError::Io {
            path: prd_path.to_string_lossy().into(),
            action: "read worktree.prd.json".into(),
            source: e,
        })?;
        let json: serde_json::Value = serde_json::from_str(&content).map_err(|e| {
            MaccError::Validation(format!("Failed to parse worktree.prd.json: {}", e))
        })?;
        let task_id = json
            .get("tasks")
            .and_then(|tasks| tasks.get(0))
            .and_then(|task| task.get("id"))
            .and_then(|id| match id {
                serde_json::Value::String(s) => Some(s.clone()),
                serde_json::Value::Number(n) => Some(n.to_string()),
                _ => None,
            })
            .ok_or_else(|| {
                MaccError::Validation("worktree.prd.json is missing tasks[0].id".into())
            })?;
        return Ok((task_id, prd_path));
    }

    let fallback_prd = repo_root.join("prd.json");
    if !fallback_prd.exists() {
        return Err(MaccError::Validation(
            "Missing worktree.prd.json and prd.json".into(),
        ));
    }
    Ok((fallback_id.to_string(), fallback_prd))
}

pub fn sync_context_files_from_root(
    repo_root: &Path,
    worktree_root: &Path,
    canonical: &CanonicalConfig,
) -> Result<()> {
    let targets = collect_context_targets(repo_root, canonical);
    for rel in targets {
        let src = repo_root.join(&rel);
        if !src.is_file() {
            continue;
        }

        let dest = worktree_root.join(&rel);
        if src == dest {
            continue;
        }

        if let Some(parent) = dest.parent() {
            std::fs::create_dir_all(parent).map_err(|e| MaccError::Io {
                path: parent.to_string_lossy().into(),
                action: "create context file parent directory in worktree".into(),
                source: e,
            })?;
        }

        std::fs::copy(&src, &dest).map_err(|e| MaccError::Io {
            path: dest.to_string_lossy().into(),
            action: "synchronize context file into worktree".into(),
            source: e,
        })?;
    }
    Ok(())
}

pub fn collect_context_targets(repo_root: &Path, canonical: &CanonicalConfig) -> Vec<String> {
    let search_paths = ToolSpecLoader::default_search_paths(repo_root);
    let loader = ToolSpecLoader::new(search_paths);
    let (specs, _) = loader.load_all_with_embedded();
    let by_id: BTreeMap<String, crate::tool::ToolSpec> = specs
        .into_iter()
        .map(|spec| (spec.id.clone(), spec))
        .collect();

    let mut targets = BTreeSet::new();
    for tool_id in &canonical.tools.enabled {
        let from_settings = context_targets_from_tool_settings(canonical, tool_id);
        if from_settings.is_empty() {
            if let Some(spec) = by_id.get(tool_id) {
                for rel in spec.gitignore.iter().filter(|entry| {
                    Path::new(entry)
                        .extension()
                        .and_then(|e| e.to_str())
                        .is_some_and(|ext| ext.eq_ignore_ascii_case("md"))
                }) {
                    if let Some(normalized) = normalize_context_target(rel) {
                        targets.insert(normalized);
                    }
                }
            }
        } else {
            for rel in from_settings {
                if let Some(normalized) = normalize_context_target(&rel) {
                    targets.insert(normalized);
                }
            }
        }

        if !targets
            .iter()
            .any(|p| p == &format!("{}.md", tool_id.to_ascii_uppercase().replace('-', "_")))
        {
            let fallback = format!("{}.md", tool_id.to_ascii_uppercase().replace('-', "_"));
            if let Some(normalized) = normalize_context_target(&fallback) {
                targets.insert(normalized);
            }
        }
    }
    targets.into_iter().collect()
}

fn context_targets_from_tool_settings(canonical: &CanonicalConfig, tool_id: &str) -> Vec<String> {
    let mut targets = Vec::new();
    let config_map_entry = canonical.tools.config.get(tool_id);
    let legacy_entry = canonical.tools.settings.get(tool_id);
    for entry in [config_map_entry, legacy_entry].into_iter().flatten() {
        targets.extend(extract_context_file_names_from_json(entry));
    }
    targets
}

fn extract_context_file_names_from_json(value: &serde_json::Value) -> Vec<String> {
    let Some(context) = value.get("context") else {
        return Vec::new();
    };
    let Some(file_name) = context.get("fileName") else {
        return Vec::new();
    };
    match file_name {
        serde_json::Value::String(s) => vec![s.clone()],
        serde_json::Value::Array(items) => items
            .iter()
            .filter_map(|item| item.as_str().map(|s| s.to_string()))
            .collect(),
        _ => Vec::new(),
    }
}

fn normalize_context_target(value: &str) -> Option<String> {
    let normalized = value.replace('\\', "/").trim().to_string();
    if normalized.is_empty() {
        return None;
    }
    if normalized.starts_with('/') {
        return None;
    }
    if normalized
        .split('/')
        .any(|part| part.is_empty() || part == "." || part == "..")
    {
        return None;
    }
    Some(normalized)
}

fn write_scope_file(path: &Path, scope: &str) -> Result<()> {
    let macc_dir = path.join(".macc");
    std::fs::create_dir_all(&macc_dir).map_err(|e| MaccError::Io {
        path: macc_dir.to_string_lossy().into(),
        action: "create .macc directory".into(),
        source: e,
    })?;
    let scope_path = macc_dir.join("scope.md");
    std::fs::write(&scope_path, scope).map_err(|e| MaccError::Io {
        path: scope_path.to_string_lossy().into(),
        action: "write scope.md".into(),
        source: e,
    })?;
    Ok(())
}

fn generate_suffix() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    format!("{:x}", nanos)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn parse_porcelain_output() {
        let sample = "worktree /repo\nHEAD 111111\nbranch refs/heads/main\n\nworktree /repo/.worktrees/feat\nHEAD 222222\nbranch refs/heads/feat\nlocked\nprunable Worktree is locked\n";
        let entries = parse_porcelain(sample);
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].path, PathBuf::from("/repo"));
        assert_eq!(entries[0].head.as_deref(), Some("111111"));
        assert_eq!(entries[0].branch.as_deref(), Some("refs/heads/main"));
        assert!(!entries[0].locked);

        assert_eq!(entries[1].path, PathBuf::from("/repo/.worktrees/feat"));
        assert_eq!(entries[1].head.as_deref(), Some("222222"));
        assert_eq!(entries[1].branch.as_deref(), Some("refs/heads/feat"));
        assert!(entries[1].locked);
        assert!(entries[1].prunable);
    }

    #[test]
    fn resolves_model_placeholder_from_tool_config() {
        let spec = crate::tool::ToolSpec {
            api_version: "v1".to_string(),
            id: "gemini".to_string(),
            display_name: "Gemini".to_string(),
            description: None,
            capabilities: Vec::new(),
            fields: vec![crate::tool::FieldSpec {
                id: "model".to_string(),
                label: "Model".to_string(),
                kind: crate::tool::FieldKindSpec::Text,
                help: None,
                pointer: Some("/tools/config/gemini/model/name".to_string()),
                default: Some(json!("default-model")),
            }],
            doctor: None,
            gitignore: Vec::new(),
            performer: None,
            install: None,
            update: None,
            version_check: None,
            defaults: Some(json!({"model": "fallback-model"})),
        };
        let canonical = CanonicalConfig {
            version: None,
            tools: crate::config::ToolsConfig {
                enabled: vec!["gemini".to_string()],
                config: BTreeMap::from([(
                    "gemini".to_string(),
                    json!({"model": {"name": "user-selected-model"}}),
                )]),
                settings: BTreeMap::new(),
            },
            standards: crate::config::StandardsConfig::default(),
            selections: None,
            automation: crate::config::AutomationConfig::default(),
            settings: crate::config::SettingsConfig::default(),
            mcp_templates: CanonicalConfig::default().mcp_templates,
        };

        assert_eq!(
            resolve_tool_model(&spec, &canonical).as_deref(),
            Some("user-selected-model")
        );
    }

    #[test]
    fn applies_model_placeholder_to_retry_and_resume_args() {
        let mut runtime = crate::tool::ToolRuntimeConfig {
            api_version: "v1".to_string(),
            id: "codex".to_string(),
            display_name: "Codex".to_string(),
            performer: crate::tool::ToolPerformerSpec {
                runner: "runner.sh".to_string(),
                command: "codex".to_string(),
                args: vec!["exec".to_string(), "-".to_string()],
                retry: Some(crate::tool::ToolPerformerCommand {
                    command: "codex".to_string(),
                    args: vec![
                        "--model".to_string(),
                        "{model}".to_string(),
                        "exec".to_string(),
                    ],
                }),
                prompt: None,
                session: Some(crate::tool::ToolPerformerSessionSpec {
                    enabled: true,
                    scope: None,
                    init_prompt: None,
                    extract_regex: None,
                    resume: Some(crate::tool::ToolPerformerCommand {
                        command: "codex".to_string(),
                        args: vec![
                            "--model".to_string(),
                            "{model}".to_string(),
                            "exec".to_string(),
                            "resume".to_string(),
                            "{session_id}".to_string(),
                        ],
                    }),
                    discover: None,
                    id_strategy: None,
                }),
            },
            defaults: None,
        };
        let placeholders = BTreeMap::from([("model".to_string(), "gpt-5.2-codex".to_string())]);

        apply_runtime_placeholders(&mut runtime, &placeholders);

        assert_eq!(
            runtime
                .performer
                .retry
                .as_ref()
                .map(|retry| retry.args.clone()),
            Some(vec![
                "--model".to_string(),
                "gpt-5.2-codex".to_string(),
                "exec".to_string(),
            ])
        );
        assert_eq!(
            runtime
                .performer
                .session
                .as_ref()
                .and_then(|session| session.resume.as_ref())
                .map(|resume| resume.args.clone()),
            Some(vec![
                "--model".to_string(),
                "gpt-5.2-codex".to_string(),
                "exec".to_string(),
                "resume".to_string(),
                "{session_id}".to_string(),
            ])
        );
    }
}
