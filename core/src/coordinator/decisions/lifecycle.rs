//! Immutable, versioned request revision and explicit withdrawal/expiry.
use super::{store::*, *};
use serde_json::json;
pub fn revision_challenge(old: &Decision, request: &Request, p: &Policy) -> serde_json::Value {
    super::secure::mutation_challenge(
        "decision.revise",
        json!({"id":old.id,"version":old.version,"subject_digest":old.subject_digest,"request":request}),
        p,
    )
}
pub fn withdraw_challenge(old: &Decision, reason: &str, p: &Policy) -> serde_json::Value {
    super::secure::mutation_challenge(
        "decision.withdraw",
        json!({"id":old.id,"version":old.version,"subject_digest":old.subject_digest,"reason":reason}),
        p,
    )
}
impl Store {
    pub fn revise_request(
        &mut self,
        id: &str,
        version: u64,
        mut request: Request,
        actor: &str,
        origin: &str,
        p: &Policy,
        proof: Option<&Attestation>,
    ) -> crate::Result<Decision> {
        let _lock = self.mutation_lock()?;
        if let Some(d) =
            self.mutation_replay(id, version, "revise", &json!(request), actor, proof)?
        {
            return Ok(d);
        }
        let input = json!(request);
        let old = self.decision(id)?;
        let plan = self.plan(&old.plan_id)?;
        if old.version != version {
            return Err(error(
                "Request changed; refresh before proposing a revision",
            ));
        }
        if !matches!(
            old.status.as_str(),
            "pending_human"
                | "investigating"
                | "rejected"
                | "expired"
                | "resolved"
                | "awaiting_effects"
        ) {
            return Err(error("This request has already been withdrawn or replaced"));
        }
        if request.task_id != old.request.task_id || request.kind != old.request.kind {
            return Err(error(
                "A revision must retain its originating task and request kind",
            ));
        }
        if request.idempotency_key == old.request.idempotency_key {
            return Err(error("A new revision needs its own idempotency key"));
        }
        if origin == "agent"
            && old.subject_digest
                != digest(&super::graph::contract(super::graph::task(
                    &plan.graph,
                    &request.task_id,
                )?))
        {
            return Err(error("Task contract changed; a fresh task invocation must investigate the revised proposal"));
        }
        let actor = if origin == "human" {
            super::secure::mutation_authority(
                &self.root,
                "decision.revise",
                &revision_challenge(&old, &request, p),
                actor,
                proof,
                p,
            )?
        } else if origin == "agent" {
            actor.into()
        } else {
            return Err(error(
                "Revision requires an attributed operator or pinned performer",
            ));
        };
        request.revision_of = Some(DecisionRef {
            id: old.id.clone(),
            version: old.version,
        });
        if old.request.scope == "project" {
            request.scope = "project".into();
        }
        for task in &old.request.affected_tasks {
            if !request.affected_tasks.contains(task) {
                request.affected_tasks.push(task.clone());
            }
        }
        if old.status == "resolved" {
            request.supersedes.push(DecisionRef {
                id: old.id.clone(),
                version: old.version,
            });
        }
        let audit = Self::mutation_audit(id, version, "revise", &input, &actor, proof);
        self.submit_locked(&plan, request, &actor, origin, p, Some(&old), Some(&audit))
    }
    pub fn withdraw_request(
        &mut self,
        id: &str,
        version: u64,
        reason: &str,
        actor: &str,
        proof: Option<&Attestation>,
    ) -> crate::Result<Decision> {
        let _lock = self.mutation_lock()?;
        if let Some(d) =
            self.mutation_replay(id, version, "withdraw", &json!(reason), actor, proof)?
        {
            return Ok(d);
        }
        let mut d = self.decision(id)?;
        if d.version != version
            || !matches!(
                d.status.as_str(),
                "pending_human" | "investigating" | "rejected" | "expired" | "awaiting_effects"
            )
        {
            return Err(error("Only the current unresolved request may be withdrawn; revoke a resolved binding explicitly"));
        }
        if reason.trim().is_empty() {
            return Err(error(
                "Withdrawal requires an explicit reason; it is not consent",
            ));
        }
        let p = policy(&self.root)?;
        let actor = super::secure::mutation_authority(
            &self.root,
            "decision.withdraw",
            &withdraw_challenge(&d, reason, &p),
            actor,
            proof,
            &p,
        )?;
        let mut audit =
            Self::mutation_audit(id, version, "withdraw", &json!(reason), &actor, proof);
        d.version += 1;
        d.status = "withdrawn".into();
        audit["result_id"] = json!(d.id);
        audit["result_version"] = json!(d.version);
        let tx = sql(self
            .db
            .transaction_with_behavior(rusqlite::TransactionBehavior::Immediate))?;
        sql(tx.execute(
            "UPDATE requests SET payload=?2 WHERE id=?1",
            rusqlite::params![id, encode(&d)?],
        ))?;
        let mut plan: Plan = decode(&sql(tx.query_row(
            "SELECT payload FROM plans WHERE id=?1",
            [&d.plan_id],
            |r| r.get::<_, String>(0),
        ))?)?;
        plan.decision_epoch += 1;
        save_plan(&tx, &plan)?;
        event(
            &tx,
            &plan.id,
            "decision.lifecycle",
            &json!({"actor":actor,"action":"withdrawn","reason":reason,"attestation":proof,"decision":d}),
        )?;
        event(&tx, &plan.id, "decision.mutation", &audit)?;
        sql(tx.commit())?;
        Ok(d)
    }
    pub fn expire_requests(&mut self, plan: &Plan) -> crate::Result<usize> {
        let _lock = self.mutation_lock()?;
        let mut changed = 0;
        for mut d in self.decisions(&plan.id)? {
            if !matches!(d.status.as_str(), "pending_human" | "investigating") {
                continue;
            }
            let Some(at) = &d.request.expires_at else {
                continue;
            };
            let at = chrono::DateTime::parse_from_rfc3339(at)
                .map_err(|_| error("Invalid persisted request expiry"))?;
            if at.timestamp() > chrono::Utc::now().timestamp() {
                continue;
            }
            d.version += 1;
            d.status = "expired".into();
            let tx = sql(self
                .db
                .transaction_with_behavior(rusqlite::TransactionBehavior::Immediate))?;
            sql(tx.execute(
                "UPDATE requests SET payload=?2 WHERE id=?1",
                rusqlite::params![d.id, encode(&d)?],
            ))?;
            let mut current: Plan = decode(&sql(tx.query_row(
                "SELECT payload FROM plans WHERE id=?1",
                [&plan.id],
                |r| r.get::<_, String>(0),
            ))?)?;
            current.decision_epoch += 1;
            save_plan(&tx, &current)?;
            event(
                &tx,
                &plan.id,
                "decision.lifecycle",
                &json!({"actor":"coordinator","action":"expired","decision":d}),
            )?;
            sql(tx.commit())?;
            changed += 1;
        }
        Ok(changed)
    }
}

