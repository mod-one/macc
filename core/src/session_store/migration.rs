use super::*;
use rusqlite::{params, OptionalExtension, TransactionBehavior};
use serde_json::{json, Value};
use std::path::Path;
pub(crate) fn stopped(root: &Path) -> Result<()> {
    use crate::process_ownership::{OwnershipStore, ProcessKind};
    for record in OwnershipStore::load(root)?.records {
        if matches!(
            record.process.kind,
            ProcessKind::Coordinator | ProcessKind::Supervisor
        ) && record
            .process
            .pid
            .is_some_and(|pid| crate::coordinator::helpers::is_pid_running(pid as i64))
        {
            return Err(error(
                "Stop coordinator and supervisor before changing session authority",
            ));
        }
    }
    Ok(())
}
fn source(root: &Path) -> Result<Vec<u8>> {
    let p = legacy_path(root);
    if p.exists() {
        bytes(&p)
    } else {
        Ok(b"{\"tools\":{}}".to_vec())
    }
}
fn stamp() -> String {
    uuid::Uuid::new_v4().to_string()
}
pub fn migrate(root: &Path, apply: bool, expected: Option<&str>) -> Result<MigrationReport> {
    let _guard = lock(root)?;
    stopped(root)?;
    if marker_path(root).exists() && is_sqlite(root)? {
        return Err(error("SQLite authority is already active"));
    }
    let raw = source(root)?;
    let sha = digest(&raw);
    if apply && expected != Some(sha.as_str()) {
        return Err(error("Migration source changed; preview again"));
    }
    let legacy: Value = json(&raw)?;
    let tools = legacy
        .get("tools")
        .and_then(Value::as_object)
        .ok_or_else(|| error("Invalid legacy tool inventory"))?;
    let mut entries = Vec::new();
    let mut quarantined = 0;
    for (tool, t) in tools {
        let sessions = t
            .get("sessions")
            .and_then(Value::as_object)
            .ok_or_else(|| error("Invalid legacy session map"))?;
        for (key, e) in sessions {
            if e["status"] == "active" {
                let pid = crate::coordinator::session_identity::owner_pid(e);
                if pid.is_none() || pid.is_some_and(crate::coordinator::helpers::is_pid_running) {
                    return Err(error(
                        "Active or ambiguous JSON session owner; stop runners before migration",
                    ));
                }
            }
            let mut identity = e["session_identity"].clone();
            let valid = super::leases::validate_import_identity(root, &identity).is_ok()
                && identity["tool_id"] == tool.as_str();
            let status = if !valid {
                "legacy_untrusted"
            } else {
                match e["status"].as_str() {
                    Some("available") => "available",
                    Some("closed") => "closed",
                    Some("archived") => "archived",
                    _ => "abandoned",
                }
            };
            if status != "available" {
                quarantined += 1;
            }
            let provider = if e.get("session_id").is_some() {
                e["session_id"].as_str().unwrap_or(key)
            } else {
                key
            };
            if !valid {
                identity = json!({"legacy_key":key,"legacy_metadata":e});
            }
            let updated = e
                .get("last_used_at")
                .or_else(|| e.get("updated_at"))
                .or_else(|| e.get("created_at"))
                .and_then(Value::as_str)
                .and_then(|s| chrono::DateTime::parse_from_rfc3339(s).ok())
                .map(|t| t.timestamp())
                .unwrap_or(0);
            entries.push((
                tool.clone(),
                provider.to_string(),
                identity,
                status.to_string(),
                updated,
                e["use_count"].as_i64().unwrap_or(0).max(0),
            ));
        }
    }
    let mut report = MigrationReport {
        direction: "json_to_sqlite".into(),
        source_sha256: sha,
        imported: entries.len(),
        quarantined,
        backup: None,
        applied: false,
    };
    if !apply {
        return Ok(report);
    }
    let backup = root.join(format!(
        ".macc/state/tool-sessions.pre-sqlite-{}.json",
        stamp()
    ));
    write(root, &backup, &raw)?;
    let prior = if marker_path(root).exists() {
        authority(root)?
    } else {
        None
    };
    let mut db = db::open(root, true)?;
    let existing_project: Option<String> = sql(db
        .query_row(
            "SELECT value FROM session_meta WHERE key='project_id'",
            [],
            |r| r.get(0),
        )
        .optional())?;
    let project = prior
        .map(|a| a.project_id)
        .or_else(|| existing_project.clone())
        .unwrap_or_else(|| uuid::Uuid::new_v4().to_string());
    if existing_project.as_deref().is_some_and(|p| p != project) {
        return Err(error("Existing SQLite project identity differs"));
    }
    let tx = sql(db.transaction_with_behavior(TransactionBehavior::Immediate))?;
    let leases: i64 = sql(tx.query_row("SELECT count(*) FROM session_leases", [], |r| r.get(0)))?;
    if leases != 0 {
        return Err(error(
            "Existing SQLite leases must be resolved before migration",
        ));
    }
    sql(tx.execute(
        "INSERT OR REPLACE INTO session_meta VALUES('project_id',?1)",
        [&project],
    ))?;
    for (tool, provider, identity, status, updated, uses) in entries {
        let existing:Option<(String,String)>=sql(tx.query_row("SELECT id,identity_json FROM logical_sessions WHERE tool_id=?1 AND provider_session_id=?2",params![tool,provider],|r|Ok((r.get(0)?,r.get(1)?))).optional())?;
        if let Some((id, old)) = existing.as_ref() {
            if old == &identity.to_string() {
                sql(tx.execute("UPDATE logical_sessions SET status=?2,updated_at=?3,use_count=MAX(use_count,?4) WHERE id=?1",params![id,status,updated,uses]))?;
                db::event(
                    &tx,
                    id,
                    "session_reimported",
                    json!({"source_sha256":report.source_sha256}),
                )?;
                continue;
            }
        }
        let id = uuid::Uuid::new_v4().to_string();
        let lineage = uuid::Uuid::new_v4().to_string();
        let collision = existing.is_some();
        let provider = if collision { None } else { Some(provider) };
        let status = if collision {
            "legacy_untrusted"
        } else {
            &status
        };
        sql(tx.execute("INSERT INTO logical_sessions(id,project_id,task_id,role,tool_id,config_fingerprint,environment_fingerprint,identity_json,provider_session_id,lineage_id,status,use_count,created_at,updated_at)
            VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?13)",params![id,project,identity["task_id"].as_str().unwrap_or(""),identity["role"].as_str().unwrap_or("unknown"),tool,
            digest(identity["execution_config"].to_string().as_bytes()),digest(identity["execution_root"].to_string().as_bytes()),identity.to_string(),provider,lineage,status,uses,updated]))?;
        db::event(
            &tx,
            &id,
            "session_imported",
            json!({"status":status,"source_sha256":report.source_sha256}),
        )?;
    }
    sql(tx.commit())?;
    let a = Authority {
        version: 1,
        mode: "sqlite".into(),
        project_id: project,
        project_root: root
            .canonicalize()
            .map_err(|e| error(e.to_string()))?
            .display()
            .to_string(),
    };
    // Marker is published LAST. A crash before publication leaves JSON authoritative.
    publish(root, &a)?;
    report.backup = Some(backup.display().to_string());
    report.applied = true;
    write(
        root,
        &root.join(".macc/reports/session-store-migration.json"),
        &serde_json::to_vec_pretty(&report).map_err(|e| error(e.to_string()))?,
    )?;
    Ok(report)
}
pub fn rollback(root: &Path, apply: bool, expected: Option<&str>) -> Result<MigrationReport> {
    let _guard = lock(root)?;
    stopped(root)?;
    let (db, mut a) = db::active(root)?;
    let leases: i64 = sql(db.query_row("SELECT count(*) FROM session_leases", [], |r| r.get(0)))?;
    if leases > 0 {
        return Err(error("Resolve all SQLite leases before rollback"));
    }
    let raw = export(&db)?;
    let sha = digest(&raw);
    let mut report = MigrationReport {
        direction: "sqlite_to_json".into(),
        source_sha256: sha,
        imported: 0,
        quarantined: 0,
        backup: None,
        applied: false,
    };
    let data: Value = json(&raw)?;
    for t in data["tools"]
        .as_object()
        .into_iter()
        .flat_map(|m| m.values())
    {
        if let Some(s) = t["sessions"].as_object() {
            report.imported += s.len();
            report.quarantined += s.values().filter(|e| e["status"] != "available").count();
        }
    }
    if !apply {
        return Ok(report);
    }
    if expected != Some(report.source_sha256.as_str()) {
        return Err(error("Rollback source changed; preview again"));
    }
    let backup = root.join(format!(
        ".macc/state/session-store.pre-json-{}.sqlite",
        stamp()
    ));
    db::snapshot(&db, &backup)?;
    a.mode = "json".into();
    let mut document: Value = json(&raw)?;
    document["session_store"] = serde_json::to_value(&a).map_err(|e| error(e.to_string()))?;
    write(
        root,
        &legacy_path(root),
        &serde_json::to_vec_pretty(&document).map_err(|e| error(e.to_string()))?,
    )?;
    report.applied = true;
    report.backup = Some(backup.display().to_string());
    write(
        root,
        &root.join(".macc/reports/session-store-rollback.json"),
        &serde_json::to_vec_pretty(&report).map_err(|e| error(e.to_string()))?,
    )?;
    Ok(report)
}
fn export(db: &rusqlite::Connection) -> Result<Vec<u8>> {
    let mut out = json!({"tools":{}});
    let mut q=sql(db.prepare("SELECT tool_id,provider_session_id,identity_json,status,updated_at,use_count FROM logical_sessions WHERE provider_session_id IS NOT NULL ORDER BY id"))?;
    let rows = sql(q.query_map([], |r| {
        Ok((
            r.get::<_, String>(0)?,
            r.get::<_, String>(1)?,
            r.get::<_, String>(2)?,
            r.get::<_, String>(3)?,
            r.get::<_, i64>(4)?,
            r.get::<_, i64>(5)?,
        ))
    }))?;
    for row in rows {
        let (tool, sid, identity, status, updated, uses) = sql(row)?;
        let identity: Value = json(identity.as_bytes())?;
        let ts = chrono::DateTime::from_timestamp(updated, 0)
            .unwrap_or_default()
            .format("%Y-%m-%dT%H:%M:%SZ")
            .to_string();
        out["tools"][&tool]["sessions"][&sid] = json!({"session_identity":identity,"status":status,"updated_at":ts,"last_used_at":ts,"use_count":uses});
    }
    serde_json::to_vec_pretty(&out).map_err(|e| error(e.to_string()))
}

