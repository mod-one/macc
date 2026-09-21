use crate::coordinator::{RuntimeStatus, WorkflowState};
use crate::{MaccError, Result};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet, HashSet};
use std::str::FromStr;

#[derive(Debug, Clone, Copy, Serialize, Deserialize, Default, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum GateVerdict {
    #[default]
    Pending,
    Accepted,
    Rejected,
}

impl FromStr for GateVerdict {
    type Err = String;

    fn from_str(value: &str) -> std::result::Result<Self, Self::Err> {
        match value.trim().to_ascii_lowercase().as_str() {
            "pending" | "unknown" => Ok(Self::Pending),
            "accepted" | "accept" | "positive" | "passed" => Ok(Self::Accepted),
            "rejected" | "reject" | "negative" | "not_accepted" | "failed" => Ok(Self::Rejected),
            other => Err(format!("unknown gate verdict: {other}")),
        }
    }
}

/// What kind of decision a gate task represents.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, Default, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum GateKind {
    /// Evaluated by the tool that runs the task (`MACC_TASK_GATE_VERDICT`).
    #[default]
    Verdict,
    /// Decided by named humans through `macc coordinator approve`. Never
    /// dispatched to a performer; an agent cannot produce this approval.
    HumanApproval,
}

/// One role that must sign off, and how many holders of it.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct RequiredApprover {
    pub role: String,
    #[serde(default = "default_one")]
    pub count: usize,
}

fn default_one() -> usize {
    1
}

/// How many of `required_approvers` must be satisfied. Written in the PRD as
/// `"all"` (default), `"any"`, or a number.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum Quorum {
    /// Every declared role must reach its count (the default).
    #[default]
    All,
    /// Any single declared role reaching its count is enough.
    Any,
    /// At least this many distinct approvals, across declared roles.
    Count(usize),
}

impl Serialize for Quorum {
    fn serialize<S: serde::Serializer>(
        &self,
        serializer: S,
    ) -> std::result::Result<S::Ok, S::Error> {
        match self {
            Quorum::All => serializer.serialize_str("all"),
            Quorum::Any => serializer.serialize_str("any"),
            Quorum::Count(n) => serializer.serialize_u64(*n as u64),
        }
    }
}

impl<'de> Deserialize<'de> for Quorum {
    fn deserialize<D: serde::Deserializer<'de>>(
        deserializer: D,
    ) -> std::result::Result<Self, D::Error> {
        match Value::deserialize(deserializer)? {
            Value::String(s) => match s.trim().to_ascii_lowercase().as_str() {
                "all" => Ok(Quorum::All),
                "any" => Ok(Quorum::Any),
                other => other.parse::<usize>().map(Quorum::Count).map_err(|_| {
                    serde::de::Error::custom(format!(
                        "invalid gate quorum '{other}': use \"all\", \"any\" or a number"
                    ))
                }),
            },
            Value::Number(n) => n
                .as_u64()
                .map(|n| Quorum::Count(n as usize))
                .ok_or_else(|| serde::de::Error::custom("gate quorum must be a positive integer")),
            other => Err(serde::de::Error::custom(format!(
                "invalid gate quorum {other}: use \"all\", \"any\" or a number"
            ))),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq, Eq)]
pub struct TaskGate {
    #[serde(default = "default_required_gate_verdict")]
    pub required_verdict: GateVerdict,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    /// `verdict` (default, agent-evaluated) or `human_approval`.
    #[serde(default)]
    pub kind: GateKind,
    /// The task whose delivered revision is being approved. Must be one of
    /// this gate's dependencies so the subject is merged before approval.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub subject_task: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub required_approvers: Vec<RequiredApprover>,
    #[serde(default)]
    pub quorum: Quorum,
    /// What an approval is bound to. Only `commit_sha` is supported: the
    /// approval names the subject revision and is invalidated when it moves.
    #[serde(default = "default_bind_to")]
    pub bind_to: String,
    /// Where the durable human proof lives (`pull_request_review`, `adr`,
    /// `changelog`, `manual`). Informational; recorded with each approval.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub evidence_type: Option<String>,
    #[serde(default = "default_true")]
    pub invalidate_on_subject_change: bool,
    /// Approvals older than this are expired and must be renewed.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub expires_after_days: Option<u32>,
    /// Risks the approvers must weigh, declared by the planner and shown with
    /// the approval request.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub risks: Vec<String>,
    /// Specification governance source the required roles were derived from.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub governance_ref: Option<String>,
    /// Why this decision needs people (`adr`, `irreversible-migration`,
    /// `security`, …), as declared by the planner.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub approval_trigger: Option<String>,
}

fn default_required_gate_verdict() -> GateVerdict {
    GateVerdict::Accepted
}

fn default_bind_to() -> String {
    "commit_sha".to_string()
}

fn default_true() -> bool {
    true
}

impl TaskGate {
    pub fn is_human_approval(&self) -> bool {
        self.kind == GateKind::HumanApproval
    }

