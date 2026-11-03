//! Typed user interventions, evidence and conditional approvals.
use super::{store::*, *};
use rusqlite::OptionalExtension;
use serde_json::json;
use std::{collections::BTreeSet, io::Read, path::Path};

impl Store {
    pub fn add_artifact(
        &mut self,
        plan_id: &str,
        label: &str,
        media_type: &str,
        data_hex: &str,
        actor: &str,
    ) -> crate::Result<EvidenceArtifact> {
        let _lock = self.mutation_lock()?;
        self.plan(plan_id)?;
        let p = policy(&self.root)?;
        if actor.trim().is_empty()
            || label.trim().is_empty()
            || label.len() > 256
            || media_type.len() > 128
            || !media_type.contains('/')
            || media_type.chars().any(char::is_control)
        {
            return Err(error(
                "Evidence requires attributed content, a readable label and media type",
            ));
        }
        let bytes = decode_hex(data_hex, p.max_request_bytes)?;
        if bytes.is_empty() {
            return Err(error("Empty evidence is not a result"));
        }
        if crate::security::scan_bytes(label, &bytes)
            .iter()
            .any(|f| f.severity == crate::security::Severity::Error)
        {
            return Err(error(
                "Evidence appears to contain credentials; supply a redacted result instead",
            ));
        }
        let hash = evidence_digest(&bytes);
        let id = digest(&json!([plan_id, hash, label, media_type]));
        let key = format!("evidence:{id}");
        if let Some(old) = sql(self
            .db
            .query_row("SELECT value FROM metadata WHERE key=?1", [&key], |r| {
                r.get::<_, String>(0)
            })
            .optional())?
        {
            return decode(&old);
        }
        let artifact = EvidenceArtifact {
            id,
            plan_id: plan_id.into(),
            label: label.into(),
            media_type: media_type.into(),
            digest: hash,
            size: bytes.len(),
            data_hex: data_hex.to_ascii_lowercase(),
            actor: actor.into(),
            recorded_at: now(),
        };
        let tx = sql(self
            .db
            .transaction_with_behavior(rusqlite::TransactionBehavior::Immediate))?;
        sql(tx.execute(
            "INSERT INTO metadata VALUES(?1,?2)",
            rusqlite::params![key, encode(&artifact)?],
        ))?;
        event(&tx, plan_id, "evidence.artifact", &artifact)?;
        sql(tx.commit())?;
        Ok(artifact)
    }
    pub fn artifacts(&self, plan_id: &str) -> crate::Result<Vec<serde_json::Value>> {
        let mut stmt = sql(self.db.prepare(
            "SELECT value FROM metadata WHERE key LIKE 'evidence:%' ORDER BY rowid DESC LIMIT 256",
        ))?;
        let mut out = Vec::new();
        for row in sql(stmt.query_map([], |r| r.get::<_, String>(0)))? {
            let a: EvidenceArtifact = decode(&sql(row)?)?;
            if a.plan_id == plan_id {
                out.push(json!({"id":a.id,"digest":a.digest,"label":a.label,"size":a.size,"media_type":a.media_type,"recorded_at":a.recorded_at,"share_content":self.evidence_deliverable(&a.id)?,"permission_version":self.evidence_permission(&a.id)?.map(|p|p.version).unwrap_or(0)}));
            }
        }
        Ok(out)
    }
    pub fn artifact(&self, id: &str) -> crate::Result<EvidenceArtifact> {
        let raw: String = sql(self.db.query_row(
            "SELECT value FROM metadata WHERE key=?1",
            [format!("evidence:{id}")],
            |r| r.get(0),
        ))?;
        let artifact: EvidenceArtifact = decode(&raw)?;
        let bytes = decode_hex(&artifact.data_hex, 262144)?;
        if artifact.id != id
            || artifact.size != bytes.len()
            || artifact.digest != evidence_digest(&bytes)
        {
            return Err(error("Evidence content/identity checksum mismatch"));
        }
        let mut stmt = sql(self
            .db
            .prepare("SELECT payload FROM events WHERE plan_id=?1 AND kind='evidence.artifact'"))?;
        let mut verified = false;
        for row in sql(stmt.query_map([&artifact.plan_id], |r| r.get::<_, String>(0)))? {
            if sql(row)? == raw {
                verified = true;
                break;
            }
        }
        if !verified {
            return Err(error("Evidence has no matching immutable journal record"));
        }
        Ok(artifact)
    }
    pub(super) fn verify_artifacts(
        &self,
        plan_id: &str,
        refs: &[ArtifactRef],
    ) -> crate::Result<()> {
        if refs.len() > 16 {
            return Err(error("At most 16 evidence artifacts may be attached"));
        }
        let mut ids = BTreeSet::new();
        for reference in refs {
            let a = self.artifact(&reference.id)?;
            if a.plan_id != plan_id || a.digest != reference.digest || !ids.insert(&reference.id) {
                return Err(error(
                    "Evidence is stale, duplicated or belongs to another plan",
                ));
            }
        }
        Ok(())
    }
    pub(super) fn validate_conditions(
        &self,
        d: &Decision,
        conditions: &[ApprovalCondition],
    ) -> crate::Result<()> {
        if conditions.len() > 16 {
            return Err(error(
                "At most 16 explicit approval conditions are supported",
            ));
        }
        let plan = self.plan(&d.plan_id)?;
        let mut ids = BTreeSet::new();
        for c in conditions {
            if c.id.trim().is_empty()
                || !ids.insert(&c.id)
                || c.description.trim().is_empty()
                || c.description.len() > 4096
                || c.task_id.is_none() && c.source_path.is_none()
            {
                return Err(error("A condition needs a unique ID, readable requirement and a checked task or exact source artifact"));
            }
            if let Some(task) = &c.task_id {
                if task == &d.request.task_id || self.decision_applies(&plan, d, task)? {
                    return Err(error("A condition cannot wait for its blocked subtree; create an independent verification task first"));
                }
                super::graph::task(&plan.graph, task)?;
            }
            if c.source_path.is_some() != c.source_digest.is_some() {
                return Err(error(
                    "Source conditions require both a project-relative path and its SHA-256",
                ));
            }
            if let Some(path) = &c.source_path {
                super::graph::projection_path(&self.root, path)?;
                if !c.source_digest.as_ref().is_some_and(|s| {
                    s.len() == 71
                        && s.starts_with("sha256:")
                        && s[7..].chars().all(|c| c.is_ascii_hexdigit())
                }) {
                    return Err(error(
                        "Condition evidence must name an exact SHA-256 digest",
                    ));
                }
            }
        }
        Ok(())
    }
    pub(super) fn check_conditions(&self, d: &Decision, p: &Policy) -> crate::Result<()> {
        let Some(r) = &d.resolution else {
            return Ok(());
        };
        if r.conditions.is_empty() {
            return Ok(());
        }
        self.validate_conditions(d, &r.conditions)?;
        let registry = registry(&self.root)?;
        let plan = self.plan(&d.plan_id)?;
        let mut remaining = p.investigation.max_source_bytes;
        if d.request.kind == "approval"
            && d.request.subject_revision.as_ref()
                != Some(&self.approval_subject_revision(&plan, &d.request)?)
        {
            return Err(error(
                "Conditional approval subject changed; reviewed renewal required",
            ));
        }
        for c in &r.conditions {
            if let Some(id) = &c.task_id {
                let archived = plan.graph["tasks_done"]
                    .as_array()
                    .is_some_and(|tasks| tasks.iter().any(|t| t["id"] == *id));
                if !archived
                    && !self
                        .scoped_registry(&plan, &registry)
                        .find_task(id)
                        .is_some_and(|t| t.is_merged())
                {
                    return Err(error(format!(
                        "Condition {} awaits verified delivery of task {id}",
                        c.id
                    )));
                }
            }
            if let Some(path) = &c.source_path {
                let bytes = if d.request.kind == "approval" {
                    let object = format!(
                        "{}:{path}",
                        d.request.subject_revision.as_deref().unwrap_or_default()
                    );
                    let size = crate::git::run_git_output_mapped(
                        &self.root,
                        &["cat-file", "-s", &object],
                        "size conditional evidence",
                    )?;
                    if !size.status.success()
                        || String::from_utf8_lossy(&size.stdout)
                            .trim()
                            .parse::<usize>()
                            .ok()
                            .is_none_or(|n| n > remaining)
                    {
                        return Err(error(
                            "Conditional artifact is missing or exceeds the bounded source budget",
                        ));
                    }
                    let output = crate::git::run_git_output_mapped(
                        &self.root,
                        &["cat-file", "blob", &object],
                        "check conditional approval artifact",
                    )?;
                    if !output.status.success() {
                        return Err(error(format!(
                            "Condition {}: artifact absent from the reviewed commit",
                            c.id
                        )));
                    }
                    output.stdout
                } else {
                    let mut bytes = Vec::new();
                    std::fs::File::open(super::graph::projection_path(&self.root, path)?)
                        .map_err(|e| error(e.to_string()))?
                        .take(remaining as u64 + 1)
                        .read_to_end(&mut bytes)
                        .map_err(|e| error(e.to_string()))?;
                    bytes
                };
                if bytes.len() > remaining || Some(evidence_digest(&bytes)) != c.source_digest {
                    return Err(error(format!(
                        "Condition {}: evidence absent, changed or above the source budget",
                        c.id
                    )));
                }
                remaining -= bytes.len();
            }
        }
        Ok(())
    }
    pub fn refresh_conditions(&mut self, plan: &Plan, p: &Policy) -> crate::Result<usize> {
        let _lock = self.mutation_lock()?;
        let mut count = 0;
        for mut d in self
            .decisions(&plan.id)?
            .into_iter()
            .filter(|d| d.status == "conditional")
        {
            // Stale authority blocks this request, not independent work.
            if self.check_conditions(&d, p).is_err() || self.verify_human_binding(&d, p).is_err() {
                continue;
            }
            let r = d.resolution.as_ref().unwrap();
            d.status = if r.effects == "replan"
                || r.effects != "no_graph_change" && d.request.has_effects()
            {
                "awaiting_effects"
            } else {
                "resolved"
            }
            .into();
            d.version += 1;
            self.save_interaction(&d, "coordinator", "conditions_satisfied")?;
            count += 1;
        }
        Ok(count)
    }
    pub fn complete_investigation(
        &mut self,
        id: &str,
        version: u64,
        summary: &str,
        refs: &[ArtifactRef],
        attempt: &str,
        phase: &str,
    ) -> crate::Result<Decision> {
        let _lock = self.mutation_lock()?;
        let mut d = self.decision(id)?;
        let p = policy(&self.root)?;
        if d.version != version
            || d.status != "investigating_requested"
            || summary.trim().is_empty()
            || summary.len() > p.max_request_bytes / 2
        {
            return Err(error(
                "Investigation changed or report is empty/above budget",
            ));
        }
        let context = self.read_context(&d.plan_id, attempt, phase)?;
        let planning = context
            .planning
            .as_ref()
            .ok_or_else(|| error("A native investigation context is required"))?;
        if planning.purpose != "investigation"
            || context.task_id != d.request.task_id
            || !planning
                .requests
                .iter()
                .any(|r| r.id == id && r.version == version)
        {
            return Err(error("Report is outside its issued investigation scope"));
        }
        if self
            .context(
                &self.plan(&d.plan_id)?,
                &context.task_id,
                attempt,
                phase,
                &p,
            )?
            .context_digest
            != context.context_digest
        {
            return Err(error(
                "Investigation context changed; preserve the draft report and refresh",
            ));
        }
        let receipt: Option<String> = sql(self.db.query_row(
            "SELECT receipt FROM contexts WHERE plan_id=?1 AND attempt_id=?2 AND phase=?3",
            rusqlite::params![d.plan_id, attempt, phase],
            |r| r.get(0),
        ))?;
        if receipt.is_none()
            || planning.artifact_digest.as_deref()
                != Some(
                    crate::session_store::checkpoints::decision_planning_evidence(Path::new(
                        &planning.worktree,
                    ))?
                    .as_str(),
                )
        {
            return Err(error(
                "Investigation was not delivered or modified retained source/Git state",
            ));
        }
        self.verify_artifacts(&d.plan_id, refs)?;
        let instruction = d
            .resolution
            .as_ref()
            .map(|r| r.text.clone())
            .unwrap_or_default();
        d.request.context.push_str(&format!("\nOperator requested investigation (not consent): {instruction}\nPerformer report (unverified interpretation, not human approval): {summary}"));
        for reference in refs {
            if !d.request.artifacts.contains(reference) {
                d.request.artifacts.push(reference.clone());
            }
        }
        self.verify_artifacts(&d.plan_id, &d.request.artifacts)?;
        if encode(&d.request)?.len() > p.max_request_bytes {
            return Err(error(
                "Investigation report exceeds request budget; attach bounded evidence instead",
            ));
        }
        d.resolution = None;
        d.requires_human_renewal = true;
        d.version += 1;
        d.status = if p.human_question_min_risk == Consultation::NeverAsk {
            "investigating"
        } else {
            "pending_human"
        }
        .into();
        self.save_interaction(
            &d,
            &format!("agent:{}", context.task_id),
            "investigation_reported",
        )?;
        Ok(d)
    }
    fn save_interaction(&mut self, d: &Decision, actor: &str, action: &str) -> crate::Result<()> {
        let tx = sql(self
            .db
            .transaction_with_behavior(rusqlite::TransactionBehavior::Immediate))?;
        sql(tx.execute(
            "UPDATE requests SET payload=?2 WHERE id=?1",
            rusqlite::params![d.id, encode(d)?],
        ))?;
        let mut plan: Plan = decode(&sql(tx.query_row(
            "SELECT payload FROM plans WHERE id=?1",
            [&d.plan_id],
            |r| r.get::<_, String>(0),
        ))?)?;
        plan.decision_epoch += 1;
        if d.status == "resolved" {
            Self::supersede_in_transaction(&tx, d, &mut plan)?;
        }
        save_plan(&tx, &plan)?;
        event(
            &tx,
            &plan.id,
            "decision.lifecycle",
            &json!({"actor":actor,"action":action,"decision":d}),
        )?;
        sql(tx.commit())?;
        Ok(())
    }
}
pub(super) fn decode_hex(data: &str, max: usize) -> crate::Result<Vec<u8>> {
    if data.len() > max.saturating_mul(2) || data.len() % 2 != 0 || !data.is_ascii() {
        return Err(error(
            "Evidence exceeds configured byte budget or has invalid binary encoding",
        ));
    }
    data.as_bytes()
        .chunks_exact(2)
        .map(|pair| {
            u8::from_str_radix(std::str::from_utf8(pair).unwrap(), 16)
                .map_err(|_| error("Invalid evidence byte encoding"))
        })
        .collect()
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct EvidencePermission {
    pub id: String,
    pub plan_id: String,
    pub digest: String,
    pub version: u64,
    pub share: bool,
    pub tools_digest: String,
    pub policy_digest: String,
    pub actor: String,
    pub recorded_at: String,
    pub attestation: Option<Attestation>,
}
fn tools_digest(root: &Path) -> crate::Result<String> {
    let cfg = crate::load_canonical_config(&crate::ProjectPaths::from_root(root).config_path)?;
    Ok(digest(&json!([cfg.tools, cfg.automation.coordinator])))
}
impl Store {
    pub fn evidence_permission(&self, id: &str) -> crate::Result<Option<EvidencePermission>> {
        sql(self
            .db
            .query_row(
                "SELECT value FROM metadata WHERE key=?1",
                [format!("evidence-permission:{id}")],
                |r| r.get::<_, String>(0),
            )
            .optional())?
        .map(|raw| decode(&raw))
        .transpose()
    }
    pub fn evidence_permission_challenge(
        &self,
        id: &str,
        version: u64,
        share: bool,
    ) -> crate::Result<serde_json::Value> {
        let a = self.artifact(id)?;
        let previous = self.evidence_permission(id)?;
        if previous.as_ref().map(|p| p.version).unwrap_or(0) != version {
            return Err(error("Evidence sharing changed; refresh before consenting"));
        }
        let p = policy(&self.root)?;
        Ok(super::secure::mutation_challenge(
            "evidence.share",
            json!({"id":id,"digest":a.digest,"plan_id":a.plan_id,"version":version,"share":share,"destination":"configured AI tools for this plan; contents may be sent to external providers","tools_digest":tools_digest(&self.root)?}),
            &p,
        ))
    }
    pub fn share_evidence(
        &mut self,
        id: &str,
        version: u64,
        share: bool,
        actor: &str,
        proof: Option<&Attestation>,
    ) -> crate::Result<EvidencePermission> {
        let _lock = self.mutation_lock()?;
        let p = policy(&self.root)?;
        let challenge = self.evidence_permission_challenge(id, version, share)?;
        let actor = super::secure::mutation_authority(
            &self.root,
            "evidence.share",
            &challenge,
            actor,
            proof,
            &p,
        )?;
        let a = self.artifact(id)?;
        let permission = EvidencePermission {
            id: id.into(),
            plan_id: a.plan_id.clone(),
            digest: a.digest,
            version: version + 1,
            share,
            tools_digest: tools_digest(&self.root)?,
            policy_digest: digest(&p),
            actor,
            recorded_at: now(),
            attestation: proof.cloned(),
        };
        let tx = sql(self
            .db
            .transaction_with_behavior(rusqlite::TransactionBehavior::Immediate))?;
        sql(tx.execute("INSERT INTO metadata(key,value) VALUES(?1,?2) ON CONFLICT(key) DO UPDATE SET value=excluded.value",rusqlite::params![format!("evidence-permission:{id}"),encode(&permission)?]))?;
        event(&tx, &a.plan_id, "evidence.permission", &permission)?;
        sql(tx.commit())?;
        Ok(permission)
    }
    pub fn can_deliver_evidence(&self, id: &str) -> crate::Result<bool> {
        self.evidence_deliverable(id)
    }
    pub(super) fn evidence_deliverable(&self, id: &str) -> crate::Result<bool> {
        let Some(permission) = self.evidence_permission(id)? else {
            return Ok(false);
        };
        if permission.version == 0 {
            return Err(error("Invalid evidence consent version"));
        }
        if !permission.share || permission.tools_digest != tools_digest(&self.root)? {
            return Ok(false);
        }
        let a = self.artifact(id)?;
        if a.digest != permission.digest || a.plan_id != permission.plan_id {
            return Err(error("Evidence consent belongs to other contents or scope"));
        }
        let raw = encode(&permission)?;
        let mut stmt = sql(self.db.prepare(
            "SELECT payload FROM events WHERE plan_id=?1 AND kind='evidence.permission' ORDER BY seq DESC",
        ))?;
        let mut found = false;
        for row in sql(stmt.query_map([&a.plan_id], |r| r.get::<_, String>(0)))? {
            let event_raw = sql(row)?;
            let event: EvidencePermission = decode(&event_raw)?;
            if event.id == id {
                found = event_raw == raw;
                break;
            }
        }
        if !found {
            return Err(error("Evidence consent lacks its immutable audit event"));
        }
        let p = policy(&self.root)?;
        if p.trust_mode == "secure_approval" {
            let proof = permission.attestation.as_ref().ok_or_else(|| {
                error("Secure evidence disclosure requires an isolated operator attestation")
            })?;
            let mut challenge = super::secure::mutation_challenge(
                "evidence.share",
                json!({"id":id,"digest":a.digest,"plan_id":a.plan_id,"version":permission.version-1,"share":true,"destination":"configured AI tools for this plan; contents may be sent to external providers","tools_digest":permission.tools_digest}),
                &p,
            );
            challenge["policy_digest"] = json!(permission.policy_digest);
            let at = chrono::DateTime::parse_from_rfc3339(&permission.recorded_at)
                .map_err(|e| error(e.to_string()))?
                .timestamp();
            if super::secure::verify_attestation(
                &self.root,
                &p,
                proof,
                "evidence.share",
                &digest(&challenge),
                at,
            )
            .is_err()
            {
                return Ok(false);
            }
        }
        Ok(true)
    }
}
