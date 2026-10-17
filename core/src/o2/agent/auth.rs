//! Derived, revocable capabilities: original lease tokens never reach MCP clients.
use super::*;
use rusqlite::{Connection, OptionalExtension};
#[derive(Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Credentials {
    pub session_id: String,
    pub generation: i64,
    pub token: String,
}
pub(crate) struct Scope {
    pub project: String,
    pub task: String,
    pub role: String,
    pub worktree: std::path::PathBuf,
    pub access: memory::Access,
    pub checkpoint: Option<store::checkpoints::Checkpoint>,
}
fn token(key: &store::LeaseKey) -> String {
    digest(
        format!(
            "macc-o2-mcp-v1:{}:{}:{}",
            key.session_id, key.generation, key.lease_token
        )
        .as_bytes(),
    )
}
pub fn options(root: &Path, key: &store::LeaseKey) -> Result<Value> {
    let p = policy::load(root)?;
    if !p.memory.enabled || !p.memory.mcp_enabled {
        return Ok(json!({"env":{}}));
    }
    let (db, _) = store::readonly_database(root)?;
    store::verify_lease(&db, key)?;
    let credentials = Credentials {
        session_id: key.session_id.clone(),
        generation: key.generation,
        token: token(key),
    };
    authorize(root, &db, &credentials)?;
    Ok(
        json!({"env":{"MACC_O2_PROJECT_ROOT":root.canonicalize().map_err(|e|error(e.to_string()))?,"MACC_O2_MCP_SESSION":key.session_id,"MACC_O2_MCP_GENERATION":key.generation.to_string(),"MACC_O2_MCP_TOKEN":credentials.token}}),
    )
}
pub(crate) fn authorize(root: &Path, db: &Connection, c: &Credentials) -> Result<Scope> {
    enabled(root)?;
    if c.session_id.len() > 64 || c.token.len() != 64 || c.generation < 1 {
        return Err(error("Invalid MCP capability"));
    }
    let a = store::authority(root)?
        .filter(|a| a.mode == "sqlite")
        .ok_or_else(|| error("SQLite authority required"))?;
    if !super::super::ready(db)? {
        return Err(error("O2 upgrade required"));
    }
    let (project,task,role,tool,identity,lease):(String,String,String,String,String,String)=sql(db.query_row(
        "SELECT s.project_id,s.task_id,s.role,s.tool_id,s.identity_json,l.lease_token FROM logical_sessions s JOIN session_leases l ON l.logical_session_id=s.id WHERE s.id=?1 AND s.generation=?2 AND s.status='active'",
        rusqlite::params![c.session_id,c.generation],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?,r.get(4)?,r.get(5)?))))?;
    if project != a.project_id {
        return Err(error("Foreign MCP project"));
    }
    let key = store::LeaseKey {
        session_id: c.session_id.clone(),
        generation: c.generation,
        lease_token: lease,
    };
    let expected = token(&key);
    if expected
        .bytes()
        .zip(c.token.bytes())
        .fold(0u8, |a, (x, y)| a | (x ^ y))
        != 0
    {
        return Err(error("Invalid MCP capability"));
    }
    store::verify_lease(db, &key)?;
    let identity: Value = store::json(identity.as_bytes())?;
    if identity["task_id"] != task || identity["role"] != role || identity["tool_id"] != tool {
        return Err(error("Inconsistent session scope"));
    }
    let worktree = providers::native_worktree(root, identity["execution_root"].as_str())?;
    let checkpoint_id:Option<String>=sql(db.query_row("SELECT json_extract(payload_json,'$.checkpoint_id') FROM session_events WHERE logical_session_id=?1 AND event_type IN ('session_reserved','session_reused') AND json_extract(payload_json,'$.generation')=?2 ORDER BY rowid DESC LIMIT 1",rusqlite::params![c.session_id,c.generation],|r|r.get(0)).optional())?.flatten();
    let checkpoint = checkpoint_id
        .map(|id| store::checkpoints::load(db, &id))
        .transpose()?;
    if let Some(cp) = &checkpoint {
        let state: String = sql(db.query_row(
            "SELECT validation_state FROM task_checkpoints WHERE id=?1",
            [&cp.id],
            |r| r.get(0),
        ))?;
        if cp.project_id != project
            || cp.task_id != task
            || cp.role != role
            || cp.worktree != worktree.to_string_lossy()
            || cp.tool_id != tool
            || !["verified", "reconciled"].contains(&state.as_str())
        {
            return Err(error(
                "Checkpoint scope or validation changed; reconcile before MCP access",
            ));
        }
    }
    let feature = checkpoint
        .as_ref()
        .and_then(|cp| context::task_feature(&cp.mandatory_task));
    Ok(Scope {
        project,
        task: task.clone(),
        role,
        worktree: worktree.clone(),
        access: memory::Access {
            task_id: Some(task),
            tool_id: Some(tool),
            worktree: Some(worktree.to_string_lossy().into_owned()),
            feature,
            user_id: None,
        },
        checkpoint,
    })
}
pub(crate) fn connection(root: &Path, c: &Credentials) -> Result<(Connection, Scope)> {
    let (db, _) = store::readonly_database(root)?;
    let scope = authorize(root, &db, c)?;
    Ok((db, scope))
}
