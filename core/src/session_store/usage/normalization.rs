use super::Counters;
/// Missing baselines and counter resets are unknown, never negative or guessed
/// zero. Persisting the latest report also breaks attribution across gaps.
pub(super) fn incremental(
    current: &Counters,
    previous: Option<&Counters>,
    fresh: bool,
    token_scope: &str,
    cost_scope: &str,
) -> (Counters, Vec<String>) {
    let mut notes = vec![];
    let values = std::array::from_fn(|i| {
        let value = current.values()[i]?;
        if (if i == 5 { cost_scope } else { token_scope }) == "invocation" {
            return Some(value);
        }
        let prior = previous.and_then(|p| p.values()[i]);
        match prior {
            Some(before) if value >= before => Some(value - before),
            Some(_) => {
                notes.push(format!("counter_reset_dimension_{i}"));
                None
            }
            None if fresh => Some(value),
            None => {
                notes.push(format!("cumulative_baseline_missing_dimension_{i}"));
                None
            }
        }
    });
    (Counters::from_values(values), notes)
}
