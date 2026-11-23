//! Content-addressed registration under the same queue lock as coordinator startup.
use super::{binding, error, CheckRef, Packet, Proposal};
use crate::config::CoordinatorConfig;
use crate::coordinator::{
    decisions as dm,
    model::{Task, TaskRegistry},
    state,
    types::CoordinatorEnvConfig,
};
use crate::{ProjectPaths, Result};
use std::{
    collections::{BTreeMap, BTreeSet},
    path::{Path, PathBuf},
    time::Duration,
};

pub(super) fn storage_args(
    config: Option<&CoordinatorConfig>,
    env: &CoordinatorEnvConfig,
) -> BTreeMap<String, String> {
    let mut args = BTreeMap::new();
    crate::service::coordinator_workflow::apply_storage_mode_args(&mut args, env, config);
    args
}

fn path(root: &Path, id: &str) -> Result<PathBuf> {
    let hash = id
        .strip_prefix("sha256:")
        .filter(|s| s.len() == 64 && s.bytes().all(|c| c.is_ascii_hexdigit()))
        .ok_or_else(|| error("Invalid check identifier"))?;
    let relative = format!(".macc/state/resume-checks/{hash}.json");
    crate::project_ownership::validate_path(&ProjectPaths::from_root(root), &relative)?;
    Ok(root.join(relative))
}

pub(super) fn save(root: &Path, packet: &Packet) -> Result<String> {
    let id = dm::digest(packet);
    crate::atomic_write(
        &ProjectPaths::from_root(root),
        &path(root, &id)?,
        &serde_json::to_vec_pretty(packet).map_err(error)?,
    )?;
    Ok(id)
}

pub(super) fn load(root: &Path, r: &CheckRef) -> Result<Packet> {
    let file = path(root, &r.id)?;
    let bytes = super::observations::read_regular(&file, 256 * 1024)?
        .ok_or_else(|| error("Saved check is missing"))?;
    let packet: Packet =
        serde_json::from_slice(&bytes).map_err(|e| error(format!("Invalid saved check: {e}")))?;
    if dm::digest(&packet) != r.id
        || packet
            .proposal
            .verifier
            .as_ref()
            .map(|v| dm::evidence_digest(v.script.as_bytes()))
            != packet.script_sha256
        || packet.proposal.category != r.category
        || packet.proposal.phase != r.phase
        || packet.proposal.criterion != r.criterion
    {
        return Err(error(
            "Saved check, script or registry reference has changed",
        ));
    }
    Ok(packet)
}

impl Packet {
    /// Only the candidate commit may change during a native handoff.
    pub(super) fn advance_candidate(&mut self, previous: &str, commit: &str) {
        self.previous_packet = Some(previous.to_owned());
        self.binding["commit"] = serde_json::json!(commit);
    }
}

pub(super) fn load_for_replay(
    root: &Path,
    prd: &Path,
    task: &Task,
    reference: &mut CheckRef,
) -> Result<Packet> {
    let mut packet = load(root, reference)?;
    let current = binding::version(root, prd, task, &packet.proposal)?;
    if current == packet.binding {
        return Ok(packet);
    }
    // The registry write may have been interrupted after the handoff. Accept only
    // a saved successor of this packet whose complete binding still matches.
    if let Some(commit) = current["commit"].as_str() {
        packet.advance_candidate(&reference.id, commit);
        if packet.binding == current {
            let mut next = reference.clone();
            next.id = dm::digest(&packet);
            if let Ok(saved) = load(root, &next) {
                reference.id = next.id;
                return Ok(saved);
            }
        }
    }
    Err(error(
        "The recorded block or candidate changed. A fresh diagnosis is required",
    ))
}

/// Capture the diagnosed commits and PRD contracts before sending the incident to the agent.
/// A task with unavailable evidence cannot authorize an automatic resume.
pub struct DiagnosisSnapshot(BTreeMap<String, OriginalBlock>);

struct OriginalBlock {
    anchor: serde_json::Value,
    decision: Option<dm::DecisionRef>,
}

