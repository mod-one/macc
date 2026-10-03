//! Apply ToolSpec settings only inside an isolated supervisor worktree.
use crate::tool::ModelConfigSpec;
use crate::{MaccError, Result};
use std::path::{Component, Path};

pub fn write(root: &Path, spec: &ModelConfigSpec, value: &str) -> Result<()> {
    if value.is_empty() {
        return Ok(());
    }
    let relative = Path::new(&spec.path);
    if relative.is_absolute()
        || relative
            .components()
            .any(|c| !matches!(c, Component::Normal(_)))
    {
        return Err(MaccError::Validation(
            "Supervisor tool settings must stay inside its worktree".into(),
        ));
    }
    let mut component_path = root.to_path_buf();
    for component in relative.components() {
        component_path.push(component);
        if component_path.is_symlink() {
            return Err(MaccError::Validation(
                "Supervisor refuses symlinked settings paths".into(),
            ));
        }
    }
    let path = root.join(relative);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| io(&path, e))?;
        if !parent
            .canonicalize()
            .map_err(|e| io(parent, e))?
            .starts_with(root.canonicalize().map_err(|e| io(root, e))?)
        {
            return Err(MaccError::Validation(
                "Supervisor settings directory escapes its worktree".into(),
            ));
        }
    }
    if path.is_symlink() {
        return Err(MaccError::Validation(
            "Supervisor refuses symlinked settings".into(),
        ));
    }
    let text = if path.exists() {
        std::fs::read_to_string(&path).map_err(|e| io(&path, e))?
    } else {
        String::new()
    };
    let invalid = |e: String| {
        MaccError::Validation(format!(
            "Invalid supervisor tool settings {}: {e}",
            path.display()
        ))
    };
    let output = match spec.format.as_str() {
        "toml" => {
            let mut document: toml::Table =
                toml::from_str(&text).map_err(|e| invalid(e.to_string()))?;
            document.insert(spec.key.clone(), toml::Value::String(value.into()));
            toml::to_string_pretty(&document).map_err(|e| invalid(e.to_string()))?
        }
        "json" => {
            let mut document: serde_json::Map<String, serde_json::Value> = if text.is_empty() {
                Default::default()
            } else {
                serde_json::from_str(&text).map_err(|e| invalid(e.to_string()))?
            };
            document.insert(spec.key.clone(), serde_json::Value::String(value.into()));
            serde_json::to_string_pretty(&document).map_err(|e| invalid(e.to_string()))?
        }
        other => return Err(invalid(format!("Unsupported format {other}"))),
    };
    std::fs::write(&path, output).map_err(|e| io(&path, e))
}
fn io(path: &Path, source: std::io::Error) -> MaccError {
    MaccError::Io {
        path: path.display().to_string(),
        action: "write supervisor tool settings".into(),
        source,
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn preserves_settings_and_rejects_escape() {
        let temp = tempfile::tempdir().unwrap();
        for format in ["toml", "json"] {
            let mut spec = ModelConfigSpec {
                path: format!(".tool/config.{format}"),
                format: format.into(),
                key: "model".into(),
            };
            write(temp.path(), &spec, "first").unwrap();
            spec.key = "effort".into();
            write(temp.path(), &spec, "high").unwrap();
            let text = std::fs::read_to_string(temp.path().join(&spec.path)).unwrap();
            assert!(text.contains("first") && text.contains("high"));
            spec.path = "../outside.json".into();
            assert!(write(temp.path(), &spec, "bad").is_err());
        }
    }
}
