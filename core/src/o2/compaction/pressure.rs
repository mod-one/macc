//! Only explicit adapter gauges describe pressure. Aggregate/cumulative usage is never a window gauge.
use super::*;
#[derive(Debug, Serialize)]
pub struct State {
    pub session: store::SessionView,
    pub decision: Decision,
    pub observed_compactions: Option<u64>,
}
pub(crate) fn observe(
    db: &rusqlite::Connection,
    root: &Path,
    key: &store::LeaseKey,
    raw: &str,
) -> Result<()> {
    if !super::super::ready(db)?
        || !super::super::policy::load(root)?
            .adaptive_compaction
            .enabled
    {
        return Ok(());
    }
    let identity: String = sql(db.query_row(
        "SELECT identity_json FROM logical_sessions WHERE id=?1",
        [&key.session_id],
        |r| r.get(0),
    ))?;
    let v: serde_json::Value = store::json(identity.as_bytes())?;
    let Some(profile) = v
        .pointer("/execution_config/performer/compaction")
        .filter(|v| !v.is_null())
    else {
        return Ok(());
    };
    let p: crate::tool::compaction::CompactionSpec =
        serde_json::from_value(profile.clone()).map_err(|e| error(e.to_string()))?;
    p.validate()?;
    let documents: Vec<serde_json::Value> = if let Ok(v) = serde_json::from_str(raw) {
        vec![v]
    } else {
        raw.lines()
            .filter_map(|s| serde_json::from_str(s).ok())
            .collect()
    };
    let read = |path: &Option<String>| {
        path.as_ref()
            .and_then(|p| {
                documents
                    .iter()
                    .rev()
                    .find_map(|v| v.pointer(p).and_then(|v| v.as_u64()))
            })
            .filter(|n| *n <= 16000000)
    };
    let tokens = read(&p.context_tokens_path);
    let window = read(&p.context_window_path).filter(|w| *w > 0);
    let count = read(&p.compaction_count_path);
    sql(db.execute("INSERT INTO compaction_pressure VALUES(?1,?2,?3,?4,?5) ON CONFLICT(session_id) DO UPDATE SET generation=excluded.generation,context_tokens=excluded.context_tokens,window=excluded.window,compactions=excluded.compactions",params![key.session_id,key.generation,tokens,window,count]))?;
    Ok(())
}
pub(crate) fn evaluate(
    db: &rusqlite::Connection,
    root: &Path,
    id: &str,
    uses: u64,
    budget: u64,
) -> Result<(Decision, Option<u64>)> {
    let p = super::super::policy::load(root)?;
    if !super::super::ready(db)? {
        return Ok((
            decision(&p.adaptive_compaction, uses, budget, None, None, false),
            None,
        ));
    }
    let observed:Option<(Option<u64>,Option<u64>,Option<u64>)>=sql(db.query_row("SELECT context_tokens,window,compactions FROM compaction_pressure p JOIN logical_sessions s ON s.id=p.session_id AND s.generation=p.generation WHERE p.session_id=?1",[id],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?))).optional())?;
    let (tokens, window, count) = observed.unwrap_or_default();
    let mut outcome = decision(&p.adaptive_compaction, uses, budget, tokens, window, false);
    if p.adaptive_compaction.enabled
        && count.is_some_and(|n| n >= p.adaptive_compaction.max_compactions_per_session)
    {
        outcome.action = "checkpoint_rotate".into();
        outcome.reason =
            "Observed native compaction count reached the configured boundary budget".into();
    }
    Ok((outcome, count))
}
pub fn states(root: &Path) -> Result<Vec<State>> {
    let view = store::inspect(root)?;
    let _guard = store::lock(root)?;
    let (db, _) = database(root)?;
    view.sessions
        .into_iter()
        .map(|s| {
            let (decision, observed_compactions) = evaluate(
                &db,
                root,
                &s.id,
                s.use_count.max(0) as u64,
                view.policy.checkpoints.rotate_after_uses,
            )?;
            Ok(State {
                session: s,
                decision,
                observed_compactions,
            })
        })
        .collect()
}
