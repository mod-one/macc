use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::BTreeMap;

#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
#[serde(rename_all = "snake_case")]
pub enum Risk {
    Low,
    Medium,
    #[default]
    High,
    Critical,
}

#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Consultation {
    Low,
    Medium,
    #[default]
    High,
    Critical,
    NeverAsk,
}
impl Consultation {
    pub fn asks(self, risk: Risk) -> bool {
        match self {
            Self::NeverAsk => false,
            Self::Low => true,
            Self::Medium => risk >= Risk::Medium,
            Self::High => risk >= Risk::High,
            Self::Critical => risk >= Risk::Critical,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(default, deny_unknown_fields)]
pub struct Policy {
    #[serde(default, skip_serializing_if = "is_false")]
    pub similarity_hints: bool,
    #[serde(
        default = "default_similarity_backend",
        skip_serializing_if = "is_lexical"
    )]
    pub similarity_backend: String,

    #[serde(
        default = "default_candidates",
        skip_serializing_if = "candidates_default"
    )]
    pub similarity_max_candidates: usize,
    #[serde(
        default = "default_similarity",
        skip_serializing_if = "similarity_default"
    )]
    pub similarity_min_percent: u8,
    pub enabled: bool,
    pub human_question_min_risk: Consultation,
    pub max_context_bytes: usize,
    pub max_request_bytes: usize,
    /// Local trust is explicit; secure approvals require an isolated authority.
    pub trust_mode: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub trust_registry: Option<std::path::PathBuf>,
    pub mandatory_human_triggers: Vec<String>,
    pub approval_reuse: ApprovalReuse,
    /// Explicit, bytewise Git subject manifests for content-equivalent reuse.
    pub approval_manifests: BTreeMap<String, Vec<String>>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub governance_rules: Vec<GovernanceRule>,
    #[serde(default, skip_serializing_if = "is_default_budget")]
    pub investigation: InvestigationBudget,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub operation_adapters: BTreeMap<String, OperationAdapter>,
}
impl Default for Policy {
    fn default() -> Self {
        Self {
            enabled: false,
            similarity_hints: false,
            similarity_backend: "lexical".into(),
            similarity_max_candidates: 128,
            similarity_min_percent: 75,
            human_question_min_risk: Consultation::High,
            max_context_bytes: 131072,
            max_request_bytes: 65536,
            trust_mode: "local_trust".into(),
            trust_registry: None,
            mandatory_human_triggers: vec!["production".into(), "irreversible".into()],
            approval_reuse: ApprovalReuse::ExactRevision,
            approval_manifests: BTreeMap::new(),
            governance_rules: vec![],
            investigation: InvestigationBudget::default(),
            operation_adapters: BTreeMap::new(),
        }
    }
}
impl Policy {
    pub fn validate(&self) -> crate::Result<()> {
        if !(2048..=262144).contains(&self.max_context_bytes)
            || !(2048..=262144).contains(&self.max_request_bytes)
        {
            return Err(super::error("Decision budgets must be 2048..262144 bytes"));
        }
        if self.investigation.max_rounds == 0
            || self.investigation.max_rounds > 10
            || self.investigation.max_source_bytes < 1024
            || self.investigation.max_source_bytes > 262144
        {
            return Err(super::error(
                "Investigation budget must be 1..10 rounds and 1024..262144 source bytes",
            ));
        }
        if !matches!(self.similarity_backend.as_str(), "lexical" | "local_lsa") {
            return Err(super::error("Decision similarity backend must be lexical or local_lsa; no remote embedding calls are made"));
        }
        if !(1..=512).contains(&self.similarity_max_candidates)
            || !(1..=100).contains(&self.similarity_min_percent)
        {
            return Err(super::error(
                "Similarity hints require 1..512 candidates and a 1..100 percent threshold",
            ));
        }
        let mut ids = std::collections::BTreeSet::new();
        for rule in &self.governance_rules {
            if rule.id.trim().is_empty() || !ids.insert(&rule.id) {
                return Err(super::error(
                    "Governance rule IDs must be unique and nonempty",
                ));
            }
            for path in &rule.evidence_paths {
                if std::path::Path::new(path).is_absolute()
                    || std::path::Path::new(path)
                        .components()
                        .any(|c| matches!(c, std::path::Component::ParentDir))
                {
                    return Err(super::error(
                        "Governance evidence paths must be project-relative",
                    ));
                }
            }
        }
        for (name, a) in &self.operation_adapters {
            if name.is_empty()
                || a.command.is_empty()
                || !std::path::Path::new(&a.command[0]).is_absolute()
                || a.allowed_targets.is_empty()
                || !(1..=3600).contains(&a.timeout_seconds)
                || !(1024..=1048576).contains(&a.max_output_bytes)
                || a.verified_idempotency.is_some()
                    && (a.idempotency_key_arg.is_none() || a.probe.is_empty())
                || a.environment_names.iter().any(|n| n.starts_with("MACC_"))
            {
                return Err(super::error("Operation adapters require an absolute executable, allowed targets, bounded timeout/output and a keyed verified probe before retries"));
            }
        }
        match self.trust_mode.as_str() {
            "local_trust" => {}
            "secure_approval" => super::secure::validate_secure_policy(self)?,
            _ => return Err(super::error("Unknown decision trust mode")),
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ApprovalReuse {
    #[default]
    ExactRevision,
    ContentEquivalent,
    Never,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Plan {
    pub id: String,
    pub project_id: String,
    pub source: String,
    pub reference_branch: String,
    pub graph_version: u64,
    pub decision_epoch: u64,
    pub graph: Value,
    pub source_digest: String,
    pub projected_version: u64,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(default, deny_unknown_fields)]
pub struct Impact {
    pub shared_contract: bool,
    pub product_scope: bool,
    pub security_privacy: bool,
    pub irreversible: bool,
    pub production: bool,
    pub evidence: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct OptionChoice {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub proposed_patch: Option<Patch>,
    #[serde(default, skip_serializing_if = "is_false")]
    pub requires_replan: bool,
    pub id: String,
    pub label: String,
    #[serde(default)]
    pub consequences: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Request {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub intervention: Option<UserIntervention>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub artifacts: Vec<ArtifactRef>,
    pub idempotency_key: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub revision_of: Option<DecisionRef>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub expires_at: Option<String>,
    pub task_id: String,
    #[serde(default = "default_scope")]
    pub scope: String,
    #[serde(default)]
    pub affected_paths: Vec<String>,
    #[serde(default)]
    pub requirement_refs: Vec<String>,
    #[serde(default)]
    pub attempt_id: String,
    #[serde(default = "default_kind")]
    pub kind: String,
    pub question: String,
    #[serde(default)]
    pub context: String,
    #[serde(default)]
    pub investigated: Vec<String>,
    #[serde(default)]
    pub options: Vec<OptionChoice>,
    #[serde(default)]
    pub recommended_option_id: Option<String>,
    #[serde(default)]
    pub recommendation: String,
    #[serde(default)]
    pub affected_tasks: Vec<String>,
    #[serde(default)]
    pub risk: Risk,
    #[serde(default)]
    pub impact: Impact,
    #[serde(default)]
    pub proposed_patch: Option<Patch>,
    #[serde(default)]
    pub operation_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub subject_revision: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub operation_spec: Option<OperationSpec>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub response_fields: Vec<ResponseField>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub topic: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub supersedes: Vec<DecisionRef>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub missing_facts: Vec<String>,
}
fn default_kind() -> String {
    "decision".into()
}
fn default_scope() -> String {
    "task".into()
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Resolution {
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub conditions: Vec<ApprovalCondition>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub artifacts: Vec<ArtifactRef>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub answered_version: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub applied_patch: Option<Patch>,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub effects: String,
    pub text: String,
    pub selected_option_id: Option<String>,
    pub actor: String,
    pub origin: String,
    pub policy_digest: String,
    pub recorded_at: String,
    #[serde(default)]
    pub advisory: bool,
    #[serde(default)]
    pub source_revision: Option<String>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub fields: BTreeMap<String, String>,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub outcome: String,
    #[serde(default, skip_serializing_if = "is_false")]
    pub patch_applied: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub attestation: Option<super::secure::Attestation>,
}
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Decision {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub effects_commit: Option<EffectCommit>,
    #[serde(default)]
    pub requester_actor: String,
    #[serde(default)]
    pub requester_origin: String,
    pub id: String,
    pub plan_id: String,
    pub version: u64,
    pub status: String,
    pub request: Request,
    pub effective_risk: Risk,
    pub resolution: Option<Resolution>,
    pub created_at: String,
    #[serde(default, skip_serializing_if = "is_false")]
    pub requires_human_renewal: bool,
    pub subject_digest: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub submission_digest: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub domain: Option<String>,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub policy_reason: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reused_from: Option<DecisionRef>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub investigation: Option<InvestigationReport>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "op", rename_all = "snake_case", deny_unknown_fields)]
pub enum Operation {
    AddTask {
        task: Value,
    },
    UpdateTask {
        task_id: String,
        changes: BTreeMap<String, Value>,
    },
    AddDependency {
        task_id: String,
        depends_on: String,
    },
    RemoveDependency {
        task_id: String,
        depends_on: String,
    },
    DisableTask {
        task_id: String,
    },
    SupersedeTask {
        task_id: String,
        replacement_task_id: String,
    },
    BindDecision {
        task_id: String,
        decision_id: String,
        version: u64,
    },
}
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Patch {
    pub plan_id: String,
    pub base_graph_version: u64,
    pub idempotency_key: String,
    pub reason: String,
    #[serde(default)]
    pub decision_refs: Vec<String>,
    pub operations: Vec<Operation>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub metadata_changes: BTreeMap<String, Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ContextPackage {
    /// Contains bytes only for artifacts explicitly released by an operator.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub artifacts: Vec<EvidenceArtifact>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub planning: Option<PlanningContext>,
    #[serde(default, skip_serializing_if = "is_zero")]
    pub graph_version: u64,
    #[serde(default, skip_serializing_if = "is_zero")]
    pub decision_epoch: u64,
    pub schema_version: u32,
    pub plan_id: String,
    pub task_id: String,
    pub attempt_id: String,
    pub phase: String,
    pub contract_digest: String,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub dependency_contracts: BTreeMap<String, Value>,
    pub policy: Policy,
    pub bindings: Vec<Decision>,
    pub context_digest: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub plan_context: Option<Value>,
}
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ContextReceipt {
    pub context_digest: String,
    pub prompt_digest: String,
    pub issued_at: String,
    pub acknowledged: bool,
    #[serde(default)]
    pub completion_status: String,
    #[serde(default)]
    pub reviewed_commit: Option<String>,
    #[serde(default)]
    pub verification_error: Option<String>,
    #[serde(default)]
    pub compliance: BTreeMap<String, Compliance>,
}
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Compliance {
    pub status: String,
    pub evidence: String,
}

fn is_zero(value: &u64) -> bool {
    *value == 0
}
fn is_false(value: &bool) -> bool {
    !*value
}
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct ResponseField {
    pub id: String,
    pub label: String,
    #[serde(default)]
    pub required: bool,
    #[serde(default)]
    pub multiline: bool,
    #[serde(default)]
    pub choices: Vec<String>,
}
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct AnswerInput {
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub conditions: Vec<ApprovalCondition>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub artifacts: Vec<ArtifactRef>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub attestation: Option<super::secure::Attestation>,
    pub text: String,
    pub option: Option<String>,
    pub advisory: bool,
    pub apply_patch: bool,
    #[serde(default, skip_serializing_if = "effects_auto")]
    pub effects: AnswerEffects,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub patch: Option<Patch>,
    pub fields: BTreeMap<String, String>,
    pub outcome: AnswerOutcome,
}
#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum AnswerOutcome {
    #[default]
    Answer,
    Reject,
    RequestChanges,
    Investigate,
}
impl AnswerOutcome {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Answer => "answer",
            Self::Reject => "reject",
            Self::RequestChanges => "request_changes",
            Self::Investigate => "investigate",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct DecisionRef {
    pub id: String,
    pub version: u64,
}
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(default, deny_unknown_fields)]
pub struct GovernanceRule {
    pub decision_domain: Option<String>,
    pub id: String,
    pub task_prefixes: Vec<String>,
    pub affected_paths: Vec<String>,
    pub requirement_refs: Vec<String>,
    pub minimum_risk: Risk,
    pub mandatory_human: bool,
    pub project_wide: bool,
    pub evidence_paths: Vec<String>,
    pub response_fields: Vec<ResponseField>,
    pub standing_answer: Option<String>,
    pub standing_choice: Option<String>,
}
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(default, deny_unknown_fields)]
pub struct InvestigationBudget {
    pub max_rounds: u32,
    pub max_source_bytes: usize,
}
impl Default for InvestigationBudget {
    fn default() -> Self {
        Self {
            max_rounds: 2,
            max_source_bytes: 32768,
        }
    }
}
fn is_default_budget(v: &InvestigationBudget) -> bool {
    v == &InvestigationBudget::default()
}
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct InvestigationReport {
    pub rounds: u32,
    pub source_digests: BTreeMap<String, String>,
    pub missing_sources: Vec<String>,
    pub diagnosis: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct OperationSpec {
    pub adapter: String,
    pub target: String,
    pub source_revision: String,
    #[serde(default)]
    pub arguments: BTreeMap<String, String>,
}
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(default, deny_unknown_fields)]
pub struct OperationAdapter {
    pub command: Vec<String>,
    pub probe: Vec<String>,
    pub allowed_targets: Vec<String>,
    pub environment_names: Vec<String>,
    pub timeout_seconds: u64,
    pub max_output_bytes: usize,
    pub verified_idempotency: Option<String>,
    pub idempotency_key_arg: Option<String>,
}
impl Default for OperationAdapter {
    fn default() -> Self {
        Self {
            command: vec![],
            probe: vec![],
            allowed_targets: vec![],
            environment_names: vec![],
            timeout_seconds: 60,
            max_output_bytes: 65536,
            verified_idempotency: None,
            idempotency_key_arg: None,
        }
    }
}
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum OperationState {
    Reserved,
    Executing,
    Succeeded,
    Failed,
    Unknown,
}
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct OperationExecution {
    pub operation_id: String,
    pub decision_id: String,
    pub plan_id: String,
    pub version: u64,
    pub state: OperationState,
    pub spec_digest: String,
    pub policy_digest: String,
    pub owner_identity: Option<String>,
    pub child_identity: Option<String>,
    pub evidence: String,
    pub output_digest: Option<String>,
    pub output_path: Option<String>,
    pub updated_at: String,
}

#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum AnswerEffects {
    #[default]
    Auto,
    NoGraphChange,
    Replan,
}
fn effects_auto(e: &AnswerEffects) -> bool {
    *e == AnswerEffects::Auto
}
impl AnswerEffects {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Auto => "auto",
            Self::NoGraphChange => "no_graph_change",
            Self::Replan => "replan",
        }
    }
}
impl Request {
    pub fn patch_for(&self, option: Option<&str>) -> Option<&Patch> {
        option
            .and_then(|id| self.options.iter().find(|o| o.id == id))
            .and_then(|o| o.proposed_patch.as_ref())
            .or_else(|| {
                if option == self.recommended_option_id.as_deref() {
                    self.proposed_patch.as_ref()
                } else {
                    None
                }
            })
    }
    pub fn has_effects(&self) -> bool {
        self.proposed_patch.is_some()
            || self
                .options
                .iter()
                .any(|o| o.proposed_patch.is_some() || o.requires_replan)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct EffectCommit {
    pub patch_digest: String,
    pub graph_version: u64,
    pub actor: String,
    pub recorded_at: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub attestation: Option<super::secure::Attestation>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq)]
pub struct PlanningContext {
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub purpose: String,
    pub requests: Vec<DecisionRef>,
    pub worktree: String,
    pub artifact_digest: Option<String>,
}

fn default_candidates() -> usize {
    128
}
fn candidates_default(n: &usize) -> bool {
    *n == 128
}
fn default_similarity() -> u8 {
    75
}
fn similarity_default(n: &u8) -> bool {
    *n == 75
}

/// An indispensable action outside the worker's available capabilities.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct UserIntervention {
    pub action: String,
    pub inaccessible_reason: String,
    pub expected_result: String,
    #[serde(default)]
    pub verification: String,
}
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct ArtifactRef {
    pub id: String,
    pub digest: String,
}
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct EvidenceArtifact {
    pub id: String,
    pub plan_id: String,
    pub label: String,
    pub media_type: String,
    pub digest: String,
    pub size: usize,
    pub data_hex: String,
    pub actor: String,
    pub recorded_at: String,
}
/// Conditions are explicit checked criteria, never inferred from approval wording.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct ApprovalCondition {
    pub id: String,
    pub description: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub task_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_path: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_digest: Option<String>,
}

fn default_similarity_backend() -> String {
    "lexical".into()
}
fn is_lexical(s: &String) -> bool {
    s == "lexical"
}
