//! Adapter-driven lifecycle composition for the single reviewed project apply.
use super::*;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Requirement {
    pub id: String,
    pub version: String,
    pub origin: String,
    pub operations: Vec<String>,
    pub network: bool,
}

pub fn requirements(policy: &super::super::policy::CodePolicy) -> Result<Vec<Requirement>> {
    registry()?
        .into_iter()
        .filter(|p| p.enabled(policy))
        .map(|p| {
            let descriptor = p.descriptor();
            let operations: Vec<_> = ["install", "deploy", "index"]
                .into_iter()
                .filter(|op| {
                    descriptor
                        .operations
                        .iter()
                        .any(|candidate| candidate == op)
                })
                .map(str::to_string)
                .collect();
            Ok(Requirement {
                network: operations.iter().any(|op| p.network_operation(op)),
                id: descriptor.id,
                version: descriptor.version,
                origin: descriptor.origin,
                operations,
            })
        })
        .collect()
}

pub fn execute(root: &Path, requirement: &Requirement) -> Result<()> {
    let policy = super::super::policy::load(root)?.code_intelligence;
    if !requirements(&policy)?.contains(requirement) {
        return Err(error(
            "Code provider requirement changed after apply review",
        ));
    }
    let adapter = provider(&requirement.id)?;
    let previous = {
        let (db, _) = readonly_database(root)?;
        receipt(&db, &requirement.id)?
    };
    let health = adapter.health(root, previous.as_ref(), &policy)?;
    if previous
        .as_ref()
        .is_some_and(|r| r.state == "healthy" && r.version == requirement.version)
        && health.status == "healthy"
        && (!adapter.requires_cached_snapshot()
            || previous.as_ref().is_some_and(|r| {
                routing::cached_compatible(root, root, &adapter.descriptor(), r, &policy)
                    .unwrap_or(false)
            }))
    {
        return Ok(());
    }
    for operation in &requirement.operations {
        // Existing managed runtimes are verified by their deploy action. A
        // version change uses the adapter's update protocol and pinned origin.
        if operation == "install"
            && health.status == "healthy"
            && previous
                .as_ref()
                .is_some_and(|r| r.version == requirement.version && r.state != "bundled")
        {
            continue;
        }
        let Capability::Supported(plan) = plan(root, &requirement.id, operation)? else {
            return Err(error(format!(
                "{} cannot prepare {operation}",
                requirement.id
            )));
        };
        apply(
            root,
            &plan.id,
            &digest(&serde_json::to_vec(&plan).map_err(|e| error(e.to_string()))?),
        )?;
    }
    verify(root, requirement)
}

pub fn verify(root: &Path, requirement: &Requirement) -> Result<()> {
    let policy = super::super::policy::load(root)?.code_intelligence;
    let adapter = provider(&requirement.id)?;
    let (db, _) = readonly_database(root)?;
    let state = receipt(&db, &requirement.id)?
        .ok_or_else(|| error("Provider deployment receipt missing"))?;
    drop(db);
    let healthy = adapter.health(root, Some(&state), &policy)?;
    if state.version != requirement.version
        || !["healthy", "deployed"].contains(&state.state.as_str())
        || healthy.status != "healthy"
    {
        return Err(error(format!(
            "Provider {} is not operational: {}",
            requirement.id,
            healthy.diagnostics.join("; ")
        )));
    }
    if adapter.requires_cached_snapshot()
        && !routing::cached_compatible(root, root, &adapter.descriptor(), &state, &policy)?
    {
        return Err(error(format!(
            "Provider {} index verification failed",
            requirement.id
        )));
    }
    Ok(())
}

/// Source edits after Apply only require a local cache refresh, never a new
/// installation or a network operation during coordinator startup.
pub fn ensure_current(root: &Path, requirement: &Requirement) -> Result<()> {
    if verify(root, requirement).is_ok() {
        return Ok(());
    }
    let adapter = provider(&requirement.id)?;
    let p = super::super::policy::load(root)?.code_intelligence;
    let (db, _) = readonly_database(root)?;
    let state = receipt(&db, &requirement.id)?;
    drop(db);
    let can_refresh = state.as_ref().is_some_and(|r| {
        r.version == requirement.version && ["healthy", "deployed"].contains(&r.state.as_str())
    }) && adapter.descriptor().operations.iter().any(|s| s == "index")
        && !adapter.network_operation("index")
        && adapter.health(root, state.as_ref(), &p)?.status == "healthy";
    if can_refresh {
        let Capability::Supported(plan) = plan(root, &requirement.id, "index")? else {
            return Err(error("Cache refresh unsupported"));
        };
        apply(
            root,
            &plan.id,
            &digest(&serde_json::to_vec(&plan).map_err(|e| error(e.to_string()))?),
        )?;
    }
    verify(root, requirement)
}
