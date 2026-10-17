//! Provider-neutral identity observations reused by memory freshness.
use super::*;
pub(crate) fn fingerprint(p: &Probe) -> Result<String> {
    if p.state != "version_help_observed"
        || p.version_sha256.is_none()
        || p.executable_sha256.is_none()
        || p.help_sha256.is_none()
    {
        return Err(error(
            "Provider evidence requires a successful explicit version/help probe",
        ));
    }
    Ok(digest(
        &serde_json::to_vec(&(
            &p.tool_id,
            &p.command_path,
            &p.executable_sha256,
            &p.profile_sha256,
            &p.version_sha256,
            &p.help_sha256,
        ))
        .map_err(|e| error(e.to_string()))?,
    ))
}
pub(crate) fn identity(root: &Path, db: &rusqlite::Connection, tool: &str) -> Result<Probe> {
    let raw: String = sql(db.query_row(
        "SELECT probe_json FROM compaction_probes WHERE tool_id=?1",
        [tool],
        |r| r.get(0),
    ))?;
    let observed: Probe = store::json(raw.as_bytes())?;
    if observed.tool_id != tool {
        return Err(error("Provider observation identity mismatch"));
    }
    let performer = specs(root)
        .into_iter()
        .find(|s| s.id == tool)
        .and_then(|s| s.performer)
        .ok_or_else(|| error("Registered performer missing"))?;
    let profile = performer
        .compaction
        .as_ref()
        .ok_or_else(|| error("Adapter has no verified version profile"))?;
    if !freshness::same_identity(&performer.command, profile, &observed)? {
        return Err(error("Provider command/profile changed; probe again"));
    }
    fingerprint(&observed)?;
    Ok(observed)
}

pub(crate) fn current(root: &Path, db: &rusqlite::Connection, tool: &str) -> Result<Probe> {
    let observed = identity(root, db, tool)?;
    let performer = specs(root)
        .into_iter()
        .find(|s| s.id == tool)
        .and_then(|s| s.performer)
        .ok_or_else(|| error("Registered performer missing"))?;
    let profile = performer
        .compaction
        .as_ref()
        .ok_or_else(|| error("Adapter version profile unavailable"))?;
    freshness::validate(root, &performer.command, profile, &observed)?;
    Ok(observed)
}