    /// Structural rules a `human_approval` gate must satisfy before the
    /// scheduler will honour it. Returned as a list so a PRD author sees every
    /// defect at once.
    pub fn validate_human_approval(&self, task_id: &str, dependencies: &[String]) -> Vec<String> {
        let mut problems = Vec::new();
        if !self.is_human_approval() {
            return problems;
        }
        match self.subject_task.as_deref().map(str::trim) {
            None | Some("") => problems.push(format!(
                "{task_id}: gate.subject_task is required for a human_approval gate"
            )),
            Some(subject) if !dependencies.iter().any(|dep| dep == subject) => {
                problems.push(format!(
                    "{task_id}: gate.subject_task '{subject}' must also be listed in dependencies so it is delivered before approval"
                ))
            }
            _ => {}
        }
        if self.required_approvers.is_empty() {
            problems.push(format!(
                "{task_id}: gate.required_approvers must name at least one role"
            ));
        }
        for approver in &self.required_approvers {
            if approver.role.trim().is_empty() {
                problems.push(format!("{task_id}: a required approver has an empty role"));
            }
            if approver.count == 0 {
                problems.push(format!(
                    "{task_id}: required approver '{}' has count 0",
                    approver.role
                ));
            }
        }
        if let Quorum::Count(n) = self.quorum {
            let max: usize = self.required_approvers.iter().map(|a| a.count).sum();
            if n == 0 || n > max {
                problems.push(format!(
                    "{task_id}: gate.quorum {n} is outside 1..={max} (the total approvals declared)"
                ));
            }
        }
        if self.bind_to != "commit_sha" {
            problems.push(format!(
                "{task_id}: gate.bind_to '{}' is not supported; use commit_sha",
                self.bind_to
            ));
        }
        problems
    }

    /// Whether `approvals` (already filtered to the current round) satisfy the
    /// declared quorum.
    pub fn quorum_met(&self, approvals: &[ApprovalRecord]) -> bool {
        // Distinct people per role: one person approving twice is one approval.
        let approved = |role: &str| {
            approvals
                .iter()
                .filter(|a| a.decision == ApprovalDecision::Approved && a.role == role)
                .map(|a| a.actor.as_str())
                .collect::<HashSet<_>>()
                .len()
        };
        match self.quorum {
            Quorum::All => self
                .required_approvers
                .iter()
                .all(|r| approved(&r.role) >= r.count),
            Quorum::Any => self
                .required_approvers
                .iter()
                .any(|r| approved(&r.role) >= r.count),
            Quorum::Count(n) => {
                let declared: HashSet<&str> = self
                    .required_approvers
                    .iter()
                    .map(|r| r.role.as_str())
                    .collect();
                approvals
                    .iter()
                    .filter(|a| {
                        a.decision == ApprovalDecision::Approved
                            && declared.contains(a.role.as_str())
                    })
                    .map(|a| (a.actor.as_str(), a.role.as_str()))
                    .collect::<HashSet<_>>()
                    .len()
                    >= n
            }
        }
    }

    /// "ROLE 1/2, ROLE2 0/1" for display.
    pub fn approval_progress(&self, approvals: &[ApprovalRecord]) -> String {
        self.required_approvers
            .iter()
            .map(|r| {
                let have = approvals
                    .iter()
                    .filter(|a| a.decision == ApprovalDecision::Approved && a.role == r.role)
                    .map(|a| a.actor.as_str())
                    .collect::<HashSet<_>>()
                    .len();
                format!("{} {}/{}", r.role, have.min(r.count), r.count)
            })
            .collect::<Vec<_>>()
            .join(", ")
    }

    pub fn required_roles(&self) -> Vec<String> {
        self.required_approvers
            .iter()
            .map(|r| r.role.clone())
            .collect()
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ApprovalDecision {
    Approved,
    Rejected,
    ChangesRequested,
}

impl ApprovalDecision {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Approved => "approved",
            Self::Rejected => "rejected",
            Self::ChangesRequested => "changes_requested",
        }
    }
}

impl FromStr for ApprovalDecision {
    type Err = String;

    fn from_str(value: &str) -> std::result::Result<Self, Self::Err> {
        match value.trim().to_ascii_lowercase().replace('-', "_").as_str() {
            "approved" | "approve" => Ok(Self::Approved),
            "rejected" | "reject" => Ok(Self::Rejected),
            "changes_requested" | "request_changes" => Ok(Self::ChangesRequested),
            other => Err(format!("unknown approval decision: {other}")),
        }
    }
}

/// One human decision, as recorded by `macc coordinator approve|reject|request-changes`.
/// Written only by the CLI path; no performer event can create or alter one.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ApprovalRecord {
    pub decision: ApprovalDecision,
    pub role: String,
    /// Who decided: `--as`, else the git identity of the operator.
    pub actor: String,
    /// The subject revision the decision applies to (`bind_to: commit_sha`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub revision: Option<String>,
    /// Durable proof: pull-request review URL, ADR path, changelog entry…
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub evidence: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
    pub recorded_at: String,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, Default, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ApprovalStatus {
    #[default]
    WaitingApproval,
    ChangesRequested,
    Approved,
    Rejected,
    Expired,
}

impl ApprovalStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::WaitingApproval => "waiting_approval",
            Self::ChangesRequested => "changes_requested",
            Self::Approved => "approved",
            Self::Rejected => "rejected",
            Self::Expired => "expired",
        }
    }
}

/// Operational approval state of a `human_approval` gate, derived every cycle
/// from the append-only decision ledger (`gate_decisions` table). It is a
/// cache for display: nothing reads it to decide whether a gate is approved.
#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq, Eq)]
pub struct ApprovalState {
    #[serde(default)]
    pub status: ApprovalStatus,
    /// The subject revision decisions must be bound to: the latest commit
    /// carrying `[macc:task <subject>]` on the reference branch, or, when the
    /// subject has no such commit, the revision named by the latest decision.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub subject_revision: Option<String>,
    /// Decisions that currently count: bound to `subject_revision`, not
    /// expired, latest per (actor, role).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub effective: Vec<ApprovalRecord>,
    /// Every decision ever recorded for this gate, oldest first (audit trail).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub history: Vec<ApprovalRecord>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub requested_at: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub approved_at: Option<String>,
    /// The revision the gate was last approved at; a different current
    /// revision means the approval was invalidated by a subject change.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub approved_revision: Option<String>,
    /// Human-readable explanation of the current status.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, Default, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ExternalBlockSource {
    #[default]
    Prd,
    Operator,
}

