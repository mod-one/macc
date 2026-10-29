//! Human approval gates (`gate.kind: human_approval`).
//!
//! Responsibilities are split deliberately:
//!
//! * the **PRD** declares the policy (`TaskGate`: subject, roles, quorum,
//!   binding, expiry);
//! * the **decision ledger** (`gate_decisions` table, append-only, written only
//!   by `macc coordinator approve|reject|request-changes`) records who decided
//!   what, in which role, on which subject revision, with which evidence;
//! * the **task state** (`waiting_approval`, `approved`, `rejected`, `expired`)
//!   is *derived* from the ledger on every cycle by [`evaluate_gate`].
//!
//! Because the state is recomputed from the ledger, writing `approved` into the
//! registry by any other path has no lasting effect: the next reconciliation
//! reverts it unless recorded decisions meet the quorum. A gate is never
//! dispatched to a performer, never retried, and never occupies a worker.

use crate::coordinator::model::{
    ApprovalDecision, ApprovalRecord, ApprovalState, ApprovalStatus, TaskGate, TaskRegistry,
};
use crate::coordinator::{RuntimeStatus, WorkflowState};
use crate::{MaccError, Result};
use std::collections::BTreeMap;

/// Shortest revision prefix accepted when binding a decision to a commit.
pub const MIN_REVISION_PREFIX: usize = 7;

/// Two revisions refer to the same commit when one is a prefix of the other
/// and the shorter one is at least [`MIN_REVISION_PREFIX`] characters.
pub fn same_revision(a: &str, b: &str) -> bool {
    let (a, b) = (a.trim(), b.trim());
    let shorter = a.len().min(b.len());
    shorter >= MIN_REVISION_PREFIX && (a.starts_with(b) || b.starts_with(a))
}

/// Result of evaluating one gate against its ledger.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GateEvaluation {
    pub state: WorkflowState,
    pub status: ApprovalStatus,
    pub revision: Option<String>,
    pub effective: Vec<ApprovalRecord>,
    pub note: String,
}

fn is_expired(record: &ApprovalRecord, gate: &TaskGate, now: &str) -> bool {
    let Some(days) = gate.expires_after_days else {
        return false;
    };
    let (Ok(now), Ok(at)) = (
        chrono::DateTime::parse_from_rfc3339(now),
        chrono::DateTime::parse_from_rfc3339(&record.recorded_at),
    ) else {
        return false;
    };
    now.signed_duration_since(at) > chrono::Duration::days(i64::from(days))
}

/// Latest decision per (actor, role), restricted to `revision`.
fn latest_per_actor<'a>(
    ledger: impl Iterator<Item = &'a ApprovalRecord>,
    revision: Option<&str>,
) -> Vec<ApprovalRecord> {
    let mut latest: BTreeMap<(String, String), ApprovalRecord> = BTreeMap::new();
    for record in ledger {
        let bound = match (revision, record.revision.as_deref()) {
            (Some(current), Some(decided)) => same_revision(current, decided),
            // No known subject revision and no revision on the decision:
            // nothing to bind against, so it counts (bind is best-effort).
            (None, None) => true,
            _ => false,
        };
        if bound {
            latest.insert((record.actor.clone(), record.role.clone()), record.clone());
        }
    }
    latest.into_values().collect()
}

/// Derive a gate's state from its declared policy and recorded decisions.
///
/// `deps_satisfied` is false while any dependency (including the subject) is
/// undelivered; the gate then stays `todo` and no approval is requested.
/// `subject_digest` is the subject's current revision on the reference branch.
pub fn evaluate_gate(
    gate: &TaskGate,
    deps_satisfied: bool,
    subject_digest: Option<&str>,
    ledger: &[ApprovalRecord],
    now: &str,
) -> Option<GateEvaluation> {
    if !deps_satisfied {
        return None;
    }
    // Bind to the subject's current revision; fall back to the revision the
    // most recent decision named when the subject has no trailer commit.
    let revision = subject_digest
        .map(str::to_string)
        .or_else(|| ledger.iter().rev().find_map(|r| r.revision.clone()));

    let bound = latest_per_actor(ledger.iter(), revision.as_deref());
    let (live, expired): (Vec<_>, Vec<_>) =
        bound.into_iter().partition(|r| !is_expired(r, gate, now));

    let subject = gate.subject_task.as_deref().unwrap_or("the subject");
    let at = revision
        .clone()
        .unwrap_or_else(|| "an unknown revision".to_string());

    if let Some(rejection) = live
        .iter()
        .find(|r| r.decision == ApprovalDecision::Rejected)
    {
        return Some(GateEvaluation {
            state: WorkflowState::Rejected,
            status: ApprovalStatus::Rejected,
            revision,
            note: format!(
                "rejected by {} ({}){}; revise {} to open a new approval",
                rejection.actor,
                rejection.role,
                rejection
                    .reason
                    .as_deref()
                    .map(|r| format!(": {r}"))
                    .unwrap_or_default(),
                subject
            ),
            effective: live,
        });
    }
    if gate.quorum_met(&live) {
        return Some(GateEvaluation {
            state: WorkflowState::Approved,
            status: ApprovalStatus::Approved,
            revision,
            note: format!("approved at {at} ({})", gate.approval_progress(&live)),
            effective: live,
        });
    }
    let expired_approvals = expired
        .iter()
        .any(|r| r.decision == ApprovalDecision::Approved);
    if expired_approvals {
        return Some(GateEvaluation {
            state: WorkflowState::Expired,
            status: ApprovalStatus::Expired,
            revision,
            note: format!(
                "approvals older than {} day(s) expired; renewal required ({})",
                gate.expires_after_days.unwrap_or_default(),
                gate.approval_progress(&live)
            ),
            effective: live,
        });
    }
    if let Some(changes) = live
        .iter()
        .find(|r| r.decision == ApprovalDecision::ChangesRequested)
    {
        return Some(GateEvaluation {
            state: WorkflowState::WaitingApproval,
            status: ApprovalStatus::ChangesRequested,
            revision,
            note: format!(
                "changes requested by {} ({}){}; revise {} — the gate re-opens on the new revision",
                changes.actor,
                changes.role,
                changes
                    .reason
                    .as_deref()
                    .map(|r| format!(": {r}"))
                    .unwrap_or_default(),
                subject
            ),
            effective: live,
        });
    }
    Some(GateEvaluation {
        state: WorkflowState::WaitingApproval,
        status: ApprovalStatus::WaitingApproval,
        revision,
        note: format!(
            "waiting for approval of {subject} at {at} ({})",
            gate.approval_progress(&live)
        ),
        effective: live,
    })
}

