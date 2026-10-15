//! Declarative machine-output contract. No provider dispatch in collectors.
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct UsageSpec {
    pub schema: String,
    /// Only these output arguments change in explicitly enabled structured mode.
    #[serde(default)]
    pub append_args: Vec<String>,
    #[serde(default)]
    pub replace_values: BTreeMap<String, String>,
    #[serde(default)]
    pub event_match: BTreeMap<String, String>,
    #[serde(default)]
    pub text_match: BTreeMap<String, String>,
    #[serde(default)]
    pub fields: BTreeMap<String, String>,
    #[serde(default)]
    pub text: Vec<String>,
    #[serde(default = "newline")]
    pub text_separator: String,
    pub session_id: Option<String>,
    pub model: Option<String>,
    /// Object containing per-model counters; absent means one flat record.
    pub model_map: Option<String>,
    #[serde(default = "invocation")]
    pub token_scope: String,
    #[serde(default = "invocation")]
    pub cost_scope: String,
    #[serde(default = "estimated")]
    pub cost_quality: String,
    pub currency: Option<String>,
    #[serde(default)]
    pub input_includes_cache_read: bool,
}
fn newline() -> String {
    "\n".into()
}
fn invocation() -> String {
    "invocation".into()
}
fn estimated() -> String {
    "estimated".into()
}
impl UsageSpec {
    pub fn validate(&self) -> crate::Result<()> {
        if !["", "\n"].contains(&self.text_separator.as_str())
            || self.schema.is_empty()
            || self.schema.len() > 120
            || !matches!(self.token_scope.as_str(), "invocation" | "session")
            || !matches!(self.cost_scope.as_str(), "invocation" | "session")
            || !matches!(
                self.cost_quality.as_str(),
                "estimated" | "exact" | "unknown"
            )
        {
            return Err(crate::MaccError::Validation(
                "Invalid usage adapter schema/scopes/quality".into(),
            ));
        }
        if self.fields.contains_key("cost")
            && self
                .currency
                .as_ref()
                .is_none_or(|s| s.len() != 3 || !s.bytes().all(|b| b.is_ascii_uppercase()))
        {
            return Err(crate::MaccError::Validation(
                "Usage cost requires an explicit three-letter currency".into(),
            ));
        }
        for key in self.fields.keys() {
            if ![
                "input_uncached",
                "cached_read",
                "cached_write",
                "output",
                "reasoning",
                "cost",
            ]
            .contains(&key.as_str())
            {
                return Err(crate::MaccError::Validation(format!(
                    "Unknown usage dimension {key}"
                )));
            }
        }
        for p in self
            .fields
            .values()
            .chain(self.text.iter())
            .chain(self.session_id.iter())
            .chain(self.model.iter())
            .chain(self.model_map.iter())
            .chain(self.event_match.keys())
            .chain(self.text_match.keys())
        {
            if !p.is_empty() && !p.starts_with('/') {
                return Err(crate::MaccError::Validation(
                    "Usage paths must be JSON pointers".into(),
                ));
            }
        }
        // Output overrides cannot alter permissions, model, effort or resume identity.
        if self
            .replace_values
            .values()
            .any(|v| !["json", "stream-json"].contains(&v.as_str()))
            || self
                .replace_values
                .keys()
                .any(|k| !["--output", "--output-format"].contains(&k.as_str()))
        {
            return Err(crate::MaccError::Validation(
                "Usage overrides may only select machine output".into(),
            ));
        }
        let mut args = self.append_args.iter();
        while let Some(flag) = args.next() {
            match flag.as_str() {
                "--json" | "--verbose" => {}
                "--output-format"
                    if matches!(
                        args.next().map(String::as_str),
                        Some("json" | "stream-json")
                    ) => {}
                _ => {
                    return Err(crate::MaccError::Validation(
                        "Invalid usage output argument sequence".into(),
                    ))
                }
            }
        }
        Ok(())
    }
}
