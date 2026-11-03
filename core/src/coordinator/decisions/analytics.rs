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
            json!({"scope":"applicable decisions in the selected plan/reference branch","decisions":decisions.len(),"by_status":states,"by_risk":risks,"by_origin":origins,"reused_answers":reused,"planning_attempts":attempts,"awaiting_plan_effects":effects_waiting,"median_human_response_seconds":median(&human_latency),"median_delegated_resolution_seconds":median(&delegated_latency),"operation_outcomes":op_states,"triage_budget":{"extra_paid_sidecar_calls":0,"max_ai_planning_rounds_per_request":policy(&self.root)?.investigation.max_rounds,"prompt_byte_limit":policy(&self.root)?.max_context_bytes,"semantic_compute_ms":200},"provider_cost":"not inferred; actual usage receipts remain the authority"}),
        )
    }
    pub fn similar_requests(&self, plan: &Plan, p: &Policy) -> crate::Result<Value> {
        if !p.similarity_hints {
            return Ok(json!({"enabled":false,"suggestions":[],"authority":false}));
        }
        let mut bytes = 0;
        let mut limited = false;
        let mut rows = Vec::new();
        let mut documents = Vec::new();
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
            documents.push(text);
        }
        let started = std::time::Instant::now();
        let mut diagnostic = String::new();
        let semantic = if p.similarity_backend == "local_lsa" && documents.len() >= 3 {
            let limits = crate::o2::semantic::EmbeddingLimits {
                dimensions: 8,
                vocabulary: 256,
                max_ms: 100,
                max_bytes: (p
                    .investigation
                    .max_source_bytes
                    .saturating_mul(16)
                    .min(4 * 1024 * 1024)) as u64,
            };
            match crate::o2::semantic::decision_hint_space(&documents, &limits) {
                Ok(space) => Some(space),
                Err(e) => {
                    diagnostic =
                        format!("Local LSA unavailable/budget exhausted; lexical fallback: {e}");
                    None
                }
            }
        } else {
            None
        };
        let mut suggestions = Vec::new();
        let mut comparisons = 0usize;
        'pairs: for i in 0..rows.len() {
            let semantic_scores = semantic
                .as_ref()
                .and_then(|space| space.similarities(&documents[i]).ok());
            for j in i + 1..rows.len() {
                if comparisons >= 8192 || started.elapsed().as_millis() > 200 {
                    limited = true;
                    break 'pairs;
                }
                comparisons += 1;
                let (a, ta) = &rows[i];
                let (b, tb) = &rows[j];
                let union = ta.union(tb).count();
                if union == 0 {
                    continue;
                }
                let lexical_score = ta.intersection(tb).count() * 100 / union;
                let semantic_score = semantic_scores
                    .as_ref()
                    .and_then(|scores| scores.get(j))
                    .filter(|s| s.is_finite())
                    .map(|s| (s.clamp(0.0, 1.0) * 100.0).round() as usize);
                let score = semantic_score.unwrap_or(lexical_score);
                let same_domain = a.domain.is_some() && a.domain == b.domain;
                let same_scope = a.plan_id == b.plan_id
                    && a.request.task_id == b.request.task_id
                    && a.subject_digest == b.subject_digest;
                if same_domain || score >= p.similarity_min_percent as usize {
                    suggestions.push(json!({"left":a.id,"right":b.id,"score_percent":score,"lexical_score_percent":lexical_score,"local_semantic_score_percent":semantic_score,"same_verified_domain":same_domain,"same_task_contract":same_scope,"action":"inspect and explicitly revise/supersede if appropriate; never inferred consent"}));
                    suggestions.sort_by_key(|v| {
                        std::cmp::Reverse(v["score_percent"].as_u64().unwrap_or(0))
                    });
                    suggestions.truncate(32);
                }
            }
        }
        suggestions.sort_by_key(|v| std::cmp::Reverse(v["score_percent"].as_u64().unwrap_or(0)));
        suggestions.truncate(32);
        Ok(
            json!({"enabled":true,"authority":false,"method":if semantic.is_some(){"bounded local TF-IDF/LSA semantic suggestions; no automatic authority"}else{"bounded lexical/declared-domain candidate retrieval; no automatic semantic authority"},"diagnostic":diagnostic,"comparisons":comparisons,"paid_model_calls":0,"max_compute_ms":200,"candidate_count":rows.len(),"input_bytes":bytes,"budget_limited":limited,"suggestions":suggestions}),
        )
    }
}

impl Store {
    pub fn reference_directions(&self, plan: &Plan) -> crate::Result<Vec<Value>> {
        let mut out = Vec::new();
        for source in self
            .plans()?
            .into_iter()
            .filter(|p| p.reference_branch == plan.reference_branch)
        {
            for d in self.decisions(&source.id)?.into_iter().filter(|d| {
                d.status == "resolved"
                    && matches!(
                        d.request.kind.as_str(),
                        "decision" | "clarification" | "feedback"
                    )
            }) {
                if let Some(r) = &d.resolution {
                    if !r.conditions.is_empty() {
                        continue;
                    }
                    out.push(json!({"id":d.id,"version":d.version,"plan_id":source.id,"source":source.source,"task":d.request.task_id,"question":d.request.question,"text":r.text,"advisory":r.advisory,"actor":r.actor,"origin":r.origin}));
                }
                if out.len() >= 256 {
                    return Ok(out);
                }
            }
        }
        Ok(out)
    }
}