/// One state change made by [`reconcile_human_gates`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HumanGateTransition {
    pub task_id: String,
    pub event: &'static str,
    pub severity: &'static str,
    pub detail: String,
}

/// Recompute every human gate from the ledger and apply the resulting state.
///
/// `ledger(task_id)` returns the task's recorded decisions, oldest first.
pub fn reconcile_human_gates(
    registry: &mut TaskRegistry,
    now: &str,
    subject_digest: &dyn Fn(&str) -> Option<String>,
    ledger: &dyn Fn(&str) -> Vec<ApprovalRecord>,
) -> Vec<HumanGateTransition> {
    // Tasks delivered by earlier PRD lots satisfy dependencies too, exactly as
    // they do for the dispatcher.
    let mut satisfied = crate::coordinator::task_selector::satisfied_dependency_ids(registry);
    satisfied.extend(registry.external_merged_task_ids.iter().cloned());
    let mut transitions = Vec::new();
    for task in &mut registry.tasks {
        let Some(gate) = task.gate.clone().filter(TaskGate::is_human_approval) else {
            continue;
        };
        let previous = task.workflow_state();
        if matches!(previous, Some(WorkflowState::Abandoned)) {
            continue;
        }
        let deps_ok = task
            .dependency_ids()
            .iter()
            .all(|dep| satisfied.contains(dep));
        let subject = gate.subject_task.clone().unwrap_or_default();
        let digest = subject_digest(&subject);
        let decisions = ledger(&task.id);
        let Some(eval) = evaluate_gate(&gate, deps_ok, digest.as_deref(), &decisions, now) else {
            // Dependencies regressed or never met: an open gate must not stay
            // approved on a subject that is no longer delivered.
            if matches!(
                previous,
                Some(
                    WorkflowState::Approved
                        | WorkflowState::WaitingApproval
                        | WorkflowState::Expired
                        | WorkflowState::Rejected
                )
            ) {
                task.set_workflow_state(WorkflowState::Todo);
                task.ensure_runtime().set_status(RuntimeStatus::Idle);
                task.touch_state_changed(now);
                transitions.push(HumanGateTransition {
                    task_id: task.id.clone(),
                    event: "approval_withdrawn",
                    severity: "warning",
                    detail: format!("a dependency of {} is no longer delivered", task.id),
                });
            }
            continue;
        };

        let prior = task.task_runtime.approval.clone().unwrap_or_default();
        let mut approval = ApprovalState {
            status: eval.status,
            subject_revision: eval.revision.clone(),
            effective: eval.effective.clone(),
            history: decisions,
            requested_at: prior.requested_at.clone().or_else(|| Some(now.to_string())),
            approved_at: prior.approved_at.clone(),
            approved_revision: prior.approved_revision.clone(),
            note: Some(eval.note.clone()),
        };

        let changed_state = previous != Some(eval.state);
        let changed_status = prior.status != eval.status && task.task_runtime.approval.is_some();
        if changed_state || changed_status {
            let invalidated = previous == Some(WorkflowState::Approved)
                && eval.state != WorkflowState::Approved
                && match (prior.approved_revision.as_deref(), eval.revision.as_deref()) {
                    (Some(was), Some(now_rev)) => !same_revision(was, now_rev),
                    _ => false,
                };
            let (event, severity, detail) = match eval.state {
                _ if invalidated => (
                    "approval_invalidated",
                    "warning",
                    format!(
                        "subject {} changed after approval ({} -> {}); approval withdrawn, waiting again",
                        subject,
                        prior.approved_revision.as_deref().unwrap_or("?"),
                        eval.revision.as_deref().unwrap_or("?")
                    ),
                ),
                WorkflowState::Approved => ("gate_approved", "info", eval.note.clone()),
                WorkflowState::Rejected => ("gate_rejected", "error", eval.note.clone()),
                WorkflowState::Expired => ("approval_expired", "warning", eval.note.clone()),
                _ if eval.status == ApprovalStatus::ChangesRequested => {
                    ("approval_changes_requested", "warning", eval.note.clone())
                }
                _ if previous == Some(WorkflowState::Approved) => {
                    ("approval_unverified", "warning", format!(
                        "state was approved without a recorded quorum; reverted — {}",
                        eval.note
                    ))
                }
                _ => ("approval_requested", "info", eval.note.clone()),
            };
            transitions.push(HumanGateTransition {
                task_id: task.id.clone(),
                event,
                severity,
                detail,
            });
        }

        if eval.state == WorkflowState::Approved {
            if previous != Some(WorkflowState::Approved) {
                approval.approved_at = Some(now.to_string());
            }
            approval.approved_revision = eval.revision.clone();
        } else {
            approval.approved_at = None;
        }
        if changed_state {
            task.set_workflow_state(eval.state);
            task.touch_state_changed(now);
        }
        let runtime = task.ensure_runtime();
        runtime.set_status(if eval.state == WorkflowState::Approved {
            RuntimeStatus::Idle
        } else {
            RuntimeStatus::WaitingForUser
        });
        runtime.approval = Some(approval);
        // A gate never carries an execution error: clear any stale one so it
        // is not reported as a blocked root with a misleading cause.
        if eval.state != WorkflowState::Rejected {
            runtime.last_error = None;
            runtime.last_error_code = None;
            runtime.last_error_origin = None;
            runtime.last_error_message = None;
        } else {
            runtime.set_last_error_details("E909", "approval", eval.note.clone());
        }
    }
    transitions
}

/// Reference branch human gates bind subject revisions to.
pub fn reference_branch(repo_root: &std::path::Path) -> String {
    let paths = crate::ProjectPaths::from_root(repo_root);
    crate::load_canonical_config(&paths.config_path)
        .ok()
        .and_then(|c| c.automation.coordinator)
        .and_then(|c| c.reference_branch)
        .unwrap_or_else(|| "main".to_string())
}

