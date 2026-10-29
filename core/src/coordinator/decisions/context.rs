use super::{store::*, *};
use rusqlite::{OptionalExtension, TransactionBehavior};
use serde_json::{json, Value};
use std::{
    collections::BTreeSet,
    path::{Path, PathBuf},
};

pub(super) fn applies(plan: &Plan, decision: &Decision, task_id: &str) -> crate::Result<bool> {
    if decision.request.scope == "project"
        || decision.request.affected_tasks.iter().any(|id| id == "*")
    {
        return Ok(true);
    }
    let node = super::graph::task(&plan.graph, task_id)?;
    if decision.request.scope == "requirements" {
        for field in ["spec_refs", "requirement_refs", "requirements"] {
            if let Some(refs) = node[field].as_array() {
                if decision.request.requirement_refs.iter().any(|id| {
                    refs.iter().any(|r| {
                        r.as_str() == Some(id)
                            || ["id", "ref", "requirement_id", "path"]
                                .iter()
                                .any(|key| r[*key].as_str() == Some(id))
                    })
                }) {
                    return Ok(true);
                }
            }
        }
    }
    if decision.request.scope == "paths" {
        if let Some(paths) = node
            .pointer("/change_scope/allowed_paths")
            .and_then(Value::as_array)
        {
            if decision.request.affected_paths.iter().any(|scope| {
                paths.iter().filter_map(Value::as_str).any(|p| {
                    let scope_prefix = scope.split(['*', '?', '[']).next().unwrap_or_default();
                    let path_prefix = p.split(['*', '?', '[']).next().unwrap_or_default();
                    path_prefix.starts_with(scope_prefix) || scope_prefix.starts_with(path_prefix)
                })
            }) {
                return Ok(true);
            }
        }
    }
    if node["decision_refs"]
        .as_array()
        .is_some_and(|ids| ids.iter().any(|id| id.as_str() == Some(&decision.id)))
    {
        return Ok(true);
    }
    let mut seen: BTreeSet<String> = decision.request.affected_tasks.iter().cloned().collect();
    loop {
        let before = seen.len();
        for task in super::graph::tasks(&plan.graph)? {
            if task["id"].as_str().is_some_and(|id| seen.contains(id)) {
                if let Some(replacement) = task["replacement_task_id"].as_str() {
                    seen.insert(replacement.into());
                }
            }
            if task["dependencies"].as_array().is_some_and(|deps| {
                deps.iter()
                    .filter_map(Value::as_str)
                    .any(|d| seen.contains(d))
            }) {
                if let Some(id) = task["id"].as_str() {
                    seen.insert(id.into());
                }
            }
        }
        if before == seen.len() {
            break;
        }
    }
    Ok(seen.contains(task_id))
}