fn is_prd_block_source(source: &ExternalBlockSource) -> bool {
    *source == ExternalBlockSource::Prd
}

#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq, Eq)]
pub struct ExternalTaskBlock {
    pub reason: String,
    pub clears_when: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tracking_id: Option<String>,
    #[serde(default, skip_serializing_if = "is_prd_block_source")]
    pub source: ExternalBlockSource,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq, Eq)]
pub struct ExternalBlockResolution {
    pub evidence: String,
    pub resolved_at: String,
    pub block_reason: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tracking_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq)]
pub struct TaskRegistry {
    #[serde(default)]
    pub tasks: Vec<Task>,
    #[serde(default)]
    pub resource_locks: BTreeMap<String, ResourceLock>,
    /// Task IDs already delivered (committed + merged) by **prior PRD files
    /// or earlier coordinator runs** whose tasks are not part of this
    /// registry's `tasks` array. Populated by `sync-prd` from commit-trailer
    /// scanning of the reference branch, and consulted by the dispatcher to
    /// satisfy cross-PRD dependency edges that would otherwise look unmet.
    #[serde(default, skip_serializing_if = "BTreeSet::is_empty")]
    pub external_merged_task_ids: BTreeSet<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub updated_at: Option<String>,
    #[serde(flatten)]
    pub extra: BTreeMap<String, Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq)]
pub struct Task {
    #[serde(default)]
    pub id: String,
    #[serde(default = "default_todo_state")]
    pub state: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub priority: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub category: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub scope: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub base_branch: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub coordinator_tool: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub assignee: Option<Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub claimed_at: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pr_url: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub review: Option<TaskReview>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub worktree: Option<TaskWorktree>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub dependencies: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub exclusive_resources: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub blocked_on_external: Option<ExternalTaskBlock>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub gate: Option<TaskGate>,
    #[serde(default, skip_serializing_if = "is_default_task_runtime")]
    pub task_runtime: TaskRuntime,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub state_changed_at: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub updated_at: Option<String>,
    #[serde(flatten)]
    pub extra: BTreeMap<String, Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq)]
pub struct TaskWorktree {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub worktree_path: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub branch: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub base_branch: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_commit: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub session_id: Option<String>,
    #[serde(flatten)]
    pub extra: BTreeMap<String, Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq)]
pub struct TaskReview {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub changed: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_reviewed_at: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reviewer: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
    #[serde(flatten)]
    pub extra: BTreeMap<String, Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq)]
pub struct TaskRuntime {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub claim_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub run_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub coordinator_epoch: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub process_group_id: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub heartbeat_seq: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub lease_expires_at: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub locked_resources_json: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub status: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pid: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub current_phase: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub progress: Option<u8>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub worker_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub worktree: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub branch: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stdout_log: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stderr_log: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub events_log: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_event_at: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub registry_updated_at: Option<String>,
    #[serde(default)]
    pub last_error: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_error_code: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_error_origin: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_error_message: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_heartbeat: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub started_at: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub phase_started_at: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub attempt: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub retries: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub completion_kind: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub metrics: Option<TaskRuntimeMetrics>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub slo_warnings: Option<BTreeMap<String, SloWarningRecord>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub merge_result_pending: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub merge_result_file: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub merge_worker_pid: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub merge_result_started_at: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_merge_result_file: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_merge_result_status: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_merge_result_rc: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_merge_result_at: Option<String>,
    /// ISO 8601 timestamp before which this task must not be re-dispatched.
    /// Set by the backoff engine when a rate-limit (E601) is received.
    /// Cleared on successful dispatch or when the timestamp is in the past.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub delayed_until: Option<String>,
    /// Number of review cycles completed for this task.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub review_cycles: Option<usize>,
    /// Active AI tool session ID currently associated with this task lifecycle.
    /// Set from worktree-scoped `tool-sessions.json` during dispatch/completion.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub active_session_id: Option<String>,
    /// Session ID from the most recent run, preserved on error so retries can resume
    /// with cached context instead of cold-starting a new session.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_session_id: Option<String>,
    /// Tool that owned `last_session_id`; used to validate the fallback before injecting.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_session_tool: Option<String>,
    /// Last known worktree path retained when the active worktree attachment
    /// is cleared after a failure, so retry salvage can inspect prior commits.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_worktree_path: Option<String>,
    /// Last known branch retained when the active worktree attachment is
    /// cleared after a failure, so retry salvage can attempt recovery merge.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_branch: Option<String>,
    /// Tool-reported error explanation extracted from `MACC_TASK_RESULT_EXP:` markers
    /// in performer output. Displayed in TUI/WEB coordinator live view.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub result_explanation: Option<String>,
    /// Preconditions the tool reported as unsatisfied when it stopped with
    /// `precondition_unmet` (`MACC_TASK_PRECONDITION:` markers). Rendered as
    /// the "cannot be implemented because…" list in every client.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub unmet_preconditions: Vec<String>,
    /// Verdict produced by a task declared with `gate`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub gate_verdict: Option<GateVerdict>,
    /// Operator evidence that clears the current external block declaration.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub external_block_resolution: Option<ExternalBlockResolution>,
    /// Approval ledger of a `human_approval` gate. Only the CLI approve /
    /// reject / request-changes commands write here.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub approval: Option<ApprovalState>,
    #[serde(flatten)]
    pub extra: BTreeMap<String, Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq)]
pub struct TaskRuntimeMetrics {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub retries: Option<i64>,
    #[serde(flatten)]
    pub extra: BTreeMap<String, i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq)]
