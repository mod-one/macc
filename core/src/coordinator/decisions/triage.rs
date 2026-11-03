//! Deterministic governance, bounded read-only investigation and subject arbitration.
use super::{store::*, *};
use rusqlite::TransactionBehavior;
use serde_json::{json, Value};
use std::collections::BTreeSet;

pub(super) struct Triage {
    pub risk: Risk,
    pub mandatory: bool,
    pub domain: Option<String>,
    pub reason: String,
    pub reuse: Option<Decision>,
    pub standing: Option<(String, Option<String>)>,
    pub report: InvestigationReport,
}
fn overlap(a: &str, b: &str) -> bool {
    let a = a
        .split(['*', '?', '['])
        .next()
        .unwrap_or_default()
        .trim_end_matches('/');
    let b = b
        .split(['*', '?', '['])
        .next()
        .unwrap_or_default()
        .trim_end_matches('/');
    a.is_empty()
        || b.is_empty()
        || a == "."
        || b == "."
        || a == b
        || a.starts_with(&format!("{b}/"))
        || b.starts_with(&format!("{a}/"))
}
impl Store {
    pub(super) fn triage(
        &self,
        plan: &Plan,
        request: &mut Request,
        policy: &Policy,
    ) -> crate::Result<Triage> {
        let node = super::graph::task(&plan.graph, &request.task_id).ok();
        let paths = node
            .and_then(|n| n.pointer("/change_scope/allowed_paths"))
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default();
        let mut risk = request.risk;
        if request.impact.shared_contract
            || request.impact.product_scope
            || request.impact.security_privacy
            || request.scope == "project"
            || request.impact.evidence.is_empty()
        {
            risk = risk.max(Risk::High);
        }
        if request.impact.production
            || request.impact.irreversible
            || request.kind == "authorization"
        {
            risk = Risk::Critical;
        }
        if let Some(node) = node {
            if let Some(r) = node
                .pointer("/routing_hints/risk_level")
                .and_then(Value::as_str)
            {
                if let Ok(r) = serde_json::from_value::<Risk>(json!(r)) {
                    risk = risk.max(r);
                }
            }
            if node.get("gate").is_some() {
                risk = risk.max(Risk::High);
            }
        }
        let mut mandatory = matches!(
            request.kind.as_str(),
            "approval" | "authorization" | "intervention"
        ) || (request.impact.production
            && policy
                .mandatory_human_triggers
                .iter()
                .any(|s| s == "production"))
            || (request.impact.irreversible
                && policy
                    .mandatory_human_triggers
                    .iter()
                    .any(|s| s == "irreversible"));
        let mut domains = BTreeSet::new();
        let mut report = InvestigationReport {
            rounds: 1,
            ..Default::default()
        };
        let mut remaining = policy.investigation.max_source_bytes;
        let mut standing = None;
        let mut governed_fields = std::collections::BTreeMap::new();
        for rule in &policy.governance_rules {
            let matches = (!rule.task_prefixes.is_empty()
                && rule
                    .task_prefixes
                    .iter()
                    .any(|s| request.task_id.starts_with(s)))
                || (!rule.affected_paths.is_empty()
                    && rule.affected_paths.iter().any(|s| {
                        request.affected_paths.iter().any(|p| overlap(s, p))
                            || paths
                                .iter()
                                .filter_map(Value::as_str)
                                .any(|p| overlap(s, p))
                    }))
                || (!rule.requirement_refs.is_empty()
                    && rule
                        .requirement_refs
                        .iter()
                        .any(|r| request.requirement_refs.contains(r)))
                || (rule.task_prefixes.is_empty()
                    && rule.affected_paths.is_empty()
                    && rule.requirement_refs.is_empty());
            if !matches {
                continue;
            }
            risk = risk.max(rule.minimum_risk);
            mandatory |= rule.mandatory_human;
            if rule.project_wide {
                request.scope = "project".into();
            }
            if let Some(domain) = rule
                .decision_domain
                .as_ref()
                .filter(|s| !s.trim().is_empty())
            {
                domains.insert(format!(
                    "{}:{}",
                    if rule.project_wide {
                        format!("{}:{}", plan.project_id, plan.reference_branch)
                    } else {
                        plan.id.clone()
                    },
                    domain
                ));
            }
            for field in &rule.response_fields {
                if let Some(prior) = governed_fields.insert(field.id.clone(), field.clone()) {
                    if prior != *field {
                        return Err(error("Matching governance rules declare incompatible response fields; reconcile policy before requesting an answer"));
                    }
                }
                if let Some(old) = request
                    .response_fields
                    .iter_mut()
                    .find(|f| f.id == field.id)
                {
                    *old = field.clone();
                } else {
                    request.response_fields.push(field.clone());
                }
            }
            let mut complete = true;
            for source in &rule.evidence_paths {
                let path = super::graph::projection_path(&self.root, source)?;
                let bytes = match std::fs::metadata(&path) {
                    Ok(m) if m.len() <= remaining as u64 => std::fs::read(&path).ok(),
                    _ => None,
                };
                if let Some(bytes) = bytes {
                    remaining = remaining.saturating_sub(bytes.len());
                    report
                        .source_digests
                        .insert(source.clone(), evidence_digest(&bytes));
                } else {
                    complete = false;
                    report.missing_sources.push(source.clone());
                }
            }
            if complete {
                if let Some(text) = &rule.standing_answer {
                    let candidate = (text.clone(), rule.standing_choice.clone());
                    if standing.as_ref().is_some_and(|old| old != &candidate) {
                        return Err(error("Conflicting standing governance answers; reconcile project policy before dispatch"));
                    }
                    standing = Some(candidate);
                }
            }
        }
        if domains.is_empty() {
            if let Some(topic) = request.topic.as_ref().filter(|s| !s.trim().is_empty()) {
                domains.insert(format!("{}:topic:{}", plan.id, topic));
            } else if request.impact.shared_contract {
                let mut refs = request.requirement_refs.clone();
                refs.extend(request.affected_paths.iter().cloned());
                if refs.is_empty() {
                    if let Some(resources) = node.and_then(|n| n["exclusive_resources"].as_array())
                    {
                        refs.extend(
                            resources
                                .iter()
                                .filter_map(Value::as_str)
                                .map(str::to_owned),
                        );
                    }
                }
                refs.sort();
                refs.dedup();
                if !refs.is_empty() {
                    domains.insert(format!("{}:shared:{}", plan.id, digest(&refs)));
                }
            }
        }
        if domains.len() > 1 {
            return Err(error("Decision spans multiple governance domains; submit a scoped conflict request rather than privately choosing between policies"));
        }
        let domain = domains.into_iter().next();
        let mut superseded = BTreeSet::new();
        for reference in &request.supersedes {
            let old = self.decision_in_scope(plan, &reference.id)?;
            if old.version != reference.version || old.status != "resolved" {
                return Err(error(
                    "Supersession references a stale or unresolved decision",
                ));
            }
            superseded.insert(old.id);
        }
        let candidates = self.applicable_decisions(plan)?;
        loop {
            let mut changed = false;
            for old in &candidates {
                if old.status == "resolved"
                    && !superseded.contains(&old.id)
                    && old
                        .reused_from
                        .as_ref()
                        .is_some_and(|r| superseded.contains(&r.id))
                {
                    superseded.insert(old.id.clone());
                    request.supersedes.push(DecisionRef {
                        id: old.id.clone(),
                        version: old.version,
                    });
                    changed = true;
                }
            }
            if !changed {
                break;
            }
        }
        let reuse = if let Some(domain) = domain.as_ref().filter(|_| {
            !matches!(
                request.kind.as_str(),
                "approval" | "authorization" | "intervention"
            )
        }) {
            candidates
                .into_iter()
                .filter(|d| {
                    d.domain.as_ref() == Some(domain)
                        && matches!(
                            d.status.as_str(),
                            "resolved" | "awaiting_effects" | "conditional"
                        )
                        && !superseded.contains(&d.id)
                        && !matches!(
                            d.request.kind.as_str(),
                            "approval" | "authorization" | "intervention"
                        )
                        && !d.resolution.as_ref().is_some_and(|r| r.advisory)
                })
                .min_by_key(|d| {
                    (
                        if d.resolution
                            .as_ref()
                            .is_some_and(|r| r.origin.starts_with("human"))
                        {
                            0
                        } else {
                            1
                        },
                        d.created_at.clone(),
                    )
                })
        } else {
            None
        };
        if !request.supersedes.is_empty() && reuse.is_some() {
            return Err(error("Another independent binding still governs this topic; include its current decision/version in the explicit supersession request"));
        }
        if let Some(existing) = &reuse {
            self.verify_human_binding(existing, policy)?;
        }
        mandatory |= !request.supersedes.is_empty();
        report.diagnosis=if !report.missing_sources.is_empty(){"Required authoritative sources are unavailable or exceed the investigation budget; no facts were fabricated"}else if !request.missing_facts.is_empty(){"Essential facts remain unresolved after bounded source checks; no safe default was established"}else{"Authoritative source checks completed within the configured read-only budget"}.into();
        let reason=if reuse.is_some(){"Existing binding takes precedence; conflicting proposals are explicitly reconciled to the recorded subject authority"}else if standing.is_some(){"Versioned project policy provides an explicit standing answer"}else if mandatory{"Mandatory governance/approval/conflict rule"}else{"Effective risk and current consultation threshold"}.into();
        Ok(Triage {
            risk,
            mandatory,
            domain,
            reason,
            reuse,
            standing,
            report,
        })
    }
    /// Re-evaluate unanswered requests at a safe scheduler boundary. Never
    /// reinterpret an existing human rejection as delegated consent.
    pub fn reevaluate_pending(&mut self, plan: &Plan, policy: &Policy) -> crate::Result<()> {
        let _fence = self.mutation_lock()?;
        self.reevaluate_pending_locked(plan, policy)
    }
    pub(super) fn reevaluate_pending_locked(
        &mut self,
        plan: &Plan,
        policy: &Policy,
    ) -> crate::Result<()> {
        let ids = self
            .applicable_decisions(plan)?
            .into_iter()
            .filter(|d| {
                matches!(
                    d.status.as_str(),
                    "pending_human" | "investigating" | "blocked_external"
                )
            })
            .map(|d| d.id)
            .collect::<Vec<_>>();
        for id in ids {
            let d = self.decision(&id)?;
            if d.status == "investigating"
                && (!d.request.missing_facts.is_empty()
                    || d.investigation
                        .as_ref()
                        .is_some_and(|r| !r.missing_sources.is_empty()))
                && d.investigation
                    .as_ref()
                    .is_some_and(|r| r.rounds >= policy.investigation.max_rounds)
            {
                continue;
            }
            let owner = self.plan(&d.plan_id)?;
            let mut request = d.request.clone();
            let result = self.triage(&owner, &mut request, policy)?;
            let unresolved =
                !result.report.missing_sources.is_empty() || !request.missing_facts.is_empty();
            let automatic = d.request.kind != "intervention"
                && !d.requires_human_renewal
                && !unresolved
                && (policy.human_question_min_risk == Consultation::NeverAsk
                    || result.reuse.is_some()
                    || result.standing.is_some()
                    || (!result.mandatory && !policy.human_question_min_risk.asks(result.risk)));
            if automatic {
                self.resolve_by_policy_locked(&id, policy)?;
                continue;
            }
            let mut latest = self.decision(&id)?;
            if latest.version != d.version
                || !matches!(
                    latest.status.as_str(),
                    "pending_human" | "investigating" | "blocked_external"
                )
            {
                continue;
            }
            let rounds = d.investigation.as_ref().map(|r| r.rounds).unwrap_or(0);
            let mut report = result.report;
            // Scheduler freshness checks must not consume the performer's
            // complementary investigation budget before it can supply evidence.
            report.rounds = rounds.max(1);
            let status = if d.request.kind == "intervention" {
                if policy.human_question_min_risk == Consultation::NeverAsk {
                    "blocked_external"
                } else {
                    "pending_human"
                }
            } else if (d.requires_human_renewal
                && policy.human_question_min_risk == Consultation::NeverAsk)
                || unresolved
                    && (policy.human_question_min_risk == Consultation::NeverAsk
                        || !policy.human_question_min_risk.asks(result.risk))
            {
                "investigating"
            } else {
                "pending_human"
            };
            if latest.request == request
                && latest.effective_risk == result.risk
                && latest.domain == result.domain
                && latest.policy_reason == result.reason
                && latest.investigation.as_ref() == Some(&report)
                && latest.status == status
            {
                continue;
            }
            latest.request = request;
            latest.effective_risk = result.risk;
            latest.domain = result.domain;
            latest.policy_reason = result.reason;
            latest.investigation = Some(report);
            latest.status = status.into();
            latest.version += 1;
            let tx = sql(self
                .db
                .transaction_with_behavior(TransactionBehavior::Immediate))?;
            sql(tx.execute(
                "UPDATE requests SET payload=?2 WHERE id=?1",
                rusqlite::params![id, encode(&latest)?],
            ))?;
            event(&tx, &latest.plan_id, "decision.requested", &latest)?;
            let mut owner: Plan = decode(&sql(tx.query_row(
                "SELECT payload FROM plans WHERE id=?1",
                [&latest.plan_id],
                |r| r.get::<_, String>(0),
            ))?)?;
            owner.decision_epoch += 1;
            save_plan(&tx, &owner)?;
            sql(tx.commit())?;
        }
        Ok(())
    }
    pub(super) fn supersede_in_transaction(
        tx: &rusqlite::Transaction<'_>,
        new: &Decision,
        plan: &mut Plan,
    ) -> crate::Result<()> {
        for reference in &new.request.supersedes {
            let mut old: Decision = decode(&sql(tx.query_row(
                "SELECT payload FROM requests WHERE id=?1",
                [&reference.id],
                |r| r.get::<_, String>(0),
            ))?)?;
            if old.version != reference.version || old.status != "resolved" {
                return Err(error("A competing decision changed during supersession"));
            }
            old.version += 1;
            old.status = "superseded".into();
            sql(tx.execute(
                "UPDATE requests SET payload=?2 WHERE id=?1",
                rusqlite::params![old.id, encode(&old)?],
            ))?;
            event(
                tx,
                &old.plan_id,
                "decision.superseded",
                &json!({"actor":new.resolution.as_ref().map(|r|&r.actor),"replacement":new.id,"decision":old}),
            )?;
            if old.plan_id == plan.id {
                plan.decision_epoch += 1;
            } else {
                let mut owner: Plan = decode(&sql(tx.query_row(
                    "SELECT payload FROM plans WHERE id=?1",
                    [&old.plan_id],
                    |r| r.get::<_, String>(0),
                ))?)?;
                owner.decision_epoch += 1;
                save_plan(tx, &owner)?;
            }
        }
        Ok(())
    }
}
