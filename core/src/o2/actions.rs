//! Shared application operations used by CLI, TUI and Web.
use super::*;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
#[derive(Debug, Serialize, Deserialize)]
#[serde(tag = "action", rename_all = "snake_case", deny_unknown_fields)]
pub enum Action {
    Status,
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
    MemoryConflict {
        left: String,
        right: String,
        reason: String,
    },
    MemoryResolveConflict {
        id: String,
        reason: String,
        confirmed: bool,
    },
    MemoryConflicts,
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
    CompactionList,
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
                | Self::PolicyPreview { .. }
                | Self::UpgradePlan
                | Self::MemorySearch { .. }
                | Self::MemoryGet { .. }
                | Self::MemoryEvidence { .. }
                | Self::MemoryConflicts
                | Self::ContextList
                | Self::ContextGet { .. }
                | Self::Providers
                | Self::CodeQuery { .. }
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
        } => {
            if !confirmed || reason.trim().is_empty() || reason.len() > 500 {
                return Err(error(
                    "Conflict resolution requires confirmed review and reason",
                ));
            }
            paths::safe_text(&reason)?;
            let _guard = store::lock(root)?;
            let (mut db, _) = database(root)?;
            let tx = sql(db.transaction())?;
            let changed = sql(tx.execute(
                "UPDATE memory_conflicts SET status='resolved' WHERE id=?1 AND status='open'",
                [&id],
            ))?;
            if changed != 1 {
                return Err(error("Open conflict not found"));
            }
            schema::audit(
                &tx,
                "memory_conflict_resolved",
                &id,
                json!({"reason":reason}),
            )?;
            sql(tx.commit())?;
            Ok(json!({"resolved":id}))
        }
        Action::MemoryConflicts => {
            let _guard = store::lock(root)?;
            let (db, _) = database(root)?;
            let mut q=sql(db.prepare("SELECT id,left_id,right_id,reason,status FROM memory_conflicts ORDER BY rowid DESC LIMIT 100"))?;
            let rows=sql(q.query_map([],|r|Ok(json!({"id":r.get::<_,String>(0)?,"left":r.get::<_,String>(1)?,"right":r.get::<_,String>(2)?,"reason":r.get::<_,String>(3)?,"status":r.get::<_,String>(4)?}))))?.map(sql).collect::<Result<Vec<_>>>()?;
            value(rows)
        }
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
