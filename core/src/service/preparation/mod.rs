//! One reviewed prepare-and-verify boundary for every interactive client.
mod command_check;
pub mod interactive;
mod mcp;
mod o2;
mod progress;
#[cfg(test)]
mod tests;
mod tools;
use crate::{MaccError, ProjectPaths, Result};
use serde::{Deserialize, Serialize};
use std::path::Path;

mod model;
pub use model::*;
mod attempt;
pub(crate) use attempt::{begin, phase, record_failure};
pub fn preview(paths: &ProjectPaths) -> Result<Option<Plan>> {
    if paths.macc_dir.join("worktree.json").is_file() || !paths.config_path.exists() {
        return Ok(None);
    }
    let config = crate::load_canonical_config(&paths.config_path)?;
    let policy = crate::o2::policy::load(&paths.root)?;
    let tools = tools::requirements(paths, &config)?;
    let mcp_cleanup = mcp::cleanup_requirements(
        paths,
        &tools,
        policy.memory.enabled && policy.memory.mcp_enabled,
    )?;
    let providers = crate::o2::providers::preparation::requirements(&policy.code_intelligence)?;
    let mut steps: Vec<_> = tools
        .iter()
        .map(|t| {
            format!(
                "Verify {}; install declared dependencies when missing",
                t.spec.id
            )
        })
        .collect();
    steps.extend(mcp_cleanup.iter().map(|s| {
        format!(
            "Remove inactive MACC MCP registration from {} (backup retained)",
            s.path
        )
    }));
    steps.extend(providers.iter().map(|p| {
        format!(
            "Prepare {} {}: {}; origin {}",
            p.id,
            p.version,
            p.operations.join(" → "),
            p.origin
        )
    }));
    if policy.memory.enabled && policy.memory.mcp_enabled {
        steps.push(
            "Register scoped memory MCP for every enabled tool; verify project configurations"
                .into(),
        );
    }
    if policy.adaptive_compaction.enabled && policy.adaptive_compaction.configure_native_auto {
        steps.push("Probe enabled tools and verify native compaction support before runners claim sessions".into());
    }
    if policy.memory.semantic_enabled {
        steps.push(
            "Verify semantic quality evidence and approve the reviewed passing benchmark".into(),
        );
    }
    if policy.context_engine.tuning_enabled {
        steps.push("Verify policy trial storage and collection; trial promotion requires reviewed evidence".into());
    }
    if crate::o2::activation::requested(&config) {
        steps.push(
            "Verify O2 schema, full-text search, enabled services and runner prerequisites".into(),
        );
    }
    if steps.is_empty() {
        return Ok(None);
    }
    let network = providers.iter().any(|p| p.network)
        || tools.iter().any(|t| {
            t.install_needed
                || t.spec
                    .performer
                    .as_ref()
                    .and_then(|p| p.preparation.as_ref())
                    .is_some_and(|p| p.setup.is_some())
        });
    Ok(Some(Plan {
        mcp_cleanup,
        inputs: Default::default(),
        revision: crate::config::transaction::revision(&config)?,
        tools,
        providers,
        steps,
        network,
    }))
}
pub fn configure(paths: &ProjectPaths, plan: &mut crate::plan::ActionPlan) -> Result<()> {
    if !paths.config_path.exists() {
        return Ok(());
    }
    let config = crate::load_canonical_config(&paths.config_path)?;
    let p = crate::o2::policy::load(&paths.root)?;
    if p.memory.enabled && p.memory.mcp_enabled {
        for requirement in tools::requirements(paths, &config)? {
            requirement
                .spec
                .performer
                .as_ref()
                .and_then(|p| p.scoped_mcp.as_ref())
                .ok_or_else(|| {
                    MaccError::Validation(format!(
                        "{} has no scoped MCP registration adapter",
                        requirement.spec.id
                    ))
                })?
                .configure(paths, plan)?;
        }
    }
    Ok(())
}
pub fn validate_review(paths: &ProjectPaths, reviewed: Option<&Plan>) -> Result<()> {
    let mut current = preview(paths)?;
    if let (Some(current), Some(reviewed)) = (&mut current, reviewed) {
        current.inputs = reviewed.inputs.clone();
    }
    if current.as_ref() != reviewed {
        return Err(MaccError::Validation(
            "Preparation requirements changed; refresh and confirm Apply again".into(),
        ));
    }
    Ok(())
}
pub fn execute(paths: &ProjectPaths, plan: &Plan) -> Result<Report> {
    let mut report = status(&paths.root)?
        .filter(|r| {
            r.status == "preparing"
                && r.revision == plan.revision
                && uuid::Uuid::parse_str(&r.run_id).is_ok()
        })
        .map(Ok)
        .unwrap_or_else(|| begin(paths, plan))?;
    let result: Result<()> = (|| {
        progress::clear(&paths.root)?;
        mcp::cleanup(paths, &plan.mcp_cleanup, &mut report)?;
        for tool in &plan.tools {
            progress::clear(&paths.root)?;
            report.current = Some(format!("Preparing tool {}", tool.spec.id));
            publish(&paths.root, &report)?;
            tools::prepare(paths, tool, true)?;
            report
                .completed
                .push(format!("Tool {} verified", tool.spec.id));
            publish(&paths.root, &report)?;
        }
        for provider in &plan.providers {
            progress::clear(&paths.root)?;
            report.current = Some(format!("Preparing provider {}", provider.id));
            publish(&paths.root, &report)?;
            if !plan.inputs.accept_degraded
                || crate::o2::providers::preparation::verify(&paths.root, provider).is_err()
            {
                crate::o2::providers::preparation::execute(&paths.root, provider)?;
            }
            report
                .completed
                .push(format!("Provider {} prepared and verified", provider.id));
            publish(&paths.root, &report)?;
        }
        progress::clear(&paths.root)?;
        for provider in &plan.providers {
            let quality =
                crate::o2::providers::preparation_quality::inspect(&paths.root, &provider.id)?;
            report.warnings.extend(quality.warnings);
            if let Some(suggestion) = quality.suggestion.filter(|_| !plan.inputs.accept_degraded) {
                report.question = Some(Question {kind: "provider_quality".into(),message: "The local index reached its build budget. Increase the budget and rebuild, or keep partial coverage with warnings.".into(),data:serde_json::to_value(suggestion).map_err(|e|MaccError::Validation(e.to_string()))?});
                return Err(MaccError::Validation(
                    "Index preparation needs a coverage decision; completed work is retained"
                        .into(),
                ));
            }
        }
        o2::prepare(paths, plan, &mut report)?;
        let current = crate::load_canonical_config(&paths.config_path)?;
        if crate::config::transaction::revision(&current)? != plan.revision {
            return Err(MaccError::Validation(
                "Configuration changed during preparation; refresh Apply".into(),
            ));
        }
        Ok(())
    })();
    match result {
        Ok(()) => {
            report.current = None;
            report.status = "ready".into();
            publish(&paths.root, &report)?;
            Ok(report)
        }
        Err(error) => {
            let detail = error.to_string().to_ascii_lowercase();
            if report.question.is_none()
                && (detail.contains("offline")
                    || detail.contains("requires network access")
                    || detail.contains("needs network access"))
                && crate::load_canonical_config(&paths.config_path)?
                    .settings
                    .offline
            {
                report.question=Some(Question{kind:"network_access".into(),message:"Preparation needs network access. Allow it for this project and review the updated Apply plan?".into(),data:serde_json::json!({"revision":report.revision})});
            }
            report.status = if report.question.is_some() {
                "waiting_for_input"
            } else {
                "blocked"
            }
            .into();
            report.error = Some(error.to_string());
            publish(&paths.root, &report)?;
            Err(error)
        }
    }
}
pub fn require_ready(root: &Path) -> Result<()> {
    let paths = ProjectPaths::from_root(root);
    let _guard = lock(&paths)?;
    let Some(plan) = preview(&paths)? else {
        return Ok(());
    };
    crate::o2::activation::require_ready(root)?;
    for tool in &plan.tools {
        if tool.install_needed {
            return Err(MaccError::Validation(format!(
                "{} is unavailable; complete Apply preparation before running",
                tool.spec.id
            )));
        }
        tools::prepare(&paths, tool, false)?;
    }
    for provider in &plan.providers {
        crate::o2::providers::preparation::ensure_current(root, provider)?;
    }
    o2::verify(&paths, &plan)
}
pub(crate) fn lock(paths: &ProjectPaths) -> Result<crate::fs_lock::AdvisoryLock> {
    crate::fs_lock::AdvisoryLock::acquire(
        &paths.macc_dir.join("state/apply-preparation.lock"),
        std::time::Duration::ZERO,
        "Apply preparation; another Apply/startup may be running",
    )
}
/// Read-only diagnostics: no login, installation, database migration or index refresh.
pub fn verify_enabled(root: &Path) -> Result<()> {
    let paths = ProjectPaths::from_root(root);
    let Some(plan) = preview(&paths)? else {
        return Ok(());
    };
    crate::o2::activation::require_ready(root)?;
    // Provider health is reported separately by Doctor. Source drift can be
    // refreshed locally at startup and is not an installation blocker.
    o2::verify(&paths, &plan)
}
fn publish(root: &Path, report: &Report) -> Result<()> {
    let paths = ProjectPaths::from_root(root);
    let bytes =
        serde_json::to_vec_pretty(report).map_err(|e| MaccError::Validation(e.to_string()))?;
    crate::atomic_write(
        &paths,
        &paths.macc_dir.join("state/apply-preparation.json"),
        &bytes,
    )?;
    if uuid::Uuid::parse_str(&report.run_id).is_ok() {
        crate::atomic_write(
            &paths,
            &paths
                .macc_dir
                .join("log/apply")
                .join(&report.run_id)
                .join("preparation.json"),
            &bytes,
        )?;
    }
    Ok(())
}

