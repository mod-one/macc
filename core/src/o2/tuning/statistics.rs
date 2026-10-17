//! Exact paired sign test at a fixed, pre-registered sample size.
use super::*;
/// P[X >= wins], X ~ Binomial(non_ties, 1/2). At most 100 task pairs.
pub(super) fn sign_p(wins: usize, losses: usize) -> f64 {
    let n = wins + losses;
    let mut mass = 2f64.powi(-(n as i32));
    let mut sum = 0.;
    for k in 0..=n {
        if k >= wins {
            sum += mass;
        }
        if k < n {
            mass *= ((n - k) as f64) / ((k + 1) as f64);
        }
    }
    sum.min(1.)
}
pub(super) fn evaluate(trial: &Trial, pairs: Vec<PairResult>) -> Report {
    let mut r=Report{id:uuid::Uuid::new_v4().to_string(),trial_id:trial.id.clone(),pairs,passed:false,wins:0,losses:0,ties:0,p_value:1.,alpha:trial.alpha,cost_reduction:0.,latency_ratio:0.,reasons:vec![],method:"Fixed-sample paired one-sided exact sign test; ties excluded. Project trial i receives at most 0.05/(i*(i+1)) alpha. Operator-attested controlled runs and task quality; exact provider incremental cost; summed invocation latency.".into()};
    let mut a = 0f64;
    let mut b = 0f64;
    let mut am = 0f64;
    let mut bm = 0f64;
    for p in &r.pairs {
        match p.candidate_cost.cmp(&p.baseline_cost) {
            std::cmp::Ordering::Less => r.wins += 1,
            std::cmp::Ordering::Greater => r.losses += 1,
            _ => r.ties += 1,
        }
        a += p.baseline_cost as f64;
        b += p.candidate_cost as f64;
        am += p.baseline_ms as f64;
        bm += p.candidate_ms as f64;
    }
    if a > 0. {
        r.cost_reduction = 1. - b / a;
    }
    if am > 0. {
        r.latency_ratio = bm / am;
    }
    r.p_value = sign_p(r.wins, r.losses);
    let p = &trial.baseline.context_engine;
    if r.pairs.len() != trial.plan.cases.len() || r.wins + r.losses < p.tuning_min_pairs as usize {
        r.reasons
            .push("Insufficient pre-registered independent non-tied task pairs".into());
    }
    if r.p_value > r.alpha {
        r.reasons.push(
            "Cost advantage is not statistically significant at the allocated trial alpha".into(),
        );
    }
    if r.cost_reduction < p.tuning_min_cost_reduction {
        r.reasons
            .push("Measured cost reduction is below the pre-registered minimum".into());
    }
    if r.latency_ratio > 1. + p.tuning_max_latency_regression {
        r.reasons
            .push("Summed invocation latency exceeds the pre-registered guardrail".into());
    }
    if r.pairs.iter().any(|p| !p.quality_passed) {
        r.reasons.push(
            "Task acceptance, test, specification, review-severity or retry guardrail failed"
                .into(),
        );
    }
    if r.pairs.iter().any(|p| p.currency != r.pairs[0].currency) {
        r.reasons
            .push("Mixed currencies cannot support a pooled cost claim".into());
    }
    r.passed = r.reasons.is_empty();
    r
}
pub fn close(root: &Path, id: &str, cancel: bool, confirmed: bool) -> Result<Value> {
    operator(confirmed)?;
    let _guard = store::lock(root)?;
    store::require_stopped(root)?;
    let (mut db, a) = database(root)?;
    let trial = trials::open(&db)?
        .filter(|t| t.id == id && t.project == a.project_id)
        .ok_or_else(|| error("Trial is already closed or foreign"))?;
    let active: i64 = sql(db.query_row("SELECT count(*) FROM session_leases", [], |r| r.get(0)))?;
    if active != 0 {
        return Err(error(
            "Finish/recover active invocations before closing the trial",
        ));
    }
    let mut report = if cancel {
        let mut r = evaluate(&trial, vec![]);
        r.reasons = vec!["Operator cancelled; sample and alpha allocation remain recorded".into()];
        r.passed = false;
        r
    } else {
        trials::evidence(root, &trial.plan.protocol_path, &trial.plan.protocol_sha256)?;
        let obs: Vec<Observation> = events(&db, "tuning_observation", id)?;
        if obs.len() != trial.plan.cases.len() {
            return Err(error(
                "Seal every pre-registered task outcome before evaluation, or explicitly cancel",
            ));
        }
        let mut pairs = vec![];
        for c in &trial.plan.cases {
            let o = obs
                .iter()
                .find(|o| o.task_id == c.task_id)
                .ok_or_else(|| error("Missing task outcome"))?;
            trials::evidence(root, &o.evidence_path, &o.evidence_sha256)?;
            pairs.push(observations::pair(&db, &trial, o)?);
        }
        evaluate(&trial, pairs)
    };
    report.trial_id = trial.id;
    let tx = sql(db.transaction())?;
    schema::audit(
        &tx,
        "tuning_report",
        &report.id,
        serde_json::to_value(&report).map_err(|e| error(e.to_string()))?,
    )?;
    sql(tx.commit())?;
    receipt(report)
}