pub struct SloWarningRecord {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub metric: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub threshold: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub value: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub warned_at: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub suggestion: Option<String>,
    #[serde(flatten)]
    pub extra: BTreeMap<String, Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq)]
pub struct ResourceLock {
    #[serde(default)]
    pub task_id: String,
    #[serde(default)]
    pub claim_id: String,
    #[serde(default)]
    pub worktree_path: String,
    #[serde(default)]
    pub locked_at: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub expires_at: Option<String>,
    #[serde(flatten)]
    pub extra: BTreeMap<String, Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq)]
pub struct PrdInput {
    #[serde(default)]
    pub tasks: Vec<PrdTaskInput>,
    #[serde(flatten)]
    pub extra: BTreeMap<String, Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq)]
pub struct PrdTaskInput {
    #[serde(deserialize_with = "deserialize_stringish")]
    pub id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub priority: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub category: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub scope: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub base_branch: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub coordinator_tool: Option<String>,
    #[serde(
        default,
        deserialize_with = "deserialize_vec_stringish",
        skip_serializing_if = "Vec::is_empty"
    )]
    pub dependencies: Vec<String>,
    #[serde(
        default,
        deserialize_with = "deserialize_vec_stringish",
        skip_serializing_if = "Vec::is_empty"
    )]
    pub exclusive_resources: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub blocked_on_external: Option<ExternalTaskBlock>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub gate: Option<TaskGate>,
    #[serde(flatten)]
    pub extra: BTreeMap<String, Value>,
}

fn default_todo_state() -> String {
    "todo".to_string()
}

fn deserialize_stringish<'de, D>(deserializer: D) -> std::result::Result<String, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let value = Value::deserialize(deserializer)?;
    match value {
        Value::String(s) => Ok(s),
        Value::Number(n) => Ok(n.to_string()),
        other => Err(serde::de::Error::custom(format!(
            "expected string or number, got {}",
            other
        ))),
    }
}

fn deserialize_vec_stringish<'de, D>(deserializer: D) -> std::result::Result<Vec<String>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let values = Vec::<Value>::deserialize(deserializer)?;
    values
        .into_iter()
        .map(|value| match value {
            Value::String(s) => Ok(s),
            Value::Number(n) => Ok(n.to_string()),
            other => Err(serde::de::Error::custom(format!(
                "expected string or number, got {}",
                other
            ))),
        })
        .collect()
}

fn is_default_task_runtime(runtime: &TaskRuntime) -> bool {
    runtime == &TaskRuntime::default()
}

impl TaskRegistry {
    pub fn from_value(value: &Value) -> Result<Self> {
        serde_json::from_value::<Self>(value.clone()).map_err(|e| MaccError::Coordinator {
            code: "registry_parse",
            message: format!("Failed to parse typed coordinator registry: {}", e),
        })
    }

    pub fn to_value(&self) -> Result<Value> {
        serde_json::to_value(self).map_err(|e| {
            MaccError::Validation(format!(
                "Failed to serialize typed coordinator registry: {}",
                e
            ))
        })
    }

    pub fn set_updated_at(&mut self, ts: String) {
        self.updated_at = Some(ts);
    }

    pub fn recompute_resource_locks(&mut self, now_iso: &str) {
        self.resource_locks.clear();
        for task in &self.tasks {
            if task.id.is_empty() {
                continue;
            }
            if !matches!(
                task.state.as_str(),
                "claimed" | "in_progress" | "pr_open" | "changes_requested" | "queued"
            ) {
                continue;
            }
            let worktree_path = task
                .worktree
                .as_ref()
                .and_then(|w| w.worktree_path.clone())
                .unwrap_or_default();
            let claim_id = task
                .task_runtime
                .claim_id
                .clone()
                .filter(|s| !s.is_empty())
                .or(task.task_runtime.active_session_id.clone())
                .unwrap_or_else(|| format!("unclaimed-{}", task.id));
            let expires_at = task.task_runtime.lease_expires_at.clone();
            for resource in &task.exclusive_resources {
                if resource.is_empty() {
                    continue;
                }
                self.resource_locks.insert(
                    resource.clone(),
                    ResourceLock {
                        task_id: task.id.clone(),
                        claim_id: claim_id.clone(),
                        worktree_path: worktree_path.clone(),
                        locked_at: now_iso.to_string(),
                        expires_at: expires_at.clone(),
                        extra: BTreeMap::new(),
                    },
                );
            }
        }
    }

    pub fn active_task_worktree_paths(&self) -> HashSet<String> {
        let mut out = HashSet::new();
        for task in &self.tasks {
            if !matches!(
                task.state.as_str(),
                "claimed" | "in_progress" | "pr_open" | "changes_requested" | "queued"
            ) {
                continue;
            }
            if let Some(path) = task
                .worktree
                .as_ref()
                .and_then(|w| w.worktree_path.as_ref())
            {
                if !path.is_empty() {
                    out.insert(path.to_string());
                }
            }
        }
        out
    }

    pub fn can_reuse_worktree_slot(&self, worktree_path: &str) -> bool {
        // A slot can be reused unless a task in an active/blocking state is assigned to it.
        // Blocking states: claimed, in_progress, pr_open, changes_requested, queued.
        // Non-blocking (safe to reset): merged, failed, abandoned, todo.
        // Orphaned worktrees (no task references at all) are also safe to reuse —
        // their previous tasks either completed, failed, or had worktree metadata cleared.
        const BLOCKING: &[&str] = &[
            "claimed",
            "in_progress",
            "pr_open",
            "changes_requested",
            "queued",
        ];
        for task in &self.tasks {
            let Some(path) = task
                .worktree
                .as_ref()
                .and_then(|w| w.worktree_path.as_ref())
            else {
                continue;
            };
            if path != worktree_path {
                continue;
            }
            if BLOCKING.contains(&task.state.as_str()) {
                return false;
            }
        }
        true
    }