/// Current revision of `subject_task` on the reference branch: the latest
/// commit carrying its `[macc:task <id>]` trailer.
pub fn subject_revision(
    repo_root: &std::path::Path,
    branch: &str,
    subject_task: &str,
) -> Option<String> {
    if subject_task.trim().is_empty() {
        return None;
    }
    crate::git::latest_commit_with_task_trailer(repo_root, branch, subject_task)
        .ok()
        .flatten()
}

/// `git show --stat` summary of a revision, for the approval request.
pub fn revision_diff_stat(repo_root: &std::path::Path, revision: &str) -> Option<String> {
    let output = std::process::Command::new("git")
        .args(["show", "--stat", "--format=%h %s%n%an, %aI", revision])
        .current_dir(repo_root)
        .output()
        .ok()?;
    output
        .status
        .success()
        .then(|| {
            String::from_utf8_lossy(&output.stdout)
                .trim_end()
                .to_string()
        })
        .filter(|s| !s.is_empty())
}

/// Reconcile every human gate in `registry` against the project's ledger and
/// git history. Used by each coordinator cycle and by the decision commands so
/// a recorded decision takes effect immediately.
pub fn reconcile_human_gates_for_project(
    repo_root: &std::path::Path,
    registry: &mut TaskRegistry,
    now: &str,
) -> Result<Vec<HumanGateTransition>> {
    let transitions = evaluate_human_gates_for_project(repo_root, registry, now)?;
    for t in &transitions {
        let _ = crate::coordinator::helpers::append_coordinator_event_with_severity(
            repo_root, t.event, &t.task_id, "approval", t.event, &t.detail, t.severity,
        );
    }
    Ok(transitions)
}

/// Apply the ledger and git history to every human gate of `registry`,
/// without emitting events. The caller decides whether to persist the result.
fn evaluate_human_gates_for_project(
    repo_root: &std::path::Path,
    registry: &mut TaskRegistry,
    now: &str,
) -> Result<Vec<HumanGateTransition>> {
    if !registry.tasks.iter().any(|t| t.is_human_approval_gate()) {
        return Ok(Vec::new());
    }
    let paths = crate::ProjectPaths::from_root(repo_root);
    let storage = crate::coordinator_storage::SqliteStorage::new(
        crate::coordinator_storage::CoordinatorStoragePaths::from_project_paths(&paths),
    );
    let p = crate::coordinator::decisions::policy(repo_root)?;
    let ledger = if p.enabled {
        crate::coordinator::decisions::approval_ledger_for_project(repo_root, registry)?
    } else {
        storage.gate_decisions_by_task()?
    };
    let branch = reference_branch(repo_root);
    Ok(reconcile_human_gates(
        registry,
        now,
        &|subject| subject_revision(repo_root, &branch, subject),
        &|task_id| ledger.get(task_id).cloned().unwrap_or_default(),
    ))
}

/// Gates waiting for a decision as they stand right now: the ledger and the
/// subject revisions are applied to a copy of `registry` (nothing is saved and
/// no event is emitted), so clients see a newly delivered subject or a new
/// subject revision without waiting for a coordinator cycle.
pub fn current_pending_approvals(
    repo_root: &std::path::Path,
    registry: &TaskRegistry,
) -> Vec<PendingApproval> {
    if crate::coordinator::decisions::policy(repo_root).is_ok_and(|p| {
        p.enabled
            && p.human_question_min_risk == crate::coordinator::decisions::Consultation::NeverAsk
    }) {
        return Vec::new();
    }
    let diff_stat = |rev: &str| revision_diff_stat(repo_root, rev);
    if !registry.tasks.iter().any(|t| t.is_human_approval_gate()) {
        return Vec::new();
    }
    let mut current = registry.clone();
    let now = crate::coordinator::helpers::now_iso_coordinator();
    if evaluate_human_gates_for_project(repo_root, &mut current, &now).is_err() {
        return pending_approvals(registry, &diff_stat);
    }
    pending_approvals(&current, &diff_stat)
}

/// A decision as submitted by an operator, before validation.
#[derive(Debug, Clone)]
pub struct DecisionRequest {
    pub task_id: String,
    pub decision: ApprovalDecision,
    pub role: String,
    pub actor: String,
    pub revision: Option<String>,
    pub evidence: Option<String>,
    pub reason: Option<String>,
}

