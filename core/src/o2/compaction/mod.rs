//! Native auto configuration is adapter-declared; explicit headless compaction is never guessed.
mod options;
mod pressure;
pub use pressure::states;
pub(crate) use pressure::{evaluate as evaluate_pressure, observe as observe_pressure};
mod probe;
use super::*;
pub use options::{execution_options, ExecutionOptions};
use rusqlite::{params, OptionalExtension};
use serde::{Deserialize, Serialize};
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Probe {
    pub tool_id: String,
    pub command_path: Option<String>,
    pub executable_sha256: Option<String>,
    pub profile_sha256: Option<String>,
    pub version: Option<String>,
    pub help_sha256: Option<String>,
    pub state: String,
    pub native_auto: String,
    pub explicit_headless: String,
    pub verified_at: i64,
    pub diagnostics: Vec<String>,
}
#[derive(Debug, Serialize)]
pub struct Decision {
    pub action: String,
    pub reason: String,
    pub pressure: Option<f64>,
    pub quality: String,
}
pub fn decision(
    p: &super::policy::CompactionPolicy,
    uses: u64,
    budget: u64,
    context_tokens: Option<u64>,
    window: Option<u64>,
    in_flight: bool,
) -> Decision {
    let pressure = context_tokens
        .zip(window)
        .filter(|(_, w)| *w > 0)
        .map(|(n, w)| n as f64 / w as f64);
    let (action, reason) = if in_flight {
        ("defer", "A provider invocation is in flight")
    } else if !p.enabled {
        (
            "provider_default",
            "MACC adaptive policy is disabled; P0 invocation rotation remains enforced",
        )
    } else if uses >= budget {
        (
            "checkpoint_rotate",
            "Bounded invocation fallback reached; no assumption about token pressure",
        )
    } else if pressure
        .is_some_and(|v| v >= p.guard_ratio || (v >= p.action_ratio && uses >= p.min_turns))
    {
        ("checkpoint_rotate","Verified context pressure requires safe rotation; explicit headless compact is unsupported")
    } else if pressure.is_some_and(|v| v >= p.warning_ratio) {
        (
            "checkpoint",
            "Pressure warning: preserve current checkpoint",
        )
    } else {
        (
            "continue",
            "Pressure is unknown/below threshold; retain scoped session and bounded fallback",
        )
    };
    Decision {
        action: action.into(),
        reason: reason.into(),
        pressure,
        quality: if pressure.is_some() {
            "explicit context/window observations; not quota or cumulative token totals"
        } else {
            "unknown"
        }
        .into(),
    }
}
pub fn specs(root: &Path) -> Vec<crate::tool::ToolSpec> {
    crate::tool::ToolSpecLoader::new(crate::tool::ToolSpecLoader::default_search_paths(root))
        .load_all_with_embedded()
        .0
}
pub fn inspect(root: &Path) -> Result<Vec<Probe>> {
    let _guard = store::lock(root)?;
    let (db, _) = database(root)?;
    let mut result = Vec::new();
    for spec in specs(root) {
        if let Some(p) = spec.performer {
            let existing: Option<String> = sql(db
                .query_row(
                    "SELECT probe_json FROM compaction_probes WHERE tool_id=?1",
                    [&spec.id],
                    |r| r.get(0),
                )
                .optional())?;
            let mut observation = existing
                .map(|s| store::json::<Probe>(s.as_bytes()))
                .transpose()?
                .unwrap_or(Probe {
                    tool_id: spec.id,
                    command_path: None,
                    executable_sha256: None,
                    profile_sha256: None,
                    version: None,
                    help_sha256: None,
                    state: "unverified".into(),
                    native_auto: if p
                        .compaction
                        .as_ref()
                        .is_some_and(|p| p.native_auto.is_some())
                    {
                        "declared; not probed"
                    } else {
                        "unsupported; checkpoint rotation"
                    }
                    .into(),
                    explicit_headless: "unsupported; checkpoint rotation".into(),
                    verified_at: 0,
                    diagnostics: vec![],
                });
            let current = p
                .compaction
                .as_ref()
                .map(|s| serde_json::to_vec(s).map(|b| digest(&b)))
                .transpose()
                .map_err(|e| error(e.to_string()))?;
            if observation.profile_sha256.is_some() && observation.profile_sha256 != current {
                observation.state = "stale".into();
                observation
                    .diagnostics
                    .push("Adapter declaration changed; probe again".into());
            }
            result.push(observation);
        }
    }
    Ok(result)
}
pub fn probe(root: &Path, tool_id: &str) -> Result<Probe> {
    let spec = specs(root)
        .into_iter()
        .find(|s| s.id == tool_id)
        .ok_or_else(|| error("Unknown registered tool"))?;
    let performer = spec
        .performer
        .ok_or_else(|| error("Tool has no performer"))?;
    probe_spec(root, tool_id, &performer)
}
pub(crate) fn probe_spec(
    root: &Path,
    tool_id: &str,
    p: &crate::tool::ToolPerformerSpec,
) -> Result<Probe> {
    let mut record = Probe {
        tool_id: tool_id.into(),
        command_path: None,
        executable_sha256: None,
        profile_sha256: None,
        version: None,
        help_sha256: None,
        state: "unverified".into(),
        native_auto: "unsupported; checkpoint rotation".into(),
        explicit_headless: "unsupported; checkpoint rotation".into(),
        verified_at: now(),
        diagnostics: vec![],
    };
    let result = (|| {
        let profile = p
            .compaction
            .as_ref()
            .ok_or_else(|| error("Adapter has no compaction declaration"))?;
        profile.validate()?;
        let executable = probe::executable(&p.command)?;
        record.command_path = Some(executable.display().to_string());
        record.executable_sha256 = Some(probe::binary_hash(&executable)?);
        record.profile_sha256 = Some(digest(
            &serde_json::to_vec(profile).map_err(|e| error(e.to_string()))?,
        ));
        let version = probe::run(root, &executable, "--version")?;
        let help = probe::run(root, &executable, "--help")?;
        if !regex::Regex::new(&profile.version_pattern)
            .map_err(|e| error(e.to_string()))?
            .is_match(&version)
        {
            return Err(error(
                "Installed version output does not match adapter declaration",
            ));
        }
        record.version = Some(version.trim().chars().take(160).collect());
        record.help_sha256 = Some(digest(help.as_bytes()));
        record.state = "version_help_observed".into();
        record.native_auto = if profile.native_auto.is_some() {
            "documented configuration; native compaction execution not observed"
        } else {
            "unsupported; checkpoint rotation"
        }
        .into();
        record.diagnostics.push("Read-only version/help probe; this does not certify successful compaction or headless /compact".into());
        Ok(())
    })();
    if let Err(e) = result {
        record.state = "unavailable".into();
        record.diagnostics.push(e.to_string());
    }
    let _guard = store::lock(root)?;
    let (mut db, _) = database(root)?;
    let tx = sql(db.transaction())?;
    sql(tx.execute("INSERT INTO compaction_probes VALUES(?1,?2) ON CONFLICT(tool_id) DO UPDATE SET probe_json=excluded.probe_json",params![tool_id,serde_json::to_string(&record).map_err(|e|error(e.to_string()))?]))?;
    super::schema::audit(
        &tx,
        "compaction_probe",
        tool_id,
        serde_json::json!({"state":record.state,"profile":record.profile_sha256}),
    )?;
    sql(tx.commit())?;
    Ok(record)
}

mod native_settings;

pub mod adoption;
