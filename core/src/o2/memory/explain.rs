//! Explicit operator diagnostics; default agent retrieval keeps its bounded contract.
use super::*;
use serde::Serialize;

#[derive(Serialize)]
pub struct Explanation {
    pub records: Vec<Hit>,
    pub exclusions: Vec<String>,
    pub exclusions_truncated: bool,
    pub candidate_limit: usize,
    pub result_limit: u64,
    pub note: String,
}
pub fn explain(root: &Path, request: &Search) -> Result<Explanation> {
    let prepared = super::prepare_search(root, request)?;
    let _guard = store::lock(root)?;
    let (mut db, a) = database(root)?;
    let tx = sql(db.transaction())?;
    let target =
        super::super::providers::native_worktree(root, request.access.worktree.as_deref())?;
    let mut results =
        super::search::search_in(&tx, &a.project_id, root, &target, request, false, &prepared)?;
    let policy = super::super::policy::load(root)?;
    if policy.memory.enabled && !request.include_inactive {
        // Diagnose inactive records in a separate scoped scan: they must never
        // displace active candidates or change the actual retrieval window.
        let mut inactive = request.clone();
        inactive.include_inactive = true;
        for m in super::search::candidates(&tx, &a.project_id, &inactive)? {
            if m.status != "active" {
                results.exclusions.push(format!(
                    "{}: status={}; excluded from normal retrieval",
                    m.id, m.status
                ));
            }
        }
    }
    let truncated = results.exclusions.len() > super::search::CANDIDATE_LIMIT;
    results.exclusions.truncate(super::search::CANDIDATE_LIMIT);
    sql(tx.commit())?;
    Ok(Explanation {
        records: results.hits,
        exclusions: results.exclusions,
        exclusions_truncated: truncated,
        candidate_limit: super::search::CANDIDATE_LIMIT,
        result_limit: policy.memory.search_limit,
        note: "Read-only operator inspection; query and scope filters apply before bounded candidate scans. No out-of-scope records are inspected. Results are limited by search_limit; this is not an exhaustive database audit.".into(),
    })
}

#[cfg(test)]
#[path = "explain_tests.rs"]
mod tests;