impl DiagnosisSnapshot {
    pub fn capture(root: &Path, config: Option<&CoordinatorConfig>, incident: &[Task]) -> Self {
        let mut versions = BTreeMap::new();
        let mut capture = || -> Result<()> {
            let prd = crate::prd_queue::active_path(root, config, None)?;
            let graph = binding::read_prd(&prd)?;
            let registry = TaskRegistry::from_value(&state::coordinator_state_registry_load(
                root,
                &storage_args(config, &CoordinatorEnvConfig::default()),
            )?)?;
            for reported in incident {
                // The diagnostic view can enrich a row with a consumer's explanation.
                // Bind the actual recorded owner, before the agent starts.
                let Some(task) = registry.find_task(&reported.id) else {
                    continue;
                };
                let head = binding::candidate(root, task)
                    .and_then(|dir| crate::git::head_commit(&dir))
                    .ok();
                let decision = crate::coordinator::phase_diagnostics::decision_wait_id(task)
                    .and_then(|id| dm::Store::open(root).and_then(|s| s.decision(id)).ok())
                    .map(|d| dm::DecisionRef {
                        id: d.id,
                        version: d.version,
                    });
                if let Ok(anchor) = binding::contract_from(&graph, &task.id)
                    .and_then(|contract| binding::anchor(&prd, task, &contract, head))
                {
                    versions.insert(task.id.clone(), OriginalBlock { anchor, decision });
                }
            }
            Ok(())
        };
        if let Err(error) = capture() {
            tracing::debug!(%error, "Cannot capture resume-check bindings for this diagnosis");
        }
        Self(versions)
    }

    pub fn register(
        &self,
        root: &Path,
        config: Option<&CoordinatorConfig>,
        proposals: &[Proposal],
    ) -> Result<Vec<String>> {
        if proposals.is_empty() {
            return Ok(vec![]);
        }
        let paths = ProjectPaths::from_root(root);
        let _queue = crate::fs_lock::AdvisoryLock::acquire(
            &paths.macc_dir.join("state/prd-queue.lock"),
            Duration::ZERO,
            "register resume checks",
        )?;
        let _unblock = crate::fs_lock::AdvisoryLock::acquire(
            &paths.macc_dir.join("state/unblock.lock"),
            Duration::ZERO,
            "bind stopped tasks",
        )?;
        let prd = crate::prd_queue::active_path(root, config, None)?;
        let args = storage_args(config, &CoordinatorEnvConfig::default());
        let mut registry =
            TaskRegistry::from_value(&state::coordinator_state_registry_load(root, &args)?)?;
        let mut ids = Vec::new();
        let mut seen = BTreeSet::new();
        for proposal in proposals {
            let original = self.0.get(&proposal.task_id).ok_or_else(|| {
                error("Task or original PRD contract is absent from the diagnosis snapshot")
            })?;
            if proposal.decision.is_some() && original.decision != proposal.decision {
                return Err(error(
                    "The proposed decision was not current at the start of diagnosis",
                ));
            }
            let mut diagnosed = original.anchor.clone();
            if proposal.verifier.is_none() {
                diagnosed["commit"] = serde_json::Value::Null;
            }
            if !seen.insert(&proposal.task_id) {
                return Err(error("Duplicate proposed task"));
            }
            let task = registry
                .find_task_mut(&proposal.task_id)
                .ok_or_else(|| error("Task disappeared from the active registry"))?;
            let binding = binding::version(root, &prd, task, proposal)?;
            if binding != diagnosed {
                return Err(error("Task changed during diagnosis"));
            }
            let packet = Packet {
                previous_packet: None,
                proposal: proposal.clone(),
                binding,
                script_sha256: proposal
                    .verifier
                    .as_ref()
                    .map(|v| dm::evidence_digest(v.script.as_bytes())),
            };
            let id = save(root, &packet)?;
            if task
                .task_runtime
                .resume_check
                .as_ref()
                .is_none_or(|r| r.id != id)
            {
                task.task_runtime.resume_check = Some(Box::new(CheckRef {
                    id: id.clone(),
                    category: proposal.category,
                    phase: proposal.phase.clone(),
                    criterion: proposal.criterion.clone(),
                    last: None,
                    checked_at: None,
                    observation_sha256: None,
                }));
            }
            ids.push(id);
        }
        state::coordinator_state_registry_save(root, &args, &registry.to_value()?)?;
        Ok(ids)
    }
}
