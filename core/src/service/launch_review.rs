//! Shared, read-only coordinator launch review and explicit GUI launch choices.
use crate::{
    config::CoordinatorConfigResolved, coordinator::preflight::*, Engine, MaccError, ProjectPaths,
    Result,
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LaunchReview {
    pub revision: String,
    pub project: String,
    pub task_source: String,
    pub queue: Vec<String>,
    pub reference_branch: String,
    pub tools: Vec<String>,
    pub max_parallel: usize,
    pub max_dispatch: usize,
    pub needs_branch_creation: bool,
    pub branch_sources: Vec<String>,
    pub dirty_reference: bool,
    pub warnings: Vec<String>,
    pub blockers: Vec<String>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LaunchDecision {
    pub confirmed: bool,
    pub revision: String,
    pub supervisor: bool,
    #[serde(default)]
    pub create_from: Option<String>,
    #[serde(default)]
    pub allow_dirty_reference: bool,
}
fn invalid(message: impl Into<String>) -> MaccError {
    MaccError::Validation(message.into())
}

pub fn inspect<E: Engine + ?Sized>(engine: &E, paths: &ProjectPaths) -> Result<LaunchReview> {
    let canonical = engine.load_canonical_config(paths)?;
    let cfg = canonical.automation.coordinator.as_ref();
    let resolved = CoordinatorConfigResolved::resolve(cfg);
    let policy = cfg
        .and_then(|c| c.reference_branch_preflight.clone())
        .unwrap_or_default()
        .resolve(cfg.and_then(|c| c.require_clean_reference_branch));
    let report = engine
        .inspect_reference_preflight(paths, &resolved.reference_branch, &policy)
        .map_err(|e| invalid(e.to_string()))?;
    let mut blockers = Vec::new();
    let missing = crate::git::missing_git_identity_fields(&paths.root);
    if !missing.is_empty() {
        blockers.push(format!("Configure Git identity: {}", missing.join(", ")));
    }
    let source = crate::prd_queue::readiness::inspect(paths, cfg);
    match &source {
        Err(error) => blockers.push(error.to_string()),
        Ok(source) if source.current.completed => {
            blockers.push("PRD queue completed; reset progress explicitly to run again.".into())
        }
        _ => {}
    }
    if matches!(
        report.status,
        ReferencePreflightStatus::InvalidBranchName
            | ReferencePreflightStatus::BareRepository
            | ReferencePreflightStatus::GitInspectionFailed
    ) {
        blockers.push(format_report_cli(&report));
    }
    let mut warnings: Vec<String> = crate::config::coordinator_config_warnings(cfg)
        .into_iter()
        .map(|w| w.to_string())
        .collect();
    if canonical.tools.enabled.is_empty() && resolved.tool_priority.is_empty() {
        blockers.push("Select at least one performer tool before launching.".into());
    }
    if resolved.max_parallel > 8 {
        warnings.push(format!(
            "Max parallel is {}; provider usage limits may be reached sooner.",
            resolved.max_parallel
        ));
    }
    let needs_dirty_ack = policy.enabled
        && report.status == ReferencePreflightStatus::Dirty
        && !matches!(
            policy.dirty_policy,
            DirtyReferencePolicy::Allow | DirtyReferencePolicy::Warn
        );
    if policy.enabled && report.status == ReferencePreflightStatus::Dirty && !needs_dirty_ack {
        warnings.push(
            "Reference branch has uncommitted changes; the configured policy allows launch.".into(),
        );
    }
    let mut branch_sources = vec!["HEAD".into()];
    branch_sources.extend(report.remote_tracking_branches.clone());
    let mut review = LaunchReview {
        revision: String::new(),
        project: paths.root.display().to_string(),
        task_source: source
            .as_ref()
            .map(|s| s.current.label())
            .unwrap_or_else(|_| "Unavailable".into()),
        queue: cfg.map(|c| c.prd_files.clone()).unwrap_or_default(),
        reference_branch: resolved.reference_branch,
        tools: if resolved.tool_priority.is_empty() {
            canonical.tools.enabled.clone()
        } else {
            resolved.tool_priority
        },
        max_parallel: resolved.max_parallel,
        max_dispatch: resolved.max_dispatch,
        needs_branch_creation: policy.enabled
            && report.status == ReferencePreflightStatus::BranchMissing,
        branch_sources,
        dirty_reference: needs_dirty_ack,
        warnings,
        blockers,
    };
    let mut hash = Sha256::new();
    hash.update(serde_json::to_vec(&canonical).map_err(|e| invalid(e.to_string()))?);
    hash.update(serde_json::to_vec(&review).map_err(|e| invalid(e.to_string()))?);
    hash.update(format!("{report:?}"));
    if let Ok(source) = source {
        if let Ok(bytes) = std::fs::read(source.path) {
            hash.update(bytes);
        }
    }
    if let Ok(output) = crate::git::run_git_output_mapped(
        &paths.root,
        &["rev-parse", "HEAD"],
        "inspect launch revision",
    ) {
        hash.update(output.stdout);
    }
    review.revision = format!("{:x}", hash.finalize());
    Ok(review)
}

pub fn launch<E: Engine + ?Sized>(
    engine: &E,
    paths: &ProjectPaths,
    decision: &LaunchDecision,
    client_id: Option<&str>,
) -> Result<()> {
    if !decision.confirmed {
        return Err(invalid("Confirm the coordinator launch review first."));
    }
    let review = inspect(engine, paths)?;
    if review.revision != decision.revision {
        return Err(invalid(
            "Launch conditions changed. Refresh and confirm a new review.",
        ));
    }
    if !review.blockers.is_empty() {
        return Err(invalid(review.blockers.join("\n")));
    }
    if review.needs_branch_creation && decision.create_from.is_none() {
        return Err(invalid("Choose a source for the missing reference branch."));
    }
    if review.dirty_reference && !decision.allow_dirty_reference {
        return Err(invalid(
            "Review and explicitly allow the dirty reference branch, or clean it before launching.",
        ));
    }
    crate::service::process_ownership_gate::gate_owner_action(
        &crate::service::process_ownership_gate::ClientContext {
            client_id: client_id.unwrap_or_default().into(),
            project_root: paths.root.clone(),
        },
        &crate::process_ownership::ProcessHandle {
            kind: crate::process_ownership::ProcessKind::Coordinator,
            project_root: paths.root.clone(),
            pid: None,
        },
    )?;
    if review.needs_branch_creation {
        let base = decision.create_from.as_deref().unwrap_or_default();
        if !review.branch_sources.iter().any(|s| s == base) {
            return Err(invalid(
                "Unknown reference branch source; refresh the review.",
            ));
        }
        let source = if base == "HEAD" {
            BranchCreateSource::CurrentHead
        } else {
            BranchCreateSource::RemoteTracking(base.into())
        };
        engine
            .create_reference_branch_via_engine(paths, &review.reference_branch, source)
            .map_err(|e| invalid(e.to_string()))?;
    }
    let mut canonical = engine.load_canonical_config(paths)?;
    // Carry the reviewed one-run allowance to the child's own preflight without
    // persisting a relaxed policy in the project's configuration.
    if review.dirty_reference && decision.allow_dirty_reference {
        canonical
            .automation
            .coordinator
            .get_or_insert_with(Default::default)
            .reference_branch_preflight
            .get_or_insert_with(Default::default)
            .dirty_policy = Some("allow".into());
    }
    engine.coordinator_start_managed_command_process(
        paths,
        &crate::service::coordinator_workflow::CoordinatorCommand::Run,
        canonical.automation.coordinator.as_ref(),
        client_id,
    )?;
    // A fork alone is not readiness, including when supervision is disabled.
    let startup: Result<()> = (|| {
        if let Some(record) =
            crate::coordinator::managed_command_registry::get_managed_command(paths, "run")?
        {
            crate::service::coordinator_readiness::wait(
                paths,
                record.pid,
                std::time::Duration::from_secs(15),
            )?;
        }
        if decision.supervisor {
            engine.coordinator_attach_supervisor(paths)?;
        }
        Ok(())
    })();
    if let Err(error) = startup {
        let stopped = engine.coordinator_stop_managed_command_process(paths, true);
        return Err(invalid(format!(
            "Coordinator startup failed: {error}. Coordinator rollback: {}",
            stopped
                .map(|_| "stopped".to_string())
                .unwrap_or_else(|e| e.to_string())
        )));
    }
    Ok(())
}

#[cfg(test)]
#[path = "launch_review_tests.rs"]
mod tests;
