#[cfg(test)]
mod tests {
    use rindex::ignore::{IgnoreEngine, IgnoreConfig};

    #[test]
    fn test_builtin_excludes_git() {
        let engine = IgnoreEngine::default();
        assert!(engine.should_ignore(".git/HEAD"));
    }

    #[test]
    fn test_builtin_excludes_node_modules() {
        let engine = IgnoreEngine::default();
        assert!(engine.should_ignore("node_modules/foo/index.js"));
    }

    #[test]
    fn test_source_file_not_ignored() {
        let engine = IgnoreEngine::default();
        assert!(!engine.should_ignore("src/main.rs"));
    }

    #[test]
    fn test_large_file_check() {
        let cfg = IgnoreConfig { max_file_size: 1_048_576 };
        let engine = IgnoreEngine::new(&cfg);
        assert!(engine.is_too_large(2_000_000));
        assert!(!engine.is_too_large(500_000));
    }

    #[test]
    fn test_gitignore_pattern() {
        let mut engine = IgnoreEngine::default();
        engine.add_gitignore_pattern("build/");
        assert!(engine.should_ignore("build/output.o"));
    }

    #[test]
    fn test_should_index_rust_file() {
        let engine = IgnoreEngine::default();
        assert!(engine.should_index("src/lib.rs", 1000, "rs"));
    }

    #[test]
    fn test_should_index_ignored_path() {
        let engine = IgnoreEngine::default();
        assert!(!engine.should_index(".git/config", 100, "txt"));
    }

    #[test]
    fn test_should_index_binary_ext() {
        let engine = IgnoreEngine::default();
        assert!(!engine.should_index("image.png", 50000, "png"));
    }

    #[test]
    fn test_should_index_too_large() {
        let engine = IgnoreEngine::default();
        assert!(!engine.should_index("big_file.rs", 2_000_000, "rs"));
    }
}
