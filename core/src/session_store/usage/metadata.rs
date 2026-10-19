//! Best-effort invocation identity, derived before taking the receipt transaction.
//! Never persist argv or infer a model from a tier that may not have been applied.
use super::*;
use serde_json::Value;
use std::{
    io::Read,
    path::{Component, Path},
};
#[derive(Default)]
pub(super) struct Metadata {
    pub model: Option<String>,
    pub version: Option<String>,
    pub notes: Vec<String>,
}
fn text(value: &str) -> Option<String> {
    let value = value.trim();
    (!value.is_empty()
        && value.chars().count() <= 160
        && value
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || "-._:/() ".contains(c))
        && crate::security::secret_scan::scan_bytes("usage_identity", value.as_bytes()).is_empty())
    .then(|| value.to_string())
}
fn argument_model(args: &[String]) -> Option<String> {
    let mut found = None;
    let mut i = 0;
    while i < args.len() {
        if args[i] == "--" {
            break;
        }
        if args[i] == "--model" || args[i] == "-m" {
            i += 1;
            found = args.get(i).and_then(|v| text(v));
        } else if let Some(v) = args[i].strip_prefix("--model=") {
            found = text(v);
        }
        i += 1;
    }
    found
}
fn config_model(identity: &Value) -> Option<String> {
    let cfg: crate::tool::ModelConfigSpec = serde_json::from_value(
        identity
            .pointer("/execution_config/performer/model_config")?
            .clone(),
    )
    .ok()?;
    let root = Path::new(identity["execution_root"].as_str()?);
    let relative = Path::new(&cfg.path);
    if relative.as_os_str().is_empty()
        || relative
            .components()
            .any(|c| !matches!(c, Component::Normal(_)))
    {
        return None;
    }
    let mut path = root.to_path_buf();
    for component in relative.components() {
        path.push(component);
        if std::fs::symlink_metadata(&path)
            .ok()?
            .file_type()
            .is_symlink()
        {
            return None;
        }
    }
    if !std::fs::metadata(&path).ok()?.is_file() {
        return None;
    }
    let mut raw = Vec::new();
    std::fs::File::open(&path)
        .ok()?
        .take(65537)
        .read_to_end(&mut raw)
        .ok()?;
    if raw.len() > 65536 {
        return None;
    }
    let value = match cfg.format.as_str() {
        "json" => serde_json::from_slice::<Value>(&raw)
            .ok()?
            .get(&cfg.key)?
            .as_str()?
            .to_string(),
        "toml" => std::str::from_utf8(&raw)
            .ok()?
            .parse::<toml::Value>()
            .ok()?
            .get(&cfg.key)?
            .as_str()?
            .to_string(),
        _ => return None,
    };
    text(&value)
}
pub(super) fn prepare(root: &Path, request: &BeginRequest) -> Result<Metadata> {
    let Some(invocation) = &request.invocation else {
        return Ok(Metadata::default());
    };
    // Snapshot identity under the normal lease fence, then release the lock
    // before config reads and bounded provider subprocesses.
    let identity: Value = {
        let _guard = lock(root)?;
        let (db, _) = db::active(root)?;
        if !ready(&db)? {
            return Ok(Metadata::default());
        }
        leases::verify(&db, &request.key)?;
        let exists: bool = sql(db.query_row(
            "SELECT EXISTS(SELECT 1 FROM invocation_usage WHERE invocation_id=?1)",
            [format!(
                "{}:{}",
                request.key.session_id, request.key.generation
            )],
            |r| r.get(0),
        ))?;
        if exists {
            return Ok(Metadata::default());
        }
        let raw: String = sql(db.query_row(
            "SELECT identity_json FROM logical_sessions WHERE id=?1",
            [&request.key.session_id],
            |r| r.get(0),
        ))?;
        json(raw.as_bytes())?
    };
    let performer = &identity["execution_config"]["performer"];
    let declared = [
        performer["command"].as_str(),
        performer
            .pointer("/session/create/command")
            .and_then(Value::as_str),
        performer
            .pointer("/session/resume/command")
            .and_then(Value::as_str),
    ];
    if !declared
        .into_iter()
        .flatten()
        .any(|c| c == invocation.command)
        || invocation.args.len() > 1024
        || invocation.args.iter().map(String::len).sum::<usize>() > 65536
    {
        return Ok(Metadata {
            notes: vec!["identity_invocation_unverified".into()],
            ..Default::default()
        });
    }
    let from_args = invocation
        .args
        .iter()
        .take_while(|a| a.as_str() != "--")
        .any(|a| a == "--model" || a == "-m" || a.starts_with("--model="));
    let model = if from_args {
        argument_model(&invocation.args)
    } else {
        config_model(&identity)
    };
    let mut notes = vec![match (&model, from_args) {
        (Some(_), true) => "identity_model_from_argv",
        (Some(_), false) => "identity_model_from_config",
        _ => "identity_model_unknown",
    }
    .into()];
    let profile = performer
        .get("compaction")
        .filter(|v| !v.is_null())
        .and_then(|v| {
            serde_json::from_value::<crate::tool::compaction::CompactionSpec>(v.clone()).ok()
        });
    let version = profile
        .and_then(|p| crate::o2::compaction::observe_version(root, &invocation.command, &p).ok())
        .and_then(|v| text(&v));
    notes.push(
        if version.is_some() {
            "identity_provider_version_verified"
        } else {
            "identity_provider_version_unknown"
        }
        .into(),
    );
    Ok(Metadata {
        model,
        version,
        notes,
    })
}

/// Preserve prepared identity when the provider omits model metadata. Structured
/// provider models (including multi-model aggregates) remain authoritative.
pub(super) fn finalize(
    record: &mut UsageRecord,
    observation: &mut Observation,
    same_session: bool,
) {
    let identity_notes = record
        .notes
        .iter()
        .filter(|n| n.starts_with("identity_"))
        .cloned()
        .collect::<Vec<_>>();
    record.notes = std::mem::take(&mut observation.notes);
    record.notes.extend(identity_notes);
    if same_session {
        if let Some(model) = observation.model.take() {
            record.model = Some(model);
            record.notes.retain(|n| n != "identity_model_unknown");
            record.notes.push("identity_model_from_provider".into());
        }
    }
}
#[cfg(all(test, unix))]
#[path = "metadata_tests.rs"]
mod tests;
