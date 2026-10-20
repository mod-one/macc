//! Optional read-side Serena/SolidLSP adapter, independent of AI tools.
mod lifecycle;
mod query;
mod runtime;
use super::super::policy::CodePolicy;
use super::*;
use serde_json::{json, Value};
struct Serena;
inventory::submit! { Registration{create:||Box::new(Serena)} }
fn supported() -> bool {
    cfg!(all(target_os = "linux", target_arch = "x86_64"))
}
impl CodeIntelligenceProvider for Serena {
    fn startup_budget_seconds(&self, policy: &crate::o2::policy::CodePolicy) -> u64 {
        policy.serena_timeout_seconds.saturating_mul(4)
    }
    fn enabled(&self, policy: &CodePolicy) -> bool {
        policy.enabled && policy.serena_enabled
    }
    fn descriptor(&self) -> Descriptor {
        Descriptor{schema_version:1,id:"serena".into(),version:runtime::VERSION.into(),origin:format!("https://pypi.org/project/serena-agent/{}/; SolidLSP subset lock sha256:{}",runtime::VERSION,digest(runtime::LOCK.as_bytes())),operations:["install","deploy","update","rollback","disable","undeploy","uninstall","query"].into_iter().map(str::to_string).collect(),local_only:true,unsupported:vec!["Linux x86_64 / Python 3.12 runtime; only Python/Jedi language supported".into(),"No durable index, edits, MCP, project hooks or language-server downloads".into(),"dependency_path, architecture_summary, impact_analysis and non-Python languages unsupported".into()]}
    }
    fn requires_cached_snapshot(&self) -> bool {
        false
    }
    fn priority(&self, q: &Query) -> u32 {
        if ["symbol_definition", "references"].contains(&q.kind.as_str()) {
            0
        } else {
            2000
        }
    }
    fn network_operation(&self, action: &str) -> bool {
        ["install", "update"].contains(&action)
    }
    fn plan_details(&self, root: &Path, action: &str, p: &CodePolicy) -> Result<Value> {
        if !supported() {
            return Ok(json!({"supported":false}));
        }
        if !["disable", "undeploy", "uninstall", "rollback"].contains(&action) && !p.serena_enabled
        {
            return Err(error("Enable Serena in Code Intelligence policies first"));
        }
        Ok(
            json!({"version":runtime::VERSION,"dependency_count":35,"download_wheel_bytes_upper_bound":9572005,"scope":"project-isolated","mode":"Serena SolidLSP read-side subset, not full agent installation","lock_sha256":digest(runtime::LOCK.as_bytes()),"bridge_sha256":digest(runtime::BRIDGE.as_bytes()),"python":"3.12","language_server":"jedi-language-server==0.46.0 (included in reviewed install)","supported_languages":["python"],"language_server_downloads":false,"source_access":"curated tracked Python copies only","timeout_seconds":p.serena_timeout_seconds,"memory_limit_mib":p.serena_memory_mb,"tools":if self.network_operation(action){runtime::tools(root,p)?}else{Value::Null}}),
        )
    }
    fn plan_paths(
        &self,
        action: &str,
        id: &str,
        previous: &Option<Receipt>,
    ) -> Result<Capability<Vec<String>>> {
        if !supported() {
            return Ok(Capability::Unsupported {
                capability: action.into(),
                reason: "Managed Serena requires Linux x86_64 and Python 3.12".into(),
            });
        }
        Ok(Capability::Supported(
            if ["install", "update"].contains(&action) {
                vec![format!(".macc/code-intelligence/serena/generations/{id}")]
            } else if action == "uninstall" {
                previous
                    .as_ref()
                    .map(|r| r.owned_paths.clone())
                    .unwrap_or_default()
            } else {
                vec![]
            },
        ))
    }
    fn detect(&self, root: &Path) -> Result<Detection> {
        let p = super::super::policy::load(root)?.code_intelligence;
        let status = if !p.serena_enabled {
            "disabled".into()
        } else if !supported() {
            "incompatible".into()
        } else {
            let (db, _) = readonly_database(root)?;
            super::receipt(&db, "serena")?
                .map(|r| r.state)
                .unwrap_or_else(|| "missing".into())
        };
        Ok(Detection{provider:self.descriptor(),status,diagnostics:vec!["Python/Jedi only; install explicitly for missing runtime/dependencies; no PATH runtime adopted, no automatic language-server setup".into()]})
    }
    fn health(&self, root: &Path, r: Option<&Receipt>, p: &CodePolicy) -> Result<Detection> {
        let mut d = self.detect(root)?;
        if p.serena_enabled && supported() {
            if let Some(r) = r.filter(|r| r.state != "missing") {
                let check = || {
                    let meta = r
                        .metadata
                        .as_ref()
                        .ok_or_else(|| error("Serena runtime receipt missing"))?;
                    runtime::verify(root, meta)?;
                    runtime::smoke(root, meta, p)
                };
                if let Err(e) = check() {
                    d.status = "degraded".into();
                    d.diagnostics.push(format!(
                        "Python/Jedi dependency or runtime probe failed: {e}"
                    ));
                }
            }
        }
        Ok(d)
    }
    fn execute_lifecycle(
        &self,
        root: &Path,
        plan: &Plan,
        previous: &Option<Receipt>,
        p: &CodePolicy,
    ) -> Result<Capability<Receipt>> {
        lifecycle::execute(root, plan, previous, p).map(Capability::Supported)
    }
    fn query(&self, root: &Path, q: &Query, p: &CodePolicy) -> Result<Capability<QueryResult>> {
        query::query(root, q, p)
    }
}
#[cfg(all(test, target_os = "linux", target_arch = "x86_64"))]
mod tests;