    /// Returns `true` if the worktree should be considered permanently stuck
    /// (its branch will never merge autonomously).
    ///
    /// This is true when:
    /// - A task assigned to this worktree is in a terminal/stuck state
    ///   (blocked, failed, abandoned), OR
    /// - No task references this worktree at all (orphaned — the task was
    ///   retried on a different worker and its worktree metadata was cleared).
    ///
    /// Used by the worktree reuse logic to decide whether to abandon an unmerged
    /// branch and reset the slot rather than deadlocking.
    pub fn task_on_worktree_is_permanently_stuck(&self, worktree_path: &str) -> bool {
        const STUCK: &[&str] = &["blocked", "failed", "abandoned"];
        let mut any_task_references_worktree = false;
        for task in &self.tasks {
            if task.worktree_path().is_some_and(|p| p == worktree_path) {
                any_task_references_worktree = true;
                if STUCK.contains(&task.state.as_str()) {
                    return true;
                }
            }
        }
        // Orphaned worktree: no task points here anymore (e.g. task was
        // retried on a different worker). The unmerged branch is abandoned.
        !any_task_references_worktree
    }

    pub fn has_in_progress_or_queued_on_worktree(&self, worktree_path: &str) -> bool {
        self.tasks.iter().any(|task| {
            matches!(
                task.workflow_state(),
                Some(WorkflowState::InProgress | WorkflowState::Queued)
            ) && task.worktree_path().is_some_and(|p| p == worktree_path)
        })
    }

    pub fn find_task(&self, task_id: &str) -> Option<&Task> {
        self.tasks.iter().find(|task| task.id == task_id)
    }

    pub fn find_task_mut(&mut self, task_id: &str) -> Option<&mut Task> {
        self.tasks.iter_mut().find(|task| task.id == task_id)
    }

    pub fn counts(&self) -> (usize, usize, usize, usize, usize) {
        let total = self.tasks.len();
        let mut todo = 0usize;
        let mut active = 0usize;
        let mut blocked = 0usize;
        let mut merged = 0usize;
        for task in &self.tasks {
            match task.workflow_state().unwrap_or(WorkflowState::Todo) {
                WorkflowState::Todo => todo += 1,
                WorkflowState::Blocked => blocked += 1,
                WorkflowState::Merged => merged += 1,
                WorkflowState::Claimed
                | WorkflowState::InProgress
                | WorkflowState::Testing
                | WorkflowState::Reviewing
                | WorkflowState::PrOpen
                | WorkflowState::ChangesRequested
                | WorkflowState::Queued => active += 1,
                WorkflowState::Approved => merged += 1,
                WorkflowState::Rejected => blocked += 1,
                // Waiting gates use no worker and are not stalled work; they
                // are counted separately by `waiting_approval_count`.
                WorkflowState::Abandoned
                | WorkflowState::WaitingApproval
                | WorkflowState::Expired => {}
            }
        }
        (total, todo, active, blocked, merged)
    }

    /// Human gates currently waiting for a decision (including expired ones,
    /// which wait for a renewed approval).
    pub fn waiting_approval_count(&self) -> usize {
        self.tasks
            .iter()
            .filter(|task| task.is_awaiting_approval())
            .count()
    }
}

/// True when a completed task's failure must pause the coordinator run rather
/// than be treated as a routine failure.
///
/// `E901` means the performer exited without persisting a valid terminal
/// `phase_result` event -- a protocol/contract failure between the performer
/// and coordinator (serialization bug, IPC rejection, version mismatch),
/// never a normal task failure. If the task's worktree still holds commits
/// that are not yet merged into the base branch, silently continuing
/// (auto-retry, salvage, or later worktree reuse) risks losing or obscuring
/// that work. See docs/prd/8_3_MACC_Coordinator_Integrity_Recommendations.md
/// §4.3 for the full rationale.
pub fn requires_integrity_pause(
    error_code: Option<&str>,
    has_commits_ahead_of_base: bool,
    branch_merged_into_base: bool,
) -> bool {
    error_code == Some("E901") && has_commits_ahead_of_base && !branch_merged_into_base
}

impl Task {
    pub fn workflow_state(&self) -> Option<WorkflowState> {
        WorkflowState::from_str(self.state.as_str()).ok()
    }

    pub fn is_active(&self) -> bool {
        matches!(
            self.workflow_state(),
            Some(
                WorkflowState::Claimed
                    | WorkflowState::InProgress
                    | WorkflowState::PrOpen
                    | WorkflowState::ChangesRequested
                    | WorkflowState::Queued
            )
        )
    }

    pub fn is_merged(&self) -> bool {
        matches!(self.workflow_state(), Some(WorkflowState::Merged))
    }

    pub fn is_human_approval_gate(&self) -> bool {
        self.gate.as_ref().is_some_and(TaskGate::is_human_approval)
    }

    pub fn is_awaiting_approval(&self) -> bool {
        matches!(
            self.workflow_state(),
            Some(WorkflowState::WaitingApproval | WorkflowState::Expired)
        )
    }

    /// Delivered work or an approved gate: satisfies dependants.
    pub fn satisfies_dependants(&self) -> bool {
        self.workflow_state()
            .is_some_and(WorkflowState::satisfies_dependants)
            && self.gate_verdict_satisfies_dependencies()
    }

    pub fn worktree_path(&self) -> Option<&str> {
        self.worktree
            .as_ref()
            .and_then(|worktree| worktree.worktree_path.as_deref())
            .filter(|path| !path.is_empty())
    }

