//! Read-only doctor integration; diagnosis never installs or repairs a provider.
use super::*;
use crate::doctor::DiagnosticFinding;
pub(crate) fn findings(root: &Path) -> Vec<DiagnosticFinding> {
    let check = || -> Result<Vec<DiagnosticFinding>> {
        let p = policy::load(root)?;
        let paths = crate::ProjectPaths::from_root(root);
        let config = if paths.config_path.exists() {
            crate::load_canonical_config(&paths.config_path)?
        } else {
            Default::default()
        };
        if !activation::requested(&config) {
            return Ok(vec![]);
        }
        if let Err(error) = activation::require_ready(root) {
            return Ok(vec![DiagnosticFinding::error("MACC-O2-UPGRADE","tools","O2 configured but not operational",&error.to_string(),Some("Run `macc apply` and confirm the SQLite/O2 activation preview, or restore a saved O2 session bundle before running."), false)]);
        }
        if let Err(error) = crate::service::preparation::verify_enabled(root) {
            return Ok(vec![DiagnosticFinding::error("MACC-O2-PREPARATION", "tools", "Enabled O2 features need preparation", &error.to_string(), Some("Complete `macc apply` in CLI, TUI or Web; resolve the questions in its preparation assistant."), false)]);
        }
        if !p.code_intelligence.enabled {
            return Ok(vec![]);
        }
        Ok(providers::inspect(root)?.providers.into_iter().map(|d| {
            if d.status == "healthy" || d.status == "disabled" {
                DiagnosticFinding::ok(&format!("MACC-O2-PROVIDER-{}",d.provider.id),"tools",&format!("Code provider {}: {}",d.provider.id,d.status))
            } else {
                DiagnosticFinding::warning(&format!("MACC-O2-PROVIDER-{}",d.provider.id),"tools",&format!("Code provider {}: {}",d.provider.id,d.status),&d.diagnostics.join("; "),Some("Inspect `macc o2 providers`; review a rebuild/update plan before applying."))
            }
        }).collect())
    };
    check().unwrap_or_else(|e| {
        vec![DiagnosticFinding::warning(
            "MACC-O2-INSPECTION",
            "tools",
            "O2 inspection failed",
            &e.to_string(),
            Some("Inspect `macc o2 status` and the O2 client diagnostics."),
        )]
    })
}
