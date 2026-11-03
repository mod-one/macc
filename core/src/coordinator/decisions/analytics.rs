//! Bounded decision diagnostics. Similarity is advisory and never merges consent.
use super::*;
use serde_json::{json, Value};
use std::collections::{BTreeMap, BTreeSet};
fn tokens(text: &str) -> BTreeSet<String> {
    text.split(|c: char| !c.is_alphanumeric() && c != '_')
        .filter(|s| s.len() > 2)
        .map(str::to_lowercase)
        .collect()
}
fn seconds(a: &str, b: &str) -> Option<i64> {
    let a = chrono::DateTime::parse_from_rfc3339(a).ok()?;
    let b = chrono::DateTime::parse_from_rfc3339(b).ok()?;
    let delta = b.signed_duration_since(a).num_seconds();
    (delta >= 0).then_some(delta)
}
impl Store {
    pub fn decision_analytics(&self, plan: &Plan) -> crate::Result<Value> {
        let decisions = self.applicable_decisions(plan)?;
        let mut states = BTreeMap::<String, usize>::new();
        let mut risks = BTreeMap::<String, usize>::new();
        let mut origins = BTreeMap::<String, usize>::new();
        let mut human_latency = Vec::new();
        let mut delegated_latency = Vec::new();
        let mut reused = 0;
        let mut attempts = 0;
        let mut effects_waiting = 0;
        for d in &decisions {
            *states.entry(d.status.clone()).or_default() += 1;
            *risks
                .entry(format!("{:?}", d.effective_risk).to_lowercase())
                .or_default() += 1;
            if d.reused_from.is_some() {
                reused += 1;
            }
            attempts += self.planning_attempts(&d.id)?;
            if d.status == "awaiting_effects" {
                effects_waiting += 1;
            }
            if let Some(r) = &d.resolution {
                *origins.entry(r.origin.clone()).or_default() += 1;
                if let Some(s) = seconds(&d.created_at, &r.recorded_at) {
                    if r.origin.starts_with("human") {
                        human_latency.push(s);
                    } else {
                        delegated_latency.push(s);
                    }
                }
            }
        }
        human_latency.sort();
        delegated_latency.sort();
        let median = |v: &Vec<i64>| v.get(v.len() / 2).copied();
        let operations = self.operations(&plan.id)?;
        let mut op_states = BTreeMap::<String, usize>::new();
        for op in operations {
            *op_states
                .entry(format!("{:?}", op.state).to_lowercase())
                .or_default() += 1;
        }
        Ok(
            json!({"scope":"applicable decisions in the selected plan/reference branch","decisions":decisions.len(),"by_status":states,"by_risk":risks,"by_origin":origins,"reused_answers":reused,"planning_attempts":attempts,"awaiting_plan_effects":effects_waiting,"median_human_response_seconds":median(&human_latency),"median_delegated_resolution_seconds":median(&delegated_latency),"operation_outcomes":op_states,"provider_cost":"not inferred; actual usage receipts remain the authority"}),
        )
    }
    pub fn similar_requests(&self, plan: &Plan, p: &Policy) -> crate::Result<Value> {
        if !p.similarity_hints {
            return Ok(json!({"enabled":false,"suggestions":[],"authority":false}));
        }
        let mut bytes = 0;
        let mut limited = false;
        let mut rows = Vec::new();
        for d in self
            .applicable_decisions(plan)?
            .into_iter()
            .rev()
            .filter(|d| {
                matches!(
                    d.request.kind.as_str(),
                    "decision" | "clarification" | "feedback"
                ) && !matches!(d.status.as_str(), "revoked" | "superseded" | "withdrawn")
            })
        {
            if rows.len() >= p.similarity_max_candidates {
                limited = true;
                break;
            }
            let text = format!("{} {}", d.request.question, d.request.context);
            if bytes + text.len() > p.investigation.max_source_bytes {
                limited = true;
                continue;
            }
            bytes += text.len();
            rows.push((d, tokens(&text)));
        }
        let mut suggestions = Vec::new();
        for i in 0..rows.len() {
            for j in i + 1..rows.len() {
                let (a, ta) = &rows[i];
                let (b, tb) = &rows[j];
                let union = ta.union(tb).count();
                if union == 0 {
                    continue;
                }
                let score = ta.intersection(tb).count() * 100 / union;
                let same_domain = a.domain.is_some() && a.domain == b.domain;
                let same_scope = a.plan_id == b.plan_id
                    && a.request.task_id == b.request.task_id
                    && a.subject_digest == b.subject_digest;
                if same_domain || score >= p.similarity_min_percent as usize {
                    suggestions.push(json!({"left":a.id,"right":b.id,"score_percent":score,"same_verified_domain":same_domain,"same_task_contract":same_scope,"action":"inspect and explicitly revise/supersede if appropriate; never inferred consent"}));
                }
            }
        }
        suggestions.sort_by_key(|v| std::cmp::Reverse(v["score_percent"].as_u64().unwrap_or(0)));
        suggestions.truncate(32);
        Ok(
            json!({"enabled":true,"authority":false,"method":"bounded lexical/declared-domain candidate retrieval; no automatic semantic authority","candidate_count":rows.len(),"input_bytes":bytes,"budget_limited":limited,"suggestions":suggestions}),
        )
    }
}