/// Validate an operator decision against the gate and the current state, and
/// turn it into a ledger record. Nothing here trusts the performer: the only
/// caller is the CLI command.
pub fn validate_decision(
    registry: &TaskRegistry,
    request: &DecisionRequest,
    subject_digest: Option<&str>,
    now: &str,
) -> Result<ApprovalRecord> {
    let task = registry
        .tasks
        .iter()
        .find(|t| t.id == request.task_id)
        .ok_or_else(|| MaccError::Validation(format!("Unknown task '{}'.", request.task_id)))?;
    let gate = task
        .gate
        .as_ref()
        .filter(|g| g.is_human_approval())
        .ok_or_else(|| {
            MaccError::Validation(format!(
                "Task '{}' is not a human approval gate (gate.kind: human_approval).",
                task.id
            ))
        })?;
    if !matches!(
        task.workflow_state(),
        Some(
            WorkflowState::WaitingApproval
                | WorkflowState::Expired
                | WorkflowState::Approved
                | WorkflowState::Rejected
        )
    ) {
        return Err(MaccError::Validation(format!(
            "Gate '{}' is not open for decisions yet (state: {}). Its dependencies, including the subject {}, must be delivered first.",
            task.id,
            task.state,
            gate.subject_task.as_deref().unwrap_or("?")
        )));
    }
    let role = request.role.trim();
    let roles = gate.required_roles();
    if !roles.iter().any(|r| r == role) {
        return Err(MaccError::Validation(format!(
            "Role '{}' is not a required approver of '{}'. Required roles: {}.",
            role,
            task.id,
            roles.join(", ")
        )));
    }
    let actor = request.actor.trim();
    if actor.is_empty() {
        return Err(MaccError::Validation(
            "The decision has no identity: pass --as <name> or configure git user.name/user.email."
                .to_string(),
        ));
    }
    let revision = request
        .revision
        .as_deref()
        .map(str::trim)
        .filter(|r| !r.is_empty())
        .map(str::to_string);
    let revision = match (request.decision, revision, subject_digest) {
        (_, Some(given), Some(current)) => {
            if !same_revision(&given, current) {
                return Err(MaccError::Validation(format!(
                    "Revision '{}' is not the current revision of {} ({}). Review the current revision and decide on it.",
                    given,
                    gate.subject_task.as_deref().unwrap_or("the subject"),
                    current
                )));
            }
            Some(current.to_string())
        }
        (_, Some(given), None) => {
            if given.len() < MIN_REVISION_PREFIX {
                return Err(MaccError::Validation(format!(
                    "Revision '{given}' is too short; give at least {MIN_REVISION_PREFIX} characters of the commit SHA."
                )));
            }
            Some(given)
        }
        (ApprovalDecision::Approved, None, _) => {
            return Err(MaccError::Validation(format!(
                "An approval must name the revision it approves: --revision <commit-sha>{}.",
                subject_digest
                    .map(|d| format!(" (current revision of the subject: {d})"))
                    .unwrap_or_default()
            )));
        }
        (_, None, current) => current.map(str::to_string),
    };
    let evidence = request
        .evidence
        .as_deref()
        .map(str::trim)
        .filter(|e| !e.is_empty())
        .map(str::to_string);
    let evidence_required = gate
        .evidence_type
        .as_deref()
        .is_some_and(|kind| kind != "manual");
    if request.decision == ApprovalDecision::Approved && evidence_required && evidence.is_none() {
        return Err(MaccError::Validation(format!(
            "Gate '{}' requires {} evidence: pass --evidence <url-or-path>.",
            task.id,
            gate.evidence_type.as_deref().unwrap_or_default()
        )));
    }
    let reason = request
        .reason
        .as_deref()
        .map(str::trim)
        .filter(|r| !r.is_empty())
        .map(str::to_string);
    if request.decision != ApprovalDecision::Approved && reason.is_none() {
        return Err(MaccError::Validation(format!(
            "A {} decision needs --reason \"…\" so the subject author knows what to change.",
            request.decision.as_str()
        )));
    }
    Ok(ApprovalRecord {
        decision: request.decision,
        role: role.to_string(),
        actor: actor.to_string(),
        revision,
        evidence,
        reason,
        recorded_at: now.to_string(),
    })
}

/// Everything an approver needs to decide, for status/TUI/web display.
///
/// The same record feeds the CLI walk-through, the TUI approval window and the
/// web approval dialog, so every client explains the question identically.
#[derive(Debug, Clone, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(default)]
pub struct PendingApproval {
    pub task_id: String,
    pub title: Option<String>,
    pub state: String,
    pub status: String,
    pub subject_task: String,
    pub subject_revision: Option<String>,
    pub description: Option<String>,
    pub risks: Vec<String>,
    pub approval_trigger: Option<String>,
    pub governance_ref: Option<String>,
    pub required: String,
    pub missing_roles: Vec<String>,
    pub evidence_type: Option<String>,
    pub note: Option<String>,
    /// `git show --stat` of the subject revision, when known.
    pub diff_stat: Option<String>,
    pub approve_command: String,
    /// The question put to the approvers, in one sentence.
    pub question: String,
    /// True while the gate needs a decision now (waiting or expired, quorum
    /// not met). Clients open their approval window for these gates only; a
    /// rejected gate or one waiting for requested changes waits for a new
    /// subject revision instead.
    pub awaiting_decision: bool,
    pub subject_title: Option<String>,
    pub subject_objective: Option<String>,
    pub subject_description: Option<String>,
    /// What the subject task was asked to deliver (`result` in the PRD).
    pub subject_result: Option<String>,
    pub subject_acceptance_criteria: Vec<String>,
    /// Every declared role, in PRD order.
    pub required_roles: Vec<String>,
    /// The quorum rule, in words.
    pub quorum: String,
    /// Whether an approval must carry `--evidence`.
    pub evidence_required: bool,
    pub expires_after_days: Option<u32>,
    pub invalidate_on_subject_change: bool,
    /// Decisions that currently count (bound to the subject revision).
    pub decisions: Vec<ApprovalRecord>,
    /// Tasks waiting on this gate, as `ID — title`.
    pub unblocks: Vec<String>,
}

fn extra_text(task: &crate::coordinator::model::Task, key: &str) -> Option<String> {
    task.extra
        .get(key)
        .and_then(|v| v.as_str())
        .map(str::trim)
        .filter(|v| !v.is_empty())
        .map(str::to_string)
}

fn extra_list(task: &crate::coordinator::model::Task, key: &str) -> Vec<String> {
    task.extra
        .get(key)
        .and_then(|v| v.as_array())
        .map(|items| {
            items
                .iter()
                .filter_map(|v| v.as_str())
                .map(str::trim)
                .filter(|v| !v.is_empty())
                .map(str::to_string)
                .collect()
        })
        .unwrap_or_default()
}

/// The quorum rule of `gate`, in words.
pub fn quorum_text(gate: &TaskGate) -> String {
    use crate::coordinator::model::Quorum;
    let counted = if gate.required_approvers.iter().any(|r| r.count > 1) {
        " (each with its declared count)"
    } else {
        ""
    };
    match gate.quorum {
        Quorum::All if gate.required_approvers.len() == 1 => {
            format!(
                "the {} role must approve{counted}",
                gate.required_approvers[0].role
            )
        }
        Quorum::All => format!("every listed role must approve{counted}: one decision per role"),
        Quorum::Any => format!("one listed role approving is enough{counted}"),
        Quorum::Count(n) => format!("{n} approval(s) from the listed roles are enough"),
    }
}

/// First 12 characters of a revision, for display.
pub fn short_revision(revision: &str) -> &str {
    revision.get(..12).unwrap_or(revision)
}

