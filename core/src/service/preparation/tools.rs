use super::command_check::{run, Failure, FailureKind};
use super::*;
use crate::doctor::{check_tool, SystemRunner, ToolStatus};
use crate::tool::{DoctorCheckKind, ToolSpec, ToolSpecLoader};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Requirement {
    pub spec: ToolSpec,
    pub install_needed: bool,
}
fn available(spec: &ToolSpec) -> bool {
    spec.performer.as_ref().is_some_and(|p| {
        matches!(
            check_tool(&SystemRunner, &DoctorCheckKind::Which, &p.command),
            ToolStatus::Installed
        )
    })
}
pub(super) fn requirements(
    paths: &ProjectPaths,
    config: &crate::config::CanonicalConfig,
) -> Result<Vec<Requirement>> {
    let (specs, diagnostics) =
        ToolSpecLoader::new(ToolSpecLoader::default_search_paths(&paths.root))
            .load_all_with_embedded();
    let mut selected = config.tools.enabled.clone();
    if let Some(tool) = config
        .automation
        .supervisor
        .as_ref()
        .and_then(|s| s.tool.as_ref())
    {
        selected.push(tool.clone());
    }
    if let Some(tool) = config
        .automation
        .coordinator
        .as_ref()
        .and_then(|s| s.coordinator_tool.as_ref())
    {
        selected.push(tool.clone());
    }
    if diagnostics
        .iter()
        .any(|d| d.path.starts_with(&paths.macc_dir))
    {
        return Err(MaccError::Validation(
            "Resolve invalid project ToolSpec before Apply".into(),
        ));
    }
    Ok(specs
        .into_iter()
        .filter(|s| selected.contains(&s.id))
        .map(|spec| {
            let install_needed = !available(&spec);
            Requirement {
                spec,
                install_needed,
            }
        })
        .collect())
}
pub(super) fn prepare(
    paths: &ProjectPaths,
    requirement: &Requirement,
    allow_setup: bool,
) -> Result<()> {
    let spec = &requirement.spec;
    if !available(spec) {
        if !requirement.install_needed {
            return Err(MaccError::Validation(format!(
                "{} became unavailable after review; refresh Apply",
                spec.id
            )));
        }
        let install = spec
            .install
            .as_ref()
            .filter(|i| !i.commands.is_empty())
            .ok_or_else(|| {
                MaccError::Validation(format!(
                    "{} is missing and its adapter has no automatic installer",
                    spec.id
                ))
            })?;
        let config = crate::load_canonical_config(&paths.config_path)?;
        if config.settings.offline {
            return Err(MaccError::Validation(format!(
                "{} installation needs network access; change Offline in Apply configuration",
                spec.id
            )));
        }
        for (i, command) in install.commands.iter().enumerate() {
            run(paths, command, &format!("{}-install-{i}", spec.id), 600)?;
        }
    }
    if !available(spec) {
        return Err(MaccError::Validation(format!(
            "{} is still unavailable after setup",
            spec.id
        )));
    }
    if let Some(version) = &spec.version_check {
        run(paths, &version.current, &format!("{}-verify", spec.id), 15)?;
    }
    if spec.performer.is_none() {
        return Err(MaccError::Validation(format!(
            "{} has no performer adapter",
            spec.id
        )));
    }
    if let Some(preparation) = spec.performer.as_ref().and_then(|p| p.preparation.as_ref()) {
        if let Err(failure) = verify(paths, &spec.id, preparation) {
            if failure.kind != FailureKind::Authentication || !allow_setup {
                return Err(failure.into());
            }
            let setup = preparation.setup.as_ref().ok_or_else(|| {
                MaccError::Validation(format!(
                    "{failure}; adapter {} has no automated account setup flow",
                    spec.id
                ))
            })?;
            if crate::load_canonical_config(&paths.config_path)?
                .settings
                .offline
            {
                return Err(MaccError::Validation(
                    "Account setup requires network access in Apply".into(),
                ));
            }
            run(paths, setup, &format!("{}-account", spec.id), 600)?;
            verify(paths, &spec.id, preparation)?;
        }
    }
    Ok(())
}
fn verify(
    paths: &ProjectPaths,
    id: &str,
    spec: &crate::tool::preparation::PreparationSpec,
) -> std::result::Result<(), Failure> {
    if !(1..=120).contains(&spec.timeout_seconds) {
        return Err(Failure::new(
            FailureKind::InvalidResponse,
            &paths.config_path,
            format!("{id} preparation timeout_seconds must be between 1 and 120"),
        ));
    }
    let path = run(
        paths,
        &spec.verify,
        &format!("{id}-account-check"),
        spec.timeout_seconds,
    )
    .map_err(|failure| failure.classify(spec))?;
    if spec.require_output
        && std::fs::read(&path)
            .map(|raw| raw.iter().all(u8::is_ascii_whitespace))
            .unwrap_or(true)
    {
        return Err(Failure::new(
            FailureKind::InvalidResponse,
            &path,
            format!("{id} readiness returned no output"),
        ));
    }
    if !spec.expected_json.is_empty() {
        let invalid = |reason: String| Failure::new(FailureKind::InvalidResponse, &path, reason);
        let raw = std::fs::read(&path).map_err(|e| invalid(e.to_string()))?;
        if raw.len() > 65536 {
            return Err(invalid("Readiness JSON exceeds bound".into()));
        }
        let start = raw.iter().position(|c| *c == b'{').unwrap_or(0);
        let value = serde_json::Deserializer::from_slice(&raw[start..])
            .into_iter::<serde_json::Value>()
            .next()
            .ok_or_else(|| invalid("Readiness JSON missing".into()))?
            .map_err(|e| invalid(e.to_string()))?;
        for (pointer, expected) in &spec.expected_json {
            if value.pointer(pointer) != Some(expected) {
                let authentication = spec
                    .authentication_json
                    .get(pointer)
                    .is_some_and(|denied| value.pointer(pointer) == Some(denied));
                return Err(Failure::new(
                    if authentication {
                        FailureKind::Authentication
                    } else {
                        FailureKind::InvalidResponse
                    },
                    &path,
                    format!("{id} readiness predicate {pointer} is not satisfied"),
                ));
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn malformed_and_empty_responses_are_not_account_failures() {
        let tmp = tempfile::tempdir().unwrap();
        let paths = ProjectPaths::from_root(tmp.path());
        for (script, extra) in [
            (
                "printf malformed",
                serde_json::json!({"expected_json":{"/ready":true}}),
            ),
            ("exit 0", serde_json::json!({"require_output":true})),
        ] {
            let mut value =
                serde_json::json!({"verify":{"command":"/bin/sh","args":["-c",script]}});
            value
                .as_object_mut()
                .unwrap()
                .extend(extra.as_object().unwrap().clone());
            let spec = serde_json::from_value(value).unwrap();
            let failure = verify(&paths, "fake", &spec).unwrap_err();
            assert_eq!(failure.kind, FailureKind::InvalidResponse);
            assert!(failure.path.is_file());
        }
    }
}
