//! Operator-only transfer: pure previews, exact confirmation and proposed-only import.
use super::transfer_format::{self as wire, Entry};
use super::*;
use rusqlite::OptionalExtension;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
pub use wire::Format;
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ExportRequest {
    pub ids: Vec<String>,
    #[serde(default)]
    pub access: Access,
    #[serde(default)]
    pub format: Format,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ImportRequest {
    pub document: String,
    #[serde(default)]
    pub access: Access,
    #[serde(default)]
    pub format: Format,
}
fn settings(root: &Path) -> Result<(u64, String)> {
    let path = crate::ProjectPaths::from_root(root).config_path;
    let config = if path.exists() {
        crate::load_canonical_config(path)?
    } else {
        Default::default()
    };
    let policy = crate::o2::policy::from_config(&config);
    policy.validate()?;
    Ok((
        policy.memory.max_body_chars,
        crate::config::transaction::revision(&config)?,
    ))
}
fn export_in(
    db: &rusqlite::Connection,
    project: &str,
    request: &ExportRequest,
    max: u64,
    revision: &str,
) -> Result<Value> {
    if request.ids.is_empty() || request.ids.len() > wire::MAX_RECORDS {
        return Err(error("Select 1–100 explicit memory IDs"));
    }
    let mut ids = request.ids.clone();
    ids.sort();
    ids.dedup();
    if ids.len() != request.ids.len() || ids.iter().any(|id| id.is_empty() || id.len() > 256) {
        return Err(error("Duplicate or invalid memory IDs in export"));
    }
    let mut rows = Vec::new();
    for id in ids {
        let m = load(db, &id, project)?;
        if !in_scope(&m.proposal, &request.access) {
            return Err(error("Export memory outside requested scope"));
        }
        let entry = Entry {
            schema_version: 1,
            checksum: hit(m.clone(), 0, "")?.checksum,
            memory: m,
        };
        wire::check(&entry, max)?;
        rows.push(entry);
    }
    let document = wire::encode(&rows, request.format)?;
    let checksum = digest(
        &serde_json::to_vec(&(project, revision, request, &document))
            .map_err(|e| error(e.to_string()))?,
    );
    Ok(
        json!({"checksum":checksum,"count":rows.len(),"document":document,"format":request.format,"notice":"Explicit scoped export; source status is historical, not current evidence certification; imported facts require local review"}),
    )
}
pub fn export_preview(root: &Path, request: &ExportRequest) -> Result<Value> {
    let (max, revision) = settings(root)?;
    let (mut db, a) = crate::o2::readonly_database(root)?;
    let tx = sql(db.transaction())?;
    export_in(&tx, &a.project_id, request, max, &revision)
}
pub fn export(
    root: &Path,
    request: &ExportRequest,
    expected: &str,
    confirmed: bool,
) -> Result<Value> {
    if !confirmed {
        return Err(error("Memory export requires explicit confirmation"));
    }
    let _guard = store::lock(root)?;
    let (max, revision) = settings(root)?;
    let (mut db, a) = database(root)?;
    let tx = sql(db.transaction())?;
    let plan = export_in(&tx, &a.project_id, request, max, &revision)?;
    if plan["checksum"].as_str() != Some(expected) {
        return Err(error("Export changed; preview again"));
    }
    crate::o2::schema::audit(
        &tx,
        "memory_exported",
        expected,
        json!({"ids":request.ids,"count":plan["count"],"format":request.format}),
    )?;
    sql(tx.commit())?;
    Ok(plan)
}
fn imported_id(project: &str, row: &Entry) -> String {
    format!(
        "import-{}",
        digest(
            format!(
                "{project}:{}:{}:{}",
                row.memory.project_id, row.memory.id, row.checksum
            )
            .as_bytes()
        )
    )
}
fn import_in(
    db: &rusqlite::Connection,
    project: &str,
    request: &ImportRequest,
    max: u64,
    revision: &str,
) -> Result<(Value, Vec<Entry>)> {
    let rows = wire::decode(&request.document, request.format, max)?;
    let mut seen = std::collections::BTreeSet::new();
    let mut preview = Vec::new();
    for row in &rows {
        if !in_scope(&row.memory.proposal, &request.access) {
            return Err(error(
                "Import scope requires matching explicit access; no scope expansion",
            ));
        }
        let id = imported_id(project, row);
        if !seen.insert(id.clone()) {
            return Err(error("Duplicate memory in import"));
        }
        let existing: Option<String> = sql(db
            .query_row(
                "SELECT checksum FROM memories WHERE id=?1 AND project_id=?2",
                params![id, project],
                |r| r.get(0),
            )
            .optional())?;
        preview.push(json!({"id":id,"source_project":row.memory.project_id,"source_id":row.memory.id,"source_status":row.memory.status,"source_checksum":row.checksum,"existing_checksum":existing,"status":"proposed","title":row.memory.proposal.title,"scope":row.memory.proposal.scope,"scope_ref":row.memory.proposal.scope_ref,"proposal":row.memory.proposal}));
    }
    let checksum = digest(
        &serde_json::to_vec(&(project, revision, request, &preview))
            .map_err(|e| error(e.to_string()))?,
    );
    Ok((
        json!({"checksum":checksum,"records":preview,"count":rows.len(),"notice":"New proposals only; existing local reviews are preserved; source evidence needs local revalidation"}),
        rows,
    ))
}
pub fn import_preview(root: &Path, request: &ImportRequest) -> Result<Value> {
    let (max, revision) = settings(root)?;
    let (mut db, a) = crate::o2::readonly_database(root)?;
    let tx = sql(db.transaction())?;
    Ok(import_in(&tx, &a.project_id, request, max, &revision)?.0)
}
pub fn import(
    root: &Path,
    request: &ImportRequest,
    expected: &str,
    confirmed: bool,
) -> Result<Value> {
    if !confirmed {
        return Err(error("Memory import requires explicit confirmation"));
    }
    let _guard = store::lock(root)?;
    let (max, revision) = settings(root)?;
    if !crate::o2::policy::load(root)?.memory.enabled {
        return Err(error("Enable Memory Ledger before importing"));
    }
    let (mut db, a) = database(root)?;
    let tx = sql(db.transaction())?;
    let (plan, rows) = import_in(&tx, &a.project_id, request, max, &revision)?;
    if plan["checksum"].as_str() != Some(expected) {
        return Err(error("Import changed; preview again"));
    }
    let mut ids = Vec::new();
    for row in rows {
        let id = imported_id(&a.project_id, &row);
        let exists: bool = sql(tx.query_row(
            "SELECT EXISTS(SELECT 1 FROM memories WHERE id=?1)",
            [&id],
            |r| r.get(0),
        ))?;
        if !exists {
            propose_in(&tx, &a.project_id, &row.memory.proposal, &id, max)?;
            crate::o2::schema::audit(
                &tx,
                "memory_imported",
                &id,
                json!({"source_project":row.memory.project_id,"source_id":row.memory.id,"source_checksum":row.checksum,"source_status":row.memory.status,"plan_checksum":expected}),
            )?;
        }
        ids.push(id);
    }
    sql(tx.commit())?;
    Ok(
        json!({"ids":ids,"count":ids.len(),"status":"proposed","notice":"Existing local decisions unchanged; review each new proposal before activation"}),
    )
}