impl Store {
    pub fn context_history(&self, plan: &str, task: &str) -> crate::Result<Vec<Value>> {
        let mut statement = sql(self
            .db
            .prepare("SELECT payload,receipt FROM contexts WHERE plan_id=?1 ORDER BY rowid DESC"))?;
        let rows = sql(statement.query_map([plan], |r| {
            Ok((r.get::<_, String>(0)?, r.get::<_, Option<String>>(1)?))
        }))?;
        let mut result = Vec::new();
        for row in rows {
            let (payload, receipt) = sql(row)?;
            let context: ContextPackage = decode(&payload)?;
            if context.task_id == task {
                result.push(json!({"context":context,"rendered":context.render(),"receipt":receipt.map(|r|decode::<Value>(&r)).transpose()?}));
                if result.len() >= 50 {
                    break;
                }
            }
        }
        Ok(result)
    }
    pub fn context(
        &self,
        plan: &Plan,
        task: &str,
        attempt: &str,
        phase: &str,
        policy: &Policy,
    ) -> crate::Result<ContextPackage> {
        let node = super::graph::task(&plan.graph, task)?;
        if node.get("decision_disposition").is_some() {
            return Err(error(
                "DM_STALE: task was retired/quarantined by a graph decision; preserve its work",
            ));
        }
        let mut bindings = Vec::new();
        for decision in self.applicable_decisions(plan)? {
            if !applies(plan, &decision, task)? {
                continue;
            }
            if decision.status == "pending_human" {
                return Err(error(format!(
                    "DM_PENDING: task {task} waits for decision {}",
                    decision.id
                )));
            }
            if decision.status == "resolved" {
                bindings.push(decision);
            }
        }
        let mut context = ContextPackage {
            schema_version: 1,
            plan_id: plan.id.clone(),
            task_id: task.into(),
            attempt_id: attempt.into(),
            phase: phase.into(),
            contract_digest: digest(&super::graph::contract(node)),
            policy: policy.clone(),
            bindings,
            context_digest: String::new(),
        };
        context.context_digest = context_hash(&context);
        if context.render().len() > policy.max_context_bytes {
            return Err(error("Mandatory decision context exceeds budget; split scope or raise the budget before dispatch; no binding was truncated"));
        }
        Ok(context)
    }
    pub fn issue(
        &mut self,
        plan: &Plan,
        task: &str,
        attempt: &str,
        phase: &str,
        worktree: &Path,
        policy: &Policy,
    ) -> crate::Result<(ContextPackage, PathBuf)> {
        let context = self.context(plan, task, attempt, phase, policy)?;
        if attempt.is_empty()
            || attempt.len() > 128
            || !attempt
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_'))
            || !matches!(phase, "dev" | "test" | "review" | "fix" | "merge_fix")
        {
            return Err(error("Invalid context attempt/phase"));
        }
        let tx = sql(self
            .db
            .transaction_with_behavior(TransactionBehavior::Immediate))?;
        let old: Option<String> = sql(tx
            .query_row(
                "SELECT payload FROM contexts WHERE plan_id=?1 AND attempt_id=?2 AND phase=?3",
                rusqlite::params![plan.id, attempt, phase],
                |r| r.get(0),
            )
            .optional())?;
        if old
            .as_deref()
            .is_some_and(|old| old != encode(&context).unwrap_or_default())
        {
            return Err(error(
                "An attempt's context is immutable; create a new attempt after decision changes",
            ));
        }
        sql(tx.execute(
            "INSERT OR IGNORE INTO contexts(plan_id,attempt_id,phase,payload) VALUES(?1,?2,?3,?4)",
            rusqlite::params![plan.id, attempt, phase, encode(&context)?],
        ))?;
        event(
            &tx,
            &plan.id,
            "task.context.prepared",
            &json!({"task":task,"attempt":attempt,"phase":phase,"digest":context.context_digest}),
        )?;
        sql(tx.commit())?;
        let paths = crate::ProjectPaths::from_root(worktree);
        let dir = worktree.join(".macc/runtime/decisions").join(attempt);
        let file = dir.join(format!("{phase}.json"));
        crate::atomic_write(&paths, &file, encode(&context)?.as_bytes())?;
        crate::atomic_write(
            &paths,
            &dir.join(format!("{phase}.md")),
            context.render().as_bytes(),
        )?;
        Ok((context, file))
    }
    pub fn issued(
        &mut self,
        plan: &str,
        attempt: &str,
        phase: &str,
        prompt: &str,
        policy: &Policy,
    ) -> crate::Result<ContextReceipt> {
        let context = self.read_context(plan, attempt, phase)?;
        let current = self.plan(plan)?;
        let fresh = self.context(&current, &context.task_id, attempt, phase, policy)?;
        if fresh.context_digest != context.context_digest {
            return Err(error("Stale context before provider invocation"));
        }
        if !prompt.contains(&context.render()) {
            return Err(error(
                "Mandatory decision text is missing from the actual runner prompt",
            ));
        }
        let receipt = ContextReceipt {
            context_digest: context.context_digest.clone(),
            prompt_digest: digest(&prompt),
            issued_at: now(),
            acknowledged: false,
            completion_status: "issued".into(),
            reviewed_commit: None,
            verification_error: None,
            compliance: Default::default(),
        };
        sql(self.db.execute(
            "UPDATE contexts SET receipt=?4 WHERE plan_id=?1 AND attempt_id=?2 AND phase=?3",
            rusqlite::params![plan, attempt, phase, encode(&receipt)?],
        ))?;
        Ok(receipt)
    }
    pub fn attest_review_artifact(
        &mut self,
        plan: &str,
        attempt: &str,
        worktree: &Path,
        expected: &str,
    ) -> crate::Result<()> {
        let _fence = self.mutation_lock()?;
        let context = self.read_context(plan, attempt, "review")?;
        let fresh = self.context(
            &self.plan(plan)?,
            &context.task_id,
            attempt,
            "review",
            &policy(&self.root)?,
        )?;
        if fresh.context_digest != context.context_digest {
            return Err(error(
                "DM_STALE: decisions changed before review postchecks completed",
            ));
        }
        if crate::git::is_dirty(worktree)? {
            return Err(error(
                "Review artifact is dirty; retained changes need validation",
            ));
        }
        let commit = crate::git::head_commit(worktree)?;
        if commit != expected {
            return Err(error("DM_STALE: artifact moved after independent review"));
        }
        let raw: String = sql(self.db.query_row(
            "SELECT receipt FROM contexts WHERE plan_id=?1 AND attempt_id=?2 AND phase='review'",
            rusqlite::params![plan, attempt],
            |r| r.get(0),
        ))?;
        let mut receipt: ContextReceipt = decode(&raw)?;
        if receipt.completion_status != "accepted" || !receipt.acknowledged {
            return Err(error("Review context/compliance was not accepted"));
        }
        receipt.reviewed_commit = Some(commit);
        let tx = sql(self
            .db
            .transaction_with_behavior(TransactionBehavior::Immediate))?;
        sql(tx.execute(
            "UPDATE contexts SET receipt=?3 WHERE plan_id=?1 AND attempt_id=?2 AND phase='review'",
            rusqlite::params![plan, attempt, encode(&receipt)?],
        ))?;
        event(
            &tx,
            plan,
            "task.context.received",
            &json!({"task":context.task_id,"attempt":attempt,"phase":"review","receipt":receipt,"postchecks":"clean, unchanged review artifact"}),
        )?;
        sql(tx.commit())?;
        Ok(())
    }
    pub fn read_context(
        &self,
        plan: &str,
        attempt: &str,
        phase: &str,
    ) -> crate::Result<ContextPackage> {
        let context: ContextPackage = decode(&sql(self.db.query_row(
            "SELECT payload FROM contexts WHERE plan_id=?1 AND attempt_id=?2 AND phase=?3",
            rusqlite::params![plan, attempt, phase],
            |r| r.get::<_, String>(0),
        ))?)?;
        if context.context_digest != context_hash(&context) {
            return Err(error("Stored context digest is corrupt"));
        }
        Ok(context)
    }
    pub fn complete(
        &mut self,
        plan: &str,
        attempt: &str,
        phase: &str,
        output: &str,
        policy: &Policy,
    ) -> crate::Result<ContextReceipt> {
        let context = self.read_context(plan, attempt, phase)?;
        let raw: String = sql(self.db.query_row(
            "SELECT receipt FROM contexts WHERE plan_id=?1 AND attempt_id=?2 AND phase=?3",
            rusqlite::params![plan, attempt, phase],
            |r| r.get(0),
        ))?;
        let mut receipt: ContextReceipt = decode(&raw)?;
        receipt.acknowledged = output.lines().any(|line| {
            line.trim()
                .strip_prefix("MACC_DECISION_ACK:")
                .is_some_and(|v| v.trim() == context.context_digest)
        });
        receipt.compliance.clear();
        receipt.reviewed_commit = None;
        let mut verification_error = if !receipt.acknowledged {
            Some("Missing exact MACC_DECISION_ACK; model acknowledgement is required, and is not proof of compliance".to_owned())
        } else {
            None
        };
        receipt.completion_status = if receipt.acknowledged {
            "accepted"
        } else {
            "missing_ack"
        }
        .into();
        match self.context(&self.plan(plan)?, &context.task_id, attempt, phase, policy) {
            Ok(fresh) if fresh.context_digest == context.context_digest => {}
            result => {
                receipt.completion_status = "stale".into();
                verification_error=Some(match result{Err(e)=>format!("DM_STALE: {e}"),_=>"DM_STALE: context changed during execution; preserve work and revalidate against current decisions".into()});
            }
        }
        for line in output.lines() {
            if let Some(value) = line.trim().strip_prefix("MACC_DECISION_COMPLIANCE:") {
                let checks: std::collections::BTreeMap<String, Compliance> =
                    serde_json::from_str(value.trim()).map_err(|e| error(e.to_string()))?;
                for (id, check) in checks {
                    if !matches!(
                        check.status.as_str(),
                        "pass" | "fail" | "needs_human_review"
                    ) || check.evidence.trim().is_empty()
                    {
                        return Err(error(
                            "Compliance needs a recognized status and concrete evidence",
                        ));
                    }
                    receipt.compliance.insert(id, check);
                }
            }
        }
        if phase == "review" {
            for binding in &context.bindings {
                if !binding.resolution.as_ref().is_some_and(|r| r.advisory)
                    && !receipt
                        .compliance
                        .get(&binding.id)
                        .is_some_and(|c| c.status == "pass")
                {
                    if verification_error.is_none() {
                        receipt.completion_status = "compliance_failed".into();
                        verification_error = Some(format!(
                            "Decision {} has no passing independent compliance evidence",
                            binding.id
                        ));
                    }
                }
            }
        }
        receipt.verification_error = verification_error.clone();
        let tx = sql(self
            .db
            .transaction_with_behavior(TransactionBehavior::Immediate))?;
        sql(tx.execute(
            "UPDATE contexts SET receipt=?4 WHERE plan_id=?1 AND attempt_id=?2 AND phase=?3",
            rusqlite::params![plan, attempt, phase, encode(&receipt)?],
        ))?;
        event(
            &tx,
            plan,
            "task.context.received",
            &json!({"task":context.task_id,"attempt":attempt,"phase":phase,"receipt":receipt}),
        )?;
        sql(tx.commit())?;
        if let Some(message) = verification_error {
            return Err(error(message));
        }
        Ok(receipt)
    }
}
fn context_hash(context: &ContextPackage) -> String {
    let mut v = serde_json::to_value(context).unwrap();
    v.as_object_mut().unwrap().remove("context_digest");
    digest(&v)
}
impl ContextPackage {
    pub fn render(&self) -> String {
        let mode = serde_json::to_value(self.policy.human_question_min_risk).unwrap();
        let mut out=format!("\n## MACC decision protocol v1\nContext digest: {}\nAutonomy: {}\nInvestigate the code, PRD and applicable decisions first. Decide routine implementation details autonomously. Do not ask interactive tool questions.\n",self.context_digest,mode);
        out.push_str("For a material unresolved choice, write a JSON Request proposal under .macc/tmp/ with idempotency_key, task_id, question, context, investigated, options, recommended_option_id, recommendation, affected_tasks, risk and impact evidence. For a required DAG change include proposed_patch (plan_id, base_graph_version, idempotency_key, reason, operations); never edit PRD files directly. This proposal file is explicitly permitted; do not edit other MACC state. Invoke `\"$MACC_DECISION_CLI\" --cwd \"$MACC_DECISION_ROOT\" decisions request --file <absolute-file>` with the inherited decision environment; do not substitute an older installed macc. Read its JSON result: resolved -> apply the recorded choice; superseded/revoked -> refresh the task context and investigate a new scoped request (never treat this as consent); pending_human -> preserve all work and return MACC_TASK_RESULT: precondition_unmet with MACC_TASK_PRECONDITION: decision <id> pending. A newly recorded resolution may rotate this attempt automatically to refresh context. For destructive/external execution use kind authorization with a stable operation_id; reserve it through decisions authorize <decision-id> --operation <operation-id> before execution. A reservation is not proof of external completion: after an unknown result, reconcile rather than repeating a non-idempotent operation. Never keep the provider process waiting for a person.\n");
        if self.policy.human_question_min_risk == Consultation::NeverAsk {
            out.push_str("NEVER ASK: select and implement your recommendation at any risk level, recording every material choice through the decision request command BEFORE dependent work relies on it. Never fabricate a human vote.\n");
        }
        out.push_str("Applicable decisions (exact source wording and attributed resolutions):\n");
        for decision in &self.bindings {
            out.push_str(&format!(
                "\nDecision {} v{}:\n{}\n",
                decision.id,
                decision.version,
                serde_json::to_string_pretty(decision).unwrap()
            ));
        }
        out.push_str(&format!(
            "\nAt the end echo exactly this line:\nMACC_DECISION_ACK: {}\n",
            self.context_digest
        ));
        if self.phase == "review" {
            out.push_str("Independently verify each binding against artifacts/tests. Print MACC_DECISION_COMPLIANCE: {\"decision-id\":{\"status\":\"pass|fail|needs_human_review\",\"evidence\":\"concrete artifacts/commands/results\"}}. A receipt/performer claim is not compliance evidence.\n");
        }
        out
    }
}
