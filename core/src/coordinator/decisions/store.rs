use super::*;
use rusqlite::{Connection, OptionalExtension, Transaction, TransactionBehavior};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::{
    path::{Path, PathBuf},
    time::Duration,
};

pub fn digest(value: &impl serde::Serialize) -> String {
    // serde_json::Map uses stable key order (preserve_order is not enabled).
    let value = serde_json::to_value(value).expect("serializable decision value");
    format!(
        "sha256:{:x}",
        Sha256::digest(serde_json::to_vec(&value).unwrap())
    )
}
pub(super) fn sql<T>(v: rusqlite::Result<T>) -> crate::Result<T> {
    v.map_err(|e| error(format!("Control store: {e}")))
}
pub(super) fn encode(v: &impl serde::Serialize) -> crate::Result<String> {
    serde_json::to_string(v).map_err(|e| error(e.to_string()))
}
pub(super) fn decode<T: serde::de::DeserializeOwned>(v: &str) -> crate::Result<T> {
    serde_json::from_str(v).map_err(|e| error(format!("Invalid durable decision data: {e}")))
}

pub struct Store {
    pub(super) root: PathBuf,
    pub path: PathBuf,
    pub project_id: String,
    pub(super) db: Connection,
}
impl Store {
    pub(super) fn mutation_lock(&self) -> crate::Result<crate::fs_lock::AdvisoryLock> {
        crate::fs_lock::AdvisoryLock::acquire(
            &self.path.with_file_name("mutation.lock"),
            Duration::from_secs(5),
            "decision mutation/publication",
        )
    }
    pub fn location(root: &Path) -> crate::Result<PathBuf> {
        let out = crate::git::run_git_output_mapped(
            root,
            &["rev-parse", "--git-common-dir"],
            "resolve decision authority",
        )?;
        if !out.status.success() {
            return Err(error("Decision authority requires a Git repository"));
        }
        let path = PathBuf::from(String::from_utf8_lossy(&out.stdout).trim());
        let common = if path.is_absolute() {
            path
        } else {
            root.join(path)
        };
        let common = common.canonicalize().map_err(|e| error(e.to_string()))?;
        Ok(common.join("macc/control.sqlite"))
    }
    pub fn open(root: &Path) -> crate::Result<Self> {
        let path = Self::location(root)?;
        if std::fs::symlink_metadata(path.parent().unwrap())
            .is_ok_and(|m| m.file_type().is_symlink())
        {
            return Err(error("Control directory cannot be a symlink"));
        }
        std::fs::create_dir_all(path.parent().unwrap()).map_err(|e| error(e.to_string()))?;
        if std::fs::symlink_metadata(&path).is_ok_and(|m| m.file_type().is_symlink()) {
            return Err(error("Control DB cannot be a symlink"));
        }
        let mut db = sql(Connection::open(&path))?;
        sql(db.busy_timeout(Duration::from_secs(5)))?;
        sql(db.execute_batch(
            "PRAGMA foreign_keys=ON; PRAGMA journal_mode=WAL; PRAGMA synchronous=FULL;",
        ))?;
        let version: i64 = sql(db.query_row("PRAGMA user_version", [], |r| r.get(0)))?;
        if version > 1 {
            return Err(error(
                "Control schema is newer than this MACC; no mutations permitted",
            ));
        }
        let tx = sql(db.transaction_with_behavior(TransactionBehavior::Immediate))?;
        sql(tx.execute_batch("CREATE TABLE IF NOT EXISTS metadata(key TEXT PRIMARY KEY,value TEXT NOT NULL);
          CREATE TABLE IF NOT EXISTS plans(id TEXT PRIMARY KEY,scope_key TEXT UNIQUE NOT NULL,payload TEXT NOT NULL);
          CREATE TABLE IF NOT EXISTS events(seq INTEGER PRIMARY KEY AUTOINCREMENT,plan_id TEXT NOT NULL,kind TEXT NOT NULL,payload TEXT NOT NULL,previous_hash TEXT NOT NULL,hash TEXT NOT NULL);
          CREATE TABLE IF NOT EXISTS requests(id TEXT PRIMARY KEY,plan_id TEXT NOT NULL,dedupe TEXT NOT NULL,payload TEXT NOT NULL,UNIQUE(plan_id,dedupe));
          CREATE TABLE IF NOT EXISTS contexts(plan_id TEXT NOT NULL,attempt_id TEXT NOT NULL,phase TEXT NOT NULL,payload TEXT NOT NULL,receipt TEXT,PRIMARY KEY(plan_id,attempt_id,phase));
          CREATE TABLE IF NOT EXISTS outbox(id INTEGER PRIMARY KEY AUTOINCREMENT,plan_id TEXT NOT NULL,kind TEXT NOT NULL,payload TEXT NOT NULL,applied INTEGER NOT NULL DEFAULT 0);
          CREATE TABLE IF NOT EXISTS operations(plan_id TEXT NOT NULL,key TEXT NOT NULL,payload TEXT NOT NULL,PRIMARY KEY(plan_id,key));
          CREATE TABLE IF NOT EXISTS legacy_approvals(plan_id TEXT NOT NULL,source_key TEXT NOT NULL,payload TEXT NOT NULL,PRIMARY KEY(plan_id,source_key));
          CREATE TABLE IF NOT EXISTS authorizations(plan_id TEXT NOT NULL,operation_id TEXT NOT NULL,decision_id TEXT NOT NULL,consumed_at TEXT NOT NULL,PRIMARY KEY(plan_id,operation_id));
          PRAGMA user_version=1;"))?;
        let id: Option<String> = sql(tx
            .query_row(
                "SELECT value FROM metadata WHERE key='project_id'",
                [],
                |r| r.get(0),
            )
            .optional())?;
        let project_id = id.unwrap_or_else(|| uuid::Uuid::new_v4().to_string());
        sql(tx.execute(
            "INSERT OR IGNORE INTO metadata VALUES('project_id',?1)",
            [&project_id],
        ))?;
        sql(tx.commit())?;
        let store = Self {
            root: root.to_path_buf(),
            path,
            project_id,
            db,
        };
        store.verify()?;
        Ok(store)
    }
    pub fn verify(&self) -> crate::Result<()> {
        let snapshot = sql(self.db.unchecked_transaction())?;
        let check: String = sql(self.db.query_row("PRAGMA quick_check", [], |r| r.get(0)))?;
        if check != "ok" {
            return Err(error(format!("Control integrity failed: {check}")));
        }
        let mut last = String::new();
        let mut graphs = std::collections::BTreeMap::<String, (u64, Value)>::new();
        let mut decisions = std::collections::BTreeMap::<String, Decision>::new();
        let mut approvals = std::collections::BTreeMap::<(String, String), usize>::new();
        let mut receipts = std::collections::BTreeMap::<(String, String, String), Value>::new();
        let mut stmt = sql(self
            .db
            .prepare("SELECT plan_id,kind,payload,previous_hash,hash FROM events ORDER BY seq"))?;
        let rows = sql(stmt.query_map([], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, String>(2)?,
                r.get::<_, String>(3)?,
                r.get::<_, String>(4)?,
            ))
        }))?;
        for row in rows {
            let (plan, kind, payload, prev, hash) = sql(row)?;
            if prev != last || hash != digest(&json!([plan, kind, payload, prev])) {
                return Err(error(
                    "Control event journal is corrupt; reconcile before dispatch",
                ));
            }
            let value: Value = decode(&payload)?;
            match kind.as_str() {
                "plan.imported" => {
                    let p: Plan = decode(&payload)?;
                    if p.project_id != self.project_id {
                        return Err(error("Plan belongs to a different project"));
                    }
                    graphs.insert(plan.clone(), (p.graph_version, p.graph));
                }
                "graph.patch.committed" => {
                    graphs.insert(
                        plan.clone(),
                        (
                            value["version"]
                                .as_u64()
                                .ok_or_else(|| error("Missing graph version"))?,
                            value["graph"].clone(),
                        ),
                    );
                }
                "decision.requested" | "decision.resolved" => {
                    let d: Decision = decode(&payload)?;
                    decisions.insert(d.id.clone(), d);
                }
                "approval.recorded" | "approval.legacy_imported" => {
                    *approvals.entry((plan.clone(), digest(&value))).or_default() += 1;
                }
                "task.context.received" => {
                    receipts.insert(
                        (
                            plan.clone(),
                            value["attempt"].as_str().unwrap_or_default().into(),
                            value["phase"].as_str().unwrap_or_default().into(),
                        ),
                        value["receipt"].clone(),
                    );
                }
                "decision.revoked" | "decision.superseded" => {
                    let d: Decision = serde_json::from_value(value["decision"].clone())
                        .map_err(|e| error(e.to_string()))?;
                    decisions.insert(d.id.clone(), d);
                }
                _ => {}
            }
            last = hash;
        }
        let plans = self.plans()?;
        if plans.len() != graphs.len() {
            return Err(error(
                "Plan projection does not match the authoritative journal",
            ));
        }
        for plan in plans {
            if graphs.get(&plan.id) != Some(&(plan.graph_version, plan.graph.clone()))
                || digest(&plan.graph) != plan.source_digest
            {
                return Err(error(
                    "Graph projection disagrees with the authoritative journal",
                ));
            }
            for d in self.decisions(&plan.id)? {
                if decisions.remove(&d.id) != Some(d) {
                    return Err(error(
                        "Decision projection disagrees with the authoritative journal",
                    ));
                }
            }
        }
        if !decisions.is_empty() {
            return Err(error("Decision projection is incomplete"));
        }
        let mut statement = sql(self
            .db
            .prepare("SELECT plan_id,payload FROM legacy_approvals"))?;
        for row in
            sql(statement.query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?))))?
        {
            let (plan, payload) = sql(row)?;
            let value: Value = decode(&payload)?;
            let key = (plan, digest(&value));
            let count = approvals
                .get_mut(&key)
                .ok_or_else(|| error("Approval projection has no authoritative event"))?;
            if *count == 0 {
                return Err(error("Approval projection duplicates evidence"));
            }
            *count -= 1;
        }
        if approvals.values().any(|count| *count != 0) {
            return Err(error("Approval projection is incomplete"));
        }
        let mut statement = sql(self.db.prepare(
            "SELECT plan_id,attempt_id,phase,receipt FROM contexts WHERE receipt IS NOT NULL",
        ))?;
        for row in sql(statement.query_map([], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, String>(2)?,
                r.get::<_, String>(3)?,
            ))
        }))? {
            let (plan, attempt, phase, receipt) = sql(row)?;
            let receipt: Value = decode(&receipt)?;
            if receipt["acknowledged"] == true
                && receipts.get(&(plan, attempt, phase)) != Some(&receipt)
            {
                return Err(error(
                    "Acknowledged context receipt disagrees with its journal",
                ));
            }
        }
        sql(snapshot.commit())?;
        Ok(())
    }
    pub fn plans(&self) -> crate::Result<Vec<Plan>> {
        let mut stmt = sql(self.db.prepare("SELECT payload FROM plans ORDER BY id"))?;
        let rows = sql(stmt.query_map([], |r| r.get::<_, String>(0)))?;
        rows.map(|r| decode(&sql(r)?)).collect()
    }
    pub fn plan(&self, id: &str) -> crate::Result<Plan> {
        let text: String =
            sql(self
                .db
                .query_row("SELECT payload FROM plans WHERE id=?1", [id], |r| r.get(0)))?;
        decode(&text)
    }
    pub fn decisions(&self, plan_id: &str) -> crate::Result<Vec<Decision>> {
        let mut stmt = sql(self
            .db
            .prepare("SELECT payload FROM requests WHERE plan_id=?1 ORDER BY rowid"))?;
        let rows = sql(stmt.query_map([plan_id], |r| r.get::<_, String>(0)))?;
        rows.map(|r| decode(&sql(r)?)).collect()
    }
    /// Explicit project scope crosses PRD plans on the same reference branch;
    /// task/path/requirement scope never leaks into a colliding task ID.
    pub fn applicable_decisions(&self, plan: &Plan) -> crate::Result<Vec<Decision>> {
        let scopes = self
            .plans()?
            .into_iter()
            .map(|p| (p.id.clone(), p))
            .collect::<std::collections::BTreeMap<_, _>>();
        let mut statement = sql(self
            .db
            .prepare("SELECT payload FROM requests ORDER BY rowid"))?;
        let rows = sql(statement.query_map([], |r| r.get::<_, String>(0)))?;
        let mut out = Vec::new();
        for row in rows {
            let d: Decision = decode(&sql(row)?)?;
            let same_project = scopes.get(&d.plan_id).is_some_and(|p| {
                p.project_id == plan.project_id && p.reference_branch == plan.reference_branch
            });
            if d.plan_id == plan.id || (same_project && d.request.scope == "project") {
                out.push(d);
            }
        }
        Ok(out)
    }
    pub fn decision_in_scope(&self, plan: &Plan, id: &str) -> crate::Result<Decision> {
        self.applicable_decisions(plan)?
            .into_iter()
            .find(|d| d.id == id)
            .ok_or_else(|| error("Decision belongs to a different plan/branch scope"))
    }
    pub fn decision(&self, id: &str) -> crate::Result<Decision> {
        decode(&sql(self.db.query_row(
            "SELECT payload FROM requests WHERE id=?1",
            [id],
            |r| r.get::<_, String>(0),
        ))?)
    }
    pub fn scoped_history(&self, plan: &Plan) -> crate::Result<Vec<Value>> {
        let shared = self
            .applicable_decisions(plan)?
            .into_iter()
            .filter(|d| d.request.scope == "project")
            .map(|d| d.id)
            .collect::<std::collections::BTreeSet<_>>();
        let mut stmt = sql(self
            .db
            .prepare("SELECT seq,plan_id,kind,payload,hash FROM events ORDER BY seq"))?;
        let rows = sql(stmt.query_map([], |r| {
            Ok((
                r.get::<_, i64>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, String>(2)?,
                r.get::<_, String>(3)?,
                r.get::<_, String>(4)?,
            ))
        }))?;
        let mut events = Vec::new();
        for row in rows {
            let (seq, scope, kind, payload, hash) = sql(row)?;
            let payload: Value = decode(&payload)?;
            let id = payload["id"]
                .as_str()
                .or_else(|| payload.pointer("/decision/id").and_then(Value::as_str));
            if scope == plan.id || scope == "project" || id.is_some_and(|id| shared.contains(id)) {
                events.push(
                    json!({"seq":seq,"plan_id":scope,"kind":kind,"payload":payload,"hash":hash}),
                );
            }
        }
        Ok(events)
    }
    pub fn events(&self, plan: &str) -> crate::Result<Vec<Value>> {
        let mut stmt = sql(self
            .db
            .prepare("SELECT seq,kind,payload,hash FROM events WHERE plan_id=?1 ORDER BY seq"))?;
        let rows = sql(stmt.query_map([plan], |r| {
            Ok((
                r.get::<_, i64>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, String>(2)?,
                r.get::<_, String>(3)?,
            ))
        }))?;
        rows.map(|r| {
            let (seq, kind, payload, hash) = sql(r)?;
            Ok(json!({"seq":seq,"kind":kind,"payload":decode::<Value>(&payload)?,"hash":hash}))
        })
        .collect()
    }
}

pub(super) fn event(
    tx: &Transaction<'_>,
    plan: &str,
    kind: &str,
    payload: &impl serde::Serialize,
) -> crate::Result<()> {
    let payload = encode(payload)?;
    let previous: String = sql(tx
        .query_row(
            "SELECT hash FROM events ORDER BY seq DESC LIMIT 1",
            [],
            |r| r.get(0),
        )
        .optional())?
    .unwrap_or_default();
    let hash = digest(&json!([plan, kind, payload, previous]));
    sql(tx.execute(
        "INSERT INTO events(plan_id,kind,payload,previous_hash,hash) VALUES(?1,?2,?3,?4,?5)",
        rusqlite::params![plan, kind, payload, previous, hash],
    ))?;
    Ok(())
}
pub(super) fn save_plan(tx: &Transaction<'_>, plan: &Plan) -> crate::Result<()> {
    sql(tx.execute(
        "UPDATE plans SET payload=?2 WHERE id=?1",
        rusqlite::params![plan.id, encode(plan)?],
    ))?;
    Ok(())
}
