use super::*;
use crate::{config::CanonicalConfig, Result};
const MAX_INPUT: usize = 8 * 1024 * 1024;
/// Output reducers are selected only through an explicit skill contract.
pub fn selected(config: &CanonicalConfig, skill: &str, bundles: &[String]) -> Result<Vec<String>> {
    let states = status(config)?;
    let mut ids = Vec::new();
    for id in bundles {
        let id = super::catalog::canonical_id(id);
        if !states.hooks.iter().any(|h| h.hook.id == id) {
            tracing::warn!(
                hook = id,
                "Unknown output hook contract; unclassified output retained"
            );
        }
        if states
            .hooks
            .iter()
            .any(|h| h.enabled && h.hook.id == id && h.hook.event == Event::ToolOutput)
            && !ids.iter().any(|v| v == id)
        {
            ids.push(id.to_string());
        }
    }
    for h in states.hooks.iter().filter(|h| {
        h.enabled && h.hook.event == Event::ToolOutput && h.hook.skills.iter().any(|s| s == skill)
    }) {
        if !ids.contains(&h.hook.id) {
            ids.push(h.hook.id.clone());
        }
    }
    Ok(ids)
}
pub fn transform(config: &CanonicalConfig, ids: &[String], raw: &str) -> Result<String> {
    if crate::context::reduction::is_protocol(raw) {
        return Ok(raw.into());
    }
    run(config, ids, raw)
}
fn run(config: &CanonicalConfig, ids: &[String], raw: &str) -> Result<String> {
    if raw.len() > MAX_INPUT {
        return Err(error("Hook input exceeds 8 MiB; raw output retained"));
    }
    let states = status(config)?;
    let mut text = raw.to_string();
    for id in ids {
        let state = states
            .hooks
            .iter()
            .find(|h| h.hook.id == super::catalog::canonical_id(id) && h.enabled)
            .ok_or_else(|| error(format!("Hook unavailable or disabled: {id}")))?;
        text = match &state.hook.handler {
            Handler::Builtin { name } => match name.as_str() {
                "performer-log-summary" => {
                    crate::context::bounded_with_suffix(
                        &text,
                        2000,
                        8000,
                        "\n[Performer evidence shortened; inspect raw log]",
                    )
                    .0
                }
                "coordinator-event-summarizer" => coordinator_events(&text),
                _ => crate::context::reduction::apply(&text, std::slice::from_ref(name))?,
            },
            Handler::Rules { steps } => {
                for step in steps {
                    let (Step::RetainLines { pattern } | Step::Replace { pattern, .. }) = step;
                    let re = regex::RegexBuilder::new(pattern)
                        .size_limit(1024 * 1024)
                        .build()
                        .map_err(|e| error(e.to_string()))?;
                    text = match step {
                        Step::RetainLines { .. } => text
                            .lines()
                            .filter(|line| re.is_match(line))
                            .collect::<Vec<_>>()
                            .join("\n"),
                        Step::Replace { replacement, .. } => {
                            // Bound expansion incrementally, including empty-pattern matches.
                            let mut output = String::new();
                            let mut last = 0;
                            for found in re.find_iter(&text) {
                                if output
                                    .len()
                                    .saturating_add(found.start() - last)
                                    .saturating_add(replacement.len())
                                    > MAX_INPUT
                                {
                                    return Err(error(
                                        "External hook output exceeds 8 MiB; raw output retained",
                                    ));
                                }
                                output.push_str(&text[last..found.start()]);
                                output.push_str(replacement);
                                last = found.end();
                            }
                            if output.len().saturating_add(text.len() - last) > MAX_INPUT {
                                return Err(error("External hook output exceeds 8 MiB"));
                            }
                            output.push_str(&text[last..]);
                            output
                        }
                    };
                }
                text
            }
        };
    }
    Ok(text)
}
fn coordinator_events(raw: &str) -> String {
    // Only collapse byte-identical events. Distinct failures, phases, status changes
    // and sequence numbers remain in their original order.
    let mut seen = std::collections::BTreeSet::new();
    let mut lines = Vec::new();
    let mut duplicates = 0;
    for line in raw.lines() {
        if serde_json::from_str::<serde_json::Value>(line).is_err() {
            return raw.into();
        }
        if seen.insert(line) {
            lines.push(line);
        } else {
            duplicates += 1;
        }
    }
    let mut text = lines.join("\n");
    if duplicates > 0 {
        text.push_str(&format!("\n[{duplicates} identical coordinator events collapsed; full evidence in events.jsonl]"));
    }
    text
}
/// Evidence processing is deliberately separate from tool protocol streams.
pub fn evidence(root: &std::path::Path, id: &str, raw: &str) -> String {
    let result = (|| {
        let config =
            crate::load_canonical_config(&crate::ProjectPaths::from_root(root).config_path)?;
        let states = status(&config)?;
        let event = match id {
            "coordinator-event-summarizer" => Event::CoordinatorEvidence,
            "performer-log-summary" => Event::PerformerEvidence,
            _ => return Err(error("Unknown evidence hook event")),
        };
        let ids = states
            .hooks
            .iter()
            .filter(|h| h.enabled && h.hook.event == event)
            .map(|h| h.hook.id.clone())
            .collect::<Vec<_>>();
        // Raw protocol streams never enter this evidence-only boundary. The source
        // logs remain intact; external event processors act on prompt copies only.
        run(&config, &ids, raw)
    })();
    match result {
        Ok(text) => text,
        Err(e) => {
            tracing::warn!(hook=id,error=%e,"Evidence hook failed; raw evidence preserved");
            raw.into()
        }
    }
}
