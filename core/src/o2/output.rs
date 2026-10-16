//! Shared artifact, redaction and complete-response budgeting for reduced output.
use super::*;
use serde::Serialize;
const INPUT_BOUND: usize = 8 * 1024 * 1024;
#[derive(Debug, Serialize)]
pub struct Reduced {
    pub summary: String,
    pub raw_ref: String,
    pub estimated_tokens: u64,
    pub quality: String,
}
pub(crate) fn render(
    root: &Path,
    raw: &str,
    bundles: &[String],
    tokens: usize,
    bytes: usize,
    hook: Option<String>,
    raw_ref: Option<String>,
) -> Result<Reduced> {
    if raw.len() > INPUT_BOUND && raw_ref.is_none() {
        return Err(error("Output transport exceeds 8 MiB bound"));
    }
    let reference = if let Some(path) = raw_ref {
        path
    } else {
        let path = format!(".macc/log/context-output-{}.txt", uuid::Uuid::new_v4());
        store::write(root, &root.join(&path), raw.as_bytes())?;
        path
    };
    let input = if raw.len() > INPUT_BOUND {
        let mut first = INPUT_BOUND / 2;
        while !raw.is_char_boundary(first) {
            first -= 1;
        }
        let mut last = raw.len() - INPUT_BOUND / 2;
        while !raw.is_char_boundary(last) {
            last += 1;
        }
        format!(
            "{}\n[Middle omitted from bounded analysis; full raw artifact: {reference}]\n{}",
            &raw[..first],
            &raw[last..]
        )
    } else {
        raw.to_string()
    };
    let summary = hook
        .map(Ok)
        .unwrap_or_else(|| crate::context::reduction::apply(&input, bundles))?;
    let mut value = serde_json::json!(summary);
    store::checkpoints::redact_evidence(&mut value);
    let safe = value.as_str().unwrap_or_default();
    // Keep the raw pointer visible even when the summary itself fits the limit.
    let suffix = format!("\n[Raw artifact: {reference}]");
    let combined = format!("{safe}{suffix}");
    let (summary, _) = crate::context::bounded_with_suffix(
        &combined,
        tokens,
        bytes.min(tokens.saturating_mul(4)),
        &suffix,
    );
    Ok(Reduced {
        estimated_tokens: crate::context::estimate_tokens(&summary) as u64,
        summary,
        raw_ref: reference,
        quality: if raw.len() > INPUT_BOUND {
            "estimated; segmented bounded analysis; inspect complete raw artifact"
        } else {
            "estimated; bounded summary including artifact reference"
        }
        .into(),
    })
}
pub fn reduce(root: &Path, bundle: &str, raw: &str) -> Result<Reduced> {
    if !crate::context::reduction::known(bundle) {
        return Err(error("Unknown output reduction bundle"));
    }
    let policy = super::policy::load(root)?;
    if !policy.context_engine.output_reduction {
        return Err(error("Output reduction disabled"));
    }
    render(
        root,
        raw,
        &[bundle.into()],
        policy.context_engine.pull_limit_tokens as usize,
        policy.context_engine.max_output_bytes as usize,
        None,
        None,
    )
}
