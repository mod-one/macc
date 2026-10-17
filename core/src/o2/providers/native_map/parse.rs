//! Selected grammar traversal; no inferred calls, references or type relationships.
use super::*;
use tree_sitter::{Language, Node, Parser};
pub(super) fn language(
    path: &str,
    p: &crate::o2::policy::CodePolicy,
) -> Option<(&'static str, Language)> {
    match Path::new(path).extension()?.to_str()? {
        "rs" if p.native_rust => Some(("rust", tree_sitter_rust::LANGUAGE.into())),
        "py" if p.native_python => Some(("python", tree_sitter_python::LANGUAGE.into())),
        "ts" if p.native_typescript => Some((
            "typescript",
            tree_sitter_typescript::LANGUAGE_TYPESCRIPT.into(),
        )),
        "tsx" if p.native_tsx => Some(("tsx", tree_sitter_typescript::LANGUAGE_TSX.into())),
        _ => None,
    }
}
pub(super) fn extract(
    language: Language,
    source: &str,
    limit: usize,
    timeout_ms: u64,
) -> Result<Vec<Symbol>> {
    let mut parser = Parser::new();
    parser
        .set_language(&language)
        .map_err(|e| error(e.to_string()))?;
    // Tree-sitter bounds C parser work even on pathological or incomplete inputs.
    #[allow(deprecated)]
    parser.set_timeout_micros(timeout_ms * 1000);
    let tree = parser
        .parse(source, None)
        .ok_or_else(|| error("Native parser timed out"))?;
    if tree.root_node().has_error() {
        return Err(error("Parse errors; no partial symbol claims emitted"));
    }
    let mut symbols = vec![];
    walk(tree.root_node(), source, "", 0, limit, &mut symbols)?;
    symbols.sort_by(|a, b| a.qualified.cmp(&b.qualified).then(a.line.cmp(&b.line)));
    Ok(symbols)
}
fn text<'a>(node: Node<'_>, source: &'a str) -> &'a str {
    &source[node.byte_range()]
}
fn declaration(node: Node<'_>) -> bool {
    matches!(
        node.kind(),
        "function_item"
            | "function_signature_item"
            | "struct_item"
            | "enum_item"
            | "trait_item"
            | "type_item"
            | "const_item"
            | "static_item"
            | "mod_item"
            | "function_definition"
            | "class_definition"
            | "function_declaration"
            | "generator_function_declaration"
            | "class_declaration"
            | "abstract_class_declaration"
            | "interface_declaration"
            | "type_alias_declaration"
            | "enum_declaration"
            | "method_definition"
            | "method_signature"
            | "abstract_method_signature"
            | "internal_module"
    ) || (node.kind() == "variable_declarator"
        && node.child_by_field_name("value").is_some_and(|v| {
            matches!(
                v.kind(),
                "arrow_function" | "function_expression" | "generator_function"
            )
        }))
}
fn owner<'a>(node: Node<'_>, source: &'a str) -> &'a str {
    if let Some(name) = node.child_by_field_name("name") {
        return text(name, source);
    }
    if node.kind() == "generic_type" {
        if let Some(ty) = node.child_by_field_name("type") {
            return owner(ty, source);
        }
    }
    text(node, source)
}
fn walk(
    node: Node<'_>,
    source: &str,
    prefix: &str,
    depth: usize,
    limit: usize,
    out: &mut Vec<Symbol>,
) -> Result<()> {
    if depth > 256 {
        return Err(error("Native syntax nesting limit exceeded"));
    }
    let mut scope = prefix.to_string();
    if node.kind() == "impl_item" {
        if let Some(ty) = node.child_by_field_name("type") {
            scope = format!("{prefix}{}/", owner(ty, source));
        }
    }
    if declaration(node) {
        if let Some(name) = node.child_by_field_name("name") {
            // Destructuring/computed declarations do not receive guessed identities.
            if matches!(
                name.kind(),
                "identifier" | "type_identifier" | "property_identifier"
            ) {
                if out.len() >= limit {
                    return Err(error(
                        "Native symbol count limit exceeded; no partial claims emitted",
                    ));
                }
                let name = text(name, source).to_string();
                let qualified = format!("{prefix}{name}");
                let body = node.child_by_field_name("body").or_else(|| {
                    if node.kind() == "variable_declarator" {
                        node.child_by_field_name("value")
                            .and_then(|v| v.child_by_field_name("body"))
                    } else {
                        None
                    }
                });
                let end = body.map_or(node.end_byte(), |b| b.start_byte());
                let signature = source[node.start_byte()..end].trim();
                if signature.len() > 2000 || qualified.len() > 512 {
                    return Err(error("Native declaration exceeds output bounds"));
                }
                out.push(Symbol {
                    name,
                    qualified: qualified.clone(),
                    kind: node.kind().into(),
                    signature: signature.into(),
                    line: node.start_position().row as u64 + 1,
                });
                scope = format!("{qualified}/");
            }
        }
    }
    let mut cursor = node.walk();
    for child in node.named_children(&mut cursor) {
        walk(child, source, &scope, depth + 1, limit, out)?;
    }
    Ok(())
}
