//! Parse only adapter-selected numeric fields. Never persist provider transcripts.
use super::{model::Observation, Counters};
use crate::tool::usage::UsageSpec;
use serde_json::Value;
const MAX_COUNTER: u64 = 1_000_000_000_000_000;
fn number(v: Option<&Value>, cost: bool) -> Option<u64> {
    let v = v?;
    if cost {
        let n = v.as_f64()?;
        if n.is_finite() && n >= 0. && n <= MAX_COUNTER as f64 / 1e9 {
            Some((n * 1e9).round() as u64)
        } else {
            None
        }
    } else {
        v.as_u64().filter(|n| *n <= MAX_COUNTER)
    }
}
fn read(v: &Value, p: &UsageSpec) -> Counters {
    let mut c = Counters::from_values(std::array::from_fn(|i| {
        let key = [
            "input_uncached",
            "cached_read",
            "cached_write",
            "output",
            "reasoning",
            "cost",
        ][i];
        number(p.fields.get(key).and_then(|path| v.pointer(path)), i == 5)
    }));
    if p.input_includes_cache_read {
        c.input_uncached = c
            .input_uncached
            .zip(c.cached_read)
            .and_then(|(a, b)| a.checked_sub(b));
    }
    c
}
fn sum(rows: Vec<Counters>) -> Counters {
    if rows.is_empty() {
        return Counters::default();
    }
    let v = std::array::from_fn(|i| {
        rows.iter()
            .try_fold(0_u64, |total, row| total.checked_add(row.values()[i]?))
    });
    Counters::from_values(v)
}
pub(super) fn documents(raw: &str) -> Vec<Value> {
    if let Ok(v) = serde_json::from_str::<Value>(raw) {
        return match v {
            Value::Array(a) => a,
            _ => vec![v],
        };
    }
    raw.lines()
        .filter_map(|line| serde_json::from_str(line).ok())
        .collect()
}
pub(super) fn observe(raw: &str, p: Option<&UsageSpec>) -> Observation {
    let Some(p) = p else {
        return Observation {
            notes: vec!["adapter_usage_unavailable".into()],
            ..Default::default()
        };
    };
    let docs = documents(raw);
    let matched: Vec<_> = docs
        .iter()
        .filter(|v| {
            p.event_match
                .iter()
                .all(|(path, value)| v.pointer(path).and_then(Value::as_str) == Some(value))
        })
        .collect();
    let Some(last) = matched.last() else {
        return Observation {
            notes: vec!["structured_usage_missing".into()],
            ..Default::default()
        };
    };
    // Terminal aggregate is authoritative for the invocation. Never sum it with
    // intermediate assistant/tool messages or repeated terminal summaries.
    let mut counters = if let Some(map) = p.model_map.as_ref() {
        sum(last
            .pointer(map)
            .and_then(Value::as_object)
            .map(|o| o.values().map(|v| read(v, p)).collect())
            .unwrap_or_default())
    } else {
        read(last, p)
    };
    if p.model_map.is_some() {
        counters.cost = number(
            p.fields.get("cost").and_then(|path| last.pointer(path)),
            true,
        );
    }
    let mut notes = vec![];
    if counters.values().iter().all(Option::is_none) {
        notes.push("no_valid_numeric_usage".into());
    }
    if p.fields.contains_key("cost") && counters.cost.is_none() {
        notes.push("reported_cost_missing_or_invalid".into());
    }
    let mut model = p
        .model
        .as_ref()
        .and_then(|path| last.pointer(path))
        .and_then(Value::as_str)
        .filter(|s| s.len() <= 160)
        .map(str::to_string);
    if model.is_none() {
        if let Some(map) = p
            .model_map
            .as_ref()
            .and_then(|path| last.pointer(path))
            .and_then(Value::as_object)
        {
            model = if map.len() == 1 {
                map.keys().next().filter(|s| s.len() <= 160).cloned()
            } else if map.len() > 1 {
                Some("multiple_models_aggregate".into())
            } else {
                None
            };
        }
    }
    if model.as_ref().is_some_and(|m| {
        !m.chars()
            .all(|c| c.is_ascii_alphanumeric() || "-._:/".contains(c))
            || !crate::security::secret_scan::scan_bytes("usage_model", m.as_bytes()).is_empty()
    }) {
        model = None;
        notes.push("model_metadata_redacted".into());
    }
    let session = p
        .session_id
        .as_ref()
        .and_then(|path| docs.iter().rev().find_map(|v| v.pointer(path)))
        .and_then(Value::as_str)
        .filter(|s| s.len() <= 512)
        .map(str::to_string);
    Observation {
        counters,
        notes,
        model,
        session,
    }
}
pub fn render(raw: &str, p: &UsageSpec) -> String {
    documents(raw)
        .iter()
        .filter(|v| {
            p.text_match
                .iter()
                .all(|(path, value)| v.pointer(path).and_then(Value::as_str) == Some(value))
        })
        .flat_map(|v| {
            p.text
                .iter()
                .filter_map(|path| v.pointer(path).and_then(Value::as_str))
        })
        .collect::<Vec<_>>()
        .join(&p.text_separator)
}
