//! Deterministic quality factors, recorded in the context manifest via rank_reason.
use super::*;
use std::collections::BTreeSet;
pub(crate) fn rank(
    hits: &mut [Hit],
    access: &Access,
    query: &str,
    p: &super::super::policy::MemoryPolicy,
) {
    let words = query.split_whitespace().collect::<BTreeSet<_>>();
    for h in hits.iter_mut() {
        let m = &h.memory;
        let task = if access.task_id.is_some() && m.proposal.source_task == access.task_id {
            p.ranking_task_weight as i64
        } else {
            0
        };
        let authority =
            if ["constraint", "decision", "security_note"].contains(&m.proposal.kind.as_str()) {
                p.ranking_authority_weight as i64
            } else {
                0
            };
        let confidence = (m.proposal.confidence * p.ranking_confidence_weight as f64) as i64;
        let evidence = m
            .proposal
            .evidence
            .iter()
            .map(|e| (&e.kind, &e.reference, &e.sha256))
            .collect::<BTreeSet<_>>()
            .len()
            .min(5) as i64
            * p.ranking_evidence_weight as i64;
        let reference=if m.proposal.evidence.iter().any(|e|words.contains(e.reference.as_str()) || matches!(&e.binding,Some(Binding::Symbol{symbol,..}) if words.contains(symbol.as_str()))) {p.ranking_reference_weight as i64}else{0};
        let length =
            (m.proposal.body.chars().count() / 400) as i64 * p.ranking_length_penalty as i64;
        h.score = task + reference + authority + confidence + evidence - length;
        h.rank_reason=format!("scope enforced; observed status={}; task={task}; exact_reference={reference}; authority_kind={authority}; confidence={confidence}; unique_evidence={evidence}; length=-{length}",m.status);
    }
    // The strongest eligible duplicate wins, with stable ID tie-breaking.
    hits.sort_by(|a, b| b.score.cmp(&a.score).then(a.memory.id.cmp(&b.memory.id)));
    let mut seen = BTreeSet::new();
    for h in hits.iter_mut() {
        let key = h
            .memory
            .proposal
            .body
            .split_whitespace()
            .collect::<Vec<_>>()
            .join(" ")
            .to_lowercase();
        let penalty = if seen.insert(key) {
            0
        } else {
            p.ranking_duplicate_penalty as i64
        };
        h.score -= penalty;
        h.rank_reason.push_str(&format!("; duplicate=-{penalty}; current evidence required for automatic use; deterministic ID tie-break"));
    }
    hits.sort_by(|a, b| b.score.cmp(&a.score).then(a.memory.id.cmp(&b.memory.id)));
}
