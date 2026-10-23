use super::*;
use crate::{config::CanonicalConfig, ProjectPaths, Result};
use serde_json::{json, Value};
use std::path::Path;
/// Bounded JSON protocol; no shell command from the event is ever executed.
pub fn dispatch(root: &Path, spec: &Spec, input: &Value) -> Result<Value> {
    let paths = ProjectPaths::from_root(root);
    let config = crate::load_canonical_config(&paths.config_path)?;
    let event = input["hook_event_name"].as_str().unwrap_or_default();
    if !enabled(&config)? || !matches!(event, "SessionStart" | "PostToolUse") {
        return Ok(json!({}));
    }
    let mut output = json!({});
    let mut applied = Vec::new();
    if event == "SessionStart" {
        let ids: Vec<_> = super::super::status(&config)?
            .hooks
            .into_iter()
            .filter(|h| h.enabled && h.hook.event == super::super::Event::ToolOutput)
            .map(|h| h.hook.id)
            .collect();
        output = json!({"hookSpecificOutput":{"hookEventName":"SessionStart","additionalContext":format!(
            "MACC output hooks available: {}. For a shell command whose output contract is known, put '# macc-hook: <hook-id>' on its first line, then the command. Use only a matching output contract. Failed, interrupted and protocol output stays intact. Full raw output is saved in .macc/log/hooks/. Unannotated commands are unchanged.",ids.join(", "))}});
    } else if input["tool_name"] == "Bash" {
        let command = input["tool_input"]["command"].as_str().unwrap_or_default();
        let ids: Vec<String> = command
            .lines()
            .next()
            .and_then(|s| s.strip_prefix("# macc-hook: "))
            .map(|s| s.split(',').map(|s| s.trim().to_string()).collect())
            .unwrap_or_default();
        let ids = if ids.is_empty() {
            Vec::new()
        } else {
            super::super::selected(&config, "native-shell", &ids)?
        };
        if reduction_enabled(&config, &spec.tool) && !ids.is_empty() {
            let response = &input["tool_response"];
            // Preserve typed fields. Unknown/failed/interrupted result schemas pass through.
            let exit = response
                .get("exit_code")
                .or_else(|| response.get("exitCode"));
            // Claude's documented successful Bash PostToolUse shape has no exit
            // code; tool failures use PostToolUseFailure, which is not registered.
            let claude_success = spec.protocol == Protocol::Replacement
                && exit.is_none()
                && response["interrupted"] == false
                && response["isImage"] == false
                && response["stderr"] == "";
            let known_success = (exit.and_then(Value::as_i64) == Some(0) || claude_success)
                && input.get("is_error").and_then(Value::as_bool) != Some(true)
                && response.get("is_error").and_then(Value::as_bool) != Some(true)
                && response.get("isError").and_then(Value::as_bool) != Some(true)
                && response.get("error").is_none();
            if known_success
                && response.get("interrupted").and_then(Value::as_bool) != Some(true)
                && response.get("isImage").and_then(Value::as_bool) != Some(true)
            {
                if let Some(stdout) = response["stdout"].as_str() {
                    let stderr = response["stderr"].as_str().unwrap_or_default();
                    if !super::super::super::context::reduction::is_protocol(stdout)
                        && !super::super::super::context::reduction::is_protocol(stderr)
                        && !structured(stdout)
                        && !structured(stderr)
                    {
                        let reduced = super::super::transform(&config, &ids, stdout)?;
                        if reduced.len().saturating_add(128) < stdout.len()
                            && reduced.len() <= 32000
                        {
                            let raw = archive(root, input)?;
                            let reduced = format!("{reduced}\n[MACC raw evidence: {raw}]");
                            output = match spec.protocol {
                                Protocol::Replacement => {
                                    let mut replacement = response.clone();
                                    replacement["stdout"] = reduced.into();
                                    json!({"hookSpecificOutput":{"hookEventName":"PostToolUse","updatedToolOutput":replacement}})
                                }
                                Protocol::Feedback => {
                                    json!({"continue":false,"stopReason": format!("{reduced}\n{stderr}")})
                                }
                            };
                            applied = ids;
                        }
                    }
                }
            }
        }
    }
    record(root, spec, &config, event, &applied)?;
    tracing::debug!(tool=%spec.tool, event, reducers=?applied, "Native hook bridge event observed");
    Ok(output)
}
fn structured(text: &str) -> bool {
    serde_json::from_str::<Value>(text).is_ok_and(|v| v.is_object() || v.is_array())
}
fn reduction_enabled(config: &CanonicalConfig, tool: &str) -> bool {
    let context = crate::o2::policy::from_config(config).context_engine;
    let legacy = config
        .context
        .as_ref()
        .and_then(|c| c.summarization.as_ref());
    (context.enabled && context.output_reduction || legacy.is_some_and(|s| s.enabled))
        && !legacy
            .and_then(|s| s.per_tool.get(tool))
            .is_some_and(|t| !t.enabled)
}
fn private_write(path: &Path, bytes: &[u8]) -> Result<()> {
    use std::io::Write;
    let mut options = std::fs::OpenOptions::new();
    options.create_new(true).write(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    options
        .open(path)
        .and_then(|mut f| f.write_all(bytes))
        .map_err(|e| {
            super::super::error(format!(
                "Write native hook evidence {}: {e}",
                path.display()
            ))
        })
}
fn archive(root: &Path, input: &Value) -> Result<String> {
    let relative = format!(".macc/log/hooks/{}.json", uuid::Uuid::new_v4());
    std::fs::create_dir_all(root.join(".macc/log/hooks"))
        .map_err(|e| super::super::error(e.to_string()))?;
    private_write(
        &root.join(&relative),
        &serde_json::to_vec(input).map_err(|e| super::super::error(e.to_string()))?,
    )?;
    Ok(relative)
}
fn record(
    root: &Path,
    spec: &Spec,
    config: &CanonicalConfig,
    event: &str,
    applied: &[String],
) -> Result<()> {
    let dir = root.join(".macc/state/native-hooks");
    std::fs::create_dir_all(&dir).map_err(|e| super::super::error(e.to_string()))?;
    let receipt = json!({"revision":crate::config::transaction::revision(config)?,"event":event,"applied":applied,
        "observed_at":chrono::Utc::now().to_rfc3339()});
    let temp = dir.join(format!("{}.tmp", uuid::Uuid::new_v4()));
    private_write(
        &temp,
        &serde_json::to_vec(&receipt).map_err(|e| super::super::error(e.to_string()))?,
    )?;
    std::fs::rename(temp, dir.join(format!("{}.json", spec.tool)))
        .map_err(|e| super::super::error(e.to_string()))
}
