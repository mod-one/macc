//! Code intelligence port and owned, reviewed lifecycle; external adapters register here.
mod compare;
#[cfg(test)]
mod filesystem_guard_tests;
mod generation;
mod graphify;
mod lifecycle;
mod model;
mod native;
mod native_lifecycle;
mod operations;
mod process;
mod response;
mod routing;
mod serena;
mod snapshot;
mod symbols;
use super::*;
pub use compare::compare;
pub use lifecycle::{inspect, plan};
pub use operations::operation;
pub use snapshot::{source_snapshot, SourceSnapshot};
pub use symbols::{fingerprint, sources as evidence_sources, SymbolFingerprint};
mod apply;
pub use apply::apply;
pub use model::*;
pub(crate) use native::worktree as native_worktree;
use rusqlite::OptionalExtension;
pub trait CodeIntelligenceProvider: Send + Sync {
    fn descriptor(&self) -> Descriptor;
    fn symbol_languages(&self) -> Vec<String> {
        vec![]
    }
    /// Read-only semantic fingerprint, distinct from literal search or graph inference.
    fn symbol_fingerprint(
        &self,
        _root: &Path,
        _target: &Path,
        _path: &str,
        _symbol: &str,
    ) -> Result<Capability<SymbolFingerprint>> {
        Ok(Capability::Unsupported {
            capability: "symbol_fingerprint".into(),
            reason: "This adapter has no verified symbol fingerprint backend".into(),
        })
    }
    /// Live LSP providers validate an exported current snapshot per query.
    fn requires_cached_snapshot(&self) -> bool {
        true
    }
    fn network_operation(&self, _action: &str) -> bool {
        !self.descriptor().local_only
    }
    fn plan_details(
        &self,
        _root: &Path,
        _action: &str,
        _policy: &super::policy::CodePolicy,
    ) -> Result<serde_json::Value> {
        Ok(serde_json::json!({}))
    }
    fn priority(&self, query: &Query) -> u32 {
        if query.kind == "file_search" {
            100
        } else {
            10
        }
    }
    fn health(
        &self,
        root: &Path,
        _receipt: Option<&Receipt>,
        _policy: &super::policy::CodePolicy,
    ) -> Result<Detection> {
        self.detect(root)
    }
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
    query_selected(root, q, None)
}
fn query_selected(
    root: &Path,
    q: &Query,
    selection: Option<&str>,
) -> Result<Capability<QueryResult>> {
    let p = super::policy::load(root)?;
    if !p.code_intelligence.enabled {
        return Err(error("Code Intelligence disabled"));
    }
    paths::safe_text(&q.text)?;
    if let Some(path) = &q.path {
        if path.len() > 4096 {
            return Err(error("Code query path exceeds bound"));
        }
        let target = native_worktree(root, q.worktree.as_deref())?;
        paths::source(&target, path, p.code_intelligence.max_file_bytes)?;
    }
    if q.text.trim().is_empty() || q.text.len() > 256 {
        return Err(error("Bounded nonempty literal query required"));
    }
    let mut adapters = if let Some(id) = selection {
        vec![provider(id)?]
    } else if p.code_intelligence.routing == "adaptive" {
        registry()?
    } else {
        vec![provider("native")?]
    };
    adapters.sort_by_key(|p| (p.priority(q), p.descriptor().id));
    let mut diagnostics = Vec::new();
    for adapter in adapters {
        let descriptor = adapter.descriptor();
        let lock_path = lifecycle::owned_path(
            root,
            &descriptor.id,
            &format!(".macc/code-intelligence/{}/operation.lock", descriptor.id),
        )?;
        std::fs::create_dir_all(
            lock_path
                .parent()
                .ok_or_else(|| error("Missing provider lock directory"))?,
        )
        .map_err(|e| error(e.to_string()))?;
        let Some(_operation) =
            crate::fs_lock::AdvisoryLock::try_acquire(&lock_path, "code provider")?
        else {
            diagnostics.push(format!("{} busy; native fallback retained", descriptor.id));
            continue;
        };
        // Read after acquiring the provider lock: lifecycle may have completed
        // since the query began (including disable/uninstall).
        let current = {
            let (db, _) = readonly_database(root)?;
            receipt(&db, &descriptor.id)?
        };
        let state = current.as_ref();
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
            if state.unwrap().version != descriptor.version
                || (adapter.requires_cached_snapshot()
                    && !routing::cached_compatible(
                        root,
                        &target,
                        &descriptor,
                        state.unwrap(),
                        &p.code_intelligence,
                    )?)
            {
                diagnostics.push(format!(
                    "{} receipt/index is stale or incompatible; fallback retained",
                    descriptor.id
                ));
                continue;
            }
        }
        // Eligibility is cheap; the adapter validates its runtime before execution.
        // Full health inspection belongs to the explicit providers view.
        match adapter.detect(root) {
            Ok(d) if d.status == "healthy" => {}
            Ok(d) => {
                diagnostics.push(format!(
                    "{} {}: {}; native fallback retained",
                    descriptor.id,
                    d.status,
                    d.diagnostics.join("; ")
                ));
                continue;
            }
            Err(e) => {
                diagnostics.push(format!(
                    "{} health check failed: {e}; native fallback retained",
                    descriptor.id
                ));
                continue;
            }
        }
        match adapter.query(root, q, &p.code_intelligence) {
            Ok(Capability::Supported(mut result)) => {
                if result.provider_id != descriptor.id {
                    diagnostics.push(format!("{} returned a foreign provider ID", descriptor.id));
                    continue;
                }
                result.diagnostics.extend(diagnostics);
                return response::bound(Capability::Supported(result), &p.code_intelligence);
            }
            Ok(Capability::Unsupported { reason, .. }) => diagnostics.push(reason),
            Err(e) if descriptor.id != "native" => {
                diagnostics.push(format!("{} unavailable: {e}", descriptor.id))
            }
            Err(e) => return Err(e),
        }
    }
    response::bound(
        Capability::Unsupported {
            capability: q.kind.clone(),
            reason: diagnostics.join("; "),
        },
        &p.code_intelligence,
    )
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
