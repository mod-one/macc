//! Content-addressed optional selection cache. Retrieval validates sources before lookup.
use super::*;
use rusqlite::{params, OptionalExtension};
use serde::{Deserialize, Serialize};
#[derive(Serialize, Deserialize)]
pub(crate) struct SelectionCache {
    pub selected: Vec<context::Selection>,
    pub exclusions: Vec<String>,
    pub used: u64,
}
pub(crate) fn select(
    db: &rusqlite::Connection,
    checkpoint: &store::checkpoints::Checkpoint,
    hits: Vec<memory::Hit>,
    limit: u64,
) -> Result<(String, bool, SelectionCache)> {
    let fingerprint = digest(serde_json::to_vec(&serde_json::json!({
        "schema":1,"project":checkpoint.project_id,"task":checkpoint.mandatory_task,
        "phase":checkpoint.phase,"role":checkpoint.role,"tool":checkpoint.tool_id,
        "revision":checkpoint.source_revision,"worktree":checkpoint.worktree_content_digest,
        "sources":checkpoint.source_hashes,"limit":limit,
        "candidates":hits.iter().map(|h|(&h.memory.id,&h.checksum,&h.rank_reason)).collect::<Vec<_>>()
    })).map_err(|e| error(e.to_string()))?.as_slice());
    let cached: Option<(String, String)> = sql(db
        .query_row(
            "SELECT content_json,checksum FROM context_content_cache WHERE fingerprint=?1",
            [&fingerprint],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .optional())?;
    if let Some((raw, checksum)) = cached {
        if digest(raw.as_bytes()) == checksum {
            return Ok((fingerprint, true, store::json(raw.as_bytes())?));
        }
        // Rebuild derived content from current validated authority after corruption.
    }
    let mut result = SelectionCache {
        selected: vec![],
        exclusions: vec![],
        used: 0,
    };
    let mut seen = std::collections::BTreeSet::new();
    for h in hits {
        let p = &h.memory.proposal;
        let size = (p.title.chars().count() + p.body.chars().count() + 3) as u64 / 4 + 40;
        if !seen.insert(digest(p.body.as_bytes())) {
            result
                .exclusions
                .push(format!("{}: duplicate body", h.memory.id));
            continue;
        }
        if result.used + size > limit {
            result
                .exclusions
                .push(format!("{}: optional memory budget", h.memory.id));
            continue;
        }
        result.used += size;
        result.selected.push(context::Selection {
            id: h.memory.id,
            checksum: h.checksum,
            source_refs: p.evidence.iter().map(|e| e.reference.clone()).collect(),
            reason: h.rank_reason,
            estimated_tokens: size,
            title: p.title.clone(),
            body: p.body.clone(),
        });
    }
    let raw = serde_json::to_string(&result).map_err(|e| error(e.to_string()))?;
    sql(db.execute("INSERT INTO context_content_cache VALUES(?1,?2,?3) ON CONFLICT(fingerprint) DO UPDATE SET content_json=excluded.content_json,checksum=excluded.checksum",params![fingerprint,raw,digest(raw.as_bytes())]))?;
    Ok((fingerprint, false, result))
}
