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
}
