//! Read-only doctor integration; diagnosis never installs or repairs a provider.
use super::*;
use crate::doctor::DiagnosticFinding;
pub(crate) fn findings(root: &Path) -> Vec<DiagnosticFinding> {
    let check = || -> Result<Vec<DiagnosticFinding>> {
        let p = policy::load(root)?;
        if !p.memory.enabled
            && !p.context_engine.enabled
            && !p.adaptive_compaction.enabled
            && !p.code_intelligence.enabled
        {
            return Ok(vec![]);
        }
        let view = policy::inspect(root)?;
        if !view.ready {
            return Ok(vec![DiagnosticFinding::warning("MACC-O2-UPGRADE","tools","O2 SQLite schema is unavailable","Enabled O2 features require the reviewed SQLite session authority and O2 schema upgrade.",Some("Open Sessions/O2 migration preview or run `macc o2 status`."))]);
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
