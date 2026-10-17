//! Human-labelled local A/B evidence. No benchmark can activate retrieval by itself.
use super::*;
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Case {
    pub query: String,
    pub relevant_ids: Vec<String>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Suite {
    pub access: Access,
    pub cases: Vec<Case>,
    pub top_k: usize,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CaseResult {
    pub query: String,
    pub fts_ids: Vec<String>,
    pub hybrid_ids: Vec<String>,
    pub fts_recall: f64,
    pub hybrid_recall: f64,
    pub fts_mrr: f64,
    pub hybrid_mrr: f64,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Report {
    pub id: String,
    pub project: String,
    pub access: Access,
    pub corpus: String,
    pub canonical: String,
    pub settings: String,
    pub backend: Descriptor,
    pub suite: Suite,
    pub documents: usize,
    pub model_bytes: usize,
    pub train_ms: u128,
    pub preflight_ms: u128,
    pub max_query_ms: u128,
    pub fts_recall: f64,
    pub hybrid_recall: f64,
    pub fts_mrr: f64,
    pub hybrid_mrr: f64,
    pub passed: bool,
    pub notice: String,
    pub results: Vec<CaseResult>,
    pub regressions: usize,
}
pub(super) fn checksum(report: &Report) -> Result<String> {
    Ok(digest(
        &serde_json::to_vec(report).map_err(|e| error(e.to_string()))?,
    ))
}
fn metric(hits: &[Hit], case: &Case, k: usize) -> (f64, f64) {
    let mut found = 0.;
    let mut reciprocal = 0.;
    for (i, h) in hits.iter().take(k).enumerate() {
        if case.relevant_ids.contains(&h.memory.id) {
            found += 1.;
            if reciprocal == 0. {
                reciprocal = 1. / (i + 1) as f64;
            }
        }
    }
    (found / case.relevant_ids.len() as f64, reciprocal)
}
pub fn benchmark(root: &Path, suite: Suite) -> Result<serde_json::Value> {
    if std::env::var_os("MACC_SESSION_ROLE").is_some() {
        return Err(error("Semantic benchmark requires a separate operator"));
    }
    if !(4..=100).contains(&suite.cases.len()) || !(1..=10).contains(&suite.top_k) {
        return Err(error(
            "Benchmark requires 4–100 distinct labelled queries and top_k 1–10",
        ));
    }
    let input = serde_json::to_string(&suite).map_err(|e| error(e.to_string()))?;
    if input.len() > 131072 {
        return Err(error("Labelled benchmark exceeds its input byte bound"));
    }
    super::super::paths::safe_text(&input)?;
    let p = policy::load(root)?.memory;
    let preparation = std::time::Instant::now();
    let pool = prepare_pool(root, &suite.access)?;
    let preflight_ms = preparation.elapsed().as_millis();
    let mut distinct = std::collections::BTreeSet::new();
    for case in &suite.cases {
        if case.query.trim().is_empty()
            || case.query.len() > 512
            || !distinct.insert(case.query.trim().to_lowercase())
            || case.relevant_ids.is_empty()
            || case.relevant_ids.len() > 10
            || case
                .relevant_ids
                .iter()
                .any(|id| !pool.records.iter().any(|m| m.id == *id))
            || case
                .relevant_ids
                .iter()
                .collect::<std::collections::BTreeSet<_>>()
                .len()
                != case.relevant_ids.len()
        {
            return Err(error("Benchmark queries must be distinct, bounded and labelled with eligible in-scope memory IDs"));
        }
        super::super::paths::safe_text(&case.query)?;
    }
    let labels = suite
        .cases
        .iter()
        .flat_map(|c| c.relevant_ids.iter())
        .collect::<std::collections::BTreeSet<_>>();
    if labels.len() < 2 {
        return Err(error("Benchmark must cover at least two reviewed memories"));
    }
    let budget_started = std::time::Instant::now();
    let start = std::time::Instant::now();
    let (backend, model) = backend::fit(&pool.records, &p)?;
    let train_ms = start.elapsed().as_millis();
    let (db, _) = readonly_database(root)?;
    let mut values = [0.; 4];
    let mut max_query_ms = 0;
    let mut results = vec![];
    let mut regressions = 0;
    for case in &suite.cases {
        if budget_started.elapsed().as_millis() > p.semantic_max_ms as u128 * 4 {
            return Err(error("Local benchmark exceeded total runtime budget"));
        }
        let start = std::time::Instant::now();
        let scores = backend::scores(model.as_ref(), &case.query, &pool.records, &p)?;
        let baseline = retrieval::ranked(
            &db,
            &pool.project,
            &pool.records,
            &suite.access,
            &case.query,
            None,
            &p,
        )?;
        let hybrid = retrieval::ranked(
            &db,
            &pool.project,
            &pool.records,
            &suite.access,
            &case.query,
            Some(&scores),
            &p,
        )?;
        max_query_ms = max_query_ms.max(start.elapsed().as_millis());
        let (baseline_recall, baseline_mrr) = metric(&baseline, case, suite.top_k);
        let (r, m) = metric(&baseline, case, suite.top_k);
        values[0] += r;
        values[2] += m;
        let (r, m) = metric(&hybrid, case, suite.top_k);
        values[1] += r;
        values[3] += m;
        if r + 1e-9 < baseline_recall || m + 1e-9 < baseline_mrr {
            regressions += 1;
        }
        results.push(CaseResult {
            query: case.query.clone(),
            fts_ids: baseline
                .iter()
                .take(suite.top_k)
                .map(|h| h.memory.id.clone())
                .collect(),
            hybrid_ids: hybrid
                .iter()
                .take(suite.top_k)
                .map(|h| h.memory.id.clone())
                .collect(),
            fts_recall: baseline_recall,
            hybrid_recall: r,
            fts_mrr: baseline_mrr,
            hybrid_mrr: m,
        });
    }
    for v in &mut values {
        *v /= suite.cases.len() as f64;
    }
    drop(db);
    let passed = regressions == 0
        && values[1] - values[0] + 1e-9 >= p.semantic_min_recall_gain
        && values[3] + 1e-9 >= values[2]
        && preflight_ms + train_ms + max_query_ms <= p.semantic_max_ms as u128
        && model.bytes() <= p.semantic_max_bytes as usize;
    let report=Report{results,regressions,preflight_ms,id:uuid::Uuid::new_v4().to_string(),project:pool.project,access:suite.access.clone(),corpus:corpus(&pool.records,&suite.access)?,canonical:pool.canonical,settings:settings(&p)?,backend,suite,documents:pool.records.len(),model_bytes:model.bytes(),train_ms,max_query_ms,fts_recall:values[0],hybrid_recall:values[1],fts_mrr:values[2],hybrid_mrr:values[3],passed,notice:"Corpus-local LSA evidence only; labels and representative coverage require operator review. No model API cost, network or installation. Corpus/settings changes require a new benchmark.".into()};
    // Revalidate every input before recording evidence; no inference under the authority lock.
    let _guard = store::lock(root)?;
    let (mut db, a) = database(root)?;
    let tx = sql(db.transaction())?;
    gate::verify_pool(
        root,
        &tx,
        &a.project_id,
        &report.access,
        &pool.prepared,
        &report.canonical,
        &report.corpus,
    )?;
    super::super::schema::audit(
        &tx,
        "semantic_benchmark",
        &report.id,
        serde_json::to_value(&report).map_err(|e| error(e.to_string()))?,
    )?;
    sql(tx.commit())?;
    Ok(serde_json::json!({"checksum":checksum(&report)?,"report":report}))
}