/// Gates waiting for a human decision, with the command to decide them.
pub fn pending_approvals(
    registry: &TaskRegistry,
    diff_stat: &dyn Fn(&str) -> Option<String>,
) -> Vec<PendingApproval> {
    registry
        .tasks
        .iter()
        .filter(|t| t.is_human_approval_gate())
        .filter(|t| {
            matches!(
                t.workflow_state(),
                Some(
                    WorkflowState::WaitingApproval
                        | WorkflowState::Expired
                        | WorkflowState::Rejected
                )
            )
        })
        .filter_map(|task| {
            let gate = task.gate.as_ref()?;
            let approval = task.task_runtime.approval.clone().unwrap_or_default();
            let missing_roles = gate
                .required_approvers
                .iter()
                .filter(|r| {
                    approval
                        .effective
                        .iter()
                        .filter(|a| a.decision == ApprovalDecision::Approved && a.role == r.role)
                        .count()
                        < r.count
                })
                .map(|r| r.role.clone())
                .collect::<Vec<_>>();
            let role = missing_roles
                .first()
                .cloned()
                .unwrap_or_else(|| "<ROLE>".to_string());
            let revision = approval
                .subject_revision
                .clone()
                .unwrap_or_else(|| "<commit-sha>".to_string());
            let evidence_required = gate
                .evidence_type
                .as_deref()
                .is_some_and(|kind| kind != "manual");
            let evidence = if evidence_required {
                " --evidence <url>"
            } else {
                ""
            };
            let subject_id = gate.subject_task.clone().unwrap_or_default();
            let subject = registry.tasks.iter().find(|t| t.id == subject_id);
            let subject_title = subject.and_then(|t| t.title.clone());
            let question = format!(
                "Do you approve {}{} as delivered at revision {}?",
                subject_id,
                subject_title
                    .as_deref()
                    .map(|t| format!(" (\"{t}\")"))
                    .unwrap_or_default(),
                approval
                    .subject_revision
                    .as_deref()
                    .map(short_revision)
                    .unwrap_or("unknown")
            );
            let unblocks = registry
                .tasks
                .iter()
                .filter(|t| t.dependencies.iter().any(|d| d == &task.id))
                .map(|t| match t.title.as_deref() {
                    Some(title) => format!("{} — {}", t.id, title),
                    None => t.id.clone(),
                })
                .collect();
            Some(PendingApproval {
                task_id: task.id.clone(),
                title: task.title.clone(),
                state: task.state.clone(),
                status: approval.status.as_str().to_string(),
                subject_task: subject_id,
                subject_revision: approval.subject_revision.clone(),
                description: gate.description.clone(),
                risks: gate.risks.clone(),
                approval_trigger: gate.approval_trigger.clone(),
                governance_ref: gate.governance_ref.clone(),
                required: gate.approval_progress(&approval.effective),
                missing_roles,
                evidence_type: gate.evidence_type.clone(),
                note: approval.note.clone(),
                diff_stat: approval.subject_revision.as_deref().and_then(diff_stat),
                approve_command: format!(
                    "macc coordinator approve {} --role {} --revision {}{}",
                    task.id, role, revision, evidence
                ),
                question,
                awaiting_decision: task.is_awaiting_approval()
                    && matches!(
                        approval.status,
                        ApprovalStatus::WaitingApproval | ApprovalStatus::Expired
                    ),
                subject_title,
                subject_objective: subject.and_then(|t| extra_text(t, "objective")),
                subject_description: subject.and_then(|t| extra_text(t, "description")),
                subject_result: subject.and_then(|t| extra_text(t, "result")),
                subject_acceptance_criteria: subject
                    .map(|t| extra_list(t, "acceptance_criteria"))
                    .unwrap_or_default(),
                required_roles: gate.required_roles(),
                quorum: quorum_text(gate),
                evidence_required,
                expires_after_days: gate.expires_after_days,
                invalidate_on_subject_change: gate.invalidate_on_subject_change,
                decisions: approval.effective.clone(),
                unblocks,
            })
        })
        .collect()
}

/// One titled block of an approval explanation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BriefSection {
    pub title: &'static str,
    pub lines: Vec<String>,
}

