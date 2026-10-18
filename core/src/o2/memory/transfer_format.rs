//! Portable, bounded memory interchange. Markdown carries the same JSONL payload.
use super::*;
pub(super) const MAX_RECORDS: usize = 100;
pub(super) const MAX_BYTES: usize = 2 * 1024 * 1024;
#[derive(Debug, Clone, Copy, Default, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Format {
    #[default]
    Jsonl,
    Markdown,
}
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Entry {
    pub schema_version: u32,
    pub memory: Memory,
    pub checksum: String,
}
const PREFIX: &str =
    "# MACC memory transfer\n\nImported facts require local review.\n\n```macc-memory-jsonl\n";
const SUFFIX: &str = "```\n";
pub(super) fn encode(entries: &[Entry], format: Format) -> Result<String> {
    let mut text = String::new();
    for entry in entries {
        text.push_str(&serde_json::to_string(entry).map_err(|e| error(e.to_string()))?);
        text.push('\n');
    }
    if matches!(format, Format::Markdown) {
        text = format!("{PREFIX}{text}{SUFFIX}");
    }
    if text.len() > MAX_BYTES {
        return Err(error("Memory transfer exceeds 2 MiB"));
    }
    Ok(text)
}
pub(super) fn decode(document: &str, format: Format, max_body: u64) -> Result<Vec<Entry>> {
    if document.len() > MAX_BYTES {
        return Err(error("Memory transfer exceeds 2 MiB"));
    }
    let text = match format {
        Format::Jsonl => document,
        Format::Markdown => document
            .strip_prefix(PREFIX)
            .and_then(|v| v.strip_suffix(SUFFIX))
            .ok_or_else(|| error("Expected MACC memory Markdown envelope"))?,
    };
    let mut entries = Vec::new();
    for line in text.lines().filter(|l| !l.trim().is_empty()) {
        if entries.len() == MAX_RECORDS {
            return Err(error("Select at most 100 memories per transfer"));
        }
        let entry: Entry = serde_json::from_str(line)
            .map_err(|e| error(format!("Invalid memory transfer row: {e}")))?;
        let raw: serde_json::Value =
            serde_json::from_str(line).map_err(|e| error(e.to_string()))?;
        crate::o2::agent::clean_output(&raw)?;
        if serde_json::to_value(&entry).map_err(|e| error(e.to_string()))? != raw {
            return Err(error(
                "Memory transfer requires canonical schema; unknown or omitted fields rejected",
            ));
        }
        check(&entry, max_body)?;
        entries.push(entry);
    }
    if entries.is_empty() {
        return Err(error("Memory transfer is empty"));
    }
    Ok(entries)
}
pub(super) fn check(entry: &Entry, max_body: u64) -> Result<()> {
    let m = &entry.memory;
    if entry.schema_version != 1
        || m.id.is_empty()
        || m.id.len() > 256
        || uuid::Uuid::parse_str(&m.project_id).is_err()
        || !["proposed", "active", "stale", "invalidated", "rejected"].contains(&m.status.as_str())
        || hit(m.clone(), 0, "")?.checksum != entry.checksum
    {
        return Err(error(
            "Invalid memory transfer identity, schema, status or checksum",
        ));
    }
    validate(&m.proposal, max_body)?;
    crate::o2::agent::clean_output(&serde_json::to_value(entry).map_err(|e| error(e.to_string()))?)
}