/// Legacy inspection never infers resumability from a last-task field.
pub(super) fn legacy_views(root: &Path) -> Result<Vec<SessionView>> {
    let value: Value = json(&source(root)?)?;
    let mut result = Vec::new();
    if let Some(tools) = value["tools"].as_object() {
        for (tool, t) in tools {
            if let Some(sessions) = t["sessions"].as_object() {
                for (key, e) in sessions {
                    let identity = &e["session_identity"];
                    result.push(SessionView {
                        id: format!("legacy:{tool}:{key}"),
                        project_id: "legacy".into(),
                        task_id: identity["task_id"].as_str().unwrap_or("").into(),
                        role: identity["role"].as_str().unwrap_or("unknown").into(),
                        tool_id: tool.clone(),
                        provider_session_id: Some(e["session_id"].as_str().unwrap_or(key).into()),
                        lineage_id: "legacy".into(),
                        status: if super::leases::validate_import_identity(root, identity).is_ok()
                            && identity["tool_id"] == tool.as_str()
                        {
                            format!("json_{}", e["status"].as_str().unwrap_or("unknown"))
                        } else {
                            "legacy_untrusted".into()
                        },
                        generation: 0,
                        use_count: e["use_count"].as_i64().unwrap_or(0).max(0),
                        owner_pid: crate::coordinator::session_identity::owner_pid(e),
                        expires_at: None,
                        updated_at: 0,
                    });
                }
            }
        }
    }
    result.truncate(500);
    Ok(result)
}
