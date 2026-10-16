//! A launcher hash alone cannot identify the runtime loaded behind that launcher.
use super::*;
pub(super) fn same_identity(
    command: &str,
    profile: &crate::tool::compaction::CompactionSpec,
    observed: &Probe,
) -> Result<bool> {
    let profile_hash = digest(&serde_json::to_vec(profile).map_err(|e| error(e.to_string()))?);
    let same_command = probe::executable(command)
        .ok()
        .is_some_and(|p| observed.command_path.as_deref() == p.to_str());
    Ok(observed.profile_sha256.as_deref() == Some(&profile_hash) && same_command)
}
pub(super) fn validate(
    root: &Path,
    command: &str,
    profile: &crate::tool::compaction::CompactionSpec,
    observed: &Probe,
) -> Result<()> {
    if observed.state != "version_help_observed" {
        return Err(error("Capability observation unavailable; probe again"));
    }
    let executable = probe::executable(command)?;
    if observed.executable_sha256 != Some(probe::binary_hash(&executable)?)
        || observed.profile_sha256
            != Some(digest(
                &serde_json::to_vec(profile).map_err(|e| error(e.to_string()))?,
            ))
    {
        return Err(error("Executable/profile changed; probe again"));
    }
    // Bounded, read-only --version also detects package updates behind stable wrappers.
    let version = probe::run(root, &executable, "--version")?;
    if !regex::Regex::new(&profile.version_pattern)
        .map_err(|e| error(e.to_string()))?
        .is_match(&version)
        || observed.version_sha256 != Some(digest(version.trim().as_bytes()))
    {
        return Err(error("Installed runtime version changed; probe again"));
    }
    Ok(())
}

pub(super) fn stale(observation: &mut Probe, reason: &str) {
    observation.state = "stale".into();
    observation.diagnostics.push(reason.into());
}
