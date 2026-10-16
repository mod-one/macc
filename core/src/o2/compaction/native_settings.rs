//! Own only the individual native setting MACC wrote; unrelated settings stay user-owned.
use super::*;
use crate::tool::compaction::NativeAuto;
pub(super) fn manage(
    db: &rusqlite::Connection,
    root: &Path,
    wt: &str,
    native: &NativeAuto,
    desired: Option<serde_json::Value>,
) -> Result<ExecutionOptions> {
    let wt = super::super::providers::native_worktree(root, Some(wt))?;
    let wt = wt.to_str().ok_or_else(|| error("Non-UTF8 worktree"))?;
    let relative = native
        .path
        .as_ref()
        .ok_or_else(|| error("Native configuration path required"))?;
    let mut path = Path::new(wt).to_path_buf();
    for component in Path::new(relative).components() {
        path.push(component);
        if std::fs::symlink_metadata(&path).is_ok_and(|m| m.file_type().is_symlink()) {
            return Err(error("Native configuration path symlink rejected"));
        }
    }
    let raw = if path.exists() {
        if std::fs::metadata(&path)
            .map_err(|e| error(e.to_string()))?
            .len()
            > 1048576
        {
            return Err(error("Native config too large"));
        }
        std::fs::read_to_string(&path).map_err(|e| error(e.to_string()))?
    } else {
        String::new()
    };
    let mut document = Document::parse(&native.format, &raw)?;
    let owned:Option<String>=sql(db.query_row("SELECT value_json FROM native_setting_ownership WHERE worktree=?1 AND path=?2 AND setting_key=?3",params![wt,relative,native.key],|r|r.get(0)).optional())?;
    let owned: Option<serde_json::Value> = owned.map(|v| store::json(v.as_bytes())).transpose()?;
    let keys = native.key.split('.').collect::<Vec<_>>();
    let current = document.current(&keys)?;
    if current.is_some() && current != owned {
        sql(db.execute(
            "DELETE FROM native_setting_ownership WHERE worktree=?1 AND path=?2 AND setting_key=?3",
            params![wt, relative, native.key],
        ))?;
        return Ok(ExecutionOptions {
            reason: "User native setting preserved; MACC does not own the current value".into(),
            ..Default::default()
        });
    }
    if desired.is_none() && owned.is_none() {
        return Ok(ExecutionOptions {
            reason: "No MACC-owned native setting to restore".into(),
            ..Default::default()
        });
    }
    document.set(&keys, desired.as_ref())?;
    let changed = current != desired;
    if changed {
        let raw = document.encode()?;
        store::write(root, &path, raw.as_bytes())?;
    }
    if let Some(value) = desired {
        sql(db.execute("INSERT INTO native_setting_ownership VALUES(?1,?2,?3,?4) ON CONFLICT(worktree,path,setting_key) DO UPDATE SET value_json=excluded.value_json",params![wt,relative,native.key,value.to_string()]))?;
    } else {
        sql(db.execute(
            "DELETE FROM native_setting_ownership WHERE worktree=?1 AND path=?2 AND setting_key=?3",
            params![wt, relative, native.key],
        ))?;
    }
    Ok(ExecutionOptions {
        applied: changed,
        reason: if document.current(&keys)?.is_some() {
            "MACC-owned native value matches current policy; compression remains unobserved"
        } else {
            "MACC-owned native override removed; provider/user defaults restored"
        }
        .into(),
        ..Default::default()
    })
}
// Keep TOML types (notably datetimes) intact when changing one unrelated key.
enum Document {
    Json(serde_json::Value),
    Toml(toml::Value),
}
impl Document {
    fn parse(format: &str, raw: &str) -> Result<Self> {
        if format == "toml" {
            Ok(Self::Toml(if raw.is_empty() {
                toml::Value::Table(Default::default())
            } else {
                toml::from_str(raw).map_err(|e| error(e.to_string()))?
            }))
        } else {
            Ok(Self::Json(if raw.is_empty() {
                serde_json::json!({})
            } else {
                serde_json::from_str(raw).map_err(|e| error(e.to_string()))?
            }))
        }
    }
    fn current(&self, keys: &[&str]) -> Result<Option<serde_json::Value>> {
        match self {
            Self::Json(root) => Ok(keys.iter().try_fold(root, |v, k| v.get(*k)).cloned()),
            Self::Toml(root) => keys
                .iter()
                .try_fold(root, |v, k| v.get(*k))
                .map(serde_json::to_value)
                .transpose()
                .map_err(|e| error(e.to_string())),
        }
    }
    fn set(&mut self, keys: &[&str], desired: Option<&serde_json::Value>) -> Result<()> {
        match self {
            Self::Json(root) => {
                let mut node = root;
                for k in &keys[..keys.len() - 1] {
                    node = node
                        .as_object_mut()
                        .ok_or_else(|| error("Native JSON object expected"))?
                        .entry(*k)
                        .or_insert(serde_json::json!({}));
                }
                let object = node
                    .as_object_mut()
                    .ok_or_else(|| error("Native JSON object expected"))?;
                if let Some(v) = desired {
                    object.insert(keys[keys.len() - 1].into(), v.clone());
                } else {
                    object.remove(keys[keys.len() - 1]);
                }
            }
            Self::Toml(root) => {
                let mut node = root;
                for k in &keys[..keys.len() - 1] {
                    node = node
                        .as_table_mut()
                        .ok_or_else(|| error("Native TOML table expected"))?
                        .entry(*k)
                        .or_insert(toml::Value::Table(Default::default()));
                }
                let object = node
                    .as_table_mut()
                    .ok_or_else(|| error("Native TOML table expected"))?;
                if let Some(v) = desired {
                    object.insert(
                        keys[keys.len() - 1].into(),
                        toml::Value::try_from(v).map_err(|e| error(e.to_string()))?,
                    );
                } else {
                    object.remove(keys[keys.len() - 1]);
                }
            }
        }
        Ok(())
    }
    fn encode(&self) -> Result<String> {
        match self {
            Self::Json(v) => serde_json::to_string_pretty(v).map_err(|e| error(e.to_string())),
            Self::Toml(v) => toml::to_string_pretty(v).map_err(|e| error(e.to_string())),
        }
    }
}
