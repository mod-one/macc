//! Optional Graphify adapter. No assistant-tool names or install hooks enter shared orchestration.
mod index;
mod lifecycle;
mod query;
mod runtime;
mod storage;
use super::*;
use serde_json::{json, Value};
pub(super) struct Graphify;
inventory::submit! {Registration{create:||Box::new(Graphify)}}
fn supported() -> bool {
    cfg!(all(target_os = "linux", target_arch = "x86_64"))
}
impl CodeIntelligenceProvider for Graphify {
    fn startup_budget_seconds(&self, policy: &crate::o2::policy::CodePolicy) -> u64 {
        policy.graphify_timeout_seconds.saturating_mul(4)
    }
    fn enabled(&self, policy: &super::super::policy::CodePolicy) -> bool {
        policy.enabled && policy.graphify_enabled
    }
    fn descriptor(&self) -> Descriptor {
        Descriptor{schema_version:1,id:"graphify".into(),version:runtime::VERSION.into(),origin:format!("https://pypi.org/project/graphifyy/{}/; lock sha256:{}",runtime::VERSION,digest(runtime::LOCK.as_bytes())),operations:["install","deploy","index","update","rollback","disable","undeploy","uninstall","query"].into_iter().map(str::to_string).collect(),local_only:true,unsupported:vec!["Managed runtime: Linux x86_64 / Python 3.12 only".into(),"Cloud, semantic documents/media, global skills, MCP and live incremental updates disabled".into(),"Precise LSP definitions, dependency_path and impact_analysis unsupported".into()]}
    }
    fn network_operation(&self, action: &str) -> bool {
        ["install", "update"].contains(&action)
    }
    fn plan_paths(
        &self,
        action: &str,
        id: &str,
        previous: &Option<Receipt>,
    ) -> Result<Capability<Vec<String>>> {
        if !supported() {
            return Ok(Capability::Unsupported{capability:action.into(),reason:"Managed Graphify lock supports Linux x86_64 / Python 3.12; native fallback remains available".into()});
        }
        Ok(Capability::Supported(
            if ["install", "index", "update"].contains(&action) {
                vec![format!(".macc/code-intelligence/graphify/generations/{id}")]
            } else if ["uninstall", "undeploy"].contains(&action) {
                previous
                    .as_ref()
                    .map(|r| r.owned_paths.clone())
                    .unwrap_or_default()
            } else {
                vec![]
            },
        ))
    }
    fn plan_details(
        &self,
        root: &Path,
        action: &str,
        p: &super::super::policy::CodePolicy,
    ) -> Result<Value> {
        if !supported() {
            return Ok(json!({"supported":false}));
        }
        if !["disable", "undeploy", "uninstall", "rollback"].contains(&action)
            && !p.graphify_enabled
        {
            return Err(error("Enable managed Graphify in Code Intelligence policies before planning this operation"));
        }
        Ok(
            json!({"package":"graphifyy","version":runtime::VERSION,"requirements_sha256":digest(runtime::LOCK.as_bytes()),"launcher_sha256":digest(runtime::LAUNCHER.as_bytes()),"download_bytes_upper_bound":34153890,"dependency_count":30,"python":"3.12","scope":"project-isolated","mode":"code-only; no cloud/API keys; no assistant registration","query_and_index_timeout_seconds":p.graphify_timeout_seconds,"memory_limit_mib":p.graphify_memory_mb,"tools":if self.network_operation(action){runtime::tools(root,p)?}else{Value::Null}}),
        )
    }
    fn detect(&self, root: &Path) -> Result<Detection> {
        let enabled = super::super::policy::load(root)?
            .code_intelligence
            .graphify_enabled;
        let status = if !enabled {
            "disabled".to_string()
        } else if !supported() {
            "incompatible".to_string()
        } else {
            let (db, _) = readonly_database(root)?;
            super::receipt(&db, "graphify")?
                .map(|r| r.state)
                .unwrap_or_else(|| "missing".into())
        };
        Ok(Detection{provider:self.descriptor(),status,diagnostics:vec!["Receipt eligibility only; health inspection and query execution verify the managed runtime. PATH packages are never adopted or removed".into()]})
    }
    fn health(
        &self,
        root: &Path,
        r: Option<&Receipt>,
        p: &super::super::policy::CodePolicy,
    ) -> Result<Detection> {
        let mut detection = self.detect(root)?;
        if !p.graphify_enabled || !supported() {
            return Ok(detection);
        }
        if let Some(r) = r {
            let check = || -> Result<()> {
                if r.state == "missing" {
                    return Ok(());
                }
                runtime::verify(
                    root,
                    r.metadata
                        .as_ref()
                        .ok_or_else(|| error("Missing managed runtime receipt"))?,
                )?;

                if let Some(path) = &r.index {
                    let cached = index::load(root, path, p)?;
                    let current = source_snapshot(root, p)?;
                    if cached.source.digest != current.digest
                        || cached.source.revision != current.revision
                    {
                        return Err(error("Graphify snapshot stale; rebuild explicitly"));
                    }
                }
                Ok(())
            };
            match check() {
                Ok(()) => detection.status = r.state.clone(),
                Err(e) => {
                    detection.status = "degraded".into();
                    detection.diagnostics.push(e.to_string());
                }
            }
        }
        Ok(detection)
    }
    fn execute_lifecycle(
        &self,
        root: &Path,
        plan: &Plan,
        previous: &Option<Receipt>,
        p: &super::super::policy::CodePolicy,
    ) -> Result<Capability<Receipt>> {
        lifecycle::execute(root, plan, previous, p).map(Capability::Supported)
    }
    fn query(
        &self,
        root: &Path,
        q: &Query,
        p: &super::super::policy::CodePolicy,
    ) -> Result<Capability<QueryResult>> {
        query::query(root, q, p)
    }
}

#[cfg(all(test, target_os = "linux", target_arch = "x86_64"))]
mod tests;