    pub fn has_worktree_attached(&self) -> bool {
        self.worktree.as_ref().is_some_and(|worktree| {
            worktree.worktree_path.is_some()
                || worktree.branch.is_some()
                || worktree.base_branch.is_some()
                || worktree.last_commit.is_some()
                || worktree.session_id.is_some()
                || !worktree.extra.is_empty()
        })
    }

    /// True when the task was parked by the same-worktree retry path and is
    /// waiting to be re-dispatched into the worktree it already holds.
    ///
    /// A tool that reports `error_with_changes` after committing work is
    /// requeued to `todo` with its worktree deliberately *kept* (see
    /// `engine::transitions::apply_state_transitions`), so the retry can resume
    /// on top of those commits. The dispatcher otherwise skips every `todo`
    /// task that has a worktree attached, which would make such a task
    /// permanently unschedulable — it is neither active nor blocked, so no
    /// recovery path reclaims it either. This predicate is what lets the
    /// selector tell "parked for retry" apart from "already assigned".
    pub fn is_awaiting_same_worktree_retry(&self) -> bool {
        self.workflow_state() == Some(WorkflowState::Todo)
            && self.runtime_status() == RuntimeStatus::Failed
            && self.branch().is_some()
            && self.worktree_path().is_some()
    }

    pub fn task_tool(&self) -> Option<&str> {
        self.tool.as_deref().filter(|tool| !tool.is_empty())
    }

    pub fn coordinator_tool(&self) -> Option<&str> {
        self.coordinator_tool
            .as_deref()
            .filter(|tool| !tool.is_empty())
    }

    pub fn category(&self) -> Option<&str> {
        self.category
            .as_deref()
            .filter(|category| !category.is_empty())
    }

    pub fn scope(&self) -> Option<&str> {
        self.scope.as_deref().filter(|scope| !scope.is_empty())
    }

    pub fn base_branch(&self, default: &str) -> String {
        self.base_branch
            .as_deref()
            .filter(|value| !value.is_empty())
            .or_else(|| {
                self.worktree
                    .as_ref()
                    .and_then(|worktree| worktree.base_branch.as_deref())
                    .filter(|value| !value.is_empty())
            })
            .unwrap_or(default)
            .to_string()
    }

    pub fn priority_rank(&self) -> i32 {
        parse_priority_value(self.priority.as_deref())
    }

    pub fn dependency_ids(&self) -> Vec<String> {
        self.dependencies.clone()
    }

    pub fn gate_verdict_satisfies_dependencies(&self) -> bool {
        self.gate.as_ref().is_none_or(|gate| {
            // A human gate is decided by its workflow state (`approved`),
            // never by a tool-reported verdict.
            gate.is_human_approval()
                || self.task_runtime.gate_verdict == Some(gate.required_verdict)
        })
    }

    pub fn external_block_is_cleared(&self) -> bool {
        let Some(block) = self.blocked_on_external.as_ref() else {
            return true;
        };
        self.task_runtime
            .external_block_resolution
            .as_ref()
            .is_some_and(|resolution| {
                resolution.tracking_id == block.tracking_id
                    && resolution.block_reason == block.reason
            })
    }

    pub fn branch(&self) -> Option<&str> {
        self.worktree
            .as_ref()
            .and_then(|worktree| worktree.branch.as_deref())
            .filter(|branch| !branch.is_empty())
    }

    pub fn last_commit(&self) -> Option<&str> {
        self.worktree
            .as_ref()
            .and_then(|worktree| worktree.last_commit.as_deref())
            .filter(|value| !value.is_empty())
    }

    pub fn session_id(&self) -> Option<&str> {
        self.worktree
            .as_ref()
            .and_then(|worktree| worktree.session_id.as_deref())
            .filter(|value| !value.is_empty())
    }

    pub fn current_phase(&self) -> &str {
        self.task_runtime
            .current_phase
            .as_deref()
            .filter(|phase| !phase.is_empty())
            .unwrap_or("dev")
    }

    pub fn runtime_status(&self) -> RuntimeStatus {
        self.task_runtime
            .status
            .as_deref()
            .unwrap_or(RuntimeStatus::Idle.as_str())
            .parse::<RuntimeStatus>()
            .unwrap_or(RuntimeStatus::Idle)
    }

    pub fn runtime_pid(&self) -> Option<i64> {
        self.task_runtime.pid
    }

    pub fn set_workflow_state(&mut self, state: WorkflowState) {
        self.state = state.as_str().to_string();
    }

    pub fn ensure_worktree(&mut self) -> &mut TaskWorktree {
        self.worktree.get_or_insert_with(TaskWorktree::default)
    }

    pub fn clear_assignment(&mut self) {
        self.assignee = None;
        self.claimed_at = None;
        self.worktree = None;
    }

    pub fn ensure_runtime(&mut self) -> &mut TaskRuntime {
        &mut self.task_runtime
    }

    pub fn touch_state_changed(&mut self, now: &str) {
        self.state_changed_at = Some(now.to_string());
        self.updated_at = Some(now.to_string());
    }
}

impl TaskRuntime {
    pub fn status(&self) -> RuntimeStatus {
        self.status
            .as_deref()
            .unwrap_or(RuntimeStatus::Idle.as_str())
            .parse::<RuntimeStatus>()
            .unwrap_or(RuntimeStatus::Idle)
    }

    pub fn set_status(&mut self, status: RuntimeStatus) {
        self.status = Some(status.as_str().to_string());
    }

    pub fn set_last_error_details(
        &mut self,
        code: impl Into<String>,
        origin: impl Into<String>,
        message: impl Into<String>,
    ) {
        self.last_error_code = Some(code.into());
        self.last_error_origin = Some(origin.into());
        self.last_error_message = Some(message.into());
    }

