//! Lease-fenced decision-wait receipts, scoped to one coordinator invocation.
use super::*;
use serde::{Deserialize, Serialize};
use serde_json::json;
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DecisionWait {
    pub task_id: String,
    pub source: String,
    pub session_id: String,
    pub generation: i64,
    pub decision: Option<crate::coordinator::decisions::PendingDecision>,
    pub reason: String,
}
pub fn heartbeat_state(root: &Path, key: &LeaseKey, source: &str) -> Result<serde_json::Value> {
    leases::with_verified_lease(root, key, |tx| {
        let task: String = sql(tx.query_row(
            "SELECT task_id FROM logical_sessions WHERE id=?1",
            [&key.session_id],
            |r| r.get(0),
        ))?;
        let decision = crate::coordinator::decisions::pending_for_task(root, &task)?;
        let reason = if let Some(decision) = &decision {
            Some(decision.reason())
        } else {
            match leases::verify_decision_pin(root,tx,&key.session_id,key.generation) {
                Ok(())=>None,
                Err(error) if error.to_string().contains("DM_STALE:")=>Some("DM_STALE: native decision binding changed; preserve work and resume the retained phase with fresh context".to_string()),
                Err(error)=>return Err(error),
            }
        };
        if let Some(reason) = reason {
            let receipt = DecisionWait {
                task_id: task,
                source: source.into(),
                session_id: key.session_id.clone(),
                generation: key.generation,
                decision,
                reason,
            };
            db::event(
                tx,
                &key.session_id,
                "decision_wait",
                serde_json::to_value(&receipt).map_err(|e| error(e.to_string()))?,
            )?;
            return Ok(json!({"status":"waiting_decision","wait":receipt}));
        }
        sql(tx.execute("UPDATE session_leases SET expires_at=?2+(expires_at-heartbeat_at),heartbeat_at=?2 WHERE logical_session_id=?1",rusqlite::params![key.session_id,now()]))?;
        Ok(json!({"status":"active"}))
    })
}
pub fn waiting_for_invocation(
    root: &Path,
    task: &str,
    source: &str,
) -> Result<Option<DecisionWait>> {
    use rusqlite::OptionalExtension;
    if source.is_empty() || authority(root)?.is_none_or(|a| a.mode != "sqlite") {
        return Ok(None);
    }
    let (db, _) = db::readonly(root)?;
    let raw:Option<String>=sql(db.query_row("SELECT payload_json FROM session_events WHERE event_type='decision_wait' AND json_extract(payload_json,'$.task_id')=?1 AND json_extract(payload_json,'$.source')=?2 ORDER BY rowid DESC LIMIT 1",[task,source],|r|r.get(0)).optional())?;
    raw.map(|s| json(s.as_bytes())).transpose()
}
