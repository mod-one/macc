//! Enforce the same emitted response budget for every registered code provider.
use super::*;
const LIMITED: &str = "Output bounded; refine the query or increase its budget.";

pub(super) fn bound(
    response: Capability<QueryResult>,
    policy: &super::super::policy::CodePolicy,
) -> Result<Capability<QueryResult>> {
    let limit = policy.output_tokens as usize;
    match response {
        Capability::Supported(mut result) => {
            let mut limited = result.hits.len() > policy.query_limit as usize;
            result.hits.truncate(policy.query_limit as usize);
            loop {
                if limited && !result.diagnostics.iter().any(|s| s == LIMITED) {
                    result.diagnostics.push(LIMITED.into());
                }
                for _ in 0..4 {
                    result.estimated_tokens =
                        crate::context::payload::json_tokens(&Capability::Supported(&result))?
                            as u64;
                }
                if result.estimated_tokens <= limit as u64 {
                    return Ok(Capability::Supported(result));
                }
                limited = true;
                if result.hits.pop().is_some() {
                    continue;
                }
                if let Some(i) = result.diagnostics.iter().position(|s| s != LIMITED) {
                    result.diagnostics.remove(i);
                    continue;
                }
                return Err(error("Code query budget cannot hold response framing"));
            }
        }
        Capability::Unsupported { capability, reason } => {
            let make = |message: String| -> Capability<QueryResult> {
                Capability::Unsupported {
                    capability: capability.clone(),
                    reason: message,
                }
            };
            let full = make(reason.clone());
            if crate::context::payload::json_tokens(&full)? <= limit {
                return Ok(full);
            }
            let chars = reason.chars().collect::<Vec<_>>();
            let fragment = |end: usize| {
                make(format!(
                    "{} [bounded]",
                    chars[..end].iter().collect::<String>()
                ))
            };
            if crate::context::payload::json_tokens(&fragment(0))? > limit {
                return Err(error("Code query budget cannot hold capability framing"));
            }
            let (mut low, mut high) = (0, chars.len());
            while low < high {
                let end = (low + high).div_ceil(2);
                if crate::context::payload::json_tokens(&fragment(end))? <= limit {
                    low = end;
                } else {
                    high = end - 1;
                }
            }
            Ok(fragment(low))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn adapter_responses_and_unsupported_unicode_reasons_share_the_envelope_budget() {
        let policy = super::super::super::policy::CodePolicy {
            output_tokens: 100,
            query_limit: 1,
            ..Default::default()
        };
        let response = Capability::Supported(QueryResult {
            provider_id: "future".into(),
            hits: vec![
                CodeHit {
                    path: "source.rs".into(),
                    line: 1,
                    text: "evidence ".repeat(500),
                    file_sha256: "f".repeat(64),
                    provenance: "literal".into()
                };
                3
            ],
            snapshot_mismatch: true,
            diagnostics: vec!["details ".repeat(500)],
            estimated_tokens: 0,
        });
        let bounded = bound(response, &policy).unwrap();
        let actual = crate::context::payload::json_tokens(&bounded).unwrap();
        assert!(actual <= 100);
        let Capability::Supported(result) = bounded else {
            panic!()
        };
        assert!(result.snapshot_mismatch);
        assert_eq!(result.estimated_tokens, actual as u64);
        assert_eq!(result.diagnostics, vec![LIMITED.to_string()]);
        let unsupported = bound(
            Capability::Unsupported {
                capability: "graph".into(),
                reason: "€ \"quoted\" ".repeat(500),
            },
            &policy,
        )
        .unwrap();
        assert!(crate::context::payload::json_tokens(&unsupported).unwrap() <= 100);
        let Capability::Unsupported { reason, .. } = unsupported else {
            panic!()
        };
        assert!(reason.ends_with("[bounded]"));
    }
}