    pub fn clear_last_error_details(&mut self) {
        self.last_error_code = None;
        self.last_error_origin = None;
        self.last_error_message = None;
        self.result_explanation = None;
        self.unmet_preconditions.clear();
    }

    pub fn ensure_metrics(&mut self) -> &mut TaskRuntimeMetrics {
        self.metrics.get_or_insert_with(TaskRuntimeMetrics::default)
    }

    pub fn metric_i64(&self, metric_name: &str) -> Option<i64> {
        match metric_name {
            "retries" => self
                .retries
                .or_else(|| self.metrics.as_ref().and_then(|metrics| metrics.retries)),
            other => self
                .metrics
                .as_ref()
                .and_then(|metrics| metrics.extra.get(other).copied()),
        }
    }

    pub fn set_metric_i64(&mut self, metric_name: &str, value: i64) {
        if metric_name == "retries" {
            self.retries = Some(value);
            self.ensure_metrics().retries = Some(value);
            return;
        }
        self.ensure_metrics()
            .extra
            .insert(metric_name.to_string(), value);
    }

    pub fn ensure_slo_warnings(&mut self) -> &mut BTreeMap<String, SloWarningRecord> {
        self.slo_warnings.get_or_insert_with(BTreeMap::new)
    }

    pub fn has_slo_warning(&self, metric_name: &str) -> bool {
        self.slo_warnings
            .as_ref()
            .is_some_and(|warnings| warnings.contains_key(metric_name))
    }

    pub fn upsert_slo_warning(
        &mut self,
        metric: &str,
        threshold: i64,
        value: i64,
        suggestion: &str,
        warned_at: &str,
    ) {
        self.ensure_slo_warnings().insert(
            metric.to_string(),
            SloWarningRecord {
                metric: Some(metric.to_string()),
                threshold: Some(threshold),
                value: Some(value),
                warned_at: Some(warned_at.to_string()),
                suggestion: Some(suggestion.to_string()),
                extra: BTreeMap::new(),
            },
        );
    }

    pub fn retries_count(&self) -> usize {
        self.metric_i64("retries")
            .and_then(|v| usize::try_from(v).ok())
            .unwrap_or(0)
    }

    pub fn increment_retries(&mut self) -> usize {
        let next = self.retries_count().saturating_add(1);
        self.set_metric_i64("retries", next as i64);
        next
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn make_registry_with_task(state: &str, worktree_path: &str) -> TaskRegistry {
        let v = json!({
            "tasks": [{
                "id": "T1",
                "state": state,
                "worktree": { "worktree_path": worktree_path }
            }]
        });
        TaskRegistry::from_value(&v).unwrap()
    }

    #[test]
    fn can_reuse_slot_merged_task() {
        let r = make_registry_with_task("merged", "/wt/worker-01");
        assert!(r.can_reuse_worktree_slot("/wt/worker-01"));
    }

    #[test]
    fn can_reuse_slot_failed_task_unblocks_slot() {
        // Regression: a failed task (e.g. commit failed on performer) must not
        // permanently block its worktree slot from being reused.
        let r = make_registry_with_task("failed", "/wt/worker-01");
        assert!(r.can_reuse_worktree_slot("/wt/worker-01"));
    }

    #[test]
    fn can_reuse_slot_todo_task_unblocks_slot() {
        // Regression: an auto-retried task (state=todo) must not block its slot.
        let r = make_registry_with_task("todo", "/wt/worker-01");
        assert!(r.can_reuse_worktree_slot("/wt/worker-01"));
    }

    #[test]
    fn can_reuse_slot_abandoned_task_unblocks_slot() {
        let r = make_registry_with_task("abandoned", "/wt/worker-01");
        assert!(r.can_reuse_worktree_slot("/wt/worker-01"));
    }

    #[test]
    fn can_reuse_slot_in_progress_task_blocks_slot() {
        let r = make_registry_with_task("in_progress", "/wt/worker-01");
        assert!(!r.can_reuse_worktree_slot("/wt/worker-01"));
    }

    #[test]
    fn can_reuse_slot_claimed_task_blocks_slot() {
        let r = make_registry_with_task("claimed", "/wt/worker-01");
        assert!(!r.can_reuse_worktree_slot("/wt/worker-01"));
    }

    #[test]
    fn can_reuse_slot_no_tasks_returns_true() {
        // Orphaned worktrees (no task references) are safe to reuse.
        let v = json!({ "tasks": [] });
        let r = TaskRegistry::from_value(&v).unwrap();
        assert!(r.can_reuse_worktree_slot("/wt/worker-01"));
    }

    #[test]
    fn can_reuse_slot_different_worktree_not_blocked() {
        let r = make_registry_with_task("in_progress", "/wt/worker-02");
        // worker-01 has no task references — orphaned, safe to reuse.
        assert!(r.can_reuse_worktree_slot("/wt/worker-01"));
    }

    #[test]
    fn requires_integrity_pause_e901_with_unmerged_commits() {
        // The exact incident scenario: performer exited 0, no terminal event
        // was persisted (E901), and the branch has unmerged commits ahead of
        // base. This must pause the coordinator rather than let the task be
        // silently classified as a routine failure.
        assert!(requires_integrity_pause(Some("E901"), true, false));
    }

    #[test]
    fn requires_integrity_pause_false_when_no_commits() {
        // A failed_clean task (no commits produced) is always safe to reuse
        // automatically, even with E901.
        assert!(!requires_integrity_pause(Some("E901"), false, false));
    }

    #[test]
    fn requires_integrity_pause_false_when_branch_already_merged() {
        // If the branch is already merged into base, there is nothing at risk.
        assert!(!requires_integrity_pause(Some("E901"), true, true));
    }

    #[test]
    fn requires_integrity_pause_false_for_other_error_codes() {
        // A routine failure (e.g. E101 runner error) with commits ahead is not
        // an integrity condition on its own -- only E901 (missing terminal
        // event) qualifies.
        assert!(!requires_integrity_pause(Some("E101"), true, false));
    }

    #[test]
    fn requires_integrity_pause_false_when_no_error_code() {
        assert!(!requires_integrity_pause(None, true, false));
    }

    #[test]
    fn prd_task_deserializes_scheduler_visible_gate_and_external_block() {
        let task: PrdTaskInput = serde_json::from_value(json!({
            "id":"GATE-1",
            "blocked_on_external":{
                "reason":"observation missing",
                "clears_when":"report exists",
                "tracking_id":"GAP-17"
            },
            "gate":{"required_verdict":"accepted","description":"release gate"}
        }))
        .expect("PRD task");

        assert_eq!(
            task.blocked_on_external.unwrap().tracking_id.as_deref(),
            Some("GAP-17")
        );
        assert_eq!(task.gate.unwrap().required_verdict, GateVerdict::Accepted);
        assert!(!task.extra.contains_key("blocked_on_external"));
        assert!(!task.extra.contains_key("gate"));
    }
}

fn parse_priority_value(priority: Option<&str>) -> i32 {
    match priority.map(|value| value.trim().to_ascii_lowercase()) {
        Some(value) if value == "p0" => 0,
        Some(value) if value == "p1" => 1,
        Some(value) if value == "p2" => 2,
        Some(value) if value == "p3" => 3,
        Some(value) if value == "p4" => 4,
        Some(value) => value.parse::<i32>().unwrap_or(99),
        None => 99,
    }
}

#[cfg(test)]
mod human_gate_validation_tests {
    use super::*;
    use serde_json::json;

