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
    pub enabled: bool,
    pub human_question_min_risk: Consultation,
    pub max_context_bytes: usize,
    pub max_request_bytes: usize,
    /// Local trust is explicit; secure approvals require an isolated authority.
    pub trust_mode: String,
    pub mandatory_human_triggers: Vec<String>,
    pub approval_reuse: ApprovalReuse,
    /// Explicit, bytewise Git subject manifests for content-equivalent reuse.
    pub approval_manifests: BTreeMap<String, Vec<String>>,
}
impl Default for Policy {
    fn default() -> Self {
        Self {
            enabled: false,
            human_question_min_risk: Consultation::High,
            max_context_bytes: 32768,
            max_request_bytes: 65536,
            trust_mode: "local_trust".into(),
            mandatory_human_triggers: vec!["production".into(), "irreversible".into()],
            approval_reuse: ApprovalReuse::ExactRevision,
            approval_manifests: BTreeMap::new(),
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
        if self.trust_mode != "local_trust" {
            return Err(super::error("secure_approval requires an isolated approver broker; local credentials cannot supply secure human approval"));
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
    pub id: String,
    pub label: String,
    #[serde(default)]
    pub consequences: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Request {
    pub idempotency_key: String,
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
}
fn default_kind() -> String {
    "decision".into()
}
fn default_scope() -> String {
    "task".into()
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Resolution {
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
}
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Decision {
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
    pub subject_digest: String,
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
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ContextPackage {
    pub schema_version: u32,
    pub plan_id: String,
    pub task_id: String,
    pub attempt_id: String,
    pub phase: String,
    pub contract_digest: String,
    pub policy: Policy,
    pub bindings: Vec<Decision>,
    pub context_digest: String,
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
