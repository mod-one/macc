//! On-demand semantic evidence, not a persistent native symbol/relationship index.
use super::*;
use quote::ToTokens;
use serde::{Deserialize, Serialize};
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SymbolFingerprint {
    pub path: String,
    pub symbol: String,
    pub sha256: String,
    pub provenance: String,
}
pub fn fingerprint(
    root: &Path,
    target: &Path,
    adapter: &str,
    path: &str,
    symbol: &str,
) -> Result<SymbolFingerprint> {
    if !super::super::policy::load(root)?.code_intelligence.enabled {
        return Err(error(
            "Enable Code Intelligence before using symbol evidence",
        ));
    }
    super::super::paths::safe_text(symbol)?;
    if symbol.is_empty() || symbol.len() > 256 || adapter.len() > 80 {
        return Err(error("Invalid bounded symbol selector"));
    }
    match provider(adapter)?.symbol_fingerprint(root, target, path, symbol)? {
        Capability::Supported(value) => {
            if value.path != path
                || value.symbol != symbol
                || value.sha256.len() != 64
                || !value.sha256.bytes().all(|b| b.is_ascii_hexdigit())
                || value.provenance.len() > 500
            {
                return Err(error("Invalid symbol fingerprint response"));
            }
            super::super::paths::safe_text(&value.provenance)?;
            Ok(value)
        }
        Capability::Unsupported { reason, .. } => Err(error(reason)),
    }
}
pub(super) fn rust(
    root: &Path,
    path: &str,
    selector: &str,
) -> Result<Capability<SymbolFingerprint>> {
    if !path.ends_with(".rs") {
        return Ok(Capability::Unsupported { capability: "symbol_fingerprint".into(), reason: "Native semantic fingerprints currently support Rust only; use whole-file evidence for other languages".into() });
    }
    let (_, raw) = super::super::paths::source(root, path, 1048576)?;
    let source = std::str::from_utf8(&raw).map_err(|e| error(e.to_string()))?;
    let parsed = syn::parse_file(source)
        .map_err(|e| error(format!("Rust symbol source cannot be parsed: {e}")))?;
    let mut matches = vec![];
    collect(
        &parsed.items,
        "",
        selector.trim_start_matches('/'),
        &mut matches,
    );
    if matches.len() != 1 {
        return Err(error(
            "Symbol missing or ambiguous; specify its full module/type/name path",
        ));
    }
    Ok(Capability::Supported(SymbolFingerprint { path: path.into(), symbol: selector.into(), sha256: digest(format!("rust-syn-tokens-v1:{}", matches[0]).as_bytes()), provenance: "Rust parsed item tokens v1; attributes/signature/body; no macro expansion or dependency inference".into() }))
}
fn collect(items: &[syn::Item], prefix: &str, selector: &str, out: &mut Vec<String>) {
    for item in items {
        let name = match item {
            syn::Item::Fn(v) => Some(v.sig.ident.to_string()),
            syn::Item::Struct(v) => Some(v.ident.to_string()),
            syn::Item::Enum(v) => Some(v.ident.to_string()),
            syn::Item::Trait(v) => Some(v.ident.to_string()),
            syn::Item::Type(v) => Some(v.ident.to_string()),
            syn::Item::Const(v) => Some(v.ident.to_string()),
            syn::Item::Static(v) => Some(v.ident.to_string()),
            syn::Item::Mod(v) => Some(v.ident.to_string()),
            _ => None,
        };
        if let Some(name) = name {
            let full = format!("{prefix}{name}");
            if selector == full {
                out.push(item.to_token_stream().to_string());
            }
            if let syn::Item::Mod(v) = item {
                if let Some((_, nested)) = &v.content {
                    collect(nested, &format!("{full}/"), selector, out);
                }
            }
        }
        if let syn::Item::Impl(v) = item {
            if let syn::Type::Path(ty) = &*v.self_ty {
                if let Some(part) = ty.path.segments.last() {
                    for method in &v.items {
                        if let syn::ImplItem::Fn(f) = method {
                            if selector
                                == format!("{prefix}{}/{name}", part.ident, name = f.sig.ident)
                            {
                                // Include impl generics/trait/where-clause, but not unrelated methods.
                                let mut owner = v.clone();
                                owner.items = vec![method.clone()];
                                out.push(owner.to_token_stream().to_string());
                            }
                        }
                    }
                }
            }
        }
    }
}

pub fn sources(root: &Path) -> Result<serde_json::Value> {
    let providers = registry()?
        .into_iter()
        .filter(|p| !p.symbol_languages().is_empty())
        .map(|p| serde_json::json!({"id":p.descriptor().id,"languages":p.symbol_languages()}))
        .collect::<Vec<_>>();
    let tools = super::super::compaction::specs(root)
        .into_iter()
        .filter(|s| s.performer.as_ref().is_some_and(|p| p.compaction.is_some()))
        .map(|s| s.id)
        .collect::<Vec<_>>();
    Ok(serde_json::json!({"providers":providers,"tools":tools}))
}
