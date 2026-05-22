#[cfg(test)]
mod tests {
    use rindex::indexer::parser::parse_code;

    #[test]
    fn test_parse_rust_function() {
        let code = "fn hello(name: &str) -> String {\n    format!(\"Hello, {}!\", name)\n}\n";
        let symbols = parse_code(code, "rust").unwrap();
        let funcs: Vec<_> = symbols.iter().filter(|s| s.symbol_type == "function").collect();
        assert!(!funcs.is_empty());
        assert!(funcs.iter().any(|f| f.name == Some("hello".to_string())));
    }

    #[test]
    fn test_parse_rust_struct() {
        let code = "struct User {\n    name: String,\n    age: u32,\n}\n";
        let symbols = parse_code(code, "rust").unwrap();
        assert!(symbols.iter().any(|s| s.name == Some("User".to_string())));
    }

    #[test]
    fn test_unsupported_language() {
        let symbols = parse_code("text", "unknown").unwrap();
        assert!(symbols.is_empty());
    }

    #[test]
    fn test_parse_python_function() {
        let code = "def hello(name):\n    return f\"Hello, {name}!\"\n";
        let symbols = parse_code(code, "python").unwrap();
        assert!(symbols.iter().any(|s| s.name == Some("hello".to_string())));
    }

    #[test]
    fn test_parse_empty_code() {
        let symbols = parse_code("", "rust").unwrap();
        assert!(symbols.is_empty());
    }
}
