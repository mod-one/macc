//! Explicit signed transport. Imports require an authenticated publisher,
//! stopped execution and a strict history prefix; divergent histories are never
//! resolved by last writer wins. Git Notes are transport, not approver identity.
use super::{store::*, *};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::{
    collections::BTreeMap,
    io::Write,
    path::Path,
    process::{Command, Stdio},
};
pub(super) const TABLES: &[&str] = &[
    "metadata",
    "plans",
    "events",
    "requests",
    "contexts",
    "outbox",
    "operations",
    "legacy_approvals",
    "authorizations",
];
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Archive {
    pub schema_version: u32,
    pub project_id: String,
    pub repository_roots: Vec<String>,
    pub tables: BTreeMap<String, Vec<Vec<Value>>>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SignedArchive {
    pub archive: Archive,
    pub principal: String,
    pub signature: String,
}
pub(super) fn roots(root: &Path) -> crate::Result<Vec<String>> {
    let output = crate::git::run_git_output_mapped(
        root,
        &["rev-list", "--max-parents=0", "HEAD"],
        "bind signed decision transport",
    )?;
    if !output.status.success() {
        return Err(error("Signed transport requires a committed repository"));
    }
    let mut values = String::from_utf8_lossy(&output.stdout)
        .lines()
        .map(str::to_owned)
        .collect::<Vec<_>>();
    values.sort();
    Ok(values)
}
impl Store {
    pub fn archive(&self) -> crate::Result<Archive> {
        let _lock = self.mutation_lock()?;
        self.verify()?;
        let snapshot = sql(self.db.unchecked_transaction())?;
        let mut tables = BTreeMap::new();
        for table in TABLES {
            let mut statement = sql(self
                .db
                .prepare(&format!("SELECT * FROM {table} ORDER BY rowid")))?;
            let columns = statement.column_count();
            let rows = sql(statement.query_map([], |r| {
                (0..columns)
                    .map(|i| match r.get_ref(i)? {
                        rusqlite::types::ValueRef::Null => Ok(Value::Null),
                        rusqlite::types::ValueRef::Integer(n) => Ok(json!(n)),
                        rusqlite::types::ValueRef::Text(s) => Ok(json!(String::from_utf8_lossy(s))),
                        _ => Err(rusqlite::Error::InvalidQuery),
                    })
                    .collect::<rusqlite::Result<Vec<_>>>()
            }))?;
            tables.insert(
                (*table).into(),
                rows.map(|r| sql(r)).collect::<crate::Result<Vec<_>>>()?,
            );
        }
        sql(snapshot.commit())?;
        let archive = Archive {
            schema_version: 1,
            project_id: self.project_id.clone(),
            repository_roots: roots(&self.root)?,
            tables,
        };
        let bytes = super::secure::canonical_bytes(&archive)?;
        if bytes.len() > 4 * 1024 * 1024 {
            return Err(error("Signed archive exceeds 4 MiB; export scoped evidence or configure retention before transport"));
        }
        if crate::security::scan_bytes("decision-archive.json", &bytes)
            .iter()
            .any(|f| f.severity == crate::security::Severity::Error)
        {
            return Err(error("Decision archive contains potential secrets; redact source evidence before publishing"));
        }
        Ok(archive)
    }
    pub fn sign_archive(&self, key: &Path, principal: &str) -> crate::Result<SignedArchive> {
        crate::session_store::require_stopped(&self.root)?;
        let archive = self.archive()?;
        let bytes = super::secure::canonical_bytes(&archive)?;
        let file = self
            .path
            .parent()
            .unwrap()
            .join(format!("transport-{}.json", uuid::Uuid::new_v4()));
        let mut options = std::fs::OpenOptions::new();
        options.create_new(true).write(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        let mut output = options.open(&file).map_err(|e| error(e.to_string()))?;
        output.write_all(&bytes).map_err(|e| error(e.to_string()))?;
        // Only an explicitly invoked operator command can ask the external SSH signer.
        let status = Command::new("/usr/bin/ssh-keygen")
            .args(["-Y", "sign", "-n", "macc-decisions", "-f"])
            .arg(key)
            .arg(&file)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status();
        let signature_file = file.with_extension("json.sig");
        let result = match status {
            Ok(s) if s.success() => {
                std::fs::read_to_string(&signature_file).map_err(|e| error(e.to_string()))
            }
            _ => Err(error(
                "External SSH signing failed; no archive was published",
            )),
        };
        let _ = std::fs::remove_file(file);
        let _ = std::fs::remove_file(signature_file);
        Ok(SignedArchive {
            archive,
            principal: principal.into(),
            signature: result?,
        })
    }
    pub fn import_archive(
        &mut self,
        bundle: &SignedArchive,
        allowed_signers: &Path,
        enroll_project: bool,
    ) -> crate::Result<()> {
        crate::session_store::require_stopped(&self.root)?;
        let _controller = crate::fs_lock::AdvisoryLock::acquire(
            &self.path.with_file_name("controller.lock"),
            std::time::Duration::ZERO,
            "signed decision import",
        )?;
        let _lock = self.mutation_lock()?;
        super::secure::transport_authority(
            &policy(&self.root)?,
            allowed_signers,
            &bundle.principal,
        )?;
        let bytes = super::secure::canonical_bytes(&bundle.archive)?;
        super::secure::verify_signature(
            &self.root,
            allowed_signers,
            &bundle.principal,
            "macc-decisions",
            &bytes,
            &bundle.signature,
        )?;
        if bundle.archive.schema_version != 1
            || bundle.archive.repository_roots != roots(&self.root)?
            || bundle.archive.tables.len() != TABLES.len()
            || TABLES
                .iter()
                .any(|t| !bundle.archive.tables.contains_key(*t))
        {
            return Err(error("Signed archive schema/repository/table set mismatch"));
        }
        let tx = sql(self.db.unchecked_transaction())?;
        let count: i64 = sql(tx.query_row("SELECT count(*) FROM events", [], |r| r.get(0)))?;
        if self.project_id != bundle.archive.project_id && (!enroll_project || count != 0) {
            return Err(error("Foreign project identity: enrollment is allowed only into an empty stopped authority"));
        }
        let incoming = &bundle.archive.tables["events"];
        if count as usize > incoming.len() {
            return Err(error(
                "Incoming history is older; current authority was retained",
            ));
        }
        if count > 0 {
            let hash: String = sql(tx.query_row(
                "SELECT hash FROM events ORDER BY seq LIMIT 1 OFFSET ?1",
                [count - 1],
                |r| r.get(0),
            ))?;
            if incoming[count as usize - 1].last().and_then(Value::as_str) != Some(hash.as_str()) {
                return Err(error("Signed histories diverge; reconcile decisions explicitly instead of replacing local authority"));
            }
        }
        // No mutation is visible until the entire imported projection verifies.
        for table in TABLES.iter().rev() {
            sql(tx.execute(&format!("DELETE FROM {table}"), []))?;
        }
        for table in TABLES {
            for row in &bundle.archive.tables[*table] {
                let placeholders = (1..=row.len())
                    .map(|i| format!("?{i}"))
                    .collect::<Vec<_>>()
                    .join(",");
                let values = row
                    .iter()
                    .map(|v| match v {
                        Value::Null => Ok(rusqlite::types::Value::Null),
                        Value::String(s) => Ok(rusqlite::types::Value::Text(s.clone())),
                        Value::Number(n) if n.as_i64().is_some() => {
                            Ok(rusqlite::types::Value::Integer(n.as_i64().unwrap()))
                        }
                        _ => Err(error("Unsupported archive cell")),
                    })
                    .collect::<crate::Result<Vec<_>>>()?;
                sql(tx.execute(
                    &format!("INSERT INTO {table} VALUES({placeholders})"),
                    rusqlite::params_from_iter(values),
                ))?;
            }
        }
        // Verify on the same connection without nesting Store::verify's transaction.
        let imported_id: String = sql(tx.query_row(
            "SELECT value FROM metadata WHERE key='project_id'",
            [],
            |r| r.get(0),
        ))?;
        if imported_id != bundle.archive.project_id {
            return Err(error(
                "Archive project identity disagrees with its authority",
            ));
        }
        self.verify_projection(&imported_id)?;
        // The receiving machine must materialize its own PRD projections.
        for plan in self.plans()? {
            sql(tx.execute(
                "INSERT INTO outbox(plan_id,kind,payload) VALUES(?1,'graph',?2)",
                rusqlite::params![plan.id, encode(&plan)?],
            ))?;
        }
        sql(tx.commit())?;
        self.project_id = imported_id;
        drop(_lock);
        self.verify()?;
        self.recover_operations()?;
        self.project(&self.root.clone())?;
        Ok(())
    }
    pub fn publish_note(&self, bundle: &SignedArchive) -> crate::Result<String> {
        let bytes = serde_json::to_vec(bundle).map_err(|e| error(e.to_string()))?;
        if bytes.len() > 4 * 1024 * 1024 {
            return Err(error("Signed note exceeds budget"));
        }
        let mut child = Command::new("git")
            .current_dir(&self.root)
            .args(["hash-object", "-w", "--stdin"])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .spawn()
            .map_err(|e| error(e.to_string()))?;
        child
            .stdin
            .take()
            .unwrap()
            .write_all(&bytes)
            .map_err(|e| error(e.to_string()))?;
        let output = child.wait_with_output().map_err(|e| error(e.to_string()))?;
        if !output.status.success() {
            return Err(error("Failed to create signed transport object"));
        }
        let oid = String::from_utf8_lossy(&output.stdout).trim().to_owned();
        let existing = Command::new("git")
            .current_dir(&self.root)
            .args(["notes", "--ref=refs/notes/macc-decisions", "show", &oid])
            .output()
            .map_err(|e| error(e.to_string()))?;
        if existing.status.success() {
            if existing
                .stdout
                .strip_suffix(b"\n")
                .unwrap_or(&existing.stdout)
                == bytes
            {
                return Ok(oid);
            }
            return Err(error(
                "Signed note object already contains different evidence",
            ));
        }
        let mut child = Command::new("git")
            .current_dir(&self.root)
            .args([
                "notes",
                "--ref=refs/notes/macc-decisions",
                "add",
                "-F",
                "-",
                &oid,
            ])
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .spawn()
            .map_err(|e| error(e.to_string()))?;
        child
            .stdin
            .take()
            .unwrap()
            .write_all(&bytes)
            .map_err(|e| error(e.to_string()))?;
        if !child.wait().map_err(|e| error(e.to_string()))?.success() {
            return Err(error(
                "Git Notes publication failed; existing notes were preserved",
            ));
        }
        Ok(oid)
    }
    pub fn read_note(&self, object: &str) -> crate::Result<SignedArchive> {
        if !object.chars().all(|c| c.is_ascii_hexdigit()) || !(40..=64).contains(&object.len()) {
            return Err(error("Explicit full transport object ID required"));
        }
        let output = crate::git::run_git_output_mapped(
            &self.root,
            &["notes", "--ref=refs/notes/macc-decisions", "show", object],
            "read signed decision note",
        )?;
        if !output.status.success() || output.stdout.len() > 4 * 1024 * 1024 {
            return Err(error("Missing or oversized signed decision note"));
        }
        serde_json::from_slice(&output.stdout).map_err(|e| error(e.to_string()))
    }
}
