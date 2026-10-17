//! Freshness is target-local evidence, never task completion or automatic approval.
use super::*;
use std::collections::BTreeMap;
pub fn validate_binding(e: &Evidence) -> Result<()> {
    let valid = match (&e.binding, e.kind.as_str()) {
        (Some(Binding::Symbol { provider, symbol }), "symbol") => {
            !provider.is_empty()
                && provider.len() <= 80
                && !symbol.is_empty()
                && symbol.len() <= 256
        }
        (
            Some(Binding::Tool {
                tool_id,
                observed_at,
            }),
            "provider",
        ) => {
            e.reference == *tool_id
                && !tool_id.is_empty()
                && *observed_at > 0
                && *observed_at <= now()
        }
        (None, "symbol" | "provider") => false,
        (None, _) => true,
        _ => false,
    };
    if !valid {
        return Err(error(
            "Evidence kind/binding mismatch or invalid observation",
        ));
    }
    if e.binding.is_some()
        && !e
            .sha256
            .as_ref()
            .is_some_and(|s| s.len() == 64 && s.bytes().all(|b| b.is_ascii_hexdigit()))
    {
        return Err(error("Bound evidence requires a SHA-256 fingerprint"));
    }
    Ok(())
}
pub fn symbol_evidence(
    root: &Path,
    path: &str,
    symbol: &str,
    provider: &str,
    worktree: Option<&str>,
) -> Result<Evidence> {
    let target = super::super::providers::native_worktree(root, worktree)?;
    let (_, raw) = super::super::paths::source(&target, path, 1048576)?;
    super::super::paths::safe_text(std::str::from_utf8(&raw).map_err(|e| error(e.to_string()))?)?;
    let value = super::super::providers::fingerprint(root, &target, provider, path, symbol)?;
    Ok(Evidence {
        kind: "symbol".into(),
        reference: path.into(),
        sha256: Some(value.sha256),
        revision: Some(crate::git::head_commit(&target)?),
        summary: value.provenance,
        binding: Some(Binding::Symbol {
            provider: provider.into(),
            symbol: symbol.into(),
        }),
    })
}
pub fn tool_evidence(root: &Path, tool: &str) -> Result<Evidence> {
    let observed = super::super::compaction::probe(root, tool)?;
    let fingerprint = super::super::compaction::observations::fingerprint(&observed)?;
    Ok(Evidence {
        kind: "provider".into(),
        reference: tool.into(),
        sha256: Some(fingerprint),
        revision: None,
        summary: "Explicit registered adapter version/help observation; not proof of the claim"
            .into(),
        binding: Some(Binding::Tool {
            tool_id: tool.into(),
            observed_at: observed.verified_at,
        }),
    })
}
pub(crate) struct Checker<'a> {
    root: &'a Path,
    target: &'a Path,
    db: &'a rusqlite::Connection,
    policy: &'a super::super::policy::MemoryPolicy,
    tools: BTreeMap<String, std::result::Result<String, String>>,
    symbols: BTreeMap<(String, String, String), std::result::Result<String, String>>,
}
impl<'a> Checker<'a> {
    pub(crate) fn new(
        root: &'a Path,
        target: &'a Path,
        db: &'a rusqlite::Connection,
        policy: &'a super::super::policy::MemoryPolicy,
    ) -> Self {
        Self {
            root,
            target,
            db,
            policy,
            tools: BTreeMap::new(),
            symbols: BTreeMap::new(),
        }
    }
    pub(crate) fn check(&mut self, m: &Memory) -> Result<()> {
        if m.proposal.kind == "provider_note"
            && !m
                .proposal
                .evidence
                .iter()
                .any(|e| matches!(e.binding, Some(Binding::Tool { .. })))
        {
            return Err(error(
                "Provider note has no version-bound observation; capture fresh provider evidence",
            ));
        }
        for e in &m.proposal.evidence {
            validate_binding(e)?;
            let current = match &e.binding {
                Some(Binding::Symbol { provider, symbol }) => {
                    let key = (provider.clone(), e.reference.clone(), symbol.clone());
                    if !self.symbols.contains_key(&key) {
                        if self.symbols.len() >= self.policy.symbol_check_limit as usize {
                            return Err(error(
                                "Symbol freshness check budget exhausted; fact excluded",
                            ));
                        }
                        let current = super::super::providers::fingerprint(
                            self.root,
                            self.target,
                            provider,
                            &e.reference,
                            symbol,
                        )
                        .map(|v| v.sha256)
                        .map_err(|e| e.to_string());
                        self.symbols.insert(key.clone(), current);
                    }
                    self.symbols[&key].clone().map_err(error)?
                }
                Some(Binding::Tool {
                    tool_id,
                    observed_at,
                }) => {
                    if now().saturating_sub(*observed_at)
                        >= (self.policy.provider_note_ttl_days * 86400) as i64
                    {
                        return Err(error(
                            "Provider observation expired; capture a new version observation",
                        ));
                    }
                    if !self.tools.contains_key(tool_id) {
                        if self.tools.len() >= self.policy.provider_note_probe_limit as usize {
                            return Err(error(
                                "Provider freshness check budget exhausted; fact excluded",
                            ));
                        }
                        let value = super::super::compaction::observations::current(
                            self.root, self.db, tool_id,
                        )
                        .and_then(|v| super::super::compaction::observations::fingerprint(&v))
                        .map_err(|e| e.to_string());
                        self.tools.insert(tool_id.clone(), value);
                    }
                    self.tools[tool_id].clone().map_err(error)?
                }
                None if ["file", "prd", "test"].contains(&e.kind.as_str()) => {
                    let (_, data) =
                        super::super::paths::source(self.target, &e.reference, 1048576)?;
                    digest(&data)
                }
                None => continue,
            };
            if e.sha256.as_deref() != Some(&current) {
                return Err(error(format!("Evidence changed: {}", e.reference)));
            }
        }
        Ok(())
    }
}
