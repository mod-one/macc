//! Shared application operations used by CLI, TUI and Web.
use super::*;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "action", rename_all = "snake_case", deny_unknown_fields)]
pub enum Action {
    Status,
    AgentStatus,
    Overview {
        query: overview::Query,
    },
    MemoryExtract {
        checkpoint_id: String,
    },
    PolicyPreview {
        revision: String,
        policy: Option<policy::Policy>,
    },
    PolicyApply {
        revision: String,
        policy: Option<policy::Policy>,
    },
    UpgradePlan,
    UpgradeApply {
        source_sha256: String,
    },
    MemorySearch {
        search: memory::Search,
    },
    MemoryPull {
        search: memory::Search,
        #[serde(default)]
        cursor: usize,
    },
    MemoryRead {
        id: String,
        access: memory::Access,
        #[serde(default)]
        offset: usize,
    },
    MemoryGet {
        id: String,
        access: memory::Access,
    },
    MemoryPropose {
        proposal: memory::Proposal,
    },
    MemoryReview {
        review: memory::Review,
    },
    MemoryEvidence {
        path: String,
        #[serde(default)]
        worktree: Option<String>,
    },
    MemorySymbolEvidence {
        path: String,
        symbol: String,
        provider: String,
        #[serde(default)]
        worktree: Option<String>,
    },
    MemoryEvidenceSources,
    MemoryToolEvidence {
        tool_id: String,
    },
    MemoryConflict {
        left: String,
        right: String,
        reason: String,
    },
    MemoryResolveConflict {
        #[serde(default)]
        resolution: memory::Resolution,
        #[serde(default)]
        expected_checksum: Option<String>,
        id: String,
        reason: String,
        confirmed: bool,
    },
    MemoryConflicts,
    SemanticStatus {
        access: memory::Access,
    },
    SemanticBenchmark {
        suite: semantic::Suite,
    },
    SemanticGate {
        report_id: String,
        checksum: String,
        enabled: bool,
        confirmed: bool,
    },
    FtsRebuild,
    ContextList,
    ContextGet {
        id: String,
    },
    Reduce {
        kind: String,
        raw: String,
    },
    Providers,
    ProviderPlan {
        provider: String,
        operation: String,
    },
    ProviderApply {
        plan_id: String,
        checksum: String,
    },
    CodeQuery {
        query: providers::Query,
    },
    ProviderOperation {
        id: String,
    },
    CodeCompare {
        provider: String,
        query: providers::Query,
    },
    CompactionList,
    NativeSettingPreview {
        id: String,
    },
    NativeSettingAdopt {
        id: String,
        checksum: String,
    },
    CompactionState,
    CheckpointCreate {
        id: String,
    },
    CompactionProbe {
        tool_id: String,
    },
    CompactionPreview {
        uses: u64,
        budget: u64,
        context_tokens: Option<u64>,
        window: Option<u64>,
        #[serde(default)]
        in_flight: bool,
    },
    Rotate {
        id: String,
        confirmed: bool,
    },
}
impl Action {
    pub fn is_mutation(&self) -> bool {
        !matches!(
            self,
            Self::Status
                | Self::SemanticStatus { .. }
                | Self::Overview { .. }
                | Self::AgentStatus
                | Self::PolicyPreview { .. }
                | Self::UpgradePlan
                | Self::MemorySearch { .. }
                | Self::MemoryGet { .. }
                | Self::MemoryPull { .. }
                | Self::MemoryRead { .. }
                | Self::MemoryEvidence { .. }
                | Self::MemorySymbolEvidence { .. }
                | Self::MemoryEvidenceSources
                | Self::MemoryConflicts
                | Self::ContextList
                | Self::ContextGet { .. }
                | Self::Providers
                | Self::CodeQuery { .. }
                | Self::CodeCompare { .. }
                | Self::ProviderOperation { .. }
                | Self::NativeSettingPreview { .. }
                | Self::CompactionList
                | Self::CompactionState
                | Self::CompactionPreview { .. }
        )
    }
}
fn value<T: Serialize>(v: T) -> Result<Value> {
    serde_json::to_value(v).map_err(|e| error(e.to_string()))
}
pub fn execute(root: &Path, action: Action) -> Result<Value> {
    match action {
        Action::Status => value(policy::inspect(root)?),
        Action::SemanticStatus { access } => semantic::status(root, &access),
        Action::SemanticBenchmark { suite } => semantic::benchmark(root, suite),
        Action::SemanticGate {
            report_id,
            checksum,
            enabled,
            confirmed,
        } => semantic::apply_gate(root, &report_id, &checksum, enabled, confirmed),
        Action::Overview { query } => value(overview::inspect(root, &query)?),
        Action::AgentStatus => agent::status(root),
        Action::MemoryExtract { checkpoint_id } => {
            extraction::from_checkpoint(root, &checkpoint_id)
        }
        Action::PolicyPreview { revision, policy } => {
            if let Some(p) = &policy {
                p.validate()?;
            }
            let current = policy::inspect(root)?;
            if current.revision != revision {
                return Err(error("Configuration revision conflict; reload and review"));
            }
            Ok(
                json!({"validated":true,"before":current.policy,"after":policy.unwrap_or_default(),"activation":"next invocation/query; native deployment remains separate"}),
            )
        }
        Action::PolicyApply { revision, policy } => value(policy::update(root, &revision, policy)?),
        Action::UpgradePlan => value(upgrade(root, false, None)?),
        Action::UpgradeApply { source_sha256 } => value(upgrade(root, true, Some(&source_sha256))?),
        Action::MemorySearch { search } => value(memory::search(root, &search)?),
        Action::MemoryPull { search, cursor } => value(memory::pull(root, &search, cursor)?),
        Action::MemoryRead { id, access, offset } => {
            value(memory::read(root, &id, &access, offset)?)
        }
        Action::MemoryGet { id, access } => value(memory::get(root, &id, &access)?),
        Action::MemoryPropose { proposal } => value(memory::propose(root, &proposal)?),
        Action::MemoryReview { review } => {
            if std::env::var_os("MACC_SESSION_ROLE").is_some() {
                return Err(error("Agent sessions may propose memory; promotion requires a separate operator review"));
            }
            value(memory::review(root, &review)?)
        }
        Action::MemoryEvidence { path, worktree } => {
            let target = providers::native_worktree(root, worktree.as_deref())?;
            let (_, bytes) = paths::source(&target, &path, 1048576)?;
            paths::safe_text(std::str::from_utf8(&bytes).map_err(|e| error(e.to_string()))?)?;
            value(memory::Evidence {
                binding: None,
                kind: "file".into(),
                reference: path,
                sha256: Some(digest(&bytes)),
                revision: Some(crate::git::head_commit(&target)?),
                summary:
                    "Exact source hash captured for review; not proof of the proposed assertion"
                        .into(),
            })
        }
        Action::MemoryConflict {
            left,
            right,
            reason,
        } => value(memory::conflict(root, &left, &right, &reason)?),
        Action::MemoryResolveConflict {
            id,
            reason,
            confirmed,
            resolution,
            expected_checksum,
        } => memory::resolve_conflict(
            root,
            &id,
            &reason,
            confirmed,
            resolution,
            expected_checksum.as_deref(),
        ),
        Action::MemoryConflicts => value(memory::conflicts(root)?),
        Action::MemorySymbolEvidence {
            path,
            symbol,
            provider,
            worktree,
        } => value(memory::symbol_evidence(
            root,
            &path,
            &symbol,
            &provider,
            worktree.as_deref(),
        )?),
        Action::MemoryEvidenceSources => providers::evidence_sources(root),
        Action::MemoryToolEvidence { tool_id } => value(memory::tool_evidence(root, &tool_id)?),
        Action::FtsRebuild => {
            memory::fts_rebuild(root)?;
            Ok(json!({"rebuilt":true}))
        }
        Action::ContextList => value(context::list(root)?),
        Action::ContextGet { id } => value(context::get(root, &id)?),
        Action::Reduce { kind, raw } => value(context::reduce(root, &kind, &raw)?),
        Action::Providers => value(providers::inspect(root)?),
        Action::ProviderPlan {
            provider,
            operation,
        } => {
            let result = providers::plan(root, &provider, &operation)?;
            let sha = match &result {
                providers::Capability::Supported(plan) => Some(digest(
                    serde_json::to_string(plan)
                        .map_err(|e| error(e.to_string()))?
                        .as_bytes(),
                )),
                _ => None,
            };
            Ok(json!({"result":result,"checksum":sha}))
        }
        Action::ProviderApply { plan_id, checksum } => {
            value(providers::apply(root, &plan_id, &checksum)?)
        }
        Action::CodeQuery { query } => value(providers::query(root, &query)?),
        Action::ProviderOperation { id } => providers::operation(root, &id),
        Action::CodeCompare { provider, query } => providers::compare(root, &provider, &query),
        Action::CompactionState => value(compaction::states(root)?),
        Action::CheckpointCreate { id } => {
            let request = {
                let _guard = store::lock(root)?;
                let (db, _) = database(root)?;
                let identity: String = sql(db.query_row(
                    "SELECT identity_json FROM logical_sessions WHERE id=?1",
                    [&id],
                    |r| r.get(0),
                ))?;
                let identity: Value = store::json(identity.as_bytes())?;
                let phase:String=sql(db.query_row("SELECT coalesce((SELECT phase FROM task_checkpoints WHERE task_id=?1 AND role=?2 ORDER BY created_at DESC,rowid DESC LIMIT 1),'dev')",rusqlite::params![identity["task_id"].as_str(),identity["role"].as_str()],|r|r.get(0)))?;
                store::checkpoints::ContextRequest { identity, phase }
            };
            value(store::checkpoints::prepare(root, &request)?)
        }
        Action::NativeSettingPreview { id } => compaction::adoption::preview(root, &id),
        Action::NativeSettingAdopt { id, checksum } => {
            compaction::adoption::apply(root, &id, &checksum)
        }
        Action::CompactionList => value(compaction::inspect(root)?),
        Action::CompactionProbe { tool_id } => value(compaction::probe(root, &tool_id)?),
        Action::CompactionPreview {
            uses,
            budget,
            context_tokens,
            window,
            in_flight,
        } => value(compaction::decision(
            &policy::load(root)?.adaptive_compaction,
            uses,
            budget,
            context_tokens,
            window,
            in_flight,
        )),
        Action::Rotate { id, confirmed } => {
            if !confirmed {
                return Err(error(
                    "Confirm rotation after inspecting the scoped session",
                ));
            }
            store::close(root, &id)?;
            Ok(
                json!({"closed":id,"effect":"next invocation creates a fresh transcript; Git work retained"}),
            )
        }
    }
}