pub fn status(root: &Path) -> Result<Option<Report>> {
    let path = root.join(".macc/state/apply-preparation.json");
    if !path.exists() {
        return Ok(None);
    }
    let raw = std::fs::read(&path).map_err(|source| MaccError::Io {
        path: path.display().to_string(),
        action: "read Apply preparation".into(),
        source,
    })?;
    let mut report: Report =
        serde_json::from_slice(&raw).map_err(|e| MaccError::Validation(e.to_string()))?;
    report.output_kind = progress::kind(root);
    report.live_output = if report.status == "preparing" {
        progress::output(root)?
    } else {
        None
    };
    Ok(Some(report))
}
pub fn set_inputs(plan: &mut crate::plan::ActionPlan, inputs: Inputs) {
    if let Some(preparation) = &mut plan.preparation {
        preparation.inputs = inputs;
    }
}
pub fn allow_network(root: &Path, revision: &str, confirmed: bool) -> Result<()> {
    if !confirmed {
        return Err(MaccError::Validation(
            "Confirm network access in Apply first".into(),
        ));
    }
    crate::config::transaction::update(&ProjectPaths::from_root(root), Some(revision), |config| {
        config.settings.offline = false;
        Ok(())
    })?;
    Ok(())
}

/// Apply an adapter-proposed setting only after the operator reviews it.
pub fn improve_quality(root: &Path, report: &Report, confirmed: bool) -> Result<()> {
    let question = report
        .question
        .as_ref()
        .filter(|q| q.kind == "provider_quality")
        .ok_or_else(|| MaccError::Validation("No pending provider quality suggestion".into()))?;
    let suggestion: crate::o2::providers::preparation_quality::SettingSuggestion =
        serde_json::from_value(question.data.clone())
            .map_err(|e| MaccError::Validation(e.to_string()))?;
    crate::o2::policy_edit::apply(
        root,
        &suggestion.path,
        &suggestion.value,
        &report.revision,
        confirmed,
    )?;
    Ok(())
}

/// Bounded probes plus provider refreshes, with a margin for storage/recovery.
pub fn startup_timeout(root: &Path) -> Result<std::time::Duration> {
    let paths = ProjectPaths::from_root(root);
    let Some(plan) = preview(&paths)? else {
        return Ok(std::time::Duration::from_secs(30));
    };
    let mut seconds = 30u64;
    for tool in &plan.tools {
        if tool.spec.version_check.is_some() {
            seconds = seconds.saturating_add(15);
        }
        if let Some(p) = tool
            .spec
            .performer
            .as_ref()
            .and_then(|p| p.preparation.as_ref())
        {
            if !(1..=120).contains(&p.timeout_seconds) {
                return Err(MaccError::Validation(format!(
                    "{} preparation timeout_seconds must be between 1 and 120",
                    tool.spec.id
                )));
            }
            seconds = seconds.saturating_add(p.timeout_seconds);
        }
    }
    let policy = crate::o2::policy::load(root)?;
    seconds = seconds.saturating_add(crate::o2::providers::preparation::startup_budget_seconds(
        &policy.code_intelligence,
    )?);
    Ok(std::time::Duration::from_secs(seconds))
}
