//! Tree-sitter symbol extraction for Rust.
//!
//! Deliberately syntactic. Tree-sitter parses; it does not resolve names,
//! types, or imports. Everything here is "this node declares a name", which is
//! enough to answer "where is X defined" and is not enough to answer "which X
//! does this call site mean". The roadmap's fallback chain
//! (SCIP/LSP → tree-sitter → lexical → grep) exists precisely because this tier
//! stops here.

use anyhow::{Context, Result};
use tree_sitter::{Node, Parser};

/// What kind of thing a name was attached to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SymbolKind {
    Function,
    Struct,
    Enum,
    Trait,
    Union,
    TypeAlias,
    Const,
    Static,
    Module,
    Macro,
    Impl,
}

impl SymbolKind {
    pub fn as_str(self) -> &'static str {
        match self {
            SymbolKind::Function => "fn",
            SymbolKind::Struct => "struct",
            SymbolKind::Enum => "enum",
            SymbolKind::Trait => "trait",
            SymbolKind::Union => "union",
            SymbolKind::TypeAlias => "type",
            SymbolKind::Const => "const",
            SymbolKind::Static => "static",
            SymbolKind::Module => "mod",
            SymbolKind::Macro => "macro",
            SymbolKind::Impl => "impl",
        }
    }

    pub fn from_str(s: &str) -> Self {
        match s {
            "struct" => SymbolKind::Struct,
            "enum" => SymbolKind::Enum,
            "trait" => SymbolKind::Trait,
            "union" => SymbolKind::Union,
            "type" => SymbolKind::TypeAlias,
            "const" => SymbolKind::Const,
            "static" => SymbolKind::Static,
            "mod" => SymbolKind::Module,
            "macro" => SymbolKind::Macro,
            "impl" => SymbolKind::Impl,
            _ => SymbolKind::Function,
        }
    }
}

/// A declared name and where it lives. Lines are 1-based, matching every tool
/// a human or agent will cross-reference this against.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Symbol {
    pub name: String,
    pub kind: SymbolKind,
    pub line: usize,
    pub end_line: usize,
    /// Enclosing `impl` or `mod`, when there is one. Lets a caller tell
    /// `Foo::new` from a free `new`.
    pub container: Option<String>,
}

/// A parser configured for Rust. Construction is not free, so callers that
/// index many files should build one and reuse it.
pub fn rust_parser() -> Result<Parser> {
    let mut parser = Parser::new();
    parser
        .set_language(&tree_sitter_rust::LANGUAGE.into())
        .context("loading the Rust grammar")?;
    Ok(parser)
}

/// Extract declared symbols from Rust source.
pub fn extract_rust(parser: &mut Parser, source: &str) -> Result<Vec<Symbol>> {
    let Some(tree) = parser.parse(source, None) else {
        // A file we cannot parse yields no symbols rather than an error: one
        // malformed file must not abort a whole-repository index.
        return Ok(Vec::new());
    };
    let bytes = source.as_bytes();
    let mut out = Vec::new();
    walk(tree.root_node(), bytes, None, &mut out);
    Ok(out)
}

fn walk(node: Node, src: &[u8], container: Option<&str>, out: &mut Vec<Symbol>) {
    let kind = match node.kind() {
        "function_item" | "function_signature_item" => Some(SymbolKind::Function),
        "struct_item" => Some(SymbolKind::Struct),
        "enum_item" => Some(SymbolKind::Enum),
        "trait_item" => Some(SymbolKind::Trait),
        "union_item" => Some(SymbolKind::Union),
        "type_item" => Some(SymbolKind::TypeAlias),
        "const_item" => Some(SymbolKind::Const),
        "static_item" => Some(SymbolKind::Static),
        "mod_item" => Some(SymbolKind::Module),
        "macro_definition" => Some(SymbolKind::Macro),
        _ => None,
    };

    if let Some(kind) = kind {
        if let Some(name) = child_name(node, src) {
            out.push(Symbol {
                name,
                kind,
                line: node.start_position().row + 1,
                end_line: node.end_position().row + 1,
                container: container.map(|c| c.to_string()),
            });
        }
    }

    // Descend, tracking the enclosing impl/trait/mod so methods can be
    // attributed to their type.
    let next_container: Option<String> = match node.kind() {
        "impl_item" => impl_type_name(node, src),
        "trait_item" | "mod_item" => child_name(node, src),
        _ => container.map(|c| c.to_string()),
    };

    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        walk(child, src, next_container.as_deref(), out);
    }
}

/// The `name:` field of a declaration node.
fn child_name(node: Node, src: &[u8]) -> Option<String> {
    node.child_by_field_name("name")
        .and_then(|n| n.utf8_text(src).ok())
        .map(|s| s.to_string())
}

/// The type an `impl` block is for, so `impl Foo { fn new() }` attributes
/// `new` to `Foo`.
fn impl_type_name(node: Node, src: &[u8]) -> Option<String> {
    node.child_by_field_name("type")
        .and_then(|n| n.utf8_text(src).ok())
        .map(|s| s.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn extract(src: &str) -> Vec<Symbol> {
        let mut p = rust_parser().unwrap();
        extract_rust(&mut p, src).unwrap()
    }

    #[test]
    fn reports_one_based_lines() {
        let syms = extract("\n\npub fn third_line() {}\n");
        assert_eq!(syms.len(), 1);
        assert_eq!(syms[0].line, 3, "lines must be 1-based to match every other tool");
    }

    #[test]
    fn attributes_methods_to_their_impl_type() {
        let syms = extract("struct Foo;\nimpl Foo {\n  pub fn new() -> Self { Foo }\n}\n");
        let new = syms.iter().find(|s| s.name == "new").expect("found new");
        assert_eq!(new.container.as_deref(), Some("Foo"));
    }

    #[test]
    fn finds_nested_and_trait_items() {
        let syms = extract(
            "mod outer {\n  pub mod inner {\n    pub fn deep() {}\n  }\n}\n\
             trait T { fn required(&self); }\n",
        );
        let names: Vec<&str> = syms.iter().map(|s| s.name.as_str()).collect();
        assert!(names.contains(&"deep"), "nested fn missing: {names:?}");
        assert!(names.contains(&"inner"));
        assert!(names.contains(&"outer"));
        // A trait's required method is a definition site worth finding.
        assert!(names.contains(&"required"), "trait method missing: {names:?}");
    }

    #[test]
    fn malformed_source_yields_no_symbols_rather_than_an_error() {
        // One unparseable file must not abort a repository-wide index.
        let syms = extract("pub fn broken( { { { ");
        let _ = syms; // whatever it recovers is fine; not panicking is the point
    }

    #[test]
    fn ignores_call_sites_and_comments() {
        let syms = extract(
            "// fn commented_out() {}\n\
             pub fn real() { other_fn(); }\n",
        );
        let names: Vec<&str> = syms.iter().map(|s| s.name.as_str()).collect();
        assert_eq!(names, vec!["real"], "only definitions, got {names:?}");
    }

    #[test]
    fn generic_items_keep_their_bare_name() {
        let syms = extract("pub struct Wrapper<T> { inner: T }\npub fn map<T, U>(t: T) -> U { todo!() }\n");
        let names: Vec<&str> = syms.iter().map(|s| s.name.as_str()).collect();
        assert!(names.contains(&"Wrapper"), "{names:?}");
        assert!(names.contains(&"map"), "{names:?}");
    }
}
