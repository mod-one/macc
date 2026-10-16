//! Reviewed ownership adoption for pre-v2 or explicitly user-owned native settings.
use super::*;
#[derive(Serialize, Deserialize)]
pub struct AdoptionPlan {
    pub session_id: String,
    pub tool_id: String,
    pub worktree: String,
    pub path: String,
    pub key: String,
    pub current_value: serde_json::Value,
    pub file_sha256: String,
    pub policy_sha256: String,
    pub effect: String,
}
fn inspect_in(db: &rusqlite::Connection, root: &Path, id: &str) -> Result<AdoptionPlan> {
    let raw: String = sql(db.query_row(
        "SELECT identity_json FROM logical_sessions WHERE id=?1",
        [id],
        |r| r.get(0),
    ))?;
    let identity: serde_json::Value = store::json(raw.as_bytes())?;
    let wt = identity["execution_root"]
        .as_str()
        .ok_or_else(|| error("Missing worktree"))?;
    let target = super::super::providers::native_worktree(root, Some(wt))?;
    let active:bool=sql(db.query_row("SELECT EXISTS(SELECT 1 FROM session_leases l JOIN logical_sessions s ON s.id=l.logical_session_id WHERE json_extract(s.identity_json,'$.execution_root')=?1)",[wt],|r|r.get(0)))?;
    if active {
        return Err(error(
            "Stop the worktree owner before adopting its native setting",
        ));
    }
    let profile: crate::tool::compaction::CompactionSpec =
        serde_json::from_value(identity["execution_config"]["performer"]["compaction"].clone())
            .map_err(|e| error(e.to_string()))?;
    profile.validate()?;
    let native = profile
        .native_auto
        .ok_or_else(|| error("Native configuration unsupported"))?;
    if native.format == "env" {
        return Err(error(
            "Environment overrides are invocation-local and need no ownership adoption",
        ));
    }
    let path = native
        .path
        .ok_or_else(|| error("Missing native configuration path"))?;
    let mut file = target.clone();
    for component in Path::new(&path).components() {
        file.push(component);
        if std::fs::symlink_metadata(&file).is_ok_and(|m| m.file_type().is_symlink()) {
            return Err(error("Native configuration symlink rejected"));
        }
    }
    if std::fs::metadata(&file)
        .map_err(|e| error(e.to_string()))?
        .len()
        > 1048576
    {
        return Err(error("Native config too large"));
    }
    let raw = std::fs::read_to_string(&file).map_err(|e| error(e.to_string()))?;
    let document = if native.format == "toml" {
        serde_json::to_value(toml::from_str::<toml::Value>(&raw).map_err(|e| error(e.to_string()))?)
            .map_err(|e| error(e.to_string()))?
    } else {
        serde_json::from_str(&raw).map_err(|e| error(e.to_string()))?
    };
    let value = document
        .pointer(&format!("/{}", native.key.replace('.', "/")))
        .cloned()
        .ok_or_else(|| error("No explicit native value to adopt"))?;
    let policy = super::super::policy::load(root)?;
    Ok(AdoptionPlan{session_id:id.into(),tool_id:identity["tool_id"].as_str().unwrap_or_default().into(),worktree:target.to_string_lossy().into_owned(),path,key:native.key,current_value:value,file_sha256:digest(raw.as_bytes()),policy_sha256:digest(&serde_json::to_vec(&policy).map_err(|e|error(e.to_string()))?),effect:"MACC will manage this one setting on subsequent invocations, including updating/removing its override according to the current policy; other settings stay user-owned".into()})
}
pub fn preview(root: &Path, id: &str) -> Result<serde_json::Value> {
    let _guard = store::lock(root)?;
    let (db, _) = database(root)?;
    let plan = inspect_in(&db, root, id)?;
    let checksum = digest(&serde_json::to_vec(&plan).map_err(|e| error(e.to_string()))?);
    Ok(serde_json::json!({"plan":plan,"checksum":checksum}))
}
pub fn apply(root: &Path, id: &str, expected: &str) -> Result<serde_json::Value> {
    let _guard = store::lock(root)?;
    let (mut db, _) = database(root)?;
    let tx = sql(db.transaction())?;
    let plan = inspect_in(&tx, root, id)?;
    if digest(&serde_json::to_vec(&plan).map_err(|e| error(e.to_string()))?) != expected {
        return Err(error(
            "Native setting or policy changed; preview adoption again",
        ));
    }
    sql(tx.execute("INSERT INTO native_setting_ownership VALUES(?1,?2,?3,?4) ON CONFLICT(worktree,path,setting_key) DO UPDATE SET value_json=excluded.value_json",params![plan.worktree,plan.path,plan.key,plan.current_value.to_string()]))?;
    super::super::schema::audit(
        &tx,
        "native_setting_adopted",
        id,
        serde_json::json!({"path":plan.path,"key":plan.key}),
    )?;
    sql(tx.commit())?;
    Ok(serde_json::json!({"adopted":true,"effect":plan.effect}))
}