    fn gate(v: serde_json::Value) -> TaskGate {
        serde_json::from_value(v).expect("gate parses")
    }

    #[test]
    fn prd_shape_parses_with_defaults() {
        let g = gate(json!({
            "kind": "human_approval",
            "subject_task": "SEC-ADR-003",
            "required_approvers": [{"role":"PRODUCT_OWNER"},{"role":"SECURITY_OWNER","count":2}],
            "quorum": "all",
            "evidence_type": "pull_request_review",
            "risks": ["irreversible migration"],
            "governance_ref": "docs/16-decisions.md"
        }));
        assert!(g.is_human_approval());
        assert_eq!(g.required_approvers[0].count, 1, "count defaults to 1");
        assert_eq!(g.bind_to, "commit_sha");
        assert!(g.invalidate_on_subject_change, "invalidation defaults on");
        assert_eq!(g.quorum, Quorum::All);
        assert!(g
            .validate_human_approval("SEC-APP-003", &["SEC-ADR-003".into()])
            .is_empty());
        let numeric = gate(json!({"kind":"human_approval","quorum":2}));
        assert_eq!(numeric.quorum, Quorum::Count(2));
        let any = gate(json!({"kind":"human_approval","quorum":"any"}));
        assert_eq!(any.quorum, Quorum::Any);
        // Round-trips in the same shape the planner writes.
        assert_eq!(serde_json::to_value(Quorum::All).unwrap(), json!("all"));
        assert_eq!(serde_json::to_value(Quorum::Count(2)).unwrap(), json!(2));
        assert!(serde_json::from_value::<TaskGate>(json!({"quorum":"most"})).is_err());
    }

    #[test]
    fn a_legacy_verdict_gate_is_unchanged() {
        let g = gate(json!({"required_verdict":"accepted"}));
        assert_eq!(g.kind, GateKind::Verdict);
        assert!(g.validate_human_approval("ACC", &[]).is_empty());
    }

    #[test]
    fn every_structural_defect_is_reported_at_once() {
        let g = gate(json!({
            "kind":"human_approval",
            "subject_task":"SEC-ADR-003",
            "required_approvers":[{"role":"","count":0}],
            "quorum": 5,
            "bind_to":"tag"
        }));
        let problems = g.validate_human_approval("SEC-APP-003", &[]);
        let text = problems.join("\n");
        assert!(
            text.contains("must also be listed in dependencies"),
            "{text}"
        );
        assert!(text.contains("empty role"), "{text}");
        assert!(text.contains("count 0"), "{text}");
        assert!(text.contains("quorum 5"), "{text}");
        assert!(text.contains("bind_to 'tag'"), "{text}");
        let missing = gate(json!({"kind":"human_approval"}));
        let text = missing.validate_human_approval("X", &[]).join("\n");
        assert!(text.contains("subject_task is required"));
        assert!(text.contains("at least one role"));
    }

    #[test]
    fn approved_satisfies_dependants_and_waiting_does_not() {
        let reg = TaskRegistry::from_value(&json!({"tasks":[
            {"id":"G1","state":"approved","gate":{"kind":"human_approval"}},
            {"id":"G2","state":"waiting_approval","gate":{"kind":"human_approval"}},
            {"id":"G3","state":"rejected","gate":{"kind":"human_approval"}},
            {"id":"G4","state":"expired","gate":{"kind":"human_approval"}}
        ]}))
        .unwrap();
        let ok: Vec<bool> = reg.tasks.iter().map(Task::satisfies_dependants).collect();
        assert_eq!(ok, vec![true, false, false, false]);
        assert_eq!(reg.waiting_approval_count(), 2, "waiting + expired");
        let (_, todo, active, blocked, merged) = reg.counts();
        assert_eq!(
            (todo, active, blocked, merged),
            (0, 0, 1, 1),
            "approved=merged, rejected=blocked, waiting uses no worker"
        );
    }
}
