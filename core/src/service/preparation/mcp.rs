use super::*;
use crate::tool::scoped_mcp::RegistrationSpec;
pub(super) fn cleanup_requirements(
    paths: &ProjectPaths,
    selected: &[tools::Requirement],
    enabled: bool,
) -> Result<Vec<RegistrationSpec>> {
    let specs = crate::tool::ToolSpecLoader::new(
        crate::tool::ToolSpecLoader::default_search_paths(&paths.root),
    )
    .load_all_with_embedded()
    .0;
    let mut result = vec![];
    for spec in specs {
        if enabled && selected.iter().any(|s| s.spec.id == spec.id) {
            continue;
        }
        if let Some(registration) = spec.performer.and_then(|p| p.scoped_mcp) {
            if registration.without_owned(&paths.root)?.is_some() {
                result.push(registration);
            }
        }
    }
    Ok(result)
}
pub(super) fn cleanup(
    paths: &ProjectPaths,
    registrations: &[RegistrationSpec],
    report: &mut Report,
) -> Result<()> {
    for registration in registrations {
        if let Some(content) = registration.without_owned(&paths.root)? {
            let path = paths.root.join(&registration.path);
            let before = std::fs::read(&path).map_err(|e| MaccError::Validation(e.to_string()))?;
            let backup = paths
                .backups_dir
                .join(format!("preparation-{}", uuid::Uuid::new_v4()))
                .join(&registration.path);
            crate::atomic_write(paths, &backup, &before)?;
            crate::atomic_write(paths, &path, &content)?;
            report.completed.push(format!(
                "Inactive scoped MCP registration removed from {}; backup {}",
                registration.path,
                backup.display()
            ));
        }
    }
    Ok(())
}
