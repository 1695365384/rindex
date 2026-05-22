use anyhow::Result;
use tree_sitter::{Language, Parser};

#[derive(Debug, Clone)]
pub struct Symbol {
    pub symbol_type: String,
    pub name: Option<String>,
    pub start_line: usize,
    pub end_line: usize,
    pub content: String,
}

pub fn get_language(name: &str) -> Option<Language> {
    match name {
        "rust" => Some(tree_sitter_rust::LANGUAGE.into()),
        "python" => Some(tree_sitter_python::LANGUAGE.into()),
        "javascript" | "js" | "jsx" => Some(tree_sitter_javascript::LANGUAGE.into()),
        "typescript" | "ts" => Some(tree_sitter_typescript::LANGUAGE_TYPESCRIPT.into()),
        "tsx" => Some(tree_sitter_typescript::LANGUAGE_TSX.into()),
        "go" => Some(tree_sitter_go::LANGUAGE.into()),
        _ => None,
    }
}

pub fn parse_code(code: &str, language_name: &str) -> Result<Vec<Symbol>> {
    let lang = match get_language(language_name) {
        Some(l) => l,
        None => return Ok(Vec::new()),
    };
    let mut parser = Parser::new();
    parser.set_language(&lang)?;
    let tree = match parser.parse(code, None) {
        Some(t) => t,
        None => return Ok(Vec::new()),
    };
    let root = tree.root_node();
    let mut symbols = Vec::new();
    extract_symbols(root, code, &mut symbols, language_name);
    Ok(symbols)
}

fn extract_symbols(node: tree_sitter::Node, source: &str, symbols: &mut Vec<Symbol>, lang: &str) {
    let kind = node.kind();
    let is_symbol = match lang {
        "rust" => matches!(
            kind,
            "function_item"
                | "struct_item"
                | "enum_item"
                | "trait_item"
                | "impl_item"
                | "macro_definition"
                | "mod_item"
        ),
        "python" => matches!(kind, "function_definition" | "class_definition"),
        "javascript" | "typescript" | "tsx" => matches!(
            kind,
            "function_declaration"
                | "class_declaration"
                | "method_definition"
                | "interface_declaration"
                | "type_alias_declaration"
        ),
        "go" => matches!(
            kind,
            "function_declaration" | "method_declaration" | "type_declaration" | "interface_type"
        ),
        _ => false,
    };

    if is_symbol {
        let start_line = node.start_position().row + 1;
        let end_line = node.end_position().row + 1;
        let content = node
            .utf8_text(source.as_bytes())
            .unwrap_or("")
            .to_string();
        let name = find_name_node(node, source);
        symbols.push(Symbol {
            symbol_type: kind_to_type(kind),
            name,
            start_line,
            end_line,
            content,
        });
    }

    for i in 0..node.child_count() {
        if let Some(child) = node.child(i as u32) {
            extract_symbols(child, source, symbols, lang);
        }
    }
}

fn kind_to_type(kind: &str) -> String {
    match kind {
        "function_item" | "function_definition" | "function_declaration" => "function".to_string(),
        "method_definition" | "method_declaration" => "method".to_string(),
        "struct_item" | "class_definition" | "class_declaration" => "class".to_string(),
        "enum_item" | "interface_declaration" | "interface_type" => "interface".to_string(),
        "trait_item" => "trait".to_string(),
        "impl_item" => "implementation".to_string(),
        "macro_definition" => "macro".to_string(),
        "mod_item" => "module".to_string(),
        "type_alias_declaration" | "type_declaration" => "type".to_string(),
        _ => kind.to_string(),
    }
}

fn find_name_node(node: tree_sitter::Node, source: &str) -> Option<String> {
    for i in 0..node.child_count() {
        if let Some(child) = node.child(i as u32) {
            if matches!(child.kind(), "name" | "identifier" | "type_identifier") {
                return child
                    .utf8_text(source.as_bytes())
                    .ok()
                    .map(|s| s.to_string());
            }
        }
    }
    None
}
