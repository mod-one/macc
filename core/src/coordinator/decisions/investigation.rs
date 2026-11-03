use super::{store::*, *};
use serde::{Deserialize, Serialize};
use std::{collections::BTreeMap, io::Read};
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FactEvidence {
    pub source_path: String,
    pub source_digest: String,
    pub excerpt: String,
}
impl Store {
    /// A fact is resolved by retained, exact source evidence, never a confidence score.
    pub fn investigate(
        &mut self,
        id: &str,
        version: u64,
        facts: &BTreeMap<String, FactEvidence>,
        actor: &str,
        p: &Policy,
    ) -> crate::Result<Decision> {
        let lock = self.mutation_lock()?;
        let mut d = self.decision(id)?;
        if d.version != version || !matches!(d.status.as_str(), "investigating" | "pending_human") {
            return Err(error("Investigation request changed; refresh its version"));
        }
        let rounds = d.investigation.as_ref().map(|r| r.rounds).unwrap_or(0);
        if rounds >= p.investigation.max_rounds {
            return Err(error("Investigation budget exhausted; preserve work and review prerequisite evidence or increase the explicit budget"));
        }
        if actor.trim().is_empty() || facts.is_empty() {
            return Err(error(
                "Investigation needs attributed concrete fact evidence",
            ));
        }
        let mut remaining = p.investigation.max_source_bytes;
        let mut sources = BTreeMap::new();
        for (fact, evidence) in facts {
            if !d.request.missing_facts.contains(fact) {
                return Err(error(
                    "Evidence does not refer to a declared unresolved fact",
                ));
            }
            let path = super::graph::projection_path(&self.root, &evidence.source_path)?;
            let mut bytes = Vec::new();
            std::fs::File::open(path)
                .map_err(|e| error(e.to_string()))?
                .take(remaining as u64 + 1)
                .read_to_end(&mut bytes)
                .map_err(|e| error(e.to_string()))?;
            if bytes.len() > remaining
                || evidence_digest(&bytes) != evidence.source_digest
                || evidence.excerpt.trim().is_empty()
                || !std::str::from_utf8(&bytes).is_ok_and(|text| text.contains(&evidence.excerpt))
            {
                return Err(error(
                    "Fact evidence is absent, stale or exceeds the bounded source budget",
                ));
            }
            remaining -= bytes.len();
            sources.insert(evidence.source_path.clone(), evidence.source_digest.clone());
            d.request.investigated.push(format!(
                "Fact {fact}; exact source {} {}: {}",
                evidence.source_path, evidence.source_digest, evidence.excerpt
            ));
        }
        d.request
            .missing_facts
            .retain(|fact| !facts.contains_key(fact));
        if encode(&d.request)?.len() > p.max_request_bytes {
            return Err(error("Investigation evidence exceeds request budget"));
        }
        let mut report = d.investigation.unwrap_or_default();
        report.rounds = rounds + 1;
        report.source_digests.extend(sources);
        report.diagnosis="Additional facts resolved from byte-verified source excerpts; this is evidence, not a human approval".into();
        d.investigation = Some(report);
        d.version += 1;
        let tx = sql(self
            .db
            .transaction_with_behavior(rusqlite::TransactionBehavior::Immediate))?;
        sql(tx.execute(
            "UPDATE requests SET payload=?2 WHERE id=?1",
            rusqlite::params![id, encode(&d)?],
        ))?;
        event(&tx, &d.plan_id, "decision.requested", &d)?;
        event(
            &tx,
            &d.plan_id,
            "decision.investigated",
            &serde_json::json!({"actor":actor,"decision_id":id,"version":d.version,"facts":facts}),
        )?;
        let mut plan: Plan = decode(&sql(tx.query_row(
            "SELECT payload FROM plans WHERE id=?1",
            [&d.plan_id],
            |r| r.get::<_, String>(0),
        ))?)?;
        plan.decision_epoch += 1;
        save_plan(&tx, &plan)?;
        sql(tx.commit())?;
        drop(lock);
        self.reevaluate_pending(&plan, p)?;
        self.decision(id)
    }
}

/// File evidence uses ordinary SHA-256 bytes, allowing sha256sum interoperability.
pub fn evidence_digest(bytes: &[u8]) -> String {
    use sha2::Digest;
    format!("sha256:{:x}", sha2::Sha256::digest(bytes))
}
