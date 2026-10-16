//! Code intelligence port and owned, reviewed lifecycle; external adapters register here.
mod lifecycle;
mod model;
mod native;
mod native_lifecycle;
mod routing;
use super::*;
pub use lifecycle::{apply, inspect, plan};
pub use model::*;
pub(crate) use native::worktree as native_worktree;
use rusqlite::OptionalExtension;
pub trait CodeIntelligenceProvider: Send + Sync {
    fn descriptor(&self) -> Descriptor;
    fn plan_paths(
        &self,
        action: &str,
        _plan_id: &str,
        _previous: &Option<Receipt>,
    ) -> Result<Capability<Vec<String>>> {
        Ok(Capability::Unsupported {
            capability: action.into(),
            reason: "Adapter has no verified lifecycle implementation".into(),
        })
    }
    fn execute_lifecycle(
        &self,
        _root: &Path,
        plan: &Plan,
        _previous: &Option<Receipt>,
        _policy: &super::policy::CodePolicy,
    ) -> Result<Capability<Receipt>> {
        Ok(Capability::Unsupported {
            capability: plan.action.clone(),
            reason: "Adapter has no verified lifecycle implementation".into(),
        })
    }

    fn detect(&self, root: &Path) -> Result<Detection>;
    fn query(
        &self,
        root: &Path,
        q: &Query,
        policy: &super::policy::CodePolicy,
    ) -> Result<Capability<QueryResult>>;
}
pub struct Registration {
    pub create: fn() -> Box<dyn CodeIntelligenceProvider>,
}
inventory::collect!(Registration);
fn registry() -> Result<Vec<Box<dyn CodeIntelligenceProvider>>> {
    let mut providers: Vec<Box<dyn CodeIntelligenceProvider>> = vec![Box::new(native::Native)];
    providers.extend(
        inventory::iter::<Registration>
            .into_iter()
            .map(|r| (r.create)()),
    );
    let mut ids = std::collections::BTreeSet::new();
    for adapter in &providers {
        let id = adapter.descriptor().id;
        if id.is_empty()
            || id.len() > 64
            || !id
                .bytes()
                .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
            || !ids.insert(id)
        {
            return Err(error("Invalid or duplicate code intelligence provider ID"));
        }
    }
    Ok(providers)
}
fn provider(id: &str) -> Result<Box<dyn CodeIntelligenceProvider>> {
    registry()?
        .into_iter()
        .find(|p| p.descriptor().id == id)
        .ok_or_else(|| {
            error(format!(
                "Unknown code intelligence provider {id}; no automatic installation"
            ))
        })
}
pub fn query(root: &Path, q: &Query) -> Result<Capability<QueryResult>> {
    let p = super::policy::load(root)?;
    if !p.code_intelligence.enabled {
        return Err(error("Code Intelligence disabled"));
    }
    paths::safe_text(&q.text)?;
    if q.text.trim().is_empty() || q.text.len() > 256 {
        return Err(error("Bounded nonempty literal query required"));
    }
    let receipts = {
        let _guard = store::lock(root)?;
        let (db, _) = database(root)?;
        registry()?
            .iter()
            .map(|adapter| {
                let id = adapter.descriptor().id;
                Ok((id.clone(), receipt(&db, &id)?))
            })
            .collect::<Result<std::collections::BTreeMap<_, _>>>()?
    };
    let mut adapters = if p.code_intelligence.routing == "adaptive" {
        registry()?
    } else {
        vec![provider("native")?]
    };
    adapters.sort_by_key(|p| p.descriptor().id == "native");
    let mut diagnostics = Vec::new();
    for adapter in adapters {
        let descriptor = adapter.descriptor();
        let state = receipts.get(&descriptor.id).and_then(|r| r.as_ref());
        if state.is_some_and(|r| r.state == "disabled") {
            diagnostics.push(format!("{} disabled by operator", descriptor.id));
            continue;
        }
        if descriptor.id != "native" && !state.is_some_and(|r| r.state == "healthy") {
            diagnostics.push(format!("{} not deployed/healthy", descriptor.id));
            continue;
        }
        if descriptor.id != "native" {
            let target = native::worktree(root, q.worktree.as_deref())?;
            if !routing::cached_compatible(
                root,
                &target,
                &descriptor,
                state.unwrap(),
                &p.code_intelligence,
            )? {
                diagnostics.push(format!(
                    "{} receipt/index is stale or incompatible; fallback retained",
                    descriptor.id
                ));
                continue;
            }
        }
        if !adapter.detect(root).is_ok_and(|d| d.status == "healthy") {
            diagnostics.push(format!("{} unavailable; fallback retained", descriptor.id));
            continue;
        }
        match adapter.query(root, q, &p.code_intelligence) {
            Ok(Capability::Supported(mut result)) => {
                result.diagnostics.extend(diagnostics);
                return Ok(Capability::Supported(result));
            }
            Ok(Capability::Unsupported { reason, .. }) => diagnostics.push(reason),
            Err(e) if descriptor.id != "native" => {
                diagnostics.push(format!("{} unavailable: {e}", descriptor.id))
            }
            Err(e) => return Err(e),
        }
    }
    Ok(Capability::Unsupported {
        capability: q.kind.clone(),
        reason: diagnostics.join("; "),
    })
}
pub(super) fn receipt(db: &rusqlite::Connection, id: &str) -> Result<Option<Receipt>> {
    let raw: Option<String> = sql(db
        .query_row(
            "SELECT receipt_json FROM provider_receipts WHERE provider_id=?1",
            [id],
            |r| r.get(0),
        )
        .optional())?;
    raw.map(|s| store::json(s.as_bytes())).transpose()
}
pub(super) fn checksum(receipt: &Option<Receipt>) -> Result<String> {
    Ok(digest(
        serde_json::to_string(receipt)
            .map_err(|e| error(e.to_string()))?
            .as_bytes(),
    ))
}