/// The full explanation of an approval request, section by section, shared by
/// the CLI walk-through and the TUI approval window.
pub fn approval_brief(p: &PendingApproval) -> Vec<BriefSection> {
    let mut sections = Vec::new();
    let mut push = |title: &'static str, lines: Vec<String>| {
        let lines: Vec<String> = lines.into_iter().filter(|l| !l.trim().is_empty()).collect();
        if !lines.is_empty() {
            sections.push(BriefSection { title, lines });
        }
    };
    push("Question", vec![p.question.clone()]);
    push(
        "Decision",
        p.description.iter().cloned().collect::<Vec<_>>(),
    );
    let mut why = Vec::new();
    if let Some(trigger) = p.approval_trigger.as_deref() {
        why.push(format!("Trigger: {trigger}"));
    }
    if let Some(governance) = p.governance_ref.as_deref() {
        why.push(format!("Decision rights: {governance}"));
    }
    push("Why people decide", why);
    let mut subject = vec![format!(
        "{}{}",
        p.subject_task,
        p.subject_title
            .as_deref()
            .map(|t| format!(" — {t}"))
            .unwrap_or_default()
    )];
    subject.push(format!(
        "Revision: {}",
        p.subject_revision
            .as_deref()
            .unwrap_or("unknown (no commit carries its [macc:task] trailer yet)")
    ));
    if let Some(objective) = p.subject_objective.as_deref() {
        subject.push(format!("Objective: {objective}"));
    }
    if let Some(result) = p.subject_result.as_deref() {
        subject.push(format!("Expected result: {result}"));
    }
    push("What is being approved", subject);
    push(
        "Subject description",
        p.subject_description
            .as_deref()
            .map(|d| d.lines().map(str::to_string).collect())
            .unwrap_or_default(),
    );
    push(
        "Acceptance criteria",
        p.subject_acceptance_criteria
            .iter()
            .map(|c| format!("- {c}"))
            .collect(),
    );
    push(
        "Changes in the revision",
        p.diff_stat
            .as_deref()
            .map(|d| d.lines().map(str::to_string).collect())
            .unwrap_or_default(),
    );
    push(
        "Risks to weigh",
        p.risks.iter().map(|r| format!("- {r}")).collect(),
    );
    let mut approvers = vec![
        format!("Progress: {}", p.required),
        format!("Rule: {}", p.quorum),
    ];
    if !p.missing_roles.is_empty() {
        approvers.push(format!("Still needed: {}", p.missing_roles.join(", ")));
    }
    push("Approvers", approvers);
    push(
        "Decisions so far",
        p.decisions
            .iter()
            .map(|d| {
                format!(
                    "- {} by {} ({}) at {}{}{}",
                    d.decision.as_str(),
                    d.actor,
                    d.role,
                    d.recorded_at,
                    d.evidence
                        .as_deref()
                        .map(|e| format!(" — evidence: {e}"))
                        .unwrap_or_default(),
                    d.reason
                        .as_deref()
                        .map(|r| format!(" — reason: {r}"))
                        .unwrap_or_default()
                )
            })
            .collect(),
    );
    let mut rules = Vec::new();
    rules.push(match (p.evidence_required, p.evidence_type.as_deref()) {
        (true, Some(kind)) => format!("An approval must cite {kind} evidence (a link or path)."),
        _ => "No evidence is required with an approval.".to_string(),
    });
    rules.push("A rejection or a change request must give a reason.".to_string());
    if p.invalidate_on_subject_change {
        rules.push(format!(
            "Decisions are bound to this revision: a new revision of {} cancels them.",
            p.subject_task
        ));
    }
    if let Some(days) = p.expires_after_days {
        rules.push(format!("Approvals expire after {days} day(s)."));
    }
    push("Rules", rules);
    let unblocks = if p.unblocks.is_empty() {
        "nothing else waits on this gate".to_string()
    } else {
        p.unblocks.join(", ")
    };
    push(
        "Consequences",
        vec![
            format!("Approve: once the quorum is met, the run continues with {unblocks}."),
            format!(
                "Reject: the gate fails (E909) and its dependants stay blocked (E905) until {} is revised.",
                p.subject_task
            ),
            format!(
                "Request changes: the gate waits for a new revision of {}.",
                p.subject_task
            ),
        ],
    );
    push("Status", p.note.iter().cloned().collect::<Vec<_>>());
    sections
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::coordinator::model::{Quorum, RequiredApprover};
    use serde_json::json;

    const SHA: &str = "abcdef1234567890abcdef1234567890abcdef12";
    const SHA2: &str = "1111111222222233333334444444555555566666";

    fn gate() -> TaskGate {
        TaskGate {
            kind: crate::coordinator::model::GateKind::HumanApproval,
            subject_task: Some("SEC-ADR-003".into()),
            required_approvers: vec![
                RequiredApprover {
                    role: "PRODUCT_OWNER".into(),
                    count: 1,
                },
                RequiredApprover {
                    role: "SECURITY_OWNER".into(),
                    count: 1,
                },
            ],
            quorum: Quorum::All,
            bind_to: "commit_sha".into(),
            evidence_type: Some("pull_request_review".into()),
            invalidate_on_subject_change: true,
            ..TaskGate::default()
        }
    }

    fn rec(
        decision: ApprovalDecision,
        role: &str,
        actor: &str,
        rev: &str,
        at: &str,
    ) -> ApprovalRecord {
        ApprovalRecord {
            decision,
            role: role.into(),
            actor: actor.into(),
            revision: Some(rev.into()),
            evidence: Some("https://example/pr/1".into()),
            reason: Some("r".into()),
            recorded_at: at.into(),
        }
    }

    const NOW: &str = "2026-09-21T12:00:00Z";

    #[test]
    fn stays_todo_until_dependencies_are_delivered() {
        assert!(evaluate_gate(&gate(), false, Some(SHA), &[], NOW).is_none());
    }

    #[test]
    fn waits_until_every_role_has_approved_then_approves() {
        let one = [rec(
            ApprovalDecision::Approved,
            "PRODUCT_OWNER",
            "alice",
            SHA,
            NOW,
        )];
        let e = evaluate_gate(&gate(), true, Some(SHA), &one, NOW).unwrap();
        assert_eq!(e.state, WorkflowState::WaitingApproval);
        assert!(
            e.note.contains("PRODUCT_OWNER 1/1, SECURITY_OWNER 0/1"),
            "{}",
            e.note
        );

        let both = [
            one[0].clone(),
            rec(
                ApprovalDecision::Approved,
                "SECURITY_OWNER",
                "bob",
                &SHA[..7],
                NOW,
            ),
        ];
        let e = evaluate_gate(&gate(), true, Some(SHA), &both, NOW).unwrap();
        assert_eq!(e.state, WorkflowState::Approved);
    }

    #[test]
    fn same_actor_twice_does_not_fill_a_count_of_two() {
        let mut g = gate();
        g.required_approvers = vec![RequiredApprover {
            role: "REVIEWER".into(),
            count: 2,
        }];
        let d = [
            rec(ApprovalDecision::Approved, "REVIEWER", "alice", SHA, NOW),
            rec(ApprovalDecision::Approved, "REVIEWER", "alice", SHA, NOW),
        ];
        let e = evaluate_gate(&g, true, Some(SHA), &d, NOW).unwrap();
        assert_eq!(e.state, WorkflowState::WaitingApproval);
    }

    #[test]
    fn a_subject_change_invalidates_prior_approvals() {
        let d = [
            rec(
                ApprovalDecision::Approved,
                "PRODUCT_OWNER",
                "alice",
                SHA,
                NOW,
            ),
            rec(
                ApprovalDecision::Approved,
                "SECURITY_OWNER",
                "bob",
                SHA,
                NOW,
            ),
        ];
        let e = evaluate_gate(&gate(), true, Some(SHA2), &d, NOW).unwrap();
        assert_eq!(e.state, WorkflowState::WaitingApproval);
        assert!(e.effective.is_empty());
    }

    #[test]
    fn a_rejection_blocks_until_the_rejecter_changes_their_decision() {
        let mut d = vec![
            rec(
                ApprovalDecision::Approved,
                "PRODUCT_OWNER",
                "alice",
                SHA,
                NOW,
            ),
            rec(
                ApprovalDecision::Rejected,
                "SECURITY_OWNER",
                "bob",
                SHA,
                NOW,
            ),
        ];
        let e = evaluate_gate(&gate(), true, Some(SHA), &d, NOW).unwrap();
        assert_eq!(e.state, WorkflowState::Rejected);
        d.push(rec(
            ApprovalDecision::Approved,
            "SECURITY_OWNER",
            "bob",
            SHA,
            NOW,
        ));
        let e = evaluate_gate(&gate(), true, Some(SHA), &d, NOW).unwrap();
        assert_eq!(
            e.state,
            WorkflowState::Approved,
            "latest decision per actor wins"
        );
    }

    #[test]
    fn changes_requested_keeps_the_gate_waiting_with_that_status() {
        let d = [rec(
            ApprovalDecision::ChangesRequested,
            "PRODUCT_OWNER",
            "alice",
            SHA,
            NOW,
        )];
        let e = evaluate_gate(&gate(), true, Some(SHA), &d, NOW).unwrap();
        assert_eq!(e.state, WorkflowState::WaitingApproval);
        assert_eq!(e.status, ApprovalStatus::ChangesRequested);
    }

    #[test]
    fn approvals_older_than_the_window_expire() {
        let mut g = gate();
        g.expires_after_days = Some(30);
        let old = "2026-07-01T00:00:00Z";
        let d = [
            rec(
                ApprovalDecision::Approved,
                "PRODUCT_OWNER",
                "alice",
                SHA,
                old,
            ),
            rec(
                ApprovalDecision::Approved,
                "SECURITY_OWNER",
                "bob",
                SHA,
                old,
            ),
        ];
        let e = evaluate_gate(&g, true, Some(SHA), &d, NOW).unwrap();
        assert_eq!(e.state, WorkflowState::Expired);
    }

    #[test]
    fn quorum_any_and_count() {
        let mut g = gate();
        let d = [rec(
            ApprovalDecision::Approved,
            "PRODUCT_OWNER",
            "alice",
            SHA,
            NOW,
        )];
        g.quorum = Quorum::Any;
        assert_eq!(
            evaluate_gate(&g, true, Some(SHA), &d, NOW).unwrap().state,
            WorkflowState::Approved
        );
        g.quorum = Quorum::Count(2);
        assert_eq!(
            evaluate_gate(&g, true, Some(SHA), &d, NOW).unwrap().state,
            WorkflowState::WaitingApproval
        );
    }

    fn registry(state: &str) -> TaskRegistry {
        TaskRegistry::from_value(&json!({"tasks":[
            {"id":"SEC-ADR-003","state":"merged"},
            {"id":"SEC-APP-003","state":state,"dependencies":["SEC-ADR-003"],
             "gate":{"kind":"human_approval","subject_task":"SEC-ADR-003",
                     "required_approvers":[{"role":"PRODUCT_OWNER","count":1}],
                     "evidence_type":"pull_request_review"}},
            {"id":"SEC-DB-004","state":"todo","dependencies":["SEC-APP-003"]}
        ]}))
        .unwrap()
    }

    #[test]
    fn reconcile_opens_the_gate_and_never_leaves_it_dispatchable() {
        let mut reg = registry("todo");
        let t = reconcile_human_gates(&mut reg, NOW, &|_| Some(SHA.into()), &|_| vec![]);
        assert_eq!(t[0].event, "approval_requested");
        let gate = reg.tasks.iter().find(|t| t.id == "SEC-APP-003").unwrap();
        assert_eq!(gate.state, "waiting_approval");
        assert_eq!(
            gate.task_runtime
                .approval
                .as_ref()
                .unwrap()
                .subject_revision
                .as_deref(),
            Some(SHA)
        );
        // The dependant is not satisfied by a waiting gate.
        let sat = crate::coordinator::task_selector::satisfied_dependency_ids(&reg);
        assert!(!sat.contains("SEC-APP-003"));
    }

    #[test]
    fn a_forged_approved_state_is_reverted_without_a_recorded_quorum() {
        let mut reg = registry("approved");
        let t = reconcile_human_gates(&mut reg, NOW, &|_| Some(SHA.into()), &|_| vec![]);
        let gate = reg.tasks.iter().find(|t| t.id == "SEC-APP-003").unwrap();
        assert_eq!(gate.state, "waiting_approval");
        assert_eq!(t[0].event, "approval_unverified");
    }

    #[test]
    fn a_recorded_quorum_approves_and_satisfies_dependants() {
        let mut reg = registry("waiting_approval");
        let ledger = vec![rec(
            ApprovalDecision::Approved,
            "PRODUCT_OWNER",
            "alice",
            SHA,
            NOW,
        )];
        let t = reconcile_human_gates(&mut reg, NOW, &|_| Some(SHA.into()), &|_| ledger.clone());
        assert_eq!(t[0].event, "gate_approved");
        let sat = crate::coordinator::task_selector::satisfied_dependency_ids(&reg);
        assert!(sat.contains("SEC-APP-003"));
    }

    #[test]
    fn a_subject_change_after_approval_emits_invalidation() {
        let mut reg = registry("waiting_approval");
        let ledger = vec![rec(
            ApprovalDecision::Approved,
            "PRODUCT_OWNER",
            "alice",
            SHA,
            NOW,
        )];
        reconcile_human_gates(&mut reg, NOW, &|_| Some(SHA.into()), &|_| ledger.clone());
        let t = reconcile_human_gates(&mut reg, NOW, &|_| Some(SHA2.into()), &|_| ledger.clone());
        assert_eq!(t[0].event, "approval_invalidated");
        let gate = reg.tasks.iter().find(|t| t.id == "SEC-APP-003").unwrap();
        assert_eq!(gate.state, "waiting_approval");
    }

    fn request(decision: ApprovalDecision, role: &str, rev: Option<&str>) -> DecisionRequest {
        DecisionRequest {
            task_id: "SEC-APP-003".into(),
            decision,
            role: role.into(),
            actor: "alice".into(),
            revision: rev.map(str::to_string),
            evidence: Some("https://example/pr/7".into()),
            reason: None,
        }
    }

    #[test]
    fn decisions_are_validated_against_the_gate() {
        let mut reg = registry("todo");
        // Not open yet.
        let err = validate_decision(
            &reg,
            &request(ApprovalDecision::Approved, "PRODUCT_OWNER", Some(SHA)),
            Some(SHA),
            NOW,
        );
        assert!(err
            .unwrap_err()
            .to_string()
            .contains("not open for decisions"));
        reconcile_human_gates(&mut reg, NOW, &|_| Some(SHA.into()), &|_| vec![]);
        // Unknown role.
        let err = validate_decision(
            &reg,
            &request(ApprovalDecision::Approved, "CEO", Some(SHA)),
            Some(SHA),
            NOW,
        );
        assert!(err
            .unwrap_err()
            .to_string()
            .contains("Required roles: PRODUCT_OWNER"));
        // Wrong revision.
        let err = validate_decision(
            &reg,
            &request(ApprovalDecision::Approved, "PRODUCT_OWNER", Some(SHA2)),
            Some(SHA),
            NOW,
        );
        assert!(err
            .unwrap_err()
            .to_string()
            .contains("not the current revision"));
        // Approval without revision.
        let err = validate_decision(
            &reg,
            &request(ApprovalDecision::Approved, "PRODUCT_OWNER", None),
            Some(SHA),
            NOW,
        );
        assert!(err.unwrap_err().to_string().contains("--revision"));
        // Missing evidence.
        let mut r = request(ApprovalDecision::Approved, "PRODUCT_OWNER", Some(&SHA[..8]));
        r.evidence = None;
        assert!(validate_decision(&reg, &r, Some(SHA), NOW)
            .unwrap_err()
            .to_string()
            .contains("--evidence"));
        // Rejection without reason.
        let err = validate_decision(
            &reg,
            &request(ApprovalDecision::Rejected, "PRODUCT_OWNER", None),
            Some(SHA),
            NOW,
        );
        assert!(err.unwrap_err().to_string().contains("--reason"));
        // Valid: short SHA is normalised to the full current revision.
        let ok = validate_decision(
            &reg,
            &request(ApprovalDecision::Approved, "PRODUCT_OWNER", Some(&SHA[..8])),
            Some(SHA),
            NOW,
        )
        .unwrap();
        assert_eq!(ok.revision.as_deref(), Some(SHA));
    }

    #[test]
    fn non_gate_tasks_cannot_be_approved() {
        let reg = registry("todo");
        let mut r = request(ApprovalDecision::Approved, "PRODUCT_OWNER", Some(SHA));
        r.task_id = "SEC-DB-004".into();
        assert!(validate_decision(&reg, &r, Some(SHA), NOW)
            .unwrap_err()
            .to_string()
            .contains("not a human approval gate"));
    }

    #[test]
    fn pending_view_names_missing_roles_and_the_exact_command() {
        let mut reg = registry("todo");
        reconcile_human_gates(&mut reg, NOW, &|_| Some(SHA.into()), &|_| vec![]);
        let pending = pending_approvals(&reg, &|_| Some(" docs/adr/003.md | 40 +".into()));
        assert_eq!(pending.len(), 1);
        assert_eq!(pending[0].missing_roles, vec!["PRODUCT_OWNER".to_string()]);
        assert_eq!(
            pending[0].approve_command,
            format!("macc coordinator approve SEC-APP-003 --role PRODUCT_OWNER --revision {SHA} --evidence <url>")
        );
        assert!(pending[0].diff_stat.as_deref().unwrap().contains("003.md"));
    }

    #[test]
    fn pending_view_explains_the_subject_rules_and_consequences() {
        let mut reg = TaskRegistry::from_value(&json!({"tasks":[
            {"id":"SEC-ADR-003","state":"merged","title":"Record the session ADR",
             "objective":"Decide the session model.","result":"ADR 003 merged.",
             "description":"Problem/Goal: sessions.\nKey actions: write ADR 003.",
             "acceptance_criteria":["ADR 003 lists the options."]},
            {"id":"SEC-APP-003","state":"todo","dependencies":["SEC-ADR-003"],
             "gate":{"kind":"human_approval","subject_task":"SEC-ADR-003",
                     "description":"Adopt ADR 003",
                     "required_approvers":[{"role":"PRODUCT_OWNER","count":1},
                                           {"role":"SECURITY_OWNER","count":1}],
                     "evidence_type":"pull_request_review","expires_after_days":30,
                     "risks":["Wrong session model."]}},
            {"id":"SEC-DB-004","state":"todo","title":"Session table","dependencies":["SEC-APP-003"]}
        ]}))
        .unwrap();
        reconcile_human_gates(&mut reg, NOW, &|_| Some(SHA.into()), &|_| {
            vec![rec(
                ApprovalDecision::Approved,
                "PRODUCT_OWNER",
                "ann",
                SHA,
                NOW,
            )]
        });
        let p = &pending_approvals(&reg, &|_| None)[0];
        assert!(p.awaiting_decision);
        assert_eq!(
            p.question,
            "Do you approve SEC-ADR-003 (\"Record the session ADR\") as delivered at revision abcdef123456?"
        );
        assert_eq!(
            p.subject_objective.as_deref(),
            Some("Decide the session model.")
        );
        assert_eq!(
            p.subject_acceptance_criteria,
            vec!["ADR 003 lists the options."]
        );
        assert_eq!(p.required_roles, vec!["PRODUCT_OWNER", "SECURITY_OWNER"]);
        assert_eq!(p.missing_roles, vec!["SECURITY_OWNER"]);
        assert_eq!(p.decisions.len(), 1);
        assert_eq!(p.unblocks, vec!["SEC-DB-004 — Session table"]);
        assert!(p.evidence_required);
        assert!(p.quorum.contains("every listed role"));

        let brief = approval_brief(p);
        let text = |title: &str| {
            brief
                .iter()
                .find(|s| s.title == title)
                .map(|s| s.lines.join("\n"))
                .unwrap_or_default()
        };
        assert!(text("Decision").contains("Adopt ADR 003"));
        assert!(text("Subject description").contains("Key actions: write ADR 003."));
        assert!(text("Decisions so far").contains("approved by ann (PRODUCT_OWNER)"));
        assert!(text("Rules").contains("expire after 30 day(s)"));
        assert!(text("Consequences").contains("SEC-DB-004 — Session table"));
        assert!(text("Approvers").contains("Still needed: SECURITY_OWNER"));
    }

    #[test]
    fn a_rejected_gate_is_listed_but_not_awaiting_a_decision() {
        let mut reg = registry("todo");
        reconcile_human_gates(&mut reg, NOW, &|_| Some(SHA.into()), &|_| {
            vec![rec(
                ApprovalDecision::Rejected,
                "PRODUCT_OWNER",
                "ann",
                SHA,
                NOW,
            )]
        });
        let pending = pending_approvals(&reg, &|_| None);
        assert_eq!(pending.len(), 1);
        assert!(!pending[0].awaiting_decision);
    }
}
