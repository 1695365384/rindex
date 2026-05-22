use anyhow::Result;
use tree_sitter::{Language, Node, Parser};

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
        "java" => Some(tree_sitter_java::LANGUAGE.into()),
        "cpp" | "c" | "c++" | "h" | "hpp" => Some(tree_sitter_cpp::LANGUAGE.into()),
        "kotlin" | "kt" | "kts" => Some(tree_sitter_kotlin_ng::LANGUAGE.into()),
        "ruby" | "rb" => Some(tree_sitter_ruby::LANGUAGE.into()),
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

fn extract_symbols(node: Node, source: &str, symbols: &mut Vec<Symbol>, lang: &str) {
    let kind = node.kind();
    let is_symbol = match lang {
        "rust" => matches!(kind,
            "function_item" | "struct_item" | "enum_item" | "trait_item"
            | "impl_item" | "macro_definition" | "mod_item"
        ),
        "python" => matches!(kind, "function_definition" | "class_definition"),
        "javascript" | "typescript" | "tsx" => matches!(kind,
            "function_declaration" | "class_declaration" | "method_definition"
            | "interface_declaration" | "type_alias_declaration"
        ),
        "go" => matches!(kind,
            "function_declaration" | "method_declaration" | "type_declaration" | "interface_type"
        ),
        "java" => matches!(kind,
            "method_declaration" | "class_declaration" | "interface_declaration"
        ),
        "cpp" | "c" => matches!(kind,
            "function_definition" | "class_specifier" | "struct_specifier"
        ),
        "kotlin" | "kt" | "kts" => matches!(kind,
            "function" | "class_declaration" | "interface_declaration"
        ),
        "ruby" | "rb" => matches!(kind,
            "method" | "class" | "module"
        ),
        _ => false,
    };

    if is_symbol {
        // Include preceding docstrings/comments by looking backwards
        let start_byte = find_content_start(node, source);
        let end_byte = node.end_byte();
        let content = source[start_byte..end_byte].to_string();
        let start_line = source[..start_byte].matches('\n').count() + 1;
        let end_line = source[..end_byte].matches('\n').count() + 1;
        let name = find_name_node(node, source);
        symbols.push(Symbol {
            symbol_type: kind_to_type(kind, lang),
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

/// Scan backwards from the symbol node to include preceding docstrings/comments
fn find_content_start(node: Node, source: &str) -> usize {
    let byte = node.start_byte();
    if byte == 0 {
        return 0;
    }

    // Look backwards up to 100 bytes for doc comments
    let lookback_start = if byte > 100 { byte - 100 } else { 0 };
    let preceding = &source[lookback_start..byte];
    let mut doc_start = byte;

    // Find the last docstring/comment block before the symbol
    let lines: Vec<&str> = preceding.lines().collect();
    let mut found_doc = false;

    for (i, line) in lines.iter().enumerate().rev() {
        let trimmed = line.trim();
        let is_doc = trimmed.starts_with("///") || trimmed.starts_with("//!")
            || trimmed.starts_with("/**") || trimmed.starts_with("*")
            || trimmed.starts_with("```") || trimmed.starts_with("#")
            || trimmed.starts_with("\"\"\"") || trimmed.starts_with("'''")
            || trimmed.starts_with("--[[") || trimmed.starts_with("--");

        if is_doc {
            found_doc = true;
        } else if found_doc {
            // non-doc line found after doc started — doc block ended on next line
            let doc_line_index = i + 1;
            let offset: usize = lines[..doc_line_index].iter().map(|l| l.len() + 1).sum();
            doc_start = lookback_start + offset;
            break;
        }
    }

    if found_doc {
        doc_start
    } else {
        byte // no doc found, start at the symbol
    }
}

fn kind_to_type(kind: &str, _lang: &str) -> String {
    match kind {
        "function_item" | "function_definition" | "function_declaration" | "function"
        | "method" | "method_declaration" | "method_definition" => "function".to_string(),
        "struct_item" | "class_definition" | "class_declaration" | "class_specifier"
        | "struct_specifier" | "class" => "class".to_string(),
        "enum_item" | "interface_declaration" | "interface_type" | "interface" => "interface".to_string(),
        "trait_item" => "trait".to_string(),
        "impl_item" => "implementation".to_string(),
        "macro_definition" => "macro".to_string(),
        "mod_item" | "module" => "module".to_string(),
        "type_alias_declaration" | "type_declaration" => "type".to_string(),
        _ => kind.to_string(),
    }
}

fn find_name_node(node: Node, source: &str) -> Option<String> {
    for i in 0..node.child_count() {
        if let Some(child) = node.child(i as u32) {
            if matches!(child.kind(), "name" | "identifier" | "type_identifier") {
                return child.utf8_text(source.as_bytes()).ok().map(|s| s.to_string());
            }
        }
    }
    None
}
