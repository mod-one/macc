//! Declarative project MCP configuration; capability values remain invocation-local.
use crate::{
    plan::{Action, ActionPlan, Scope},
    MaccError, ProjectPaths, Result,
};
use serde::{Deserialize, Serialize};

pub const SERVER: &str = "macc-o2";
pub const ENVIRONMENT: [&str; 4] = [
    "MACC_O2_PROJECT_ROOT",
    "MACC_O2_MCP_SESSION",
    "MACC_O2_MCP_GENERATION",
    "MACC_O2_MCP_TOKEN",
];

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RegistrationSpec {
    pub path: String,
    pub format: String,
    pub servers_key: String,
    /// Empty means the MCP client inherits its parent's environment.
    #[serde(default)]
    pub environment_allowlist_key: Option<String>,
}

impl RegistrationSpec {
    pub fn validate(&self) -> Result<()> {
        if !["json", "toml"].contains(&self.format.as_str())
            || self.path.is_empty()
            || self.servers_key.is_empty()
            || std::path::Path::new(&self.path)
                .components()
                .any(|c| !matches!(c, std::path::Component::Normal(_)))
            || self
                .environment_allowlist_key
                .as_ref()
                .is_some_and(|s| s.is_empty())
        {
            return Err(MaccError::Validation(
                "Invalid scoped MCP registration specification".into(),
            ));
        }
        Ok(())
    }
    pub(crate) fn server(&self) -> serde_json::Value {
        let command = std::env::current_exe()
            .ok()
            .filter(|p| p.file_name().is_some_and(|s| s == "macc"))
            .map(|p| p.display().to_string())
            .unwrap_or_else(|| "macc".into());
        let mut value = serde_json::json!({"command": command,"args":["o2","mcp"]});
        if let Some(key) = &self.environment_allowlist_key {
            value[key] = serde_json::json!(ENVIRONMENT);
        }
        value
    }
    pub fn configure(&self, paths: &ProjectPaths, plan: &mut ActionPlan) -> Result<()> {
        self.validate()?;
        let generated = plan.actions.iter().rev().find_map(|a| match a {
            Action::WriteFile { path, content, .. } if path == &self.path => Some(content.clone()),
            _ => None,
        });
        let bytes = match generated {
            Some(bytes) => bytes,
            None => match std::fs::read(paths.root.join(&self.path)) {
                Ok(bytes) => bytes,
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => vec![],
                Err(source) => {
                    return Err(MaccError::Io {
                        path: self.path.clone(),
                        action: "read MCP config".into(),
                        source,
                    })
                }
            },
        };
        let content = if self.format == "json" {
            let mut document: serde_json::Value = if bytes.is_empty() {
                serde_json::json!({})
            } else {
                serde_json::from_slice(&bytes).map_err(invalid)?
            };
            if !document.is_object() {
                return Err(invalid("MCP config must be an object"));
            }
            if document.get(&self.servers_key).is_none() {
                document[&self.servers_key] = serde_json::json!({});
            }
            if !document[&self.servers_key].is_object() {
                return Err(invalid("MCP server map must be an object"));
            }
            document[&self.servers_key][SERVER] = self.server();
            serde_json::to_vec_pretty(&document).map_err(invalid)?
        } else {
            let mut document: toml::Value = if bytes.is_empty() {
                toml::Value::Table(Default::default())
            } else {
                toml::from_str(&String::from_utf8(bytes).map_err(invalid)?).map_err(invalid)?
            };
            let table = document
                .as_table_mut()
                .ok_or_else(|| invalid("MCP config must be a table"))?;
            let servers = table
                .entry(self.servers_key.clone())
                .or_insert_with(|| toml::Value::Table(Default::default()))
                .as_table_mut()
                .ok_or_else(|| invalid("MCP server map must be a table"))?;
            servers.insert(
                SERVER.into(),
                toml::Value::try_from(self.server()).map_err(invalid)?,
            );
            toml::to_string_pretty(&document)
                .map_err(invalid)?
                .into_bytes()
        };
        plan.actions
            .retain(|a| !matches!(a,Action::WriteFile{path,..} if path == &self.path));
        plan.add_action(Action::WriteFile {
            path: self.path.clone(),
            content,
            scope: Scope::Project,
        });
        Ok(())
    }
    pub fn verify(&self, root: &std::path::Path) -> Result<()> {
        self.validate()?;
        let raw = std::fs::read_to_string(root.join(&self.path)).map_err(invalid)?;
        let document: serde_json::Value = if self.format == "json" {
            serde_json::from_str(&raw).map_err(invalid)?
        } else {
            serde_json::to_value(toml::from_str::<toml::Value>(&raw).map_err(invalid)?)
                .map_err(invalid)?
        };
        if document[&self.servers_key][SERVER] != self.server() {
            return Err(invalid(
                "Scoped MCP registration differs from reviewed configuration",
            ));
        }
        Ok(())
    }
    pub fn without_owned(&self, root: &std::path::Path) -> Result<Option<Vec<u8>>> {
        self.validate()?;
        let path = root.join(&self.path);
        if !path.exists() {
            return Ok(None);
        }
        let raw = std::fs::read_to_string(path).map_err(invalid)?;
        if raw.len() > 1048576 {
            return Err(invalid("MCP configuration exceeds 1 MiB"));
        }
        let owned = |server: &serde_json::Value| {
            server["args"] == serde_json::json!(["o2", "mcp"])
                && server["command"].as_str().is_some_and(|c| {
                    std::path::Path::new(c)
                        .file_name()
                        .is_some_and(|name| name == "macc")
                })
        };
        if self.format == "json" {
            let mut value: serde_json::Value = serde_json::from_str(&raw).map_err(invalid)?;
            if !owned(&value[&self.servers_key][SERVER]) {
                return Ok(None);
            }
            value[&self.servers_key]
                .as_object_mut()
                .ok_or_else(|| invalid("Invalid MCP map"))?
                .remove(SERVER);
            return serde_json::to_vec_pretty(&value).map(Some).map_err(invalid);
        }
        let mut value: toml::Value = toml::from_str(&raw).map_err(invalid)?;
        let Some(map) = value
            .get_mut(&self.servers_key)
            .and_then(toml::Value::as_table_mut)
        else {
            return Ok(None);
        };
        if !map
            .get(SERVER)
            .map(serde_json::to_value)
            .transpose()
            .map_err(invalid)?
            .as_ref()
            .is_some_and(owned)
        {
            return Ok(None);
        }
        map.remove(SERVER);
        toml::to_string_pretty(&value)
            .map(|s| Some(s.into_bytes()))
            .map_err(invalid)
    }
}
fn invalid(e: impl std::fmt::Display) -> MaccError {
    MaccError::Validation(format!("Scoped MCP setup: {e}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn project_registration_preserves_toml_types_other_servers_and_runtime_secret_inheritance() {
        let tmp = tempfile::tempdir().unwrap();
        let paths = ProjectPaths::from_root(tmp.path());
        let spec = RegistrationSpec {
            path: ".future/config.toml".into(),
            format: "toml".into(),
            servers_key: "mcp_servers".into(),
            environment_allowlist_key: Some("env_vars".into()),
        };
        std::fs::create_dir_all(tmp.path().join(".future")).unwrap();
        std::fs::write(
            tmp.path().join(&spec.path),
            "date = 2026-10-19T12:00:00Z\n[mcp_servers.other]\ncommand='other'\n",
        )
        .unwrap();
        let mut plan = ActionPlan::new();
        spec.configure(&paths, &mut plan).unwrap();
        let operations = crate::plan::collect_plan_operations(&paths, &plan);
        crate::apply_operations(&paths, &operations, false, |_, _, _| {}).unwrap();
        spec.verify(tmp.path()).unwrap();
        let raw = std::fs::read_to_string(tmp.path().join(&spec.path)).unwrap();
        let parsed: toml::Value = toml::from_str(&raw).unwrap();
        assert!(parsed["date"].is_datetime());
        assert_eq!(
            parsed["mcp_servers"]["other"]["command"].as_str(),
            Some("other")
        );
        assert!(raw.contains("MACC_O2_MCP_TOKEN"));
        assert!(!raw.contains("lease_token"));
        assert_eq!(
            parsed["mcp_servers"][SERVER]["env_vars"]
                .as_array()
                .unwrap()
                .len(),
            4
        );
    }
    #[test]
    fn malformed_or_escaping_registration_is_rejected_before_writes() {
        let tmp = tempfile::tempdir().unwrap();
        let paths = ProjectPaths::from_root(tmp.path());
        let spec = RegistrationSpec {
            path: "../config.json".into(),
            format: "json".into(),
            servers_key: "mcpServers".into(),
            environment_allowlist_key: None,
        };
        assert!(spec.configure(&paths, &mut ActionPlan::new()).is_err());
    }
}
