//! Optional memory evidence is prepared before authority/lease acquisition.
use super::*;
pub(super) fn preflight(
    root: &Path,
    request: &ContextRequest,
) -> Result<crate::o2::memory::PreparedFreshness> {
    // Live adapter observations and AST parsing must never hold authority/lease locks.
    // The canonical checkpoint and memory proofs are rechecked after acquisition.
    let o2_policy = crate::o2::policy::load(root)?;
    let freshness = if o2_policy.memory.enabled
        && o2_policy.context_engine.enabled
        && crate::session_store::is_sqlite(root)?
    {
        let (db, a) = crate::session_store::readonly_database(root)?;
        if ready(&db)? && crate::o2::ready(&db)? {
            drop(db);
            let candidate = make(root, &a, request, "freshness_preflight")?;
            crate::o2::context::prepare_freshness(root, &candidate)?
        } else {
            Default::default()
        }
    } else {
        Default::default()
    };
    Ok(freshness)
}