impl Store {
    pub(super) fn mutation_audit(
        id: &str,
        version: u64,
        action: &str,
        input: &serde_json::Value,
        actor: &str,
        proof: Option<&Attestation>,
    ) -> serde_json::Value {
        json!({"id":id,"version":version,"action":action,"input_digest":digest(input),"actor":proof.map(|p|p.body.principal.as_str()).unwrap_or(actor),"attestation":proof})
    }
    pub(super) fn mutation_replay(
        &self,
        id: &str,
        version: u64,
        action: &str,
        input: &serde_json::Value,
        actor: &str,
        proof: Option<&Attestation>,
    ) -> crate::Result<Option<Decision>> {
        let expected = Self::mutation_audit(id, version, action, input, actor, proof);
        let mut stmt = sql(self.db.prepare(
            "SELECT payload FROM events WHERE kind='decision.mutation' ORDER BY seq DESC",
        ))?;
        for row in sql(stmt.query_map([], |r| r.get::<_, String>(0)))? {
            let recorded: serde_json::Value = decode(&sql(row)?)?;
            if expected
                .as_object()
                .unwrap()
                .iter()
                .all(|(k, v)| recorded.get(k) == Some(v))
            {
                return self
                    .decision(
                        recorded["result_id"]
                            .as_str()
                            .ok_or_else(|| error("Missing lifecycle result"))?,
                    )
                    .map(Some);
            }
        }
        Ok(None)
    }
    pub fn reconcile_waiting_decisions(&mut self, plan: &Plan, p: &Policy) -> crate::Result<usize> {
        let _lock = self.mutation_lock()?;
        let mut count = 0;
        // Iterate to a fixed point because an alias may depend on another alias.
        loop {
            let mut changed = false;
            for mut d in self
                .applicable_decisions(plan)?
                .into_iter()
                .filter(|d| d.status == "waiting_dependency")
            {
                let reference = d
                    .reused_from
                    .clone()
                    .ok_or_else(|| error("Waiting decision has no originating authority"))?;
                let parent = self.decision(&reference.id)?;
                if matches!(
                    parent.status.as_str(),
                    "awaiting_effects" | "waiting_dependency"
                ) {
                    continue;
                }
                if parent.status == "resolved"
                    && parent.resolution.as_ref().map(|r| &r.text)
                        == d.resolution.as_ref().map(|r| &r.text)
                    && self.verify_human_binding(&parent, p).is_ok()
                {
                    d.status = "resolved".into();
                    d.reused_from = Some(DecisionRef {
                        id: parent.id,
                        version: parent.version,
                    });
                } else {
                    d.status = "revoked".into();
                }
                d.version += 1;
                let tx = sql(self
                    .db
                    .transaction_with_behavior(rusqlite::TransactionBehavior::Immediate))?;
                sql(tx.execute(
                    "UPDATE requests SET payload=?2 WHERE id=?1",
                    rusqlite::params![d.id, encode(&d)?],
                ))?;
                let mut owner: Plan = decode(&sql(tx.query_row(
                    "SELECT payload FROM plans WHERE id=?1",
                    [&d.plan_id],
                    |r| r.get::<_, String>(0),
                ))?)?;
                owner.decision_epoch += 1;
                save_plan(&tx, &owner)?;
                event(
                    &tx,
                    &owner.id,
                    "decision.lifecycle",
                    &json!({"actor":"coordinator","action":"originating-effects-reconciled","decision":d}),
                )?;
                sql(tx.commit())?;
                count += 1;
                changed = true;
            }
            if !changed {
                break;
            }
        }
        Ok(count)
    }
}
