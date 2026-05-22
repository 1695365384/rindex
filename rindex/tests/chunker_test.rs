#[cfg(test)]
mod tests {
    use rindex::indexer::chunker::chunk_file;

    #[test]
    fn test_chunk_rust_file() {
        let code = "fn hello(name: &str) -> String {\n    format!(\"Hello, {}!\", name)\n}\n\nstruct User {\n    name: String,\n}\n";
        let chunks = chunk_file(code, "rust").unwrap();
        assert!(!chunks.is_empty());
        assert!(chunks.iter().any(|c| c.name == Some("hello".to_string())));
        assert!(chunks.iter().any(|c| c.name == Some("User".to_string())));
    }

    #[test]
    fn test_chunk_unknown_language_falls_back() {
        let code = "Some plain text content.\n\nAnother paragraph here.\n";
        let chunks = chunk_file(code, "unknown").unwrap();
        assert!(!chunks.is_empty());
        assert_eq!(chunks[0].chunk_type, "paragraph");
    }

    #[test]
    fn test_empty_code() {
        let chunks = chunk_file("", "rust").unwrap();
        assert!(chunks.is_empty());
    }

    #[test]
    fn test_chunk_single_line_without_trailing_newline() {
        let code = "fn foo() {}";
        let chunks = chunk_file(code, "rust").unwrap();
        assert!(!chunks.is_empty());
        assert_eq!(chunks[0].name.as_deref(), Some("foo"));
    }

    #[test]
    fn test_chunk_multiple_functions() {
        let code = "fn a() {}\nfn b() {}\nfn c() {}\n";
        let chunks = chunk_file(code, "rust").unwrap();
        let names: Vec<&str> = chunks.iter().filter_map(|c| c.name.as_deref()).collect();
        assert!(names.contains(&"a"));
        assert!(names.contains(&"b"));
        assert!(names.contains(&"c"));
    }

    #[test]
    fn test_chunk_python_class_and_method() {
        let code = "class User:\n    def __init__(self, name):\n        self.name = name\n";
        let chunks = chunk_file(code, "python").unwrap();
        assert!(chunks.iter().any(|c| c.name == Some("User".to_string())));
    }

    #[test]
    fn test_paragraph_chunk_empty_lines() {
        let code = "\n\n\n";
        let chunks = chunk_file(code, "unknown").unwrap();
        assert!(chunks.is_empty());
    }

    #[test]
    fn test_paragraph_chunk_single_word() {
        let code = "hello";
        let chunks = chunk_file(code, "unknown").unwrap();
        assert_eq!(chunks.len(), 1);
        assert_eq!(chunks[0].chunk_type, "paragraph");
    }
}
